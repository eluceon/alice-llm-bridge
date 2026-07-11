//! Dialogue engine for the Alice LLM bridge.
//!
//! The domain layer: family profiles, voice command parsing, prompt and
//! context assembly, deferred answers. Depends on abstractions only —
//! [`llm_providers::ChatProvider`] for models and [`ConversationStore`]
//! for persistence — so it stays free of HTTP and database concerns.

pub mod command;
mod error;
mod mode;
mod profile;
mod prompt;
mod store;
pub mod testing;

pub use error::{CoreError, Result};
pub use mode::Mode;
pub use profile::{FamilyRoster, Profile, ProfileRole};
pub use prompt::{PromptContext, build_system_prompt};
pub use store::{
    ConversationStore, ExchangeRecord, MessageRole, StoreError, StoredMessage, Summary, UsageStats,
};
