//! Answers that missed the webhook deadline, waiting for the next utterance.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::time::Instant;

const THINKING_TTL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
enum PendingState {
    Thinking { since: Instant },
    Ready(String),
}

/// Outcome of asking whether an answer is ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    None,
    StillThinking,
    Answer(String),
}

/// In-memory map of running/finished deferred answers, keyed by Alice
/// `user_id`. Entries live only in memory: after a restart the user simply
/// asks again.
#[derive(Debug, Clone, Default)]
pub struct PendingAnswers {
    inner: Arc<DashMap<String, PendingState>>,
}

impl PendingAnswers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_thinking(&self, key: &str) {
        self.inner.insert(
            key.to_string(),
            PendingState::Thinking {
                since: Instant::now(),
            },
        );
    }

    /// Upgrades a thinking entry to ready. A no-op if the caller already
    /// delivered the answer synchronously and cleared the key.
    pub fn complete(&self, key: &str, text: String) {
        if let Some(mut entry) = self.inner.get_mut(key) {
            *entry = PendingState::Ready(text);
        }
    }

    pub fn clear(&self, key: &str) {
        self.inner.remove(key);
    }

    pub fn poll(&self, key: &str) -> Poll {
        let state = match self.inner.get(key) {
            Some(entry) => entry.clone(),
            None => return Poll::None,
        };
        match state {
            PendingState::Ready(_) => match self.inner.remove(key) {
                Some((_, PendingState::Ready(text))) => Poll::Answer(text),
                _ => Poll::None,
            },
            PendingState::Thinking { since } => {
                if since.elapsed() > THINKING_TTL {
                    self.inner.remove(key);
                    Poll::None
                } else {
                    Poll::StillThinking
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn ready_answer_is_returned_once() {
        let pending = PendingAnswers::new();
        pending.mark_thinking("u1");
        pending.complete("u1", "ответ".to_string());
        assert!(matches!(pending.poll("u1"), Poll::Answer(t) if t == "ответ"));
        assert!(matches!(pending.poll("u1"), Poll::None));
    }

    #[tokio::test]
    async fn thinking_reports_still_thinking() {
        let pending = PendingAnswers::new();
        pending.mark_thinking("u1");
        assert!(matches!(pending.poll("u1"), Poll::StillThinking));
        assert!(matches!(pending.poll("u2"), Poll::None));
    }

    #[tokio::test]
    async fn complete_without_waiter_is_dropped() {
        let pending = PendingAnswers::new();
        pending.mark_thinking("u1");
        pending.clear("u1");
        pending.complete("u1", "поздно".to_string());
        assert!(matches!(pending.poll("u1"), Poll::None));
    }

    #[tokio::test(start_paused = true)]
    async fn stale_thinking_expires() {
        let pending = PendingAnswers::new();
        pending.mark_thinking("u1");
        tokio::time::advance(Duration::from_secs(121)).await;
        assert!(matches!(pending.poll("u1"), Poll::None));
    }
}
