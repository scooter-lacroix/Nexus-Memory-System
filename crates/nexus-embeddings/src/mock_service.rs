//! Mock embedding service for testing without model loading

use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{DEFAULT_MODEL_NAME, EMBEDDING_DIMENSION};

/// Mock embedding service for testing purposes
///
/// This service generates deterministic embeddings based on text content,
/// useful for testing without loading the actual model.
pub struct MockEmbeddingService {
    /// Dimension of embeddings
    dimension: usize,
    /// Model name for identification
    model_name: String,
    /// Whether to generate random embeddings
    random_mode: bool,
    /// Seed for deterministic random generation
    seed: AtomicU64,
    /// Call counts for testing
    call_counts: RwLock<HashMap<String, u64>>,
}

impl MockEmbeddingService {
    /// Create a new mock embedding service
    pub fn new() -> Self {
        Self {
            dimension: EMBEDDING_DIMENSION,
            model_name: DEFAULT_MODEL_NAME.to_string(),
            random_mode: false,
            seed: AtomicU64::new(42),
            call_counts: RwLock::new(HashMap::new()),
        }
    }

    /// Create a mock service with custom dimension
    pub fn with_dimension(dimension: usize) -> Self {
        Self {
            dimension,
            ..Self::new()
        }
    }

    /// Create a mock service with random mode enabled
    pub fn with_random_mode(seed: u64) -> Self {
        Self {
            random_mode: true,
            seed: AtomicU64::new(seed),
            ..Self::new()
        }
    }

    /// Create a mock service with a custom model name
    pub fn with_model_name(model_name: impl Into<String>) -> Self {
        Self {
            model_name: model_name.into(),
            ..Self::new()
        }
    }

    /// Generate a deterministic embedding based on text hash
    fn generate_embedding(&self, text: &str) -> Vec<f32> {
        // Record call
        {
            let mut counts = self.call_counts.write();
            *counts.entry(text.to_string()).or_insert(0) += 1;
        }

        if self.random_mode {
            self.generate_random_embedding(text)
        } else {
            self.generate_deterministic_embedding(text)
        }
    }

    /// Generate deterministic embedding from text hash
    fn generate_deterministic_embedding(&self, text: &str) -> Vec<f32> {
        let mut embedding = Vec::with_capacity(self.dimension);

        // Use text bytes to seed the embedding generation
        let bytes = text.as_bytes();

        for i in 0..self.dimension {
            // Mix text content with position for each dimension
            let byte_idx = i % bytes.len();
            let base = bytes[byte_idx] as f32 / 255.0;
            let position_factor = (i as f32 + 1.0) / (self.dimension as f32);

            // Create a value between -1 and 1
            let value = (base * 2.0 - 1.0) * position_factor.sin();
            embedding.push(value);
        }

        // Normalize to unit length
        Self::normalize(&embedding)
    }

    /// Generate pseudo-random embedding
    fn generate_random_embedding(&self, text: &str) -> Vec<f32> {
        // Simple linear congruential generator seeded by text
        let text_hash = Self::hash_text(text);
        let seed = self.seed.load(Ordering::Relaxed) ^ text_hash;

        let mut state = seed;
        let mut embedding = Vec::with_capacity(self.dimension);

        for _ in 0..self.dimension {
            // LCG: state = state * 6364136223846793005 + 1442695040888963407
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);

            // Convert to float between -1 and 1
            let value = ((state >> 32) as i32 as f32) / (i32::MAX as f32);
            embedding.push(value);
        }

        Self::normalize(&embedding)
    }

    /// Simple hash function for text
    fn hash_text(text: &str) -> u64 {
        let mut hash: u64 = 5381;
        for byte in text.bytes() {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        hash
    }

    /// Normalize vector to unit length
    fn normalize(v: &[f32]) -> Vec<f32> {
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            v.iter().map(|x| x / norm).collect()
        } else {
            v.to_vec()
        }
    }

    /// Get call count for a specific text
    pub fn call_count(&self, text: &str) -> u64 {
        let counts = self.call_counts.read();
        counts.get(text).copied().unwrap_or(0)
    }

    /// Get total call count
    pub fn total_calls(&self) -> u64 {
        let counts = self.call_counts.read();
        counts.values().sum()
    }

    /// Reset call counts
    pub fn reset_counts(&self) {
        let mut counts = self.call_counts.write();
        counts.clear();
    }

    /// Compute cosine similarity between two embeddings
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a > 0.0 && norm_b > 0.0 {
            dot / (norm_a * norm_b)
        } else {
            0.0
        }
    }
}

impl Default for MockEmbeddingService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl nexus_core::traits::EmbeddingService for MockEmbeddingService {
    async fn embed(&self, text: &str) -> nexus_core::Result<Vec<f32>> {
        if text.trim().is_empty() {
            return Err(nexus_core::NexusError::InvalidInput(
                "Cannot embed empty text".to_string(),
            ));
        }
        Ok(self.generate_embedding(text))
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

        Ok(texts
            .iter()
            .map(|text| self.generate_embedding(text))
            .collect())
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::traits::EmbeddingService;

    #[tokio::test]
    async fn test_mock_embed_single() {
        let service = MockEmbeddingService::new();
        let embedding = service.embed("hello world").await.unwrap();

        assert_eq!(embedding.len(), EMBEDDING_DIMENSION);
    }

    #[tokio::test]
    async fn test_mock_embed_batch() {
        let service = MockEmbeddingService::new();
        let texts: Vec<String> = vec!["hello".into(), "world".into(), "test".into()];
        let embeddings = service.embed_batch(&texts).await.unwrap();

        assert_eq!(embeddings.len(), 3);
        assert!(embeddings.iter().all(|e| e.len() == EMBEDDING_DIMENSION));
    }

    #[tokio::test]
    async fn test_mock_empty_text_error() {
        let service = MockEmbeddingService::new();
        let result = service.embed("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_deterministic() {
        let service = MockEmbeddingService::new();

        let e1 = service.embed("test text").await.unwrap();
        let e2 = service.embed("test text").await.unwrap();

        // Same text should produce same embedding
        for i in 0..e1.len() {
            assert!((e1[i] - e2[i]).abs() < 1e-6);
        }
    }

    #[tokio::test]
    async fn test_mock_different_texts_different_embeddings() {
        let service = MockEmbeddingService::new();

        let e1 = service.embed("text one").await.unwrap();
        let e2 = service.embed("text two").await.unwrap();

        // Different texts should produce different embeddings
        let similarity = MockEmbeddingService::cosine_similarity(&e1, &e2);
        assert!(similarity < 1.0);
    }

    #[tokio::test]
    async fn test_mock_normalized() {
        let service = MockEmbeddingService::new();
        let embedding = service.embed("test").await.unwrap();

        // Check unit length
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_mock_custom_dimension() {
        let service = MockEmbeddingService::with_dimension(128);
        let embedding = service.embed("test").await.unwrap();

        assert_eq!(embedding.len(), 128);
        assert_eq!(service.dimension(), 128);
    }

    #[tokio::test]
    async fn test_mock_random_mode() {
        let service = MockEmbeddingService::with_random_mode(42);

        let e1 = service.embed("test").await.unwrap();
        let e2 = service.embed("test").await.unwrap();

        // With same seed and text, should be deterministic
        for i in 0..e1.len() {
            assert!((e1[i] - e2[i]).abs() < 1e-6);
        }
    }

    #[tokio::test]
    async fn test_mock_call_counts() {
        let service = MockEmbeddingService::new();

        service.embed("text1").await.unwrap();
        service.embed("text1").await.unwrap();
        service.embed("text2").await.unwrap();

        assert_eq!(service.call_count("text1"), 2);
        assert_eq!(service.call_count("text2"), 1);
        assert_eq!(service.total_calls(), 3);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let _b = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];
        let d = vec![0.5, 0.5, 0.0];

        // Same vector
        assert!((MockEmbeddingService::cosine_similarity(&a, &a) - 1.0).abs() < 1e-6);

        // Orthogonal
        assert!((MockEmbeddingService::cosine_similarity(&a, &c) - 0.0).abs() < 1e-6);

        // 45 degrees
        let expected = 0.70710678; // cos(45 degrees)
        assert!((MockEmbeddingService::cosine_similarity(&a, &d) - expected).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_model_name() {
        let service = MockEmbeddingService::with_model_name("custom-model");
        assert_eq!(service.model_name(), "custom-model");
    }
}
