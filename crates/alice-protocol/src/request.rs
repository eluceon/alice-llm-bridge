use serde::Deserialize;

/// Incoming webhook call from Yandex Dialogs.
///
/// Mirrors the JSON documented at
/// <https://yandex.ru/dev/dialogs/alice/doc/request.html>; fields the bridge
/// does not use are deliberately omitted and ignored during deserialization.
#[derive(Debug, Clone, Deserialize)]
pub struct WebhookRequest {
    pub meta: Meta,
    pub session: Session,
    pub request: UserRequest,
    pub version: String,
}

/// Device and locale information.
#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub locale: String,
    pub timezone: String,
    #[serde(default)]
    pub client_id: String,
}

/// Dialogue session state passed with every request.
#[derive(Debug, Clone, Deserialize)]
pub struct Session {
    pub message_id: u64,
    pub session_id: String,
    pub skill_id: String,
    /// True for the first request of a session.
    pub new: bool,
    /// Present only when the device is signed into a Yandex account.
    #[serde(default)]
    pub user: Option<AccountUser>,
    pub application: Application,
}

/// Yandex account identity; stable across the user's devices.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountUser {
    pub user_id: String,
}

/// Device instance identity; used as a fallback when no account is attached.
#[derive(Debug, Clone, Deserialize)]
pub struct Application {
    pub application_id: String,
}

/// What the user said or pressed.
#[derive(Debug, Clone, Deserialize)]
pub struct UserRequest {
    /// Normalized utterance (lowercased, numbers as digits).
    #[serde(default)]
    pub command: String,
    /// Utterance exactly as recognized.
    #[serde(default)]
    pub original_utterance: String,
    #[serde(rename = "type")]
    pub kind: RequestKind,
}

/// Request type; variants the bridge does not handle collapse into [`RequestKind::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum RequestKind {
    SimpleUtterance,
    ButtonPressed,
    #[serde(other)]
    Other,
}
