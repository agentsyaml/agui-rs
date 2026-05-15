use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgUiError {
    #[error("AG-UI protocol error: {0}")]
    Protocol(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error("JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("connect not implemented for this agent")]
    ConnectNotImplemented,

    #[error("event validation failed: {0}")]
    Validation(String),

    #[error("unsupported operation: {0}")]
    Unsupported(String),

    #[error("{0}")]
    Other(String),
}

impl AgUiError {
    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol(msg.into())
    }

    pub fn cancelled() -> Self {
        Self::Cancelled
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::Unsupported(msg.into())
    }

    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, AgUiError>;
