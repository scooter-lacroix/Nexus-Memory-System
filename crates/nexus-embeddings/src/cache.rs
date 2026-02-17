//! Embedding cache for reducing redundant computations

use crate::Result;
use parking_lot::RwLock;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Cache entry containing an embedding and access metadata
#[derive(Debug, Clone)]
struct CacheEntry {
    /// The cached embedding vector
    embedding: Vec<f32>,
    /// Number of times this entry has been accessed
    access_count: u64,
}

/// LRU-style cache for embeddings
#[derive(Debug)]
pub struct EmbeddingCache {
    /// The cache storage
    cache: RwLock<HashMap<u64, CacheEntry>>,
    /// Maximum number of entries
    max_size: usize,
    /// Total cache hits
    hits: RwLock<u64>,
    /// Total cache misses
    misses: RwLock<u64>,
}

impl EmbeddingCache {
    /// Create a new embedding cache with the specified maximum size
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::with_capacity(max_size)),
            max_size,
            hits: RwLock::new(0),
            misses: RwLock::new(0),
        }
    }

    /// Compute hash for a text string
    fn hash_text(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    /// Get an embedding from the cache
    pub fn get(&self, text: &str) -> Option<Vec<f32>> {
        let hash = Self::hash_text(text);

        // First, check if the entry exists and get the embedding
        let embedding = {
            let cache = self.cache.read();
            if let Some(entry) = cache.get(&hash) {
                Some(entry.embedding.clone())
            } else {
                // Update miss counter
                let mut misses = self.misses.write();
                *misses += 1;
                return None;
            }
        };

        // Entry exists, update access count and hit counter
        {
            let mut cache = self.cache.write();
            if let Some(entry) = cache.get_mut(&hash) {
                entry.access_count += 1;
            }
        }

        // Update hit counter
        {
            let mut hits = self.hits.write();
            *hits += 1;
        }

        embedding
    }

    /// Put an embedding into the cache
    pub fn put(&self, text: &str, embedding: Vec<f32>) {
        let hash = Self::hash_text(text);

        let mut cache = self.cache.write();

        // Check if we need to evict entries
        if cache.len() >= self.max_size && !cache.contains_key(&hash) {
            // Simple eviction: remove the entry with lowest access count
            if let Some((&min_key, _)) = cache.iter().min_by_key(|(_, entry)| entry.access_count) {
                cache.remove(&min_key);
            }
        }

        cache.insert(
            hash,
            CacheEntry {
                embedding,
                access_count: 1,
            },
        );
    }

    /// Get or compute an embedding
    pub fn get_or_compute<F>(&self, text: &str, compute: F) -> Result<Vec<f32>>
    where
        F: FnOnce() -> Result<Vec<f32>>,
    {
        if let Some(embedding) = self.get(text) {
            return Ok(embedding);
        }

        let embedding = compute()?;
        self.put(text, embedding.clone());
        Ok(embedding)
    }

    /// Clear the cache
    pub fn clear(&self) {
        let mut cache = self.cache.write();
        cache.clear();

        // Reset counters
        let mut hits = self.hits.write();
        let mut misses = self.misses.write();
        *hits = 0;
        *misses = 0;
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let cache = self.cache.read();
        let hits = *self.hits.read();
        let misses = *self.misses.read();

        CacheStats {
            size: cache.len(),
            max_size: self.max_size,
            hits,
            misses,
            hit_rate: if hits + misses > 0 {
                hits as f64 / (hits + misses) as f64
            } else {
                0.0
            },
        }
    }

    /// Check if the cache contains a key
    pub fn contains(&self, text: &str) -> bool {
        let hash = Self::hash_text(text);
        self.cache.read().contains_key(&hash)
    }

    /// Get the current size of the cache
    pub fn len(&self) -> usize {
        self.cache.read().len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.read().is_empty()
    }
}

impl Default for EmbeddingCache {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Statistics about the cache
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Current number of entries
    pub size: usize,
    /// Maximum capacity
    pub max_size: usize,
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Hit rate (0.0 to 1.0)
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_put_get() {
        let cache = EmbeddingCache::new(10);
        let embedding = vec![0.1, 0.2, 0.3];

        cache.put("hello", embedding.clone());

        let result = cache.get("hello");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), embedding);
    }

    #[test]
    fn test_cache_miss() {
        let cache = EmbeddingCache::new(10);

        let result = cache.get("not found");
        assert!(result.is_none());

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
    }

    #[test]
    fn test_cache_hit_counter() {
        let cache = EmbeddingCache::new(10);
        let embedding = vec![0.1, 0.2, 0.3];

        cache.put("test", embedding);

        // Access multiple times
        cache.get("test");
        cache.get("test");
        cache.get("test");

        let stats = cache.stats();
        assert_eq!(stats.hits, 3);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_cache_eviction() {
        let cache = EmbeddingCache::new(3);

        // Fill cache
        cache.put("a", vec![1.0]);
        cache.put("b", vec![2.0]);
        cache.put("c", vec![3.0]);

        // Access 'a' multiple times to increase its access count
        cache.get("a");
        cache.get("a");

        // Add a new entry - should evict the least accessed
        cache.put("d", vec![4.0]);

        assert_eq!(cache.len(), 3);
        // 'a' should still be there (most accessed)
        assert!(cache.contains("a"));
    }

    #[test]
    fn test_cache_clear() {
        let cache = EmbeddingCache::new(10);

        cache.put("test", vec![1.0]);
        assert!(!cache.is_empty());

        cache.clear();
        assert!(cache.is_empty());

        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_get_or_compute() {
        let cache = EmbeddingCache::new(10);
        let mut compute_count = 0;

        // First call should compute
        let result = cache
            .get_or_compute("test", || {
                compute_count += 1;
                Ok(vec![1.0, 2.0, 3.0])
            })
            .unwrap();

        assert_eq!(result, vec![1.0, 2.0, 3.0]);
        assert_eq!(compute_count, 1);

        // Second call should use cache
        let result = cache
            .get_or_compute("test", || {
                compute_count += 1;
                Ok(vec![1.0, 2.0, 3.0])
            })
            .unwrap();

        assert_eq!(result, vec![1.0, 2.0, 3.0]);
        assert_eq!(compute_count, 1); // Should not have computed again
    }

    #[test]
    fn test_cache_stats() {
        let cache = EmbeddingCache::new(100);

        cache.put("a", vec![1.0]);
        cache.put("b", vec![2.0]);

        cache.get("a"); // hit
        cache.get("a"); // hit
        cache.get("c"); // miss

        let stats = cache.stats();
        assert_eq!(stats.size, 2);
        assert_eq!(stats.max_size, 100);
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 0.6666666666666666).abs() < 0.0001);
    }
}
