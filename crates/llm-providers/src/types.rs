use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Author of a chat message, serialized in OpenAI wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// One turn of a chat conversation.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// A completion request, provider-agnostic.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
}

/// Token counts reported by the provider; used for cost accounting.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// A successful completion.
#[derive(Debug, Clone)]
pub struct ChatCompletion {
    pub text: String,
    pub usage: TokenUsage,
}

/// Anything that can answer a chat request: real HTTP clients in production,
/// scripted fakes in tests.
#[async_trait::async_trait]
pub trait ChatProvider: Send + Sync {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatCompletion>;
}
