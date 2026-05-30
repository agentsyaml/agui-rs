use thiserror::Error;

/// The unified error type for the AG-UI SDK.
///
/// The variant set is intentionally `#[non_exhaustive]` so new categories can be
/// added without a breaking change. Match with a wildcard arm when consuming it
/// outside of this crate.
///
/// Use [`AgUiError::is_retryable`] to decide whether an operation can be safely
/// retried (e.g. with backoff), and [`AgUiError::is_user_input`] to distinguish
/// caller mistakes from transient/transport failures.
#[derive(Debug, Error)]
#[non_exhaustive]
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

    /// A non-success HTTP status returned by an agent endpoint.
    ///
    /// `status` is the raw status code, kept transport-agnostic (the core crate
    /// does not depend on any HTTP library). The response `body` and
    /// `content_type` are preserved for diagnostics.
    #[error("{}", format_http(*status, url.as_deref(), content_type.as_deref(), body))]
    Http {
        status: u16,
        url: Option<String>,
        content_type: Option<String>,
        body: String,
    },

    /// A transport-level failure (connection refused, timeout, DNS, etc.).
    ///
    /// `retryable` is set by the layer that produced the error (which has access
    /// to transport details) and is surfaced through [`AgUiError::is_retryable`].
    #[error("transport error: {message}")]
    Transport { message: String, retryable: bool },

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

    /// Builds an [`AgUiError::Http`] from a status code and response body.
    pub fn http(status: u16, body: impl Into<String>) -> Self {
        Self::Http {
            status,
            url: None,
            content_type: None,
            body: body.into(),
        }
    }

    /// Builds a transport error, explicitly flagging whether a retry is sensible.
    pub fn transport(msg: impl Into<String>, retryable: bool) -> Self {
        Self::Transport {
            message: msg.into(),
            retryable,
        }
    }

    /// The HTTP status code, if this error carries one.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Whether retrying the failed operation is likely to help.
    ///
    /// Retryable cases:
    /// - transport failures flagged retryable (connect/timeout/request errors)
    /// - HTTP `5xx` server errors
    /// - HTTP `429 Too Many Requests` (rate limiting / throttling)
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { retryable, .. } => *retryable,
            Self::Http { status, .. } => *status >= 500 || *status == 429,
            _ => false,
        }
    }

    /// Whether the error stems from invalid caller input rather than a
    /// transient or transport condition.
    pub fn is_user_input(&self) -> bool {
        matches!(self, Self::Validation(_))
    }
}

/// Renders the human-readable message for [`AgUiError::Http`], preserving the
/// `HTTP <status> from <url> (content-type: <ct>) <body>` shape.
fn format_http(status: u16, url: Option<&str>, content_type: Option<&str>, body: &str) -> String {
    let mut message = format!("HTTP {status}");
    if let Some(url) = url {
        message.push_str(&format!(" from {url}"));
    }
    match content_type {
        Some(content_type) if !content_type.is_empty() => {
            message.push_str(&format!(" (content-type: {content_type})"));
        }
        _ => {}
    }
    if !body.is_empty() {
        message.push(' ');
        message.push_str(body);
    }
    message
}

pub type Result<T> = std::result::Result<T, AgUiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_error_message_includes_status_url_and_body() {
        let error = AgUiError::Http {
            status: 404,
            url: Some("http://localhost:3000/".into()),
            content_type: Some("application/json".into()),
            body: r#"{"message":"User not found"}"#.into(),
        };

        let message = error.to_string();
        assert!(message.contains("HTTP 404"));
        assert!(message.contains("http://localhost:3000/"));
        assert!(message.contains("application/json"));
        assert!(message.contains("User not found"));
    }

    #[test]
    fn http_error_message_omits_optional_parts() {
        assert_eq!(AgUiError::http(503, "").to_string(), "HTTP 503");
    }

    #[test]
    fn server_errors_and_rate_limit_are_retryable() {
        assert!(AgUiError::http(500, "boom").is_retryable());
        assert!(AgUiError::http(503, "boom").is_retryable());
        assert!(AgUiError::http(429, "slow down").is_retryable());
    }

    #[test]
    fn client_errors_are_not_retryable() {
        assert!(!AgUiError::http(404, "nope").is_retryable());
        assert!(!AgUiError::http(400, "bad").is_retryable());
    }

    #[test]
    fn transport_retryability_is_explicit() {
        assert!(AgUiError::transport("connection refused", true).is_retryable());
        assert!(!AgUiError::transport("malformed", false).is_retryable());
    }

    #[test]
    fn validation_is_user_input_and_not_retryable() {
        let error = AgUiError::validation("bad field");
        assert!(error.is_user_input());
        assert!(!error.is_retryable());
    }

    #[test]
    fn status_accessor_returns_code_only_for_http() {
        assert_eq!(AgUiError::http(418, "teapot").status(), Some(418));
        assert_eq!(AgUiError::other("x").status(), None);
    }
}
