//! Vector database implementation
//!
//! This module provides vector storage and retrieval with graph tree organization
//! for efficient hierarchical memory management.
//!
//! ## Performance Targets
//! - Search latency: <10ms for 1k vectors
//! - Embedding dimension: 384 (all-MiniLM-L6-v2)
//! - Cosine similarity for semantic search

use crate::graph::GraphTree;
use crate::{SearchLatency, VectorEntry, EMBEDDING_DIMENSION};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Default search limit
pub const DEFAULT_SEARCH_LIMIT: usize = 10;

/// Default similarity threshold
pub const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.0;

/// Vector database for storing and searching embeddings
#[derive(Debug, Default)]
pub struct VectorDatabase {
    /// In-memory vector storage
    vectors: HashMap<i64, VectorEntry>,

    /// Graph tree for hierarchical organization
    tree: GraphTree,

    /// Embedding dimension
    dimension: usize,

    /// Namespace index for fast filtering
    namespace_index: HashMap<i64, Vec<i64>>,

    /// Category index for fast filtering
    category_index: HashMap<String, Vec<i64>>,
}

/// Result of a vector search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResult {
    /// Memory ID
    pub id: i64,

    /// Similarity score (0.0 to 1.0)
    pub similarity: f32,

    /// Boosted score (after tree-based boosting)
    pub boosted_score: f32,
}

impl VectorDatabase {
    /// Create a new vector database
    pub fn new() -> Self {
        Self {
            vectors: HashMap::new(),
            tree: GraphTree::new(),
            dimension: EMBEDDING_DIMENSION,
            namespace_index: HashMap::new(),
            category_index: HashMap::new(),
        }
    }

    /// Create with custom dimension
    pub fn with_dimension(dimension: usize) -> Self {
        Self {
            vectors: HashMap::new(),
            tree: GraphTree::new(),
            dimension,
            namespace_index: HashMap::new(),
            category_index: HashMap::new(),
        }
    }

    /// Create a new in-memory database (async for API compatibility)
    pub async fn in_memory() -> crate::Result<Self> {
        Ok(Self::new())
    }

    /// Insert a vector with priority for graph tree
    pub fn insert_with_priority(
        &mut self,
        entry: VectorEntry,
        priority: Option<u8>,
    ) -> crate::Result<()> {
        if entry.embedding.len() != self.dimension {
            return Err(nexus_core::NexusError::InvalidInput(format!(
                "Vector dimension mismatch: expected {}, got {}",
                self.dimension,
                entry.embedding.len()
            )));
        }

        let id = entry.id;
        let namespace_id = entry.namespace_id;
        let category = entry.category.clone();
        let lane_type = entry.memory_lane_type.clone();

        // Add to graph tree
        self.tree
            .add_memory(id, &category, lane_type.as_deref(), priority);

        // Update indices
        self.namespace_index
            .entry(namespace_id)
            .or_default()
            .push(id);

        self.category_index.entry(category).or_default().push(id);

        // Store the entry
        self.vectors.insert(id, entry);
        Ok(())
    }

    /// Insert a vector (backward compatible)
    pub fn insert(&mut self, entry: VectorEntry) -> crate::Result<()> {
        self.insert_with_priority(entry, None)
    }

    /// Get a vector by ID
    pub fn get(&self, id: i64) -> Option<&VectorEntry> {
        self.vectors.get(&id)
    }

    /// Remove a vector by ID
    pub fn remove(&mut self, id: i64) -> Option<VectorEntry> {
        if let Some(entry) = self.vectors.remove(&id) {
            // Remove from tree
            self.tree.remove_memory(id);

            // Remove from indices
            if let Some(ns_vec) = self.namespace_index.get_mut(&entry.namespace_id) {
                ns_vec.retain(|&i| i != id);
            }

            if let Some(cat_vec) = self.category_index.get_mut(&entry.category) {
                cat_vec.retain(|&i| i != id);
            }

            Some(entry)
        } else {
            None
        }
    }

    /// Get all vector IDs
    pub fn ids(&self) -> Vec<i64> {
        self.vectors.keys().copied().collect()
    }

    /// Get vector count
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Get vectors by namespace
    pub fn by_namespace(&self, namespace_id: i64) -> Vec<&VectorEntry> {
        self.vectors
            .values()
            .filter(|v| v.namespace_id == namespace_id)
            .collect()
    }

    /// Get vectors by category
    pub fn by_category(&self, category: &str) -> Vec<&VectorEntry> {
        self.vectors
            .values()
            .filter(|v| v.category == category)
            .collect()
    }

    /// Get the embedding dimension
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Get reference to the graph tree
    pub fn tree(&self) -> &GraphTree {
        &self.tree
    }

    /// Get mutable reference to the graph tree
    pub fn tree_mut(&mut self) -> &mut GraphTree {
        &mut self.tree
    }

    /// Search for similar vectors
    ///
    /// Returns results sorted by boosted score (descending).
    /// Target latency: <10ms
    pub fn search(
        &self,
        query: &[f32],
        namespace_id: i64,
        limit: usize,
        threshold: f32,
    ) -> crate::Result<(Vec<VectorSearchResult>, SearchLatency)> {
        let start = Instant::now();

        // Validate query dimension
        if query.len() != self.dimension {
            return Err(nexus_core::NexusError::InvalidInput(format!(
                "Query dimension mismatch: expected {}, got {}",
                self.dimension,
                query.len()
            )));
        }

        // Get candidate IDs from namespace
        let candidate_ids = self
            .namespace_index
            .get(&namespace_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        // Calculate similarities
        let mut results: Vec<VectorSearchResult> = candidate_ids
            .iter()
            .filter_map(|&id| {
                let entry = self.vectors.get(&id)?;
                let similarity = cosine_similarity(query, &entry.embedding);

                if similarity >= threshold {
                    let boosted_score = self.tree.calculate_boosted_score(id, similarity);
                    Some(VectorSearchResult {
                        id,
                        similarity,
                        boosted_score,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by boosted score (descending)
        results.sort_by(|a, b| {
            b.boosted_score
                .partial_cmp(&a.boosted_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Truncate to limit
        results.truncate(limit);

        let total_time = start.elapsed();

        let latency = SearchLatency {
            total_ms: total_time.as_millis() as u64,
            vector_comparison_ms: total_time.as_millis() as u64,
            graph_traversal_ms: None,
        };

        Ok((results, latency))
    }

    /// Search with category filter
    pub fn search_by_category(
        &self,
        query: &[f32],
        namespace_id: i64,
        category: &str,
        limit: usize,
        threshold: f32,
    ) -> crate::Result<(Vec<VectorSearchResult>, SearchLatency)> {
        let start = Instant::now();

        // Validate query dimension
        if query.len() != self.dimension {
            return Err(nexus_core::NexusError::InvalidInput(format!(
                "Query dimension mismatch: expected {}, got {}",
                self.dimension,
                query.len()
            )));
        }

        // Get candidates filtered by category
        let category_ids: std::collections::HashSet<i64> = self
            .category_index
            .get(category)
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default();

        let namespace_ids: std::collections::HashSet<i64> = self
            .namespace_index
            .get(&namespace_id)
            .map(|v| v.iter().copied().collect())
            .unwrap_or_default();

        // Intersect namespace and category
        let candidate_ids: Vec<i64> = category_ids.intersection(&namespace_ids).copied().collect();

        // Calculate similarities
        let mut results: Vec<VectorSearchResult> = candidate_ids
            .iter()
            .filter_map(|&id| {
                let entry = self.vectors.get(&id)?;
                let similarity = cosine_similarity(query, &entry.embedding);

                if similarity >= threshold {
                    let boosted_score = self.tree.calculate_boosted_score(id, similarity);
                    Some(VectorSearchResult {
                        id,
                        similarity,
                        boosted_score,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by boosted score (descending)
        results.sort_by(|a, b| {
            b.boosted_score
                .partial_cmp(&a.boosted_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Truncate to limit
        results.truncate(limit);

        let total_time = start.elapsed();

        let latency = SearchLatency {
            total_ms: total_time.as_millis() as u64,
            vector_comparison_ms: total_time.as_millis() as u64,
            graph_traversal_ms: None,
        };

        Ok((results, latency))
    }

    /// Batch insert multiple vectors
    ///
    /// More efficient than individual inserts for bulk loading.
    pub fn insert_batch(&mut self, entries: Vec<VectorEntry>) -> crate::Result<usize> {
        let mut success_count = 0;
        for entry in entries {
            match self.insert(entry) {
                Ok(()) => success_count += 1,
                Err(_) => continue, // Skip invalid entries
            }
        }
        Ok(success_count)
    }

    /// Batch insert with priorities
    pub fn insert_batch_with_priorities(
        &mut self,
        entries: Vec<(VectorEntry, Option<u8>)>,
    ) -> crate::Result<usize> {
        let mut success_count = 0;
        for (entry, priority) in entries {
            match self.insert_with_priority(entry, priority) {
                Ok(()) => success_count += 1,
                Err(_) => continue,
            }
        }
        Ok(success_count)
    }

    /// Batch remove multiple vectors
    pub fn remove_batch(&mut self, ids: &[i64]) -> Vec<Option<VectorEntry>> {
        ids.iter().map(|&id| self.remove(id)).collect()
    }

    /// Search with multiple queries (for batch operations)
    pub fn search_batch(
        &self,
        queries: &[Vec<f32>],
        namespace_id: i64,
        limit: usize,
        threshold: f32,
    ) -> crate::Result<Vec<(Vec<VectorSearchResult>, SearchLatency)>> {
        let mut results = Vec::with_capacity(queries.len());
        for query in queries {
            results.push(self.search(query, namespace_id, limit, threshold)?);
        }
        Ok(results)
    }

    /// Find similar vectors to a stored vector by ID
    pub fn find_similar(
        &self,
        memory_id: i64,
        limit: usize,
        threshold: f32,
    ) -> crate::Result<(Vec<VectorSearchResult>, SearchLatency)> {
        let start = Instant::now();

        let entry = self
            .vectors
            .get(&memory_id)
            .ok_or(nexus_core::NexusError::MemoryNotFound(memory_id))?;

        let query = entry.embedding.clone();
        let namespace_id = entry.namespace_id;

        let (mut results, latency) = self.search(&query, namespace_id, limit + 1, threshold)?;

        // Remove the query vector itself from results
        results.retain(|r| r.id != memory_id);
        results.truncate(limit);

        let total_time = start.elapsed();
        let adjusted_latency = SearchLatency {
            total_ms: total_time.as_millis() as u64,
            vector_comparison_ms: latency.vector_comparison_ms,
            graph_traversal_ms: latency.graph_traversal_ms,
        };

        Ok((results, adjusted_latency))
    }

    /// Get statistics about the vector database
    pub fn stats(&self) -> VectorDatabaseStats {
        let mut category_counts = HashMap::new();
        let mut namespace_counts = HashMap::new();

        for entry in self.vectors.values() {
            *category_counts.entry(entry.category.clone()).or_insert(0) += 1;
            *namespace_counts.entry(entry.namespace_id).or_insert(0) += 1;
        }

        VectorDatabaseStats {
            total_vectors: self.vectors.len(),
            dimension: self.dimension,
            category_counts,
            namespace_counts,
            tree_stats: self.tree.stats(),
        }
    }

    /// Clear all vectors from the database
    pub fn clear(&mut self) {
        self.vectors.clear();
        self.namespace_index.clear();
        self.category_index.clear();
        self.tree = GraphTree::new();
    }

    /// Check if a vector exists
    pub fn contains(&self, id: i64) -> bool {
        self.vectors.contains_key(&id)
    }

    /// Get all vectors as a slice (read-only access)
    pub fn all_vectors(&self) -> Vec<&VectorEntry> {
        self.vectors.values().collect()
    }

    /// Update an existing vector's embedding
    pub fn update_embedding(&mut self, id: i64, new_embedding: Vec<f32>) -> crate::Result<()> {
        if new_embedding.len() != self.dimension {
            return Err(nexus_core::NexusError::InvalidInput(format!(
                "Vector dimension mismatch: expected {}, got {}",
                self.dimension,
                new_embedding.len()
            )));
        }

        let entry = self
            .vectors
            .get_mut(&id)
            .ok_or(nexus_core::NexusError::MemoryNotFound(id))?;

        entry.embedding = new_embedding;
        entry.created_at = chrono::Utc::now();
        Ok(())
    }
}

/// Statistics about the vector database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDatabaseStats {
    /// Total number of vectors
    pub total_vectors: usize,
    /// Embedding dimension
    pub dimension: usize,
    /// Count by category
    pub category_counts: HashMap<String, usize>,
    /// Count by namespace
    pub namespace_counts: HashMap<i64, usize>,
    /// Graph tree statistics
    pub tree_stats: crate::graph::TreeStats,
}

/// Calculate cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    (dot_product / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

/// Calculate euclidean distance between two vectors
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return f32::MAX;
    }

    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// Calculate dot product between two vectors
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Normalize a vector in place
pub fn normalize_vector(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Batch similarity computation using SIMD-friendly approach
pub fn batch_cosine_similarity(query: &[f32], vectors: &[&[f32]]) -> Vec<f32> {
    vectors
        .iter()
        .map(|v| cosine_similarity(query, v))
        .collect()
}

/// Find top-k similar vectors
pub fn top_k_similar(
    query: &[f32],
    vectors: &[(i64, &[f32])],
    k: usize,
    threshold: f32,
) -> Vec<(i64, f32)> {
    let mut scored: Vec<(i64, f32)> = vectors
        .iter()
        .filter_map(|(id, vec)| {
            let sim = cosine_similarity(query, vec);
            if sim >= threshold {
                Some((*id, sim))
            } else {
                None
            }
        })
        .collect();

    // Partial sort for top-k
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VectorEntry;

    fn create_test_entry(id: i64, namespace_id: i64) -> VectorEntry {
        VectorEntry::new(
            id,
            vec![0.1; EMBEDDING_DIMENSION],
            "general".to_string(),
            namespace_id,
        )
    }

    fn create_test_entry_with_embedding(id: i64, namespace_id: i64, value: f32) -> VectorEntry {
        VectorEntry::new(
            id,
            vec![value; EMBEDDING_DIMENSION],
            "general".to_string(),
            namespace_id,
        )
    }

    #[test]
    fn test_insert_and_get() {
        let mut db = VectorDatabase::new();
        let entry = create_test_entry(1, 1);

        db.insert(entry.clone()).unwrap();

        assert!(db.get(1).is_some());
        assert_eq!(db.len(), 1);
    }

    #[test]
    fn test_remove() {
        let mut db = VectorDatabase::new();
        db.insert(create_test_entry(1, 1)).unwrap();

        let removed = db.remove(1);

        assert!(removed.is_some());
        assert!(db.is_empty());
    }

    #[test]
    fn test_dimension_mismatch() {
        let mut db = VectorDatabase::new();
        let bad_entry = VectorEntry::new(1, vec![0.1; 100], "general".to_string(), 1);

        let result = db.insert(bad_entry);

        assert!(result.is_err());
    }

    #[test]
    fn test_by_namespace() {
        let mut db = VectorDatabase::new();
        db.insert(create_test_entry(1, 1)).unwrap();
        db.insert(create_test_entry(2, 1)).unwrap();
        db.insert(create_test_entry(3, 2)).unwrap();

        let ns1 = db.by_namespace(1);
        let ns2 = db.by_namespace(2);

        assert_eq!(ns1.len(), 2);
        assert_eq!(ns2.len(), 1);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![0.5; EMBEDDING_DIMENSION];
        let b = vec![0.5; EMBEDDING_DIMENSION];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let mut a = vec![0.0; EMBEDDING_DIMENSION];
        let mut b = vec![0.0; EMBEDDING_DIMENSION];
        for i in 0..EMBEDDING_DIMENSION {
            if i < EMBEDDING_DIMENSION / 2 {
                a[i] = 1.0;
            } else {
                b[i] = 1.0;
            }
        }
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_search_basic() {
        let mut db = VectorDatabase::new();

        // Store similar vectors
        db.insert(create_test_entry_with_embedding(1, 1, 0.5))
            .unwrap();
        db.insert(create_test_entry_with_embedding(2, 1, 0.51))
            .unwrap();
        db.insert(create_test_entry_with_embedding(3, 1, 0.1))
            .unwrap();

        let query = vec![0.5; EMBEDDING_DIMENSION];
        let (results, latency) = db.search(&query, 1, 10, 0.0).unwrap();

        assert_eq!(results.len(), 3);
        // Most similar should be first
        assert!(results[0].similarity >= results[1].similarity);
        println!("Search latency: {:?}", latency);
    }

    #[test]
    fn test_search_with_threshold() {
        let mut db = VectorDatabase::new();

        // Create embeddings with different directions (not just different magnitudes)
        // Uniform vectors always have cosine similarity of 1.0, so we need to vary the direction
        let mut embedding1 = vec![0.5; EMBEDDING_DIMENSION];
        embedding1[0] = 1.0; // Different direction

        let mut embedding2 = vec![0.1; EMBEDDING_DIMENSION];
        embedding2[0] = -1.0; // Opposite direction in first dimension

        let entry1 = VectorEntry::new(1, embedding1.clone(), "general".to_string(), 1);
        let entry2 = VectorEntry::new(2, embedding2, "general".to_string(), 1);

        db.insert(entry1).unwrap();
        db.insert(entry2).unwrap();

        let query = embedding1.clone();
        let (results, _) = db.search(&query, 1, 10, 0.9).unwrap();

        // Only entry1 should match with high threshold since entry2 has opposite first dimension
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 1);
    }

    #[test]
    fn test_search_by_category() {
        let mut db = VectorDatabase::new();

        let entry1 = VectorEntry::new(1, vec![0.5; EMBEDDING_DIMENSION], "general".to_string(), 1);
        let entry2 = VectorEntry::new(2, vec![0.5; EMBEDDING_DIMENSION], "facts".to_string(), 1);

        db.insert(entry1).unwrap();
        db.insert(entry2).unwrap();

        let query = vec![0.5; EMBEDDING_DIMENSION];
        let (results, _) = db
            .search_by_category(&query, 1, "general", 10, 0.0)
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 1);
    }

    #[test]
    fn test_search_latency_target() {
        let mut db = VectorDatabase::new();

        // Add many vectors
        for i in 0..1000 {
            db.insert(create_test_entry_with_embedding(i, 1, 0.5))
                .unwrap();
        }

        let query = vec![0.5; EMBEDDING_DIMENSION];
        let (_, latency) = db.search(&query, 1, 10, 0.0).unwrap();

        // Should be very fast for in-memory search
        println!("Search latency: {:?}", latency);
        assert!(
            latency.total_ms < 100,
            "Search took {}ms, expected <100ms",
            latency.total_ms
        );
    }

    #[test]
    fn test_insert_with_priority() {
        let mut db = VectorDatabase::new();

        let entry = create_test_entry(1, 1);
        db.insert_with_priority(entry, Some(1)).unwrap(); // High priority

        // Check tree has the node with boosted weight
        let tree = db.tree();
        let node = tree.get(1);
        assert!(node.is_some());
        let node = node.unwrap();
        assert!((node.weight - 1.5).abs() < 0.01); // High priority weight
    }

    #[tokio::test]
    async fn test_in_memory_creation() {
        let db = VectorDatabase::in_memory().await.unwrap();
        assert!(db.is_empty());
    }

    #[test]
    fn test_batch_insert() {
        let mut db = VectorDatabase::new();
        let entries = vec![
            create_test_entry(1, 1),
            create_test_entry(2, 1),
            create_test_entry(3, 1),
        ];

        let count = db.insert_batch(entries).unwrap();
        assert_eq!(count, 3);
        assert_eq!(db.len(), 3);
    }

    #[test]
    fn test_batch_insert_with_invalid() {
        let mut db = VectorDatabase::new();
        let entries = vec![
            create_test_entry(1, 1),
            VectorEntry::new(2, vec![0.1; 100], "general".to_string(), 1), // Invalid dimension
            create_test_entry(3, 1),
        ];

        let count = db.insert_batch(entries).unwrap();
        assert_eq!(count, 2); // Only valid entries inserted
        assert_eq!(db.len(), 2);
    }

    #[test]
    fn test_batch_remove() {
        let mut db = VectorDatabase::new();
        db.insert(create_test_entry(1, 1)).unwrap();
        db.insert(create_test_entry(2, 1)).unwrap();
        db.insert(create_test_entry(3, 1)).unwrap();

        let removed = db.remove_batch(&[1, 2, 999]);
        assert_eq!(removed.len(), 3);
        assert!(removed[0].is_some());
        assert!(removed[1].is_some());
        assert!(removed[2].is_none()); // 999 doesn't exist
        assert_eq!(db.len(), 1);
    }

    #[test]
    fn test_find_similar() {
        let mut db = VectorDatabase::new();

        // Create vectors with different similarities
        let mut e1 = vec![0.5; EMBEDDING_DIMENSION];
        e1[0] = 1.0;

        let mut e2 = vec![0.5; EMBEDDING_DIMENSION];
        e2[0] = 0.95;

        let mut e3 = vec![0.1; EMBEDDING_DIMENSION];
        e3[0] = -1.0;

        db.insert(VectorEntry::new(1, e1.clone(), "general".to_string(), 1))
            .unwrap();
        db.insert(VectorEntry::new(2, e2, "general".to_string(), 1))
            .unwrap();
        db.insert(VectorEntry::new(3, e3, "general".to_string(), 1))
            .unwrap();

        let (results, _) = db.find_similar(1, 10, 0.0).unwrap();

        // Should not include the query vector itself
        assert!(!results.iter().any(|r| r.id == 1));
        // Most similar should be first
        assert_eq!(results[0].id, 2);
    }

    #[test]
    fn test_stats() {
        let mut db = VectorDatabase::new();
        db.insert(VectorEntry::new(
            1,
            vec![0.1; EMBEDDING_DIMENSION],
            "general".to_string(),
            1,
        ))
        .unwrap();
        db.insert(VectorEntry::new(
            2,
            vec![0.1; EMBEDDING_DIMENSION],
            "general".to_string(),
            1,
        ))
        .unwrap();
        db.insert(VectorEntry::new(
            3,
            vec![0.1; EMBEDDING_DIMENSION],
            "facts".to_string(),
            2,
        ))
        .unwrap();

        let stats = db.stats();
        assert_eq!(stats.total_vectors, 3);
        assert_eq!(stats.dimension, EMBEDDING_DIMENSION);
        assert_eq!(*stats.category_counts.get("general").unwrap_or(&0), 2);
        assert_eq!(*stats.category_counts.get("facts").unwrap_or(&0), 1);
    }

    #[test]
    fn test_clear() {
        let mut db = VectorDatabase::new();
        db.insert(create_test_entry(1, 1)).unwrap();
        db.insert(create_test_entry(2, 1)).unwrap();

        db.clear();
        assert!(db.is_empty());
    }

    #[test]
    fn test_contains() {
        let mut db = VectorDatabase::new();
        db.insert(create_test_entry(1, 1)).unwrap();

        assert!(db.contains(1));
        assert!(!db.contains(2));
    }

    #[test]
    fn test_update_embedding() {
        let mut db = VectorDatabase::new();
        db.insert(create_test_entry(1, 1)).unwrap();

        let new_embedding = vec![0.9; EMBEDDING_DIMENSION];
        db.update_embedding(1, new_embedding.clone()).unwrap();

        let entry = db.get(1).unwrap();
        assert_eq!(entry.embedding, new_embedding);
    }

    #[test]
    fn test_update_embedding_nonexistent() {
        let mut db = VectorDatabase::new();
        let result = db.update_embedding(999, vec![0.1; EMBEDDING_DIMENSION]);
        assert!(result.is_err());
    }

    #[test]
    fn test_euclidean_distance() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let dist = euclidean_distance(&a, &b);
        assert!((dist - 2.0_f32.sqrt()).abs() < 0.001);
    }

    #[test]
    fn test_dot_product() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        let prod = dot_product(&a, &b);
        assert!((prod - 32.0).abs() < 0.001); // 1*4 + 2*5 + 3*6 = 32
    }

    #[test]
    fn test_normalize_vector() {
        let mut v = vec![3.0, 4.0];
        normalize_vector(&mut v);
        assert!((v[0] - 0.6).abs() < 0.001);
        assert!((v[1] - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_top_k_similar() {
        let query = vec![1.0, 0.0];
        let vectors: Vec<(i64, &[f32])> = vec![
            (1, &[1.0, 0.0]),     // similarity 1.0
            (2, &[0.0, 1.0]),     // similarity 0.0
            (3, &[0.707, 0.707]), // similarity ~0.707
            (4, &[0.9, 0.1]),     // similarity ~0.9
        ];

        let top_k = top_k_similar(&query, &vectors, 2, 0.0);
        assert_eq!(top_k.len(), 2);
        assert_eq!(top_k[0].0, 1); // Most similar
        assert_eq!(top_k[1].0, 4); // Second most similar
    }
}
