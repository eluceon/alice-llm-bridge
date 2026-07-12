//! Persistence port of the dialogue engine.

use chrono::{DateTime, Utc};

/// Author of a stored message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
}

/// One stored conversation turn.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub role: MessageRole,
    pub content: String,
}

/// A completed question/answer pair with its accounting data.
#[derive(Debug, Clone)]
pub struct ExchangeRecord {
    pub profile: String,
    pub user_text: String,
    pub assistant_text: String,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cost_micros: i64,
}

/// Rolling compression of history older than the context window.
#[derive(Debug, Clone)]
pub struct Summary {
    pub content: String,
    /// Highest message id folded into this summary.
    pub covers_until_message_id: i64,
}

/// Aggregated spending over a period.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageStats {
    /// Number of model answers.
    pub requests: i64,
    /// Total cost in micro-dollars.
    pub cost_micros: i64,
}

/// Storage failure; the engine logs it and keeps the dialogue alive.
#[derive(Debug, thiserror::Error)]
#[error("storage error: {0}")]
pub struct StoreError(pub String);

/// Persistence operations the engine needs. Implemented by Postgres in
/// production and by an in-memory fake in tests.
#[async_trait::async_trait]
pub trait ConversationStore: Send + Sync {
    async fn record_exchange(&self, exchange: &ExchangeRecord) -> Result<(), StoreError>;
    /// Last `limit` messages of the profile, oldest first.
    async fn recent_messages(
        &self,
        profile: &str,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, StoreError>;
    async fn summary(&self, profile: &str) -> Result<Option<Summary>, StoreError>;
    async fn upsert_summary(&self, profile: &str, summary: &Summary) -> Result<(), StoreError>;
    /// Messages newer than the current summary but older than the newest
    /// `keep_last`, oldest first, with their ids.
    async fn unsummarized(
        &self,
        profile: &str,
        keep_last: usize,
    ) -> Result<Vec<(i64, StoredMessage)>, StoreError>;
    async fn clear_profile(&self, profile: &str) -> Result<(), StoreError>;
    /// Spending across all profiles since the given instant.
    async fn usage_since(&self, since: DateTime<Utc>) -> Result<UsageStats, StoreError>;
}
