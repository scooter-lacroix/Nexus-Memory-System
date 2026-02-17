//! Error types for Nexus Memory System

use thiserror::Error;

/// Main error type for Nexus operations
#[derive(Debug, Error)]
pub enum NexusError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Namespace not found: {0}")]
    NamespaceNotFound(String),

    #[error("Memory not found: {0}")]
    MemoryNotFound(i64),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Vector search error: {0}")]
    VectorSearch(String),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Not initialized")]
    NotInitialized,

    #[error("Already initialized")]
    AlreadyInitialized,

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias for Nexus operations
pub type Result<T> = std::result::Result<T, NexusError>;
