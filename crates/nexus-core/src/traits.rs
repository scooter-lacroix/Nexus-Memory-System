//! Core traits for Nexus Memory System

use crate::{
    AgentNamespace, Memory, NamespaceStats, SearchQuery, SearchResult, StoreMemoryRequest,
};
use async_trait::async_trait;

/// Trait for memory storage backends
#[async_trait]
pub trait MemoryStorage: Send + Sync {
    /// Store a new memory
    async fn store(&self, request: StoreMemoryRequest) -> crate::Result<Memory>;

    /// Search memories
    async fn search(&self, query: SearchQuery) -> crate::Result<SearchResult>;

    /// Get a memory by ID
    async fn get_by_id(&self, id: i64) -> crate::Result<Option<Memory>>;

    /// Delete a memory
    async fn delete(&self, id: i64) -> crate::Result<bool>;

    /// Update a memory
    async fn update(&self, memory: Memory) -> crate::Result<Memory>;
}

/// Trait for namespace management
#[async_trait]
pub trait NamespaceManager: Send + Sync {
    /// Get or create a namespace
    async fn get_or_create(&self, name: &str, agent_type: &str) -> crate::Result<AgentNamespace>;

    /// Get a namespace by name
    async fn get_by_name(&self, name: &str) -> crate::Result<Option<AgentNamespace>>;

    /// List all namespaces
    async fn list_all(&self) -> crate::Result<Vec<AgentNamespace>>;

    /// Get statistics for a namespace
    async fn stats(&self, name: &str) -> crate::Result<NamespaceStats>;
}

/// Trait for embedding services
#[async_trait]
pub trait EmbeddingService: Send + Sync {
    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> crate::Result<Vec<f32>>;

    /// Generate embeddings for multiple texts
    async fn embed_batch(&self, texts: &[String]) -> crate::Result<Vec<Vec<f32>>>;

    /// Get the dimension of embeddings
    fn dimension(&self) -> usize;

    /// Get the model name
    fn model_name(&self) -> &str;
}

/// Trait for vector search operations
#[async_trait]
pub trait VectorSearch: Send + Sync {
    /// Find similar vectors
    async fn find_similar(
        &self,
        embedding: &[f32],
        namespace_id: i64,
        limit: usize,
        threshold: f32,
    ) -> crate::Result<Vec<(i64, f32)>>;

    /// Store a vector
    async fn store_vector(&self, memory_id: i64, embedding: &[f32]) -> crate::Result<()>;

    /// Delete a vector
    async fn delete_vector(&self, memory_id: i64) -> crate::Result<()>;
}
