use serde::Serialize;

/// Errors returned by the Tragentics API client, mapped from HTTP status +
/// the platform's `{"error": "..."}` body shape (lib/utils/api.ts `err()`).
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApiError {
    /// 401 — invalid key, revoked, archived, or auto-disabled (message carries which).
    #[error("unauthorized: {message}")]
    Unauthorized { message: String },
    /// 403 — token/agent mismatch or auto-disabled.
    #[error("forbidden: {message}")]
    Forbidden { message: String },
    /// 404 — agent not found.
    #[error("not found: {message}")]
    NotFound { message: String },
    /// 409 — agent no longer available (revoked/archived/locked mid-flight).
    #[error("conflict: {message}")]
    Conflict { message: String },
    /// 429 — rate limited; retry_after from the Retry-After header (seconds).
    #[error("rate limited (retry after {retry_after_secs}s)")]
    RateLimited { retry_after_secs: u64 },
    /// 5xx from the platform.
    #[error("server error {status}: {message}")]
    Server { status: u16, message: String },
    /// Any other unexpected HTTP status.
    #[error("unexpected status {status}: {message}")]
    Other { status: u16, message: String },
    /// DNS/TLS/connect/timeout — no HTTP response at all.
    #[error("network error: {message}")]
    Network { message: String },
    /// Response body did not match the expected shape.
    #[error("decode error: {message}")]
    Decode { message: String },
}

impl ApiError {
    /// Short machine label used in history records and the activity feed.
    pub fn label(&self) -> &'static str {
        match self {
            ApiError::Unauthorized { .. } => "unauthorized",
            ApiError::Forbidden { .. } => "forbidden",
            ApiError::NotFound { .. } => "not_found",
            ApiError::Conflict { .. } => "conflict",
            ApiError::RateLimited { .. } => "rate_limited",
            ApiError::Server { .. } => "server_error",
            ApiError::Other { .. } => "http_error",
            ApiError::Network { .. } => "network",
            ApiError::Decode { .. } => "decode",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ApiError::Unauthorized { message }
            | ApiError::Forbidden { message }
            | ApiError::NotFound { message }
            | ApiError::Conflict { message }
            | ApiError::Server { message, .. }
            | ApiError::Other { message, .. }
            | ApiError::Network { message }
            | ApiError::Decode { message } => message.clone(),
            ApiError::RateLimited { retry_after_secs } => {
                format!("Rate limited — retry after {retry_after_secs}s")
            }
        }
    }

    pub fn http_status(&self) -> Option<u16> {
        match self {
            ApiError::Unauthorized { .. } => Some(401),
            ApiError::Forbidden { .. } => Some(403),
            ApiError::NotFound { .. } => Some(404),
            ApiError::Conflict { .. } => Some(409),
            ApiError::RateLimited { .. } => Some(429),
            ApiError::Server { status, .. } | ApiError::Other { status, .. } => Some(*status),
            ApiError::Network { .. } | ApiError::Decode { .. } => None,
        }
    }
}

/// App-level error surfaced to the frontend as a plain string.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("vault error: {0}")]
    Vault(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl AppError {
    pub fn msg(m: impl Into<String>) -> Self {
        AppError::Msg(m.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
