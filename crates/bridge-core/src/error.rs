/// Domain-level errors of the dialogue engine.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("unknown profile: {0}")]
    UnknownProfile(String),
}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, CoreError>;
