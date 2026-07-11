//! Postgres implementation of [`bridge_core::ConversationStore`].

use bridge_core::{
    ConversationStore, ExchangeRecord, MessageRole, StoreError, StoredMessage, Summary, UsageStats,
};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn db_err(err: sqlx::Error) -> StoreError {
    StoreError(err.to_string())
}

fn role_from_str(role: &str) -> MessageRole {
    if role == "assistant" {
        MessageRole::Assistant
    } else {
        MessageRole::User
    }
}

#[async_trait::async_trait]
impl ConversationStore for PgStore {
    async fn record_exchange(&self, exchange: &ExchangeRecord) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query("INSERT INTO bridge.messages (profile, role, content) VALUES ($1, 'user', $2)")
            .bind(&exchange.profile)
            .bind(&exchange.user_text)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query(
            "INSERT INTO bridge.messages \
             (profile, role, content, model, prompt_tokens, completion_tokens, cost_micros) \
             VALUES ($1, 'assistant', $2, $3, $4, $5, $6)",
        )
        .bind(&exchange.profile)
        .bind(&exchange.assistant_text)
        .bind(&exchange.model)
        .bind(exchange.prompt_tokens as i32)
        .bind(exchange.completion_tokens as i32)
        .bind(exchange.cost_micros)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn recent_messages(
        &self,
        profile: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, StoreError> {
        let rows = sqlx::query(
            "SELECT role, content FROM bridge.messages WHERE profile = $1 ORDER BY id DESC LIMIT $2",
        )
        .bind(profile)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let mut messages: Vec<StoredMessage> = rows
            .into_iter()
            .map(|row| StoredMessage {
                role: role_from_str(row.get("role")),
                content: row.get("content"),
            })
            .collect();
        messages.reverse();
        Ok(messages)
    }

    async fn summary(&self, profile: &str) -> Result<Option<Summary>, StoreError> {
        let row = sqlx::query(
            "SELECT content, covers_until_message_id FROM bridge.summaries WHERE profile = $1",
        )
        .bind(profile)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(row.map(|row| Summary {
            content: row.get("content"),
            covers_until_message_id: row.get("covers_until_message_id"),
        }))
    }

    async fn upsert_summary(&self, profile: &str, summary: &Summary) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO bridge.summaries (profile, content, covers_until_message_id, updated_at) \
             VALUES ($1, $2, $3, now()) \
             ON CONFLICT (profile) DO UPDATE \
             SET content = $2, covers_until_message_id = $3, updated_at = now()",
        )
        .bind(profile)
        .bind(&summary.content)
        .bind(summary.covers_until_message_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn unsummarized(
        &self,
        profile: &str,
        keep_last: usize,
    ) -> Result<Vec<(i64, StoredMessage)>, StoreError> {
        let rows = sqlx::query(
            "SELECT m.id, m.role, m.content FROM bridge.messages m \
             WHERE m.profile = $1 \
               AND m.id > COALESCE(\
                   (SELECT covers_until_message_id FROM bridge.summaries WHERE profile = $1), 0) \
             ORDER BY m.id",
        )
        .bind(profile)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        let mut result: Vec<(i64, StoredMessage)> = rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<i64, _>("id"),
                    StoredMessage {
                        role: role_from_str(row.get("role")),
                        content: row.get("content"),
                    },
                )
            })
            .collect();
        result.truncate(result.len().saturating_sub(keep_last));
        Ok(result)
    }

    async fn clear_profile(&self, profile: &str) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query("DELETE FROM bridge.messages WHERE profile = $1")
            .bind(profile)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query("DELETE FROM bridge.summaries WHERE profile = $1")
            .bind(profile)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)
    }

    async fn usage_since(&self, since: DateTime<Utc>) -> Result<UsageStats, StoreError> {
        let row = sqlx::query(
            "SELECT COUNT(*) FILTER (WHERE role = 'assistant') AS requests, \
                    COALESCE(SUM(cost_micros), 0)::BIGINT AS cost_micros \
             FROM bridge.messages WHERE created_at >= $1",
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(UsageStats {
            requests: row.get::<i64, _>("requests"),
            cost_micros: row.get::<i64, _>("cost_micros"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{ConversationStore, ExchangeRecord, MessageRole, Summary};

    fn exchange(profile: &str, n: u32) -> ExchangeRecord {
        ExchangeRecord {
            profile: profile.to_string(),
            user_text: format!("вопрос {n}"),
            assistant_text: format!("ответ {n}"),
            model: "test-model".to_string(),
            prompt_tokens: 10,
            completion_tokens: 20,
            cost_micros: 100,
        }
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn records_and_reads_recent_messages(pool: sqlx::PgPool) {
        let store = PgStore::new(pool);
        for n in 1..=3 {
            store.record_exchange(&exchange("Дима", n)).await.unwrap();
        }
        let recent = store.recent_messages("Дима", 4).await.unwrap();
        assert_eq!(recent.len(), 4);
        assert_eq!(recent[0].role, MessageRole::User);
        assert_eq!(recent[0].content, "вопрос 2");
        assert_eq!(recent[3].content, "ответ 3");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn summary_roundtrip_and_unsummarized(pool: sqlx::PgPool) {
        let store = PgStore::new(pool);
        for n in 1..=4 {
            store.record_exchange(&exchange("Дима", n)).await.unwrap();
        }
        let pending = store.unsummarized("Дима", 4).await.unwrap();
        assert_eq!(pending.len(), 4);

        let last_id = pending.last().unwrap().0;
        store
            .upsert_summary(
                "Дима",
                &Summary {
                    content: "резюме".to_string(),
                    covers_until_message_id: last_id,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store.summary("Дима").await.unwrap().unwrap().content,
            "резюме"
        );
        assert!(store.unsummarized("Дима", 4).await.unwrap().is_empty());

        store
            .upsert_summary(
                "Дима",
                &Summary {
                    content: "новое".to_string(),
                    covers_until_message_id: last_id,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store.summary("Дима").await.unwrap().unwrap().content,
            "новое"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn usage_and_clear(pool: sqlx::PgPool) {
        let store = PgStore::new(pool);
        store.record_exchange(&exchange("Дима", 1)).await.unwrap();
        store.record_exchange(&exchange("Маша", 2)).await.unwrap();

        let stats = store
            .usage_since(chrono::Utc::now() - chrono::Duration::hours(1))
            .await
            .unwrap();
        assert_eq!(stats.requests, 2);
        assert_eq!(stats.cost_micros, 200);

        store.clear_profile("Дима").await.unwrap();
        assert!(store.recent_messages("Дима", 10).await.unwrap().is_empty());
        assert_eq!(store.recent_messages("Маша", 10).await.unwrap().len(), 2);
    }
}
