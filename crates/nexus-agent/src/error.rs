//! Agent-specific error types

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Ingest error: {0}")]
    Ingest(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Consolidation error: {0}")]
    Consolidation(String),

    #[error("Supervisor error: {0}")]
    Supervisor(String),

    #[error("Derivation error: {0}")]
    Derivation(String),

    #[error("Digest error: {0}")]
    Digest(String),

    #[error("Reflection error: {0}")]
    Reflection(String),
}

pub type Result<T> = std::result::Result<T, AgentError>;
