//! Dialogue engine for the Alice LLM bridge.
//!
//! The domain layer: family profiles, voice command parsing, prompt and
//! context assembly, deferred answers. Depends on abstractions only —
//! [`llm_providers::ChatProvider`] for models and [`ConversationStore`]
//! for persistence — so it stays free of HTTP and database concerns.

mod error;
mod profile;

pub use error::{CoreError, Result};
pub use profile::{FamilyRoster, Profile, ProfileRole};
