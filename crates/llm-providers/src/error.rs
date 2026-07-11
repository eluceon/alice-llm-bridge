/// Failures a chat provider can report, classified so the dialogue layer
/// can phrase each of them differently for the user.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("request timed out")]
    Timeout,
    #[error("rate limited")]
    RateLimited,
    #[error("authentication failed")]
    Auth,
    #[error("provider returned {status}: {message}")]
    Api { status: u16, message: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected response shape: {0}")]
    InvalidResponse(String),
}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, ProviderError>;
