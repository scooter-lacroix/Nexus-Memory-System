//! Error types for the embedding service

use thiserror::Error;

/// Error type for embedding operations
#[derive(Debug, Error)]
pub enum EmbeddingError {
    /// Model file not found or inaccessible
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    /// Failed to load ONNX model
    #[error("Failed to load ONNX model: {0}")]
    ModelLoadFailed(String),

    /// Tokenization failed
    #[error("Tokenization error: {0}")]
    TokenizationError(String),

    /// ONNX inference failed
    #[error("ONNX inference error: {0}")]
    InferenceError(String),

    /// Invalid input text
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Service not initialized
    #[error("Embedding service not initialized")]
    NotInitialized,

    /// Cache error
    #[error("Cache error: {0}")]
    CacheError(String),

    /// IO error wrapper
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type alias for embedding operations
pub type Result<T> = std::result::Result<T, EmbeddingError>;

impl From<EmbeddingError> for nexus_core::NexusError {
    fn from(err: EmbeddingError) -> Self {
        nexus_core::NexusError::Embedding(err.to_string())
    }
}
