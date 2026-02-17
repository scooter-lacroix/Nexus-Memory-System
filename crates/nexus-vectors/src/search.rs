//! Semantic search implementation
//!
//! This module provides semantic search capabilities for vector embeddings.

use crate::{VectorEntry, EMBEDDING_DIMENSION, graph::GraphTree};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Search options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Maximum number of results
    pub limit: usize,

    /// Minimum similarity threshold (0.0 to 1.0)
    pub threshold: f32,

    /// Whether to use graph boosting
    pub use_graph_boost: bool,

    /// Filter by category
    pub category: Option<String>,

    /// Filter by namespace
    pub namespace_id: Option<i64>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            threshold: 0.5,
            use_graph_boost: true,
            category: None,
            namespace_id: None,
        }
    }
}

impl SearchOptions {
    /// Create new search options with limit
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            ..Default::default()
        }
    }

    /// Set threshold
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Set category filter
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    /// Set namespace filter
    pub fn with_namespace(mut self, namespace_id: i64) -> Self {
        self.namespace_id = Some(namespace_id);
        self
    }
}

/// Search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Memory ID
    pub id: i64,

    /// Similarity score (0.0 to 1.0)
    pub score: f32,

    /// Boosted score (if graph boosting enabled)
    pub boosted_score: Option<f32>,

    /// Vector entry data
    pub entry: VectorEntry,
}

impl PartialEq for SearchResult {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for SearchResult {}

impl PartialOrd for SearchResult {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchResult {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Sort by boosted score if available, otherwise by base score
        let self_score = self.boosted_score.unwrap_or(self.score);
        let other_score = other.boosted_score.unwrap_or(other.score);
        other_score.partial_cmp(&self_score).unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Semantic search engine
pub struct SemanticSearch {
    /// Graph tree for boosted search
    graph_tree: GraphTree,
}

impl SemanticSearch {
    /// Create a new semantic search engine
    pub fn new() -> Self {
        Self {
            graph_tree: GraphTree::new(),
        }
    }

    /// Create with existing graph tree
    pub fn with_graph_tree(graph_tree: GraphTree) -> Self {
        Self { graph_tree }
    }

    /// Search for similar vectors
    pub fn search(
        &self,
        query: &[f32],
        vectors: &[VectorEntry],
        options: &SearchOptions,
    ) -> crate::Result<(Vec<SearchResult>, crate::SearchLatency)> {
        let start = Instant::now();
        let vector_start = Instant::now();

        if query.len() != EMBEDDING_DIMENSION {
            return Err(nexus_core::NexusError::InvalidInput(format!(
                "Query dimension mismatch: expected {}, got {}",
                EMBEDDING_DIMENSION,
                query.len()
            )));
        }

        let mut results: Vec<SearchResult> = vectors
            .iter()
            .filter(|v| {
                // Apply filters
                if let Some(ref cat) = options.category {
                    if v.category != *cat {
                        return false;
                    }
                }
                if let Some(ns) = options.namespace_id {
                    if v.namespace_id != ns {
                        return false;
                    }
                }
                true
            })
            .filter_map(|entry| {
                let score = cosine_similarity(query, &entry.embedding);
                if score >= options.threshold {
                    Some((entry, score))
                } else {
                    None
                }
            })
            .map(|(entry, score)| {
                let boosted_score = if options.use_graph_boost {
                    Some(self.graph_tree.calculate_boosted_score(entry.id, score))
                } else {
                    None
                };

                SearchResult {
                    id: entry.id,
                    score,
                    boosted_score,
                    entry: entry.clone(),
                }
            })
            .collect();

        let vector_time = vector_start.elapsed().as_millis() as u64;

        // Sort results
        results.sort();

        // Limit results
        results.truncate(options.limit);

        let total_time = start.elapsed().as_millis() as u64;

        let latency = crate::SearchLatency {
            total_ms: total_time,
            vector_comparison_ms: vector_time,
            graph_traversal_ms: if options.use_graph_boost {
                Some(total_time.saturating_sub(vector_time))
            } else {
                None
            },
        };

        Ok((results, latency))
    }

    /// Get the graph tree
    pub fn graph_tree(&self) -> &GraphTree {
        &self.graph_tree
    }

    /// Get mutable graph tree
    pub fn graph_tree_mut(&mut self) -> &mut GraphTree {
        &mut self.graph_tree
    }
}

impl Default for SemanticSearch {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_entry(id: i64, embedding: Vec<f32>) -> VectorEntry {
        VectorEntry::new(id, embedding, "general".to_string(), 1)
    }

    #[test]
    fn test_search_options_default() {
        let opts = SearchOptions::default();
        assert_eq!(opts.limit, 10);
        assert!((opts.threshold - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_search_options_builder() {
        let opts = SearchOptions::with_limit(5)
            .with_threshold(0.8)
            .with_category("facts");

        assert_eq!(opts.limit, 5);
        assert!((opts.threshold - 0.8).abs() < 0.01);
        assert_eq!(opts.category, Some("facts".to_string()));
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_semantic_search_basic() {
        let search = SemanticSearch::new();

        let query = vec![1.0; EMBEDDING_DIMENSION];
        let vectors = vec![
            create_test_entry(1, vec![0.9; EMBEDDING_DIMENSION]),
            create_test_entry(2, vec![0.1; EMBEDDING_DIMENSION]),
        ];

        let opts = SearchOptions::with_limit(10).with_threshold(0.0);
        let (results, latency) = search.search(&query, &vectors, &opts).unwrap();

        assert_eq!(results.len(), 2);
        // First result should be more similar
        assert!(results[0].score >= results[1].score);
    }

    #[test]
    fn test_semantic_search_threshold() {
        let search = SemanticSearch::new();

        let query = vec![1.0; EMBEDDING_DIMENSION];
        let vectors = vec![
            create_test_entry(1, vec![1.0; EMBEDDING_DIMENSION]), // Very similar
            create_test_entry(2, vec![0.0; EMBEDDING_DIMENSION]), // Not similar
        ];

        let opts = SearchOptions::with_limit(10).with_threshold(0.9);
        let (results, _) = search.search(&query, &vectors, &opts).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 1);
    }

    #[test]
    fn test_semantic_search_category_filter() {
        let search = SemanticSearch::new();

        let query = vec![1.0; EMBEDDING_DIMENSION];

        let mut entry1 = create_test_entry(1, vec![1.0; EMBEDDING_DIMENSION]);
        entry1.category = "facts".to_string();

        let mut entry2 = create_test_entry(2, vec![1.0; EMBEDDING_DIMENSION]);
        entry2.category = "general".to_string();

        let vectors = vec![entry1, entry2];

        let opts = SearchOptions::with_limit(10).with_threshold(0.0).with_category("facts");
        let (results, _) = search.search(&query, &vectors, &opts).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.category, "facts");
    }

    #[test]
    fn test_search_result_ordering() {
        let r1 = SearchResult {
            id: 1,
            score: 0.9,
            boosted_score: None,
            entry: create_test_entry(1, vec![0.1; EMBEDDING_DIMENSION]),
        };

        let r2 = SearchResult {
            id: 2,
            score: 0.8,
            boosted_score: None,
            entry: create_test_entry(2, vec![0.1; EMBEDDING_DIMENSION]),
        };

        assert!(r1 < r2); // Higher score should be "less" for correct sorting
    }
}
