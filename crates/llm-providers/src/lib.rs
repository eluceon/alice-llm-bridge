//! Chat provider abstraction and an OpenAI-compatible HTTP client.
//!
//! The [`ChatProvider`] trait is the only thing the dialogue engine depends
//! on; [`OpenAiCompatClient`] implements it for any endpoint that speaks the
//! OpenAI chat completions wire format.

mod error;
mod openai;
mod types;

pub use error::{ProviderError, Result};
pub use openai::OpenAiCompatClient;
pub use types::{ChatCompletion, ChatMessage, ChatProvider, ChatRequest, Role, TokenUsage};
