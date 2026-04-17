//! Cognitive cache data models for tiering and ranking.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tracing::{debug, warn};

use crate::context_builder::ColdRecall;
use crate::error::AgentError;
use nexus_core::{EmbeddingService, Memory, ProjectIdentity};
use nexus_storage::repository::MemoryRepository;
use nexus_vectors::{SearchOptions, SemanticSearch, VectorEntry};

/// Confidence tier for memory surfacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConfidenceTier {
    /// Score < 0.72 - One-liner, model decides
    Whisper,
    /// Score >= 0.72 - Present, lightly compressed
    Clear,
    /// Score >= 0.85 - Full content, direct injection
    Loud,
}

impl ConfidenceTier {
    /// Determine confidence tier from a raw relevance score.
    pub fn from_score(score: f32) -> Self {
        if score >= 0.85 {
            ConfidenceTier::Loud
        } else if score >= 0.72 {
            ConfidenceTier::Clear
        } else {
            ConfidenceTier::Whisper
        }
    }
}

/// Entry in the hot cognitive cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotCacheEntry {
    pub memory_id: i64,
    pub content: String,
    pub relevance_score: f32,
    pub tier: ConfidenceTier,
    pub promoted_at: DateTime<Utc>,
    pub last_surfaced: DateTime<Utc>,
    pub hot_streak: u32,
    pub pinned: bool,
    pub source_agent: Option<String>,
}

impl HotCacheEntry {
    /// Calculate a composite score for eviction.
    /// Combines relevance, hot streak (frequency), and recency (LRU).
    pub fn eviction_score(&self) -> f32 {
        if self.pinned {
            return f32::MAX;
        }

        let now = Utc::now();
        let age_secs = now
            .signed_duration_since(self.last_surfaced)
            .num_seconds()
            .max(1) as f32;

        // Decay factor: items not surfaced for 24h lose significant score
        let recency_penalty = (age_secs / 86400.0).exp();

        // Boost for repeated use (frequency)
        let frequency_boost = (self.hot_streak as f32).ln().max(1.0);

        (self.relevance_score * frequency_boost) / recency_penalty
    }
}

/// Hot cognitive cache - holds the most active context for a project.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HotCache {
    pub entries: Vec<HotCacheEntry>,
    pub last_updated: Option<DateTime<Utc>>,
    pub last_session_id: Option<String>,
}

impl HotCache {
    /// Promote a new entry to the hot cache.
    pub fn promote(&mut self, entry: HotCacheEntry, max_entries: usize) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|e| e.memory_id == entry.memory_id)
        {
            existing.content = entry.content;
            existing.relevance_score = entry.relevance_score;
            existing.tier = entry.tier;
            existing.hot_streak += 1;
            existing.last_surfaced = Utc::now();
            return;
        }

        if self.entries.len() >= max_entries {
            // PHASE 10: LRU-Aware Eviction
            let mut candidates: Vec<(usize, f32)> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.pinned)
                .map(|(i, e)| (i, e.eviction_score()))
                .collect();

            if !candidates.is_empty() {
                // Sort by composite eviction score ascending (lowest score gets evicted)
                candidates
                    .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                self.entries.remove(candidates[0].0);
            } else if !entry.pinned {
                // All entries are pinned and new one is not - drop it
                return;
            }
        }

        self.entries.push(entry);
        self.last_updated = Some(Utc::now());
    }
}

/// Entry in the cold cache index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdIndexEntry {
    pub memory_id: i64,
    pub project_relevance: f32,
    pub last_surfaced: Option<DateTime<Utc>>,
}

/// Cold cache index - references memories that might be relevant later.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ColdCacheIndex {
    pub entries: Vec<ColdIndexEntry>,
    pub last_reindexed: Option<DateTime<Utc>>,
}

/// Unified cognitive cache for a project.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CognitiveCache {
    pub hot_cache: HotCache,
    pub cold_index: ColdCacheIndex,
}

impl CognitiveCache {
    /// Perform morning recall to surface project-relevant memories.
    pub async fn morning_recall(
        &self,
        project: &ProjectIdentity,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
        embedding_service: Option<&dyn EmbeddingService>,
    ) -> Vec<ColdRecall> {
        let _start = std::time::Instant::now();
        let query_string = format!(
            "{} {} project context",
            project.display_name,
            project.git_remote.as_deref().unwrap_or("")
        );
        let hot_ids: std::collections::HashSet<i64> =
            self.hot_cache.entries.iter().map(|e| e.memory_id).collect();

        let mut results = Vec::new();

        if let Some(service) = embedding_service {
            match tokio::time::timeout(Duration::from_millis(500), async {
                if let Ok(embedding) = service.embed(&query_string).await {
                    // Fetch recent memories for candidate matching
                    let filters = nexus_storage::repository::ListMemoryFilters {
                        category: None,
                        since: None,
                        until: None,
                        content_like: None,
                        include_raw: false,
                        limit: 50,
                        offset: 0,
                    };

                    if let Ok(memories) = memory_repo.list_filtered(namespace_id, filters).await {
                        let entries: Vec<VectorEntry> = memories
                            .into_iter()
                            .filter_map(|m| {
                                m.content_embedding.as_ref().map(|emb| {
                                    VectorEntry::new(
                                        m.id,
                                        emb.clone(),
                                        m.category.to_string(),
                                        namespace_id,
                                    )
                                })
                            })
                            .collect();

                        let search = SemanticSearch::new();
                        let options = SearchOptions::with_limit(20).with_threshold(0.65);

                        if let Ok((search_results, _)) =
                            search.search(&embedding, &entries, &options)
                        {
                            // Batch-fetch content for matches (SearchResult doesn't hold content directly)
                            let filtered_results: Vec<_> = search_results
                                .into_iter()
                                .filter(|r| !hot_ids.contains(&r.id))
                                .take(10)
                                .collect();

                            let ids: Vec<i64> = filtered_results.iter().map(|r| r.id).collect();

                            let memories = memory_repo.get_by_ids(&ids).await.unwrap_or_default();

                            // Preserve ordering from search_results by mapping id→memory
                            let memory_by_id: HashMap<i64, Memory> =
                                memories.into_iter().map(|m| (m.id, m)).collect();

                            let mut recalls = Vec::new();
                            for r in filtered_results {
                                if let Some(m) = memory_by_id.get(&r.id) {
                                    recalls.push(ColdRecall {
                                        memory_id: r.id,
                                        content: m.content.clone(),
                                        relevance_score: r.score,
                                        tier: ConfidenceTier::from_score(r.score),
                                    });
                                }
                            }
                            return Ok::<Vec<ColdRecall>, AgentError>(recalls);
                        }
                    }
                }
                Ok(Vec::new())
            })
            .await
            {
                Ok(Ok(recalls)) => results = recalls,
                Ok(Err(e)) => warn!("Morning recall vector search failed: {}", e),
                Err(_) => warn!("Morning recall vector search timed out"),
            }
        }

        if results.is_empty() {
            let filters = nexus_storage::repository::ListMemoryFilters {
                category: None,
                since: None,
                until: None,
                content_like: Some(&project.display_name),
                include_raw: false,
                limit: 10,
                offset: 0,
            };

            if let Ok(memories) = memory_repo.list_filtered(namespace_id, filters).await {
                results = memories
                    .into_iter()
                    .filter(|m| !hot_ids.contains(&m.id))
                    .take(10)
                    .map(|m| ColdRecall {
                        memory_id: m.id,
                        content: m.content,
                        relevance_score: 0.65,
                        tier: ConfidenceTier::Whisper,
                    })
                    .collect();
            }
        }

        debug!(
            "Morning recall found {} items in {:?}",
            results.len(),
            _start.elapsed()
        );
        results
    }

    /// Load cognitive cache from disk or initialize if missing.
    pub fn load_or_init(nexus_dir: &Path) -> Self {
        let cache_dir = nexus_dir.join("cache");
        let hot_path = cache_dir.join("hot.json");
        let cold_path = cache_dir.join("cold_index.json");

        let hot_cache = if hot_path.exists() {
            std::fs::read_to_string(&hot_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            HotCache::default()
        };

        let cold_index = if cold_path.exists() {
            std::fs::read_to_string(&cold_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            ColdCacheIndex::default()
        };

        Self {
            hot_cache,
            cold_index,
        }
    }

    /// Save cognitive cache to disk.
    pub fn save(&self, nexus_dir: &Path) -> std::io::Result<()> {
        let cache_dir = nexus_dir.join("cache");
        std::fs::create_dir_all(&cache_dir)?;

        let hot_json = serde_json::to_string_pretty(&self.hot_cache)?;
        std::fs::write(cache_dir.join("hot.json"), hot_json)?;

        let cold_json = serde_json::to_string_pretty(&self.cold_index)?;
        std::fs::write(cache_dir.join("cold_index.json"), cold_json)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_confidence_tier_boundaries() {
        assert_eq!(ConfidenceTier::from_score(0.85), ConfidenceTier::Loud);
        assert_eq!(ConfidenceTier::from_score(0.84), ConfidenceTier::Clear);
        assert_eq!(ConfidenceTier::from_score(0.72), ConfidenceTier::Clear);
        assert_eq!(ConfidenceTier::from_score(0.71), ConfidenceTier::Whisper);
        assert_eq!(ConfidenceTier::from_score(0.50), ConfidenceTier::Whisper);
    }

    #[test]
    fn test_hot_cache_promote_and_evict() {
        let mut hot = HotCache::default();
        let max = 2;

        let e1 = HotCacheEntry {
            memory_id: 1,
            content: "e1".into(),
            relevance_score: 0.9,
            tier: ConfidenceTier::Loud,
            promoted_at: Utc::now(),
            last_surfaced: Utc::now(),
            hot_streak: 1,
            pinned: false,
            source_agent: None,
        };
        let e2 = HotCacheEntry {
            memory_id: 2,
            content: "e2".into(),
            relevance_score: 0.8,
            tier: ConfidenceTier::Clear,
            promoted_at: Utc::now(),
            last_surfaced: Utc::now(),
            hot_streak: 1,
            pinned: false,
            source_agent: None,
        };
        let e3 = HotCacheEntry {
            memory_id: 3,
            content: "e3".into(),
            relevance_score: 0.95,
            tier: ConfidenceTier::Loud,
            promoted_at: Utc::now(),
            last_surfaced: Utc::now(),
            hot_streak: 1,
            pinned: false,
            source_agent: None,
        };

        hot.promote(e1, max);
        hot.promote(e2, max);
        assert_eq!(hot.entries.len(), 2);

        hot.promote(e3, max);
        assert_eq!(hot.entries.len(), 2);
        // e2 should be evicted as it has the lowest relevance/eviction score
        assert!(hot.entries.iter().any(|e| e.memory_id == 1));
        assert!(hot.entries.iter().any(|e| e.memory_id == 3));
    }

    #[test]
    fn test_hot_cache_never_evicts_pinned() {
        let mut hot = HotCache::default();
        let max = 1;

        let pinned = HotCacheEntry {
            memory_id: 1,
            content: "pinned".into(),
            relevance_score: 0.1,
            tier: ConfidenceTier::Whisper,
            promoted_at: Utc::now(),
            last_surfaced: Utc::now(),
            hot_streak: 1,
            pinned: true,
            source_agent: None,
        };
        let high = HotCacheEntry {
            memory_id: 2,
            content: "high".into(),
            relevance_score: 0.99,
            tier: ConfidenceTier::Loud,
            promoted_at: Utc::now(),
            last_surfaced: Utc::now(),
            hot_streak: 1,
            pinned: false,
            source_agent: None,
        };

        hot.promote(pinned, max);
        hot.promote(high, max);

        assert_eq!(hot.entries.len(), 1);
        assert_eq!(hot.entries[0].memory_id, 1);
    }

    #[test]
    fn test_cache_persistence_roundtrip() {
        let dir = tempdir().unwrap();
        let nexus_dir = dir.path();

        let mut cache = CognitiveCache::default();
        cache.hot_cache.entries.push(HotCacheEntry {
            memory_id: 1,
            content: "test".into(),
            relevance_score: 0.9,
            tier: ConfidenceTier::Loud,
            promoted_at: Utc::now(),
            last_surfaced: Utc::now(),
            hot_streak: 1,
            pinned: false,
            source_agent: None,
        });

        cache.save(nexus_dir).unwrap();
        let loaded = CognitiveCache::load_or_init(nexus_dir);

        assert_eq!(loaded.hot_cache.entries.len(), 1);
        assert_eq!(loaded.hot_cache.entries[0].content, "test");
    }

    #[test]
    fn test_load_or_init_handles_missing_and_corrupt() {
        let dir = tempdir().unwrap();
        let nexus_dir = dir.path();

        // Missing
        let cache = CognitiveCache::load_or_init(nexus_dir);
        assert_eq!(cache.hot_cache.entries.len(), 0);

        // Corrupt
        let cache_dir = nexus_dir.join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("hot.json"), "invalid json").unwrap();

        let cache = CognitiveCache::load_or_init(nexus_dir);
        assert_eq!(cache.hot_cache.entries.len(), 0);
    }
}
