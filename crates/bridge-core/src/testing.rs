//! Test support: in-memory fakes shared by unit and integration tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use llm_providers::{ChatCompletion, ChatProvider, ChatRequest, ProviderError, TokenUsage};

use crate::store::{
    ConversationStore, ExchangeRecord, MessageRole, StoreError, StoredMessage, Summary, UsageStats,
};

/// [`ChatProvider`] fake with a scripted reply, an optional delay and a log
/// of every request it received.
pub struct ScriptedProvider {
    pub delay: Duration,
    /// `Ok(reply)` or `Err(kind)` where kind is one of
    /// `"timeout" | "rate" | "auth"`; anything else maps to an API error.
    pub script: std::result::Result<String, String>,
    pub calls: Mutex<Vec<ChatRequest>>,
}

impl ScriptedProvider {
    pub fn replying(text: &str) -> Arc<Self> {
        Arc::new(Self {
            delay: Duration::ZERO,
            script: Ok(text.to_string()),
            calls: Mutex::new(Vec::new()),
        })
    }

    pub fn slow(text: &str, delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            delay,
            script: Ok(text.to_string()),
            calls: Mutex::new(Vec::new()),
        })
    }

    pub fn failing(kind: &str) -> Arc<Self> {
        Arc::new(Self {
            delay: Duration::ZERO,
            script: Err(kind.to_string()),
            calls: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait::async_trait]
impl ChatProvider for ScriptedProvider {
    async fn chat(&self, request: &ChatRequest) -> llm_providers::Result<ChatCompletion> {
        self.calls
            .lock()
            .expect("scripted provider mutex poisoned")
            .push(request.clone());
        tokio::time::sleep(self.delay).await;
        match &self.script {
            Ok(text) => Ok(ChatCompletion {
                text: text.clone(),
                usage: TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                },
            }),
            Err(kind) => Err(match kind.as_str() {
                "timeout" => ProviderError::Timeout,
                "rate" => ProviderError::RateLimited,
                "auth" => ProviderError::Auth,
                other => ProviderError::Api {
                    status: 500,
                    message: other.to_string(),
                },
            }),
        }
    }
}

#[derive(Debug, Clone)]
struct Row {
    id: i64,
    profile: String,
    role: MessageRole,
    content: String,
    cost_micros: i64,
    created_at: DateTime<Utc>,
}

/// In-memory [`ConversationStore`] with the same semantics as the
/// production Postgres implementation.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    rows: Vec<Row>,
    summaries: HashMap<String, Summary>,
    next_id: i64,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl ConversationStore for MemoryStore {
    async fn record_exchange(&self, exchange: &ExchangeRecord) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store mutex poisoned");
        let now = Utc::now();
        for (role, content, cost) in [
            (MessageRole::User, &exchange.user_text, 0),
            (
                MessageRole::Assistant,
                &exchange.assistant_text,
                exchange.cost_micros,
            ),
        ] {
            inner.next_id += 1;
            let id = inner.next_id;
            inner.rows.push(Row {
                id,
                profile: exchange.profile.clone(),
                role,
                content: content.clone(),
                cost_micros: cost,
                created_at: now,
            });
        }
        Ok(())
    }

    async fn recent_messages(
        &self,
        profile: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, StoreError> {
        let inner = self.inner.lock().expect("memory store mutex poisoned");
        let mut messages: Vec<StoredMessage> = inner
            .rows
            .iter()
            .filter(|r| r.profile == profile)
            .rev()
            .take(limit)
            .map(|r| StoredMessage {
                role: r.role,
                content: r.content.clone(),
            })
            .collect();
        messages.reverse();
        Ok(messages)
    }

    async fn summary(&self, profile: &str) -> Result<Option<Summary>, StoreError> {
        Ok(self
            .inner
            .lock()
            .expect("memory store mutex poisoned")
            .summaries
            .get(profile)
            .cloned())
    }

    async fn upsert_summary(&self, profile: &str, summary: &Summary) -> Result<(), StoreError> {
        self.inner
            .lock()
            .expect("memory store mutex poisoned")
            .summaries
            .insert(profile.to_string(), summary.clone());
        Ok(())
    }

    async fn unsummarized(
        &self,
        profile: &str,
        keep_last: usize,
    ) -> Result<Vec<(i64, StoredMessage)>, StoreError> {
        let inner = self.inner.lock().expect("memory store mutex poisoned");
        let covers_until = inner
            .summaries
            .get(profile)
            .map(|s| s.covers_until_message_id)
            .unwrap_or(0);
        let mut rows: Vec<(i64, StoredMessage)> = inner
            .rows
            .iter()
            .filter(|r| r.profile == profile && r.id > covers_until)
            .map(|r| {
                (
                    r.id,
                    StoredMessage {
                        role: r.role,
                        content: r.content.clone(),
                    },
                )
            })
            .collect();
        rows.truncate(rows.len().saturating_sub(keep_last));
        Ok(rows)
    }

    async fn clear_profile(&self, profile: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.lock().expect("memory store mutex poisoned");
        inner.rows.retain(|r| r.profile != profile);
        inner.summaries.remove(profile);
        Ok(())
    }

    async fn usage_since(&self, since: DateTime<Utc>) -> Result<UsageStats, StoreError> {
        let inner = self.inner.lock().expect("memory store mutex poisoned");
        let mut stats = UsageStats::default();
        for row in inner.rows.iter().filter(|r| r.created_at >= since) {
            if row.role == MessageRole::Assistant {
                stats.requests += 1;
            }
            stats.cost_micros += row.cost_micros;
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExchangeRecord, MessageRole, Summary};

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

    #[tokio::test]
    async fn records_and_reads_recent_messages() {
        let store = MemoryStore::new();
        for n in 1..=3 {
            store.record_exchange(&exchange("Дима", n)).await.unwrap();
        }
        let recent = store.recent_messages("Дима", 4).await.unwrap();
        assert_eq!(recent.len(), 4);
        assert_eq!(recent[0].role, MessageRole::User);
        assert_eq!(recent[0].content, "вопрос 2");
        assert_eq!(recent[3].content, "ответ 3");
        assert!(store.recent_messages("Маша", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn summary_roundtrip_and_unsummarized() {
        let store = MemoryStore::new();
        for n in 1..=4 {
            store.record_exchange(&exchange("Дима", n)).await.unwrap();
        }
        let pending = store.unsummarized("Дима", 4).await.unwrap();
        assert_eq!(pending.len(), 4);
        assert_eq!(pending[0].1.content, "вопрос 1");

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
    }

    #[tokio::test]
    async fn usage_counts_assistant_replies() {
        let store = MemoryStore::new();
        store.record_exchange(&exchange("Дима", 1)).await.unwrap();
        store.record_exchange(&exchange("Маша", 2)).await.unwrap();
        let stats = store
            .usage_since(chrono::Utc::now() - chrono::Duration::hours(1))
            .await
            .unwrap();
        assert_eq!(stats.requests, 2);
        assert_eq!(stats.cost_micros, 200);
        let none = store
            .usage_since(chrono::Utc::now() + chrono::Duration::hours(1))
            .await
            .unwrap();
        assert_eq!(none.requests, 0);
    }

    #[tokio::test]
    async fn clear_profile_removes_history_and_summary() {
        let store = MemoryStore::new();
        store.record_exchange(&exchange("Дима", 1)).await.unwrap();
        store
            .upsert_summary(
                "Дима",
                &Summary {
                    content: "s".to_string(),
                    covers_until_message_id: 1,
                },
            )
            .await
            .unwrap();
        store.clear_profile("Дима").await.unwrap();
        assert!(store.recent_messages("Дима", 10).await.unwrap().is_empty());
        assert!(store.summary("Дима").await.unwrap().is_none());
    }
}
