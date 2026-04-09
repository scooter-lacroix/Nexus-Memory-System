//! ONNX Runtime embedding service implementation

use async_trait::async_trait;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Value;
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;
use tracing::{debug, info};

use crate::cache::EmbeddingCache;
use crate::config::EmbeddingConfig;
use crate::error::{EmbeddingError, Result};
use crate::{DEFAULT_MODEL_NAME, EMBEDDING_DIMENSION};

/// ONNX Runtime-based embedding service
///
/// This service uses the ONNX Runtime to generate embeddings using the
/// all-MiniLM-L6-v2 model. It produces 384-dimensional vectors compatible
/// with the sentence-transformers reference implementation.
pub struct OrtEmbeddingService {
    /// ONNX Runtime session guarded for the mutable `run` API.
    session: Arc<Mutex<Session>>,
    /// Tokenizer for text preprocessing
    tokenizer: Tokenizer,
    /// Configuration
    config: EmbeddingConfig,
    /// Optional embedding cache
    cache: Option<Arc<EmbeddingCache>>,
    /// Model name for identification
    model_name: String,
}

impl OrtEmbeddingService {
    /// Create a new embedding service with the given configuration
    pub async fn new(config: EmbeddingConfig) -> Result<Self> {
        info!("Initializing ONNX embedding service");

        // Verify model file exists
        if !config.model_path.exists() {
            return Err(EmbeddingError::ModelNotFound(
                config.model_path.display().to_string(),
            ));
        }

        // Load ONNX session
        let session = Session::builder()
            .map_err(|e| EmbeddingError::ModelLoadFailed(e.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| EmbeddingError::ModelLoadFailed(e.to_string()))?
            .with_intra_threads(config.intra_op_num_threads as usize)
            .map_err(|e| EmbeddingError::ModelLoadFailed(e.to_string()))?
            .commit_from_file(&config.model_path)
            .map_err(|e| EmbeddingError::ModelLoadFailed(e.to_string()))?;

        debug!("ONNX session created successfully");

        // Load tokenizer
        let tokenizer_path = config.tokenizer_path.join("tokenizer.json");
        let tokenizer = if tokenizer_path.exists() {
            Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| EmbeddingError::TokenizationError(e.to_string()))?
        } else {
            // Try to load from the model directory directly
            let alt_path = config
                .model_path
                .parent()
                .ok_or_else(|| {
                    EmbeddingError::TokenizationError(
                        "Cannot determine tokenizer directory".to_string(),
                    )
                })?
                .join("tokenizer.json");

            if alt_path.exists() {
                Tokenizer::from_file(&alt_path)
                    .map_err(|e| EmbeddingError::TokenizationError(e.to_string()))?
            } else {
                return Err(EmbeddingError::TokenizationError(format!(
                    "Tokenizer not found at {:?} or {:?}",
                    tokenizer_path, alt_path
                )));
            }
        };

        debug!("Tokenizer loaded successfully");

        // Create cache if enabled
        let cache = if config.enable_cache {
            Some(Arc::new(EmbeddingCache::new(config.cache_size)))
        } else {
            None
        };

        info!("Embedding service initialized successfully");

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            tokenizer,
            config,
            cache,
            model_name: DEFAULT_MODEL_NAME.to_string(),
        })
    }

    /// Create a new service with a custom model name
    pub async fn with_model_name(config: EmbeddingConfig, model_name: String) -> Result<Self> {
        let mut service = Self::new(config).await?;
        service.model_name = model_name;
        Ok(service)
    }

    /// Tokenize text for the model
    fn tokenize(&self, text: &str) -> Result<(Vec<i64>, Vec<i64>)> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| EmbeddingError::TokenizationError(e.to_string()))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();

        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&mask| mask as i64)
            .collect();

        Ok((input_ids, attention_mask))
    }

    /// Run inference on the ONNX model
    async fn run_inference(
        session: &Arc<Mutex<Session>>,
        normalize: bool,
        input_ids: Vec<i64>,
        attention_mask: Vec<i64>,
    ) -> Result<Vec<f32>> {
        let session = Arc::clone(session);
        tokio::task::spawn_blocking(move || {
            let seq_length = input_ids.len();

            let input_ids_value = Value::from_array((vec![1usize, seq_length], input_ids))
                .map_err(|e| EmbeddingError::InferenceError(e.to_string()))?;

            let attention_mask_value =
                Value::from_array((vec![1usize, seq_length], attention_mask))
                    .map_err(|e| EmbeddingError::InferenceError(e.to_string()))?;

            let mut session = session.lock().map_err(|_| {
                EmbeddingError::InferenceError("Failed to lock session".to_string())
            })?;

            let outputs = session
                .run(ort::inputs![input_ids_value, attention_mask_value])
                .map_err(|e| EmbeddingError::InferenceError(e.to_string()))?;

            let (_name, output_value) = outputs.iter().next().ok_or_else(|| {
                EmbeddingError::InferenceError("No output found in model".to_string())
            })?;

            let (shape, data) = output_value
                .try_extract_tensor::<f32>()
                .map_err(|e| EmbeddingError::InferenceError(e.to_string()))?;

            let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();

            let embedding = if dims.len() == 3 {
                let seq_len = dims[1];
                let hidden_size = dims[2];

                let mut pooled = vec![0.0f32; hidden_size];
                for i in 0..hidden_size {
                    let mut sum = 0.0;
                    for j in 0..seq_len {
                        sum += data[j * hidden_size + i];
                    }
                    pooled[i] = sum / seq_len as f32;
                }
                pooled
            } else if dims.len() == 2 {
                data.to_vec()
            } else {
                return Err(EmbeddingError::InferenceError(format!(
                    "Unexpected output shape: {:?}",
                    dims
                )));
            };

            Ok(if normalize {
                Self::normalize_embedding(&embedding)
            } else {
                embedding
            })
        })
        .await
        .map_err(|e| EmbeddingError::InferenceError(e.to_string()))?
    }

    /// Normalize embedding to unit length
    fn normalize_embedding(embedding: &[f32]) -> Vec<f32> {
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            embedding.iter().map(|x| x / norm).collect()
        } else {
            embedding.to_vec()
        }
    }

    /// Encode a single text (async)
    async fn encode(&self, text: &str) -> Result<Vec<f32>> {
        // Check cache first
        if let Some(ref cache) = self.cache {
            if let Some(cached) = cache.get(text) {
                debug!("Cache hit for text embedding");
                return Ok(cached);
            }
        }

        // Tokenize
        let (input_ids, attention_mask) = self.tokenize(text)?;

        // Run inference
        let embedding = Self::run_inference(
            &self.session,
            self.config.normalize,
            input_ids,
            attention_mask,
        )
        .await?;

        // Store in cache
        if let Some(ref cache) = self.cache {
            cache.put(text, embedding.clone());
        }

        Ok(embedding)
    }

    /// Encode multiple texts (async)
    async fn encode_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.encode(text).await?);
        }
        Ok(results)
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> Option<crate::cache::CacheStats> {
        self.cache.as_ref().map(|c| c.stats())
    }

    /// Clear the embedding cache
    pub fn clear_cache(&self) {
        if let Some(ref cache) = self.cache {
            cache.clear();
        }
    }

    /// Check if the service is initialized
    pub fn is_initialized(&self) -> bool {
        true
    }
}

#[async_trait]
impl nexus_core::traits::EmbeddingService for OrtEmbeddingService {
    async fn embed(&self, text: &str) -> nexus_core::Result<Vec<f32>> {
        if text.trim().is_empty() {
            return Err(nexus_core::NexusError::InvalidInput(
                "Cannot embed empty text".to_string(),
            ));
        }

        self.encode(text)
            .await
            .map_err(|e| nexus_core::NexusError::Embedding(e.to_string()))
    }

    async fn embed_batch(&self, texts: &[String]) -> nexus_core::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        for text in texts {
            if text.trim().is_empty() {
                return Err(nexus_core::NexusError::InvalidInput(
                    "Cannot embed empty text".to_string(),
                ));
            }
        }

        self.encode_batch(texts)
            .await
            .map_err(|e| nexus_core::NexusError::Embedding(e.to_string()))
    }

    fn dimension(&self) -> usize {
        EMBEDDING_DIMENSION
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require the model files to be present
    // They are marked to only run when the model exists

    #[test]
    fn test_normalize_embedding() {
        let embedding = vec![3.0, 4.0];
        let normalized = OrtEmbeddingService::normalize_embedding(&embedding);

        // Check unit length
        let norm: f32 = normalized.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_normalize_zero_vector() {
        let embedding = vec![0.0, 0.0, 0.0];
        let normalized = OrtEmbeddingService::normalize_embedding(&embedding);

        // Should return the original zero vector
        assert_eq!(normalized, vec![0.0, 0.0, 0.0]);
    }
}
