//! Application error types — no string-based errors allowed.

use thiserror::Error;

use super::api::ErrorMessage;

/// Raw response body from a failed API call.
#[derive(Debug, Clone)]
#[must_use]
pub struct ResponseBody(pub(super) String);

impl From<String> for ResponseBody {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// All errors in the application — no string-based errors allowed.
#[derive(Debug, Error)]
#[must_use]
#[non_exhaustive]
pub enum AppError {
    /// No API key found in env, OAuth token, or credentials file.
    #[error("no API key found — set ANTHROPIC_API_KEY or login with `claude`")]
    NoApiKey,

    /// HTTP request to API failed before getting a response.
    #[error("API request failed: {source}")]
    ApiRequest {
        #[source]
        source: reqwest::Error,
    },

    /// API returned a non-2xx status code.
    #[error("API error {status}: {body}")]
    ApiStatus {
        status: reqwest::StatusCode,
        body: ResponseBody,
    },

    /// SSE stream read failed mid-response.
    #[error("stream read error: {source}")]
    StreamRead {
        #[source]
        source: reqwest::Error,
    },

    /// API returned an error event inside the SSE stream.
    #[error("API stream error: {message}")]
    ApiStreamError { message: ErrorMessage },

    /// Terminal / crossterm / ratatui I/O error.
    #[error("terminal error: {0}")]
    Terminal(#[from] std::io::Error),

    /// Serialization/deserialization failure.
    #[error("JSON error: {source}")]
    Json {
        #[source]
        source: serde_json::Error,
    },

    /// Path traversal attempt — tool tried to escape the working directory.
    #[error("path traversal blocked: {attempted} is outside working directory")]
    PathTraversal { attempted: String },

    /// File exceeds the maximum allowed size for the operation.
    #[error("file too large: {message}")]
    FileTooLarge { message: String },

    /// Invalid URL provided to `WebFetch`.
    #[error("{message}")]
    InvalidUrl { message: String },

    /// HTTP fetch failed (network, timeout, redirect).
    #[error("{message}")]
    HttpError { message: String },

    /// Filesystem validation failed (directory not found, device path, etc.).
    #[error("{message}")]
    FsValidation { message: String },
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, AppError>;
