//! Configuration for embedding services

use std::path::PathBuf;

/// Configuration for the embedding service
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// Path to the ONNX model file
    pub model_path: PathBuf,

    /// Path to the tokenizer files directory
    pub tokenizer_path: PathBuf,

    /// Maximum sequence length (default: 256 for all-MiniLM-L6-v2)
    pub max_seq_length: usize,

    /// Embedding dimension (default: 384 for all-MiniLM-L6-v2)
    pub dimension: usize,

    /// Whether to normalize embeddings to unit length
    pub normalize: bool,

    /// Number of threads for ONNX Runtime inference
    pub intra_op_num_threads: i32,

    /// Enable embedding cache
    pub enable_cache: bool,

    /// Maximum cache size (number of entries)
    pub cache_size: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("models/all-MiniLM-L6-v2.onnx"),
            tokenizer_path: PathBuf::from("models/all-MiniLM-L6-v2-tokenizer"),
            max_seq_length: 256,
            dimension: 384,
            normalize: true,
            intra_op_num_threads: 4,
            enable_cache: true,
            cache_size: 1000,
        }
    }
}

impl EmbeddingConfig {
    /// Create a new configuration with the specified model path
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        let path = model_path.into();
        let tokenizer_path = path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();

        Self {
            model_path: path,
            tokenizer_path,
            ..Default::default()
        }
    }

    /// Create configuration from environment variables
    pub fn from_env() -> Self {
        let model_path = std::env::var("NEXUS_EMBEDDING_MODEL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("models/all-MiniLM-L6-v2.onnx"));

        let tokenizer_path = std::env::var("NEXUS_TOKENIZER_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                model_path.parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .to_path_buf()
            });

        let max_seq_length = std::env::var("NEXUS_MAX_SEQ_LENGTH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(256);

        let intra_op_num_threads = std::env::var("NEXUS_EMBEDDING_THREADS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4);

        let enable_cache = std::env::var("NEXUS_EMBEDDING_CACHE")
            .ok()
            .map(|s| s.to_lowercase() != "false")
            .unwrap_or(true);

        let cache_size = std::env::var("NEXUS_EMBEDDING_CACHE_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);

        Self {
            model_path,
            tokenizer_path,
            max_seq_length,
            dimension: 384,
            normalize: true,
            intra_op_num_threads,
            enable_cache,
            cache_size,
        }
    }

    /// Set the model path
    pub fn with_model_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.model_path = path.into();
        self
    }

    /// Set the tokenizer path
    pub fn with_tokenizer_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.tokenizer_path = path.into();
        self
    }

    /// Set whether to normalize embeddings
    pub fn with_normalize(mut self, normalize: bool) -> Self {
        self.normalize = normalize;
        self
    }

    /// Set the number of inference threads
    pub fn with_threads(mut self, threads: i32) -> Self {
        self.intra_op_num_threads = threads;
        self
    }

    /// Enable or disable caching
    pub fn with_cache(mut self, enable: bool) -> Self {
        self.enable_cache = enable;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.dimension, 384);
        assert_eq!(config.max_seq_length, 256);
        assert!(config.normalize);
        assert!(config.enable_cache);
    }

    #[test]
    fn test_config_builder() {
        let config = EmbeddingConfig::default()
            .with_model_path("/custom/model.onnx")
            .with_normalize(false)
            .with_threads(8)
            .with_cache(false);

        assert_eq!(config.model_path, PathBuf::from("/custom/model.onnx"));
        assert!(!config.normalize);
        assert_eq!(config.intra_op_num_threads, 8);
        assert!(!config.enable_cache);
    }

    #[test]
    fn test_config_new() {
        let config = EmbeddingConfig::new("/path/to/model.onnx");
        assert_eq!(config.model_path, PathBuf::from("/path/to/model.onnx"));
        assert_eq!(config.tokenizer_path, PathBuf::from("/path/to"));
    }
}
