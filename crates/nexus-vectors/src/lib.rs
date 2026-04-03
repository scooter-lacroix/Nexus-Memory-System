//! Nexus Vectors - Semantic search over storage-backed embeddings
//!
//! This crate provides semantic search capabilities for vector embeddings,
//! used by the cognition engine (`nexus-agent`) to retrieve relevant memories.
//!
//! ## Runtime Retrieval Path
//! The live cognition path uses **`SemanticSearch`** over `VectorEntry` slices
//! fetched from `nexus-storage`. `SemanticSearch` performs cosine similarity
//! ranking with optional graph-tree-based score boosting.
//!
//! ## Internal/Test Abstractions
//! `VectorDatabase` is an in-memory vector store with its own HashMap-based
//! storage. It is **not** used by the shipped retrieval path and exists for
//! testing and development. It is deprecated and should not be used for
//! runtime retrieval.
//!
//! ## Features
//! - **384-dimensional embeddings**: Compatible with all-MiniLM-L6-v2
//! - **Cosine similarity search**: Fast semantic search
//! - **Graph tree organization**: Hierarchical memory management with relevance boosting
//! - **Priority-based boosting**: High-priority memories get boosted scores
//!
//! ## Performance Targets
//! - Search latency: <10ms for 1k vectors
//! - Memory efficiency: In-memory storage with indexing
//!
//! ## Usage (Runtime Path)
//! ```ignore
//! use nexus_memory_vectors::{SemanticSearch, SearchOptions, VectorEntry};
//!
//! // Vectors come from storage, not an in-memory DB
//! let vectors: Vec<VectorEntry> = /* fetched from nexus-storage */;
//! let search = SemanticSearch::new();
//! let options = SearchOptions::with_limit(10).with_threshold(0.5);
//! let (results, latency) = search.search(&query_embedding, &vectors, &options).unwrap();
//! ```

pub mod database;
pub mod graph;
pub mod search;

#[allow(deprecated)]
pub use database::{
    batch_cosine_similarity, cosine_similarity, dot_product, euclidean_distance, normalize_vector,
    top_k_similar, VectorDatabase, VectorDatabaseStats, VectorSearchResult, DEFAULT_SEARCH_LIMIT,
    DEFAULT_SIMILARITY_THRESHOLD,
};
pub use graph::{GraphNode, GraphNode as Node, GraphTree, NodeType, TreeNode, TreeStats};
pub use search::{SearchOptions, SearchResult, SemanticSearch};

use serde::{Deserialize, Serialize};

/// Result type for vector operations
pub type Result<T> = std::result::Result<T, nexus_core::NexusError>;

/// Embedding dimension (all-MiniLM-L6-v2 uses 384 dimensions)
pub const EMBEDDING_DIMENSION: usize = 384;

/// Vector embedding type
pub type Embedding = Vec<f32>;

/// Vector with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    /// Unique identifier (memory ID)
    pub id: i64,

    /// The embedding vector
    pub embedding: Embedding,

    /// Category for filtering
    pub category: String,

    /// Optional memory lane type
    pub memory_lane_type: Option<String>,

    /// Namespace ID for isolation
    pub namespace_id: i64,

    /// Timestamp for freshness
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl VectorEntry {
    /// Create a new vector entry
    pub fn new(id: i64, embedding: Embedding, category: String, namespace_id: i64) -> Self {
        Self {
            id,
            embedding,
            category,
            memory_lane_type: None,
            namespace_id,
            created_at: chrono::Utc::now(),
        }
    }

    /// Create with all fields
    pub fn with_memory_lane_type(mut self, memory_lane_type: impl Into<String>) -> Self {
        self.memory_lane_type = Some(memory_lane_type.into());
        self
    }
}

/// Search latency information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchLatency {
    /// Total search time in milliseconds
    pub total_ms: u64,

    /// Vector comparison time in milliseconds
    pub vector_comparison_ms: u64,

    /// Graph traversal time in milliseconds (if applicable)
    pub graph_traversal_ms: Option<u64>,
}

impl SearchLatency {
    /// Check if search meets the <10ms target
    pub fn meets_target(&self) -> bool {
        self.total_ms < 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_entry_new() {
        let embedding = vec![0.1; EMBEDDING_DIMENSION];
        let entry = VectorEntry::new(1, embedding.clone(), "general".to_string(), 1);

        assert_eq!(entry.id, 1);
        assert_eq!(entry.embedding, embedding);
        assert_eq!(entry.category, "general");
        assert_eq!(entry.namespace_id, 1);
        assert!(entry.memory_lane_type.is_none());
    }

    #[test]
    fn test_vector_entry_with_memory_lane_type() {
        let embedding = vec![0.1; EMBEDDING_DIMENSION];
        let entry = VectorEntry::new(1, embedding, "general".to_string(), 1)
            .with_memory_lane_type("correction");

        assert_eq!(entry.memory_lane_type, Some("correction".to_string()));
    }

    #[test]
    fn test_search_latency_meets_target() {
        let good = SearchLatency {
            total_ms: 5,
            vector_comparison_ms: 3,
            graph_traversal_ms: Some(1),
        };
        assert!(good.meets_target());

        let bad = SearchLatency {
            total_ms: 15,
            vector_comparison_ms: 10,
            graph_traversal_ms: Some(4),
        };
        assert!(!bad.meets_target());
    }
}
