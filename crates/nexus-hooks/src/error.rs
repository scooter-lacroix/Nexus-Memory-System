//! Error types for the hooks system

use thiserror::Error;

/// Hook-specific errors
#[derive(Debug, Error)]
pub enum HookError {
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Hook installation failed: {0}")]
    InstallationFailed(String),

    #[error("Session detection failed: {0}")]
    DetectionFailed(String),

    #[error("Context extraction failed: {0}")]
    ExtractionFailed(String),

    #[error("Buffer error: {0}")]
    BufferError(String),

    #[error("Process monitoring error: {0}")]
    MonitorError(String),

    #[error("Signal handling error: {0}")]
    SignalError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Agent not supported: {0}")]
    NotSupported(String),

    #[error("Hook not initialized")]
    NotInitialized,

    #[error("Session not active")]
    SessionNotActive,

    #[error("Timeout exceeded")]
    Timeout,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias for hook operations
pub type Result<T> = std::result::Result<T, HookError>;
