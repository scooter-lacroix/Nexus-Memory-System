//! Mid-session relevance re-scorer for active agent sessions.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, info};

use nexus_core::{EmbeddingService, ProjectIdentity};
use nexus_memory_agent::cognitive_cache::{CognitiveCache, ConfidenceTier};
use nexus_memory_agent::context_builder::build_context_md;

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
            rescore_interval,
            current_topic_embedding: RwLock::new(None),
            drift_threshold,
            nexus_dir,
        }
    }

    /// Process a new turn. Returns true if a re-score was triggered.
    pub async fn on_turn(
        &self,
        turn_content: &str,
        embedder: Option<&dyn EmbeddingService>,
    ) -> bool {
        let turns = self.turns_since_rescore.fetch_add(1, Ordering::SeqCst) + 1;

        // 1. Interval-based trigger
        if turns >= self.rescore_interval {
            debug!("Triggering re-score due to interval ({} turns)", turns);
            self.turns_since_rescore.store(0, Ordering::SeqCst);
            return true;
        }

        // 2. Drift-based trigger
        if let Some(service) = embedder {
            if let Ok(turn_embedding) = service.embed(turn_content).await {
                let mut topic_lock = self.current_topic_embedding.write().await;
                if let Some(baseline) = topic_lock.as_ref() {
                    let similarity = cosine_similarity(baseline, &turn_embedding);
                    if similarity < self.drift_threshold {
                        info!(
                            "Topic drift detected (similarity: {:.2}). Triggering re-score.",
                            similarity
                        );
                        *topic_lock = Some(turn_embedding);
                        self.turns_since_rescore.store(0, Ordering::SeqCst);
                        return true;
                    }
                } else {
                    // Initialize baseline on first turn
                    *topic_lock = Some(turn_embedding);
                }
            }
        }

        false
    }

    /// Execute the re-scoring pipeline.
    pub async fn rescore(&self, embedder: Option<&dyn EmbeddingService>) -> anyhow::Result<()> {
        let _start = std::time::Instant::now();

        // 1. Load current cache
        let mut cache = CognitiveCache::load_or_init(&self.nexus_dir);
        if cache.hot_cache.entries.is_empty() {
            return Ok(());
        }

        // 2. Re-score entries against current topic
        if let Some(service) = embedder {
            let topic_lock = self.current_topic_embedding.read().await;
            if let Some(topic) = topic_lock.as_ref() {
                for entry in cache.hot_cache.entries.iter_mut() {
                    // In a production environment, we'd store embeddings in the HotCacheEntry
                    // or re-fetch from DB. For now, we use a best-effort approach.
                    // If we can't get an embedding for the entry content, we leave the score.
                    if let Ok(entry_emb) = service.embed(&entry.content).await {
                        entry.relevance_score = cosine_similarity(topic, &entry_emb);
                        entry.tier = ConfidenceTier::from_score(entry.relevance_score);
                    }
                }
            }
        }

        // 3. Rebuild context.md
        let config = nexus_core::Config::from_env().unwrap_or_default();
        let max_context_tokens =
            (200_000.0 * config.cognitive_system.context_allocation_pct) as usize;
        let context_md = build_context_md(&cache.hot_cache, &[], max_context_tokens);

        // 4. Atomic write
        let context_path = self.nexus_dir.join("context.md");
        let tmp_path = context_path.with_extension("tmp");

        std::fs::write(&tmp_path, context_md)?;
        std::fs::rename(&tmp_path, &context_path)?;

        // 5. Save updated scores to cache
        let _ = cache.save(&self.nexus_dir);

        debug!("Re-score completed in {:?}", _start.elapsed());
        Ok(())
    }
}

/// Standalone cosine similarity helper.
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

        assert!(!rescorer.on_turn("t1", None).await);
        assert!(!rescorer.on_turn("t2", None).await);
        assert!(rescorer.on_turn("t3", None).await); // Hits interval
        assert!(!rescorer.on_turn("t4", None).await); // Reset
    }
}
