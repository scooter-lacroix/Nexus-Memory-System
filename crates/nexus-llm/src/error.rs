//! LLM-specific error types

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Missing API key: env var '{0}' not set")]
    MissingApiKey(String),

    #[error("Unsupported provider: {0}")]
    UnsupportedProvider(String),

    #[error("Response did not contain valid content")]
    EmptyResponse,

    #[error("Invalid JSON response from LLM: {0}")]
    InvalidJsonResponse(String),

    #[error("Timeout after {0}s")]
    Timeout(u64),

    #[error("Invalid configuration: {0}")]
    Configuration(String),
}

pub type Result<T> = std::result::Result<T, LlmError>;
