//! Nexus Embeddings - Text embedding service using ONNX Runtime
//!
//! This crate provides embedding generation for the Nexus Memory System using
//! the all-MiniLM-L6-v2 model via ONNX Runtime. It produces 384-dimensional
//! vectors compatible with the sentence-transformers reference implementation.
//!
//! # Features
//!
//! - **ONNX Runtime backend** - Fast CPU inference using ort
//! - **OpenAI-compatible HTTP backend** - Works with remote providers and local runtimes
//! - **Async batch processing** - Non-blocking batch encoding
//! - **Thread-safe** - Safe for concurrent access
//! - **Mock implementation** - For testing without model loading
//!
//! # Example
//!
//! ```rust,no_run
//! use nexus_memory_embeddings::{OrtEmbeddingService, EmbeddingConfig};
//! use nexus_core::traits::EmbeddingService;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = EmbeddingConfig::default();
//!     let service = OrtEmbeddingService::new(config).await?;
//!
//!     // Single text encoding
//!     let embedding = service.embed("Hello, world!").await?;
//!     assert_eq!(embedding.len(), 384);
//!
//!     // Batch encoding
//!     let texts: Vec<String> = vec!["Hello".into(), "World".into()];
//!     let embeddings = service.embed_batch(&texts).await?;
//!     assert_eq!(embeddings.len(), 2);
//!
//!     Ok(())
//! }
//! ```

pub mod cache;
pub mod config;
pub mod error;
pub mod http_service;
pub mod mock_service;
pub mod ort_service;

pub use cache::EmbeddingCache;
pub use config::EmbeddingConfig;
pub use error::{EmbeddingError, Result};
pub use http_service::HttpEmbeddingService;
pub use mock_service::MockEmbeddingService;
pub use ort_service::OrtEmbeddingService;
use std::sync::Arc;

/// Create an embedding service from the public Nexus configuration.
pub async fn create_service(
    config: &nexus_core::Config,
) -> Result<Option<Arc<dyn nexus_core::traits::EmbeddingService>>> {
    if !config.embedding.enabled {
        return Ok(None);
    }

    let runtime = EmbeddingConfig::from_nexus_config(&config.embedding, &config.llm);
    let backend = runtime.backend.to_lowercase();
    if matches!(backend.as_str(), "local" | "onnx" | "local-onnx") {
        return Ok(Some(Arc::new(OrtEmbeddingService::new(runtime).await?)));
    }
    if matches!(
        backend.as_str(),
        "openai-compatible" | "openai_compatible" | "remote" | "http"
    ) {
        return Ok(Some(Arc::new(HttpEmbeddingService::new(runtime)?)));
    }

    Err(EmbeddingError::ConfigurationError(format!(
        "Unsupported embedding backend: {}",
        config.embedding.backend
    )))
}

/// Embedding dimension for all-MiniLM-L6-v2 model
pub const EMBEDDING_DIMENSION: usize = 384;

/// Default model name
pub const DEFAULT_MODEL_NAME: &str = "all-MiniLM-L6-v2";

/// Maximum sequence length for the model
pub const MAX_SEQ_LENGTH: usize = 256;
