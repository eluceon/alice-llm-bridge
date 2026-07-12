use serde::Serialize;

/// The Dialogs API rejects `text`/`tts` longer than 1024 characters.
pub const MAX_TEXT_LEN: usize = 1024;

/// Webhook reply envelope expected by Yandex Dialogs.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookResponse {
    pub response: SkillResponse,
    pub version: String,
}

/// The spoken part of the reply.
#[derive(Debug, Clone, Serialize)]
pub struct SkillResponse {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tts: Option<String>,
    pub end_session: bool,
}

impl WebhookResponse {
    /// Replies with `text` and keeps the session open for the next utterance.
    pub fn say(text: impl Into<String>) -> Self {
        Self::build(text.into(), false)
    }

    /// Replies with `text` and closes the session.
    pub fn say_and_close(text: impl Into<String>) -> Self {
        Self::build(text.into(), true)
    }

    fn build(text: String, end_session: bool) -> Self {
        let text = clip_to_limit(&text);
        Self {
            response: SkillResponse {
                tts: Some(text.clone()),
                text,
                end_session,
            },
            version: "1.0".to_string(),
        }
    }
}

/// Clips text to the protocol limit, preferring a sentence boundary and
/// falling back to the last whitespace so words are never cut in half.
pub fn clip_to_limit(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT_LEN {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_TEXT_LEN).collect();
    if let Some(pos) = cut.rfind(['.', '!', '?']) {
        return cut[..=pos].to_string();
    }
    match cut.rfind(char::is_whitespace) {
        Some(pos) => format!("{}…", cut[..pos].trim_end()),
        None => cut,
    }
}
