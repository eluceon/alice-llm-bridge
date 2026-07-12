use std::time::Duration;

use serde::Deserialize;

use crate::error::{ProviderError, Result};
use crate::types::{ChatCompletion, ChatMessage, ChatProvider, ChatRequest, TokenUsage};

/// Client for any OpenAI-compatible chat completions API
/// (DeepSeek, OpenRouter, OpenAI, Ollama, ...).
pub struct OpenAiCompatClient {
    http: reqwest::Client,
    chat_url: String,
    api_key: String,
}

impl OpenAiCompatClient {
    pub fn new(base_url: &str, api_key: String, timeout: Duration) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        Ok(Self {
            http,
            chat_url: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            api_key,
        })
    }

    async fn send_once(&self, request: &ChatRequest) -> Result<ChatCompletion> {
        let body = ApiRequest {
            model: &request.model,
            messages: &request.messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
        };
        let response = self
            .http
            .post(&self.chat_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(classify_transport_error)?;

        let status = response.status();
        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                401 | 403 => ProviderError::Auth,
                429 => ProviderError::RateLimited,
                code => ProviderError::Api {
                    status: code,
                    message,
                },
            });
        }

        let parsed: ApiResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(e.to_string()))?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::InvalidResponse("empty choices".to_string()))?;
        Ok(ChatCompletion {
            text: choice.message.content,
            usage: parsed.usage.unwrap_or_default(),
        })
    }
}

#[async_trait::async_trait]
impl ChatProvider for OpenAiCompatClient {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatCompletion> {
        match self.send_once(request).await {
            Err(err) if is_retryable(&err) => {
                tracing::debug!(error = %err, "retrying chat request");
                self.send_once(request).await
            }
            result => result,
        }
    }
}

fn is_retryable(err: &ProviderError) -> bool {
    matches!(
        err,
        ProviderError::Network(_)
            | ProviderError::Api {
                status: 500..=599,
                ..
            }
    )
}

fn classify_transport_error(err: reqwest::Error) -> ProviderError {
    if err.is_timeout() {
        ProviderError::Timeout
    } else {
        ProviderError::Network(err.to_string())
    }
}

#[derive(serde::Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct ApiResponse {
    choices: Vec<ApiChoice>,
    usage: Option<TokenUsage>,
}

#[derive(Deserialize)]
struct ApiChoice {
    message: ApiMessage,
}

#[derive(Deserialize)]
struct ApiMessage {
    content: String,
}
