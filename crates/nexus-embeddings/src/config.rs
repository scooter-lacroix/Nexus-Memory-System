//! Configuration for embedding services

use std::path::PathBuf;

/// Runtime configuration for the embedding service.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// Backend kind (`local` or `openai-compatible`)
    pub backend: String,
    /// Provider/profile label (`openai`, `gemini`, `lmstudio`, etc.)
    pub provider: String,
    /// Model identifier passed to the backend
    pub model: String,
    /// Optional API key environment variable used for remote providers
    pub api_key_env: Option<String>,
    /// Optional base URL override for remote or local OpenAI-compatible runtimes
    pub base_url: Option<String>,
    /// Path to the ONNX model file
    pub model_path: PathBuf,
    /// Path to the tokenizer files directory
    pub tokenizer_path: PathBuf,
    /// Maximum sequence length
    pub max_seq_length: usize,
    /// Embedding dimension
    pub dimension: usize,
    /// Whether to normalize embeddings to unit length
    pub normalize: bool,
    /// Number of threads for ONNX Runtime inference
    pub intra_op_num_threads: i32,
    /// Enable embedding cache
    pub enable_cache: bool,
    /// Maximum cache size (number of entries)
    pub cache_size: usize,
    /// Request timeout for remote providers
    pub timeout_secs: u64,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            backend: "local".to_string(),
            provider: "local".to_string(),
            model: "all-MiniLM-L6-v2".to_string(),
            api_key_env: None,
            base_url: None,
            model_path: PathBuf::from("models/all-MiniLM-L6-v2.onnx"),
            tokenizer_path: PathBuf::from("models/all-MiniLM-L6-v2-tokenizer"),
            max_seq_length: 256,
            dimension: 384,
            normalize: true,
            intra_op_num_threads: 4,
            enable_cache: true,
            cache_size: 1000,
            timeout_secs: 60,
        }
    }
}

impl EmbeddingConfig {
    /// Create a new local configuration with the specified model path.
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        let path = model_path.into();
        let tokenizer_path = path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();

        Self {
            model_path: path,
            tokenizer_path,
            ..Default::default()
        }
    }

    /// Create configuration from the public Nexus configuration model.
    pub fn from_nexus_config(
        embedding: &nexus_core::config::EmbeddingConfig,
        llm: &nexus_core::config::LlmConfig,
    ) -> Self {
        let provider = if embedding.provider.eq_ignore_ascii_case("inherit") {
            llm.provider.clone()
        } else {
            embedding.provider.clone()
        };
        let model = if embedding.model.eq_ignore_ascii_case("inherit") {
            llm.model.clone()
        } else {
            embedding.model.clone()
        };

        let api_key_env = if embedding.provider.eq_ignore_ascii_case("inherit") {
            Some(llm.api_key_env.clone())
        } else if embedding.api_key_env.trim().is_empty() {
            default_api_key_env(&provider).map(str::to_string)
        } else {
            Some(embedding.api_key_env.clone())
        };

        let base_url = if embedding.provider.eq_ignore_ascii_case("inherit") {
            embedding
                .base_url
                .clone()
                .or_else(|| llm.base_url.clone())
                .or_else(|| default_base_url(&provider).map(str::to_string))
        } else {
            embedding
                .base_url
                .clone()
                .or_else(|| default_base_url(&provider).map(str::to_string))
        };

        Self {
            backend: embedding.backend.clone(),
            provider,
            model,
            api_key_env,
            base_url,
            model_path: embedding
                .local_model_path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("models/all-MiniLM-L6-v2.onnx")),
            tokenizer_path: embedding
                .local_tokenizer_path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("models/all-MiniLM-L6-v2-tokenizer")),
            max_seq_length: std::env::var("NEXUS_MAX_SEQ_LENGTH")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(256),
            dimension: embedding.dimension,
            normalize: true,
            intra_op_num_threads: std::env::var("NEXUS_EMBEDDING_THREADS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4),
            enable_cache: std::env::var("NEXUS_EMBEDDING_CACHE")
                .ok()
                .map(|s| s.to_lowercase() != "false")
                .unwrap_or(true),
            cache_size: std::env::var("NEXUS_EMBEDDING_CACHE_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1000),
            timeout_secs: embedding.timeout_secs,
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

fn default_base_url(provider: &str) -> Option<&'static str> {
    match provider.to_lowercase().as_str() {
        "openai" => Some("https://api.openai.com/v1"),
        "gemini" | "google" => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "vllm" => Some("http://127.0.0.1:8000/v1"),
        "lmstudio" => Some("http://127.0.0.1:1234/v1"),
        "llamacpp" => Some("http://127.0.0.1:8080/v1"),
        _ => None,
    }
}

fn default_api_key_env(provider: &str) -> Option<&'static str> {
    match provider.to_lowercase().as_str() {
        "openai" => Some("OPENAI_API_KEY"),
        "gemini" | "google" => Some("GEMINI_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "mistral" => Some("MISTRAL_API_KEY"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.backend, "local");
        assert_eq!(config.provider, "local");
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

    #[test]
    fn test_config_from_nexus_config_inherits_llm() {
        let embedding = nexus_core::config::EmbeddingConfig {
            enabled: true,
            backend: "openai-compatible".to_string(),
            provider: "inherit".to_string(),
            model: "inherit".to_string(),
            api_key_env: "IGNORED".to_string(),
            base_url: None,
            dimension: 768,
            timeout_secs: 45,
            local_model_path: None,
            local_tokenizer_path: None,
        };
        let llm = nexus_core::config::LlmConfig {
            provider: "gemini".to_string(),
            model: "gemini-3.1-flash-lite-preview".to_string(),
            api_key_env: "GEMINI_API_KEY".to_string(),
            base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai".to_string()),
            timeout_secs: 60,
            max_tokens: 1000,
            temperature: 0.3,
        };

        let config = EmbeddingConfig::from_nexus_config(&embedding, &llm);
        assert_eq!(config.provider, "gemini");
        assert_eq!(config.model, "gemini-3.1-flash-lite-preview");
        assert_eq!(config.api_key_env.as_deref(), Some("GEMINI_API_KEY"));
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://generativelanguage.googleapis.com/v1beta/openai")
        );
        assert_eq!(config.dimension, 768);
        assert_eq!(config.timeout_secs, 45);
    }

    #[test]
    fn test_config_from_nexus_config_uses_provider_defaults_for_remote_embeddings() {
        let embedding = nexus_core::config::EmbeddingConfig {
            enabled: true,
            backend: "openai-compatible".to_string(),
            provider: "openrouter".to_string(),
            model: "openai/text-embedding-3-small".to_string(),
            api_key_env: String::new(),
            base_url: None,
            dimension: 1536,
            timeout_secs: 30,
            local_model_path: None,
            local_tokenizer_path: None,
        };

        let config = EmbeddingConfig::from_nexus_config(&embedding, &Default::default());
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://openrouter.ai/api/v1")
        );
        assert_eq!(config.api_key_env.as_deref(), Some("OPENROUTER_API_KEY"));
    }
}
