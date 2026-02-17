//! Nexus Embeddings - Text embedding service using ONNX Runtime
//!
//! This crate provides embedding generation for the Nexus Memory System using
//! the all-MiniLM-L6-v2 model via ONNX Runtime. It produces 384-dimensional
//! vectors compatible with the Python sentence-transformers implementation.
//!
//! # Features
//!
//! - **ONNX Runtime backend** - Fast CPU inference using ort
//! - **Async batch processing** - Non-blocking batch encoding
//! - **Thread-safe** - Safe for concurrent access
//! - **Mock implementation** - For testing without model loading
//!
//! # Example
//!
//! ```rust,no_run
//! use nexus_embeddings::{OrtEmbeddingService, EmbeddingConfig};
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

pub mod config;
pub mod error;
pub mod ort_service;
pub mod mock_service;
pub mod cache;

pub use config::EmbeddingConfig;
pub use error::{EmbeddingError, Result};
pub use ort_service::OrtEmbeddingService;
pub use mock_service::MockEmbeddingService;
pub use cache::EmbeddingCache;

/// Embedding dimension for all-MiniLM-L6-v2 model
pub const EMBEDDING_DIMENSION: usize = 384;

/// Default model name
pub const DEFAULT_MODEL_NAME: &str = "all-MiniLM-L6-v2";

/// Maximum sequence length for the model
pub const MAX_SEQ_LENGTH: usize = 256;
