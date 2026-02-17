//! Error types for the Orchestrator

use thiserror::Error;

/// Main error type for Orchestrator operations
#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("Session error: {0}")]
    Session(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session already exists: {0}")]
    SessionAlreadyExists(String),

    #[error("Session expired: {0}")]
    SessionExpired(String),

    #[error("Event bus error: {0}")]
    EventBus(String),

    #[error("Event publish failed: {0}")]
    EventPublishFailed(String),

    #[error("Sync error: {0}")]
    Sync(String),

    #[error("Sync conflict: {0}")]
    SyncConflict(String),

    #[error("Context enhancement error: {0}")]
    ContextEnhancement(String),

    #[error("Storage error: {0}")]
    Storage(#[from] nexus_core::NexusError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Not initialized")]
    NotInitialized,

    #[error("Already initialized")]
    AlreadyInitialized,

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Channel send error: {0}")]
    ChannelSend(String),

    #[error("Operation timeout")]
    Timeout,

    #[error("Operation cancelled")]
    Cancelled,
}

/// Result type alias for Orchestrator operations
pub type Result<T> = std::result::Result<T, OrchestratorError>;

impl From<tokio::sync::broadcast::error::SendError<crate::Event>> for OrchestratorError {
    fn from(err: tokio::sync::broadcast::error::SendError<crate::Event>) -> Self {
        OrchestratorError::ChannelSend(err.to_string())
    }
}

impl From<tokio::sync::broadcast::error::RecvError> for OrchestratorError {
    fn from(err: tokio::sync::broadcast::error::RecvError) -> Self {
        OrchestratorError::EventBus(err.to_string())
    }
}
