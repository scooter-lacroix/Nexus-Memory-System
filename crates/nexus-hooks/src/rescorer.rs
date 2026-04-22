//! Mid-session relevance re-scorer for active agent sessions.

use nexus_core::fsutil::atomic_write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, info};

use nexus_core::{cosine_similarity, EmbeddingService, ProjectIdentity};
use nexus_memory_agent::cognitive_cache::{CognitiveCache, ConfidenceTier};
use nexus_memory_agent::context_builder::build_context_md;
use nexus_memory_agent::token_budget::TokenBudget;

/// Tracks session topic drift and triggers re-scoring of the hot cache.
#[derive(Debug)]
pub struct SessionRescorer {
    turns_since_rescore: AtomicU32,
    rescore_interval: u32,
    current_topic_embedding: RwLock<Option<Vec<f32>>>,
    drift_threshold: f32,
    nexus_dir: PathBuf,
}

impl SessionRescorer {
    /// Create a new session rescorer.
    pub fn new(project: ProjectIdentity, rescore_interval: u32, drift_threshold: f32) -> Self {
        let nexus_dir = project.root_dir.join(".nexus");
        Self {
            turns_since_rescore: AtomicU32::new(0),
            rescore_interval: rescore_interval.max(1),
            current_topic_embedding: RwLock::new(None),
            drift_threshold,
            nexus_dir,
        }
    }

    /// Process a new turn. Returns Some(last_similarity) if re-score triggered, None otherwise.
    ///
    /// Note: on_turn uses single embed() for the current turn text (1 embedding).
    /// The batch optimization is in rescore() which embeds all hot cache entries at once.
    pub async fn on_turn(
        &self,
        turn_content: &str,
        embedder: Option<&dyn EmbeddingService>,
    ) -> Option<f32> {
        let turns = self.turns_since_rescore.fetch_add(1, Ordering::SeqCst) + 1;

        // 1. Interval-based trigger
        if turns >= self.rescore_interval {
            debug!("Triggering re-score due to interval ({} turns)", turns);
            self.turns_since_rescore.store(0, Ordering::SeqCst);
            // Update the topic baseline with the current turn so the
            // subsequent rescore ranks against the latest context.
            if let Some(service) = embedder {
                if let Ok(turn_embedding) = service.embed(turn_content).await {
                    let mut topic_lock = self.current_topic_embedding.write().await;
                    *topic_lock = Some(turn_embedding);
                }
            }
            return Some(1.0); // Interval trigger
        }

        // 2. Drift-based trigger
        if let Some(service) = embedder {
            // Compute embedding outside any lock
            if let Ok(turn_embedding) = service.embed(turn_content).await {
                // Read lock to check baseline
                let (should_rescore, similarity) = {
                    let topic_lock = self.current_topic_embedding.read().await;
                    match topic_lock.as_ref() {
                        Some(baseline) => {
                            let similarity = cosine_similarity(baseline, &turn_embedding);
                            (similarity < self.drift_threshold, similarity)
                        }
                        None => (true, 0.0), // No baseline yet
                    }
                }; // read lock dropped here

                if should_rescore {
                    // Write lock only for updating
                    let mut topic_lock = self.current_topic_embedding.write().await;
                    info!(
                        "Topic drift detected (similarity={:.3}). Triggering re-score.",
                        similarity
                    );
                    *topic_lock = Some(turn_embedding);
                    self.turns_since_rescore.store(0, Ordering::SeqCst);
                    return Some(similarity);
                }
            }
        }

        None
    }

    /// Execute the re-scoring pipeline.
    pub async fn rescore(
        &self,
        embedder: Option<&dyn EmbeddingService>,
        agent_type: &str,
    ) -> anyhow::Result<()> {
        let _start = std::time::Instant::now();

        // 1. Load current cache
        let mut cache = CognitiveCache::load_or_init(&self.nexus_dir);
        if cache.hot_cache.entries.is_empty() {
            return Ok(());
        }

        if let Some(service) = embedder {
            let topic = {
                let topic_lock = self.current_topic_embedding.read().await;
                topic_lock.clone()
            };
            if let Some(topic) = topic {
                // Batch-embed all entries for efficiency
                let contents: Vec<String> = cache
                    .hot_cache
                    .entries
                    .iter()
                    .map(|e| e.content.clone())
                    .collect();
                match service.embed_batch(&contents).await {
                    Ok(embeddings) if embeddings.len() == cache.hot_cache.entries.len() => {
                        for (entry, emb) in cache.hot_cache.entries.iter_mut().zip(embeddings) {
                            entry.relevance_score = cosine_similarity(&topic, &emb);
                            entry.tier = ConfidenceTier::from_score(entry.relevance_score);
                        }
                    }
                    Ok(embeddings) => {
                        debug!(
                            "embed_batch cardinality mismatch: got {}, expected {}",
                            embeddings.len(),
                            cache.hot_cache.entries.len()
                        );
                    }
                    Err(e) => {
                        debug!("embed_batch failed during rescore: {e}");
                    }
                }
            }
        }

        // 3. Rebuild context.md
        let config = nexus_core::Config::from_env().unwrap_or_default();
        let window_size = TokenBudget::estimate_window(agent_type) as f32;
        let max_context_tokens =
            (window_size * config.cognitive_system.context_allocation_pct) as usize;
        let context_md = build_context_md(&cache.hot_cache, &[], max_context_tokens);

        // 4. Atomic write
        let context_path = self.nexus_dir.join("context.md");
        atomic_write(&context_path, &context_md)?;

        // 5. Save updated scores to cache
        cache.save(&self.nexus_dir)?;

        debug!("Re-score completed in {:?}", _start.elapsed());
        Ok(())
    }

    /// Returns the configured drift threshold.
    pub fn drift_threshold(&self) -> f32 {
        self.drift_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::ProjectIdentity;
    use tempfile::tempdir;

    #[test]
    fn test_cosine_similarity() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];
        let v3 = vec![0.0, 1.0, 0.0];

        assert!((cosine_similarity(&v1, &v2) - 1.0).abs() < 1e-6);
        assert!((cosine_similarity(&v1, &v3) - 0.0).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[tokio::test]
    async fn test_rescorer_interval_trigger() {
        let dir = tempdir().unwrap();
        let project = ProjectIdentity {
            root_dir: dir.path().to_path_buf(),
            git_remote: None,
            display_name: "test".into(),
        };
        let rescorer = SessionRescorer::new(project, 3, 0.7);

        assert!(rescorer.on_turn("t1", None).await.is_none());
        assert!(rescorer.on_turn("t2", None).await.is_none());
        assert!(rescorer.on_turn("t3", None).await.is_some()); // Hits interval
        assert!(rescorer.on_turn("t4", None).await.is_none()); // Reset
    }
}
