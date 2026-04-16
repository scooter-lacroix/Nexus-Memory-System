//! Dream cycle orchestration — signal collection, adaptive scheduling,
//! job enqueue, and cognition draining.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

use nexus_core::config::{AgentConfig, CognitionConfig, CognitiveSystemConfig};
use nexus_core::traits::EmbeddingService;
use nexus_core::{CognitiveLevel, Memory, PerspectiveKey};
use nexus_llm::LlmClient;
use nexus_storage::models::EnqueueJobParams;
use nexus_storage::repository::MemoryRepository;
use serde_json::json;

use crate::cognitive_cache::{CognitiveCache, ColdIndexEntry};
use crate::context_builder::build_context_md;
use crate::distill;
use crate::error::AgentError;
use crate::job_processor;
use crate::session_manager::SessionManager;
use crate::util::{flush_metric_samples, stage_metric_sample};
use crate::RuntimeShutdownReason;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

// ── Public request types ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DreamCycleRequest<'a> {
    pub namespace_id: i64,
    pub lease_owner: &'a str,
    pub perspective: Option<&'a PerspectiveKey>,
    pub session_key: Option<&'a str>,
    pub reflect_reason: &'a str,
    pub digest_reason: &'a str,
}

// ── Internal scheduling types ─────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DreamScheduleAction {
    ImmediateBounded,
    DelayedEnqueue,
    DigestOnly,
    Skip,
}

#[derive(Debug, Clone)]
pub(crate) struct DreamSchedulePlan {
    pub action: DreamScheduleAction,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DreamSignals {
    pub raw_event_count: usize,
    pub explicit_count: usize,
    pub derived_count: usize,
    pub contradiction_count: usize,
    pub has_digest_gap: bool,
    /// Total non-raw memories in the session scope.
    pub total_non_raw_count: usize,
    /// Ratio of contradictions to total non-raw (0.0 when no memories).
    pub contradiction_density: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NapResult {
    pub memories_processed: usize,
    pub hot_cache_updated: bool,
    pub elapsed_ms: u64,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamResult {
    pub memories_derived: usize,
    pub connections_found: usize,
    pub hot_cold_reranked: bool,
}

/// Bundled runtime services and configuration for dream-cycle functions.
/// Reduces argument count while keeping the dependency graph explicit.
pub struct DreamServices {
    pub pool: sqlx::SqlitePool,
    pub cognition: CognitionConfig,
    pub agent: AgentConfig,
    pub llm: Arc<dyn LlmClient>,
    pub embeddings: Option<Arc<dyn EmbeddingService>>,
    pub cognitive_system: CognitiveSystemConfig,
}

/// Run a bounded "nap" cycle on session end or idle.
pub async fn run_nap(
    session_id: &str,
    project_root: &Path,
    namespace_id: i64,
    services: &DreamServices,
    timeout: Duration,
) -> Result<NapResult, AgentError> {
    let start = Instant::now();
    let nexus_dir = project_root.join(".nexus");

    let result = tokio::time::timeout(timeout, async {
        // 1. Process raw -> derived for this session
        let processed = drain_cognition_jobs(
            services.pool.clone(),
            namespace_id,
            &services.cognition,
            &services.agent,
            services.llm.clone(),
            services.embeddings.clone(),
            &format!("nap-{}", session_id),
        )
        .await?;

        // 2. Merge session scratch into hot cache
        let mut cache = CognitiveCache::load_or_init(&nexus_dir);
        let session_manager = SessionManager::new(project_root);
        let merged = session_manager.merge_session(
            session_id,
            &mut cache.hot_cache,
            services.cognitive_system.hot_cache_max_entries,
        )?;

        // 3. Update context.md
        let max_context_tokens =
            (200_000.0 * services.cognitive_system.context_allocation_pct) as usize;
        let context_md = build_context_md(&cache.hot_cache, &[], max_context_tokens);
        let context_path = nexus_dir.join("context.md");
        let _ = std::fs::write(context_path, context_md);

        // 4. Save cache
        let _ = cache.save(&nexus_dir);

        Ok::<NapResult, AgentError>(NapResult {
            memories_processed: processed,
            hot_cache_updated: merged > 0,
            elapsed_ms: start.elapsed().as_millis() as u64,
            timed_out: false,
        })
    })
    .await;

    match result {
        Ok(Ok(res)) => Ok(res),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            warn!(
                "Nap timed out after {:?}; leased cognition jobs remain in queue \
                 and will be re-claimed on the next cycle once their lease expires",
                timeout
            );
            Ok(NapResult {
                memories_processed: 0,
                hot_cache_updated: false,
                elapsed_ms: start.elapsed().as_millis() as u64,
                timed_out: true,
            })
        }
    }
}

/// Run a comprehensive "dream" cycle triggered by memory accumulation.
pub async fn run_dream(
    project_root: &Path,
    namespace_id: i64,
    services: &DreamServices,
) -> Result<DreamResult, AgentError> {
    let nexus_dir = project_root.join(".nexus");

    // 1. Run standard dream cycle (includes reflect/digest)
    let processed = drain_cognition_jobs(
        services.pool.clone(),
        namespace_id,
        &services.cognition,
        &services.agent,
        services.llm.clone(),
        services.embeddings.clone(),
        "dream-threshold",
    )
    .await?;

    // 2. Load cache (reranking deferred to deep-dream cycle)
    let cache = CognitiveCache::load_or_init(&nexus_dir);

    // 3. Update context.md
    let max_context_tokens =
        (200_000.0 * services.cognitive_system.context_allocation_pct) as usize;
    let context_md = build_context_md(&cache.hot_cache, &[], max_context_tokens);
    let context_path = nexus_dir.join("context.md");
    let _ = std::fs::write(context_path, context_md);

    // 4. Save cache
    let _ = cache.save(&nexus_dir);

    Ok(DreamResult {
        memories_derived: processed,
        connections_found: 0,
        hot_cold_reranked: false,
    })
}

// ── Top-level dream cycle ─────────────────────────────────────────────

pub async fn run_dream_cycle(
    pool: sqlx::SqlitePool,
    cognition: &CognitionConfig,
    agent: &AgentConfig,
    llm: Arc<dyn LlmClient>,
    embeddings: Option<Arc<dyn EmbeddingService>>,
    request: DreamCycleRequest<'_>,
) -> Result<usize, AgentError> {
    let repo = MemoryRepository::new(pool.clone());
    let total_started = Instant::now();
    let mut metrics = Vec::new();
    let enqueue_started = Instant::now();
    enqueue_dream_jobs(
        &repo,
        request.namespace_id,
        request.perspective,
        request.session_key,
        request.reflect_reason,
        request.digest_reason,
    )
    .await?;
    metrics.push(stage_metric_sample(
        request.namespace_id,
        "cognition.dream.enqueue_ms",
        enqueue_started.elapsed().as_secs_f64() * 1000.0,
        "enqueue",
    ));
    let drain_started = Instant::now();
    let processed = drain_cognition_jobs(
        pool,
        request.namespace_id,
        cognition,
        agent,
        llm,
        embeddings,
        request.lease_owner,
    )
    .await?;
    metrics.push(stage_metric_sample(
        request.namespace_id,
        "cognition.dream.drain_ms",
        drain_started.elapsed().as_secs_f64() * 1000.0,
        "drain",
    ));
    metrics.push(stage_metric_sample(
        request.namespace_id,
        "cognition.dream.total_ms",
        total_started.elapsed().as_secs_f64() * 1000.0,
        "total",
    ));
    flush_metric_samples(&repo, &metrics).await;
    Ok(processed)
}

// ── Cognition draining ────────────────────────────────────────────────

pub async fn drain_cognition_jobs(
    pool: sqlx::SqlitePool,
    namespace_id: i64,
    cognition: &CognitionConfig,
    agent: &AgentConfig,
    llm: Arc<dyn LlmClient>,
    embeddings: Option<Arc<dyn EmbeddingService>>,
    lease_owner: &str,
) -> Result<usize, AgentError> {
    let repo = MemoryRepository::new(pool);
    let mut total_processed = 0usize;

    for _ in 0..3 {
        let mut progressed = 0usize;

        if cognition.activity_distill_enabled {
            progressed += distill::process_activity_distill_jobs(
                &repo,
                namespace_id,
                cognition,
                llm.clone(),
                lease_owner,
            )
            .await?;
        }
        if cognition.derive_enabled {
            progressed += job_processor::process_derive_jobs(
                &repo,
                namespace_id,
                cognition,
                agent,
                llm.clone(),
                embeddings.clone(),
                lease_owner,
            )
            .await?;
        }
        if cognition.reflect_enabled {
            progressed += job_processor::process_reflect_jobs(
                &repo,
                namespace_id,
                cognition,
                agent,
                embeddings.clone(),
                lease_owner,
            )
            .await?;
            progressed += job_processor::process_reflect_namespace_jobs(
                &repo,
                namespace_id,
                cognition,
                agent,
                embeddings.clone(),
                lease_owner,
            )
            .await?;
        }
        if cognition.digest_enabled {
            progressed += job_processor::process_digest_jobs(
                &repo,
                namespace_id,
                cognition,
                agent,
                llm.clone(),
                embeddings.clone(),
                lease_owner,
            )
            .await?;
        }

        total_processed += progressed;
        if progressed == 0 {
            break;
        }
    }

    Ok(total_processed)
}

// ── Dream job enqueue ─────────────────────────────────────────────────

pub async fn enqueue_dream_jobs(
    repo: &MemoryRepository,
    namespace_id: i64,
    perspective: Option<&PerspectiveKey>,
    session_key: Option<&str>,
    reflect_reason: &str,
    digest_reason: &str,
) -> Result<usize, AgentError> {
    let mut queued = 0usize;

    if let Some(perspective) = perspective {
        let perspective_json = serde_json::to_value(perspective)
            .map_err(|error| AgentError::Reflection(error.to_string()))?;
        let payload = json!({
            "reason": reflect_reason,
            "session_key": perspective.session_key,
        });
        if job_processor::enqueue_job_if_absent(
            repo,
            EnqueueJobParams {
                namespace_id,
                job_type: job_processor::REFLECT_PERSPECTIVE_JOB,
                priority: 100,
                perspective: Some(&perspective_json),
                payload: &payload,
            },
        )
        .await?
        {
            queued += 1;
        }
    } else {
        let payload = json!({
            "reason": reflect_reason,
            "session_key": session_key,
        });
        if job_processor::enqueue_job_if_absent(
            repo,
            EnqueueJobParams {
                namespace_id,
                job_type: job_processor::REFLECT_NAMESPACE_JOB,
                priority: 100,
                perspective: None,
                payload: &payload,
            },
        )
        .await?
        {
            queued += 1;
        }
    }

    if let Some(session_key) = session_key {
        let payload = json!({
            "session_key": session_key,
            "reason": digest_reason,
        });
        if job_processor::enqueue_job_if_absent(
            repo,
            EnqueueJobParams {
                namespace_id,
                job_type: job_processor::DIGEST_SESSION_JOB,
                priority: 110,
                perspective: None,
                payload: &payload,
            },
        )
        .await?
        {
            queued += 1;
        }
    }

    Ok(queued)
}

// ── Signal collection ─────────────────────────────────────────────────

pub(crate) async fn collect_dream_signals(
    repo: &MemoryRepository,
    namespace_id: i64,
    session_key: &str,
) -> Result<DreamSignals, AgentError> {
    let memories = repo
        .list_by_session_key(namespace_id, session_key, 512, true)
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;
    let has_digest_gap = repo
        .count_digests(namespace_id, Some(session_key))
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?
        == 0;

    let mut signals = DreamSignals {
        has_digest_gap,
        ..DreamSignals::default()
    };
    for memory in memories
        .iter()
        .filter(|memory| memory_matches_session_key(memory, session_key))
    {
        if is_raw_event(memory) {
            signals.raw_event_count += 1;
        } else {
            signals.total_non_raw_count += 1;

            let level = nexus_core::cognitive_level_from_metadata(&memory.metadata);
            match level {
                CognitiveLevel::Explicit => signals.explicit_count += 1,
                CognitiveLevel::Derived => signals.derived_count += 1,
                CognitiveLevel::Contradiction => signals.contradiction_count += 1,
                _ => {}
            }
            // Also count contradictions from times_contradicted field
            // (avoid double-count: skip if already counted via CognitiveLevel)
            if let Some(cog) = nexus_core::CognitiveMetadata::from_metadata(&memory.metadata) {
                if cog.times_contradicted > 0 && level != CognitiveLevel::Contradiction {
                    signals.contradiction_count += 1;
                }
            }
        }
    }

    if signals.total_non_raw_count > 0 {
        signals.contradiction_density =
            signals.contradiction_count as f32 / signals.total_non_raw_count as f32;
    }

    Ok(signals)
}

fn memory_matches_session_key(memory: &Memory, session_key: &str) -> bool {
    if let Some(cog) = nexus_core::CognitiveMetadata::from_metadata(&memory.metadata) {
        if cog.session_key.as_deref() == Some(session_key) {
            return true;
        }
        if cog.session_keys.iter().any(|k| k == session_key) {
            return true;
        }
    }
    false
}

fn is_raw_event(memory: &Memory) -> bool {
    // A raw event is any memory labeled raw-activity regardless of category,
    // or one whose cognitive level is explicitly Raw.
    memory.labels.iter().any(|l| l == "raw-activity")
        || nexus_core::cognitive_level_from_metadata(&memory.metadata) == CognitiveLevel::Raw
}

// ── Deep Dream & Cross-Project Logic ─────────────────────────────────

use crate::activity_monitor::ActivityMonitor;
use crate::soul::{SoulBuilder, SoulCandidate};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepDreamResult {
    pub soul_updated: bool,
    pub memories_pruned: usize,
    pub cross_project_patterns: usize,
    pub cold_caches_reindexed: usize,
}

/// Run a comprehensive deep dream cycle across all namespaces.
pub async fn run_deep_dream(
    pool: sqlx::SqlitePool,
    cognition: &CognitionConfig,
    agent: &AgentConfig,
    llm: Arc<dyn LlmClient>,
    embeddings: Option<Arc<dyn EmbeddingService>>,
    soul_builder: &SoulBuilder,
    activity_monitor: &mut ActivityMonitor,
) -> Result<DeepDreamResult, AgentError> {
    let repo = MemoryRepository::new(pool.clone());
    let ns_repo = nexus_storage::repository::NamespaceRepository::new(pool.clone());

    // 1. Run standard dream for all namespaces
    if let Ok(namespaces) = ns_repo.list_all().await {
        for ns in namespaces {
            let _ = drain_cognition_jobs(
                pool.clone(),
                ns.id,
                cognition,
                agent,
                llm.clone(),
                embeddings.clone(),
                "deep-dream-cleanup",
            )
            .await;
        }
    }

    // 2. Cross-project pattern extraction
    let mut memories_by_project: HashMap<String, Vec<Memory>> = HashMap::new();
    if let Ok(namespaces) = ns_repo.list_all().await {
        for ns in namespaces {
            let filters = nexus_storage::repository::ListMemoryFilters {
                category: None,
                since: None,
                until: None,
                content_like: None,
                include_raw: false,
                limit: 1000,
                offset: 0,
            };
            if let Ok(memories) = repo.list_filtered(ns.id, filters).await {
                for m in memories {
                    let level = m.level();
                    if level == CognitiveLevel::Derived || level == CognitiveLevel::Explicit {
                        let project_name = m
                            .metadata
                            .get("runtime")
                            .and_then(|r| r.get("project_name"))
                            .and_then(|p| p.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        memories_by_project.entry(project_name).or_default().push(m);
                    }
                }
            }
        }
    }

    let candidates = extract_cross_project_patterns(memories_by_project);
    let total_patterns = candidates.len();

    // 3. Rebuild Soul
    let soul_updated = if !candidates.is_empty() {
        soul_builder.rebuild_soul(&candidates).await.is_ok()
    } else {
        false
    };

    // 4. Prune redundant archived raw-activity memories
    let mut memories_pruned = 0usize;
    let prune_cutoff = chrono::Utc::now() - chrono::Duration::days(30);
    if let Ok(namespaces) = ns_repo.list_all().await {
        for ns in &namespaces {
            if let Ok(candidates) = repo
                .list_archived_raw_cleanup_candidates(ns.id, prune_cutoff, 500)
                .await
            {
                if !candidates.is_empty() {
                    let ids: Vec<i64> = candidates.iter().map(|m| m.id).collect();
                    match repo.delete_batch(&ids).await {
                        Ok(deleted) => memories_pruned += deleted as usize,
                        Err(e) => {
                            warn!(
                                error = %e, count = ids.len(),
                                "Failed to delete archived raw memories; skipping"
                            );
                        }
                    }
                }
            }
        }
    }

    // 5. Cold-cache reindexing across discoverable project roots
    let mut cold_caches_reindexed = 0usize;
    if let Ok(namespaces) = ns_repo.list_all().await {
        // Discover project roots from memory metadata (cwd field)
        let mut project_roots: HashSet<PathBuf> = HashSet::new();
        for ns in &namespaces {
            let filters = nexus_storage::repository::ListMemoryFilters {
                category: None,
                since: Some(chrono::Utc::now() - chrono::Duration::days(90)),
                until: None,
                content_like: None,
                include_raw: true,
                limit: 200,
                offset: 0,
            };
            if let Ok(memories) = repo.list_filtered(ns.id, filters).await {
                for m in &memories {
                    if let Some(cwd) = m
                        .metadata
                        .get("runtime")
                        .and_then(|r| r.get("cwd"))
                        .and_then(|v| v.as_str())
                    {
                        let root = PathBuf::from(cwd);
                        if root.join(".nexus").exists() {
                            project_roots.insert(root);
                        }
                    }
                }
            }
        }

        let reindex_cutoff = chrono::Utc::now() - chrono::Duration::days(7);
        for root in &project_roots {
            let nexus_dir = root.join(".nexus");
            let mut cache = CognitiveCache::load_or_init(&nexus_dir);

            // Collect recent memory IDs across all namespaces for this project
            let mut fresh_entries: Vec<ColdIndexEntry> = Vec::new();
            for ns in &namespaces {
                let filters = nexus_storage::repository::ListMemoryFilters {
                    category: None,
                    since: Some(reindex_cutoff),
                    until: None,
                    content_like: None,
                    include_raw: false,
                    limit: 50,
                    offset: 0,
                };
                if let Ok(memories) = repo.list_filtered(ns.id, filters).await {
                    for m in memories {
                        let cwd_match = m
                            .metadata
                            .get("runtime")
                            .and_then(|r| r.get("cwd"))
                            .and_then(|v| v.as_str())
                            .map(|c| Path::new(c) == root.as_path())
                            .unwrap_or(false);
                        if cwd_match {
                            fresh_entries.push(ColdIndexEntry {
                                memory_id: m.id,
                                project_relevance: 0.7,
                                last_surfaced: Some(m.created_at),
                            });
                        }
                    }
                }
            }

            // Merge: keep existing entries whose memories are still valid,
            // add new entries, update timestamp
            let existing_ids: HashSet<i64> = cache
                .cold_index
                .entries
                .iter()
                .map(|e| e.memory_id)
                .collect();
            for entry in fresh_entries {
                if !existing_ids.contains(&entry.memory_id) {
                    cache.cold_index.entries.push(entry);
                }
            }
            cache.cold_index.last_reindexed = Some(chrono::Utc::now());

            if cache.save(&nexus_dir).is_ok() && !cache.cold_index.entries.is_empty() {
                cold_caches_reindexed += 1;
            }
        }
    }

    // 6. Update monitor
    activity_monitor.last_deep_dream = Some(chrono::Utc::now());
    let _ = activity_monitor.save();

    Ok(DeepDreamResult {
        soul_updated,
        memories_pruned,
        cross_project_patterns: total_patterns,
        cold_caches_reindexed,
    })
}

/// Extract patterns that appear across multiple projects.
pub fn extract_cross_project_patterns(
    memories_by_project: HashMap<String, Vec<Memory>>,
) -> Vec<SoulCandidate> {
    let mut candidates = Vec::new();
    let projects: Vec<&String> = memories_by_project.keys().collect();

    if projects.len() < 2 {
        return Vec::new();
    }

    let mut pattern_map: HashMap<String, (u32, Vec<String>)> = HashMap::new();

    for (project_name, memories) in &memories_by_project {
        for m in memories {
            let normalized = m.content.to_lowercase();
            let entry = pattern_map.entry(normalized).or_insert((0, Vec::new()));
            entry.0 += 1;
            if !entry.1.contains(project_name) {
                entry.1.push(project_name.clone());
            }
        }
    }

    for (content, (count, projects)) in pattern_map {
        if projects.len() >= 2 {
            candidates.push(SoulCandidate {
                content,
                source_project: projects.join(", "),
                observation_count: count,
                category: "TechnicalLearning".into(),
                source_agent: "nexus-dream-engine".into(),
            });
        }
    }

    candidates
}

// ── Adaptive scheduling ───────────────────────────────────────────────

pub(crate) fn choose_dream_schedule(
    _cognition: &CognitionConfig,
    signals: &DreamSignals,
    reason: RuntimeShutdownReason,
) -> DreamSchedulePlan {
    // No signals at all -> skip
    if signals.total_non_raw_count == 0
        && signals.raw_event_count == 0
        && signals.explicit_count == 0
        && signals.derived_count == 0
        && signals.contradiction_count == 0
    {
        return DreamSchedulePlan {
            action: DreamScheduleAction::Skip,
            reason: "no signals",
        };
    }

    // --- Density-based decisions (when density is nonzero) ---
    if signals.contradiction_density > 0.0 {
        // High contradiction density -> immediate regardless of reason
        if signals.contradiction_density > 0.15 {
            return DreamSchedulePlan {
                action: DreamScheduleAction::ImmediateBounded,
                reason: "high contradiction density",
            };
        }

        if reason == RuntimeShutdownReason::SessionEnded && signals.contradiction_density >= 0.10 {
            return DreamSchedulePlan {
                action: DreamScheduleAction::ImmediateBounded,
                reason: "moderate contradiction density at session end",
            };
        }

        // Low density with contradictions: defer to IdleTimeout or DelayedEnqueue
        if reason == RuntimeShutdownReason::IdleTimeout {
            if signals.explicit_count > 0 || signals.derived_count > 0 {
                return DreamSchedulePlan {
                    action: DreamScheduleAction::DelayedEnqueue,
                    reason: "delays idle medium signal sessions",
                };
            }
            return DreamSchedulePlan {
                action: DreamScheduleAction::Skip,
                reason: "idle without signal",
            };
        }

        // Density present but not high enough for immediate -> fall through to
        // count-based or explicit/derived logic below only at session end.
        if reason == RuntimeShutdownReason::SessionEnded
            && (signals.explicit_count > 0 || signals.derived_count > 0)
        {
            return DreamSchedulePlan {
                action: DreamScheduleAction::ImmediateBounded,
                reason: "session end flushes explicit reflection",
            };
        }

        // Fallback for density-present cases: DelayedEnqueue or DigestOnly
        if signals.has_digest_gap && signals.raw_event_count > 0 {
            return DreamSchedulePlan {
                action: DreamScheduleAction::DigestOnly,
                reason: "digest only for light digest gap",
            };
        }
        return DreamSchedulePlan {
            action: DreamScheduleAction::DelayedEnqueue,
            reason: "default_background",
        };
    }

    // --- Count-based decisions (density is zero) ---
    if signals.contradiction_count > 0 {
        return DreamSchedulePlan {
            action: DreamScheduleAction::ImmediateBounded,
            reason: "contradiction detected",
        };
    }

    if reason == RuntimeShutdownReason::SessionEnded
        && (signals.explicit_count > 0 || signals.derived_count > 0)
    {
        return DreamSchedulePlan {
            action: DreamScheduleAction::ImmediateBounded,
            reason: "session end flushes explicit reflection",
        };
    }

    if signals.has_digest_gap && signals.raw_event_count > 0 {
        return DreamSchedulePlan {
            action: DreamScheduleAction::DigestOnly,
            reason: "digest only for light digest gap",
        };
    }

    if reason == RuntimeShutdownReason::IdleTimeout {
        if signals.explicit_count > 0 || signals.derived_count > 0 {
            return DreamSchedulePlan {
                action: DreamScheduleAction::DelayedEnqueue,
                reason: "delays idle medium signal sessions",
            };
        }
        return DreamSchedulePlan {
            action: DreamScheduleAction::Skip,
            reason: "idle without signal",
        };
    }

    DreamSchedulePlan {
        action: DreamScheduleAction::DelayedEnqueue,
        reason: "default_background",
    }
}

pub(crate) async fn compute_adaptive_dream_interval(
    pool: sqlx::SqlitePool,
    namespace_id: i64,
    base_interval_secs: u64,
    cognition: &CognitionConfig,
) -> Duration {
    let repo = MemoryRepository::new(pool);
    let filters = nexus_storage::repository::ListMemoryFilters {
        category: None,
        since: None,
        until: None,
        content_like: None,
        include_raw: true,
        limit: 100,
        offset: 0,
    };

    if let Ok(recent_memories) = repo.list_filtered(namespace_id, filters).await {
        if recent_memories.is_empty() {
            return Duration::from_secs(base_interval_secs);
        }

        let contradiction_count = recent_memories
            .iter()
            .filter(|m| {
                m.metadata
                    .get("cognitive")
                    .and_then(|c| c.get("times_contradicted"))
                    .and_then(|t| t.as_i64())
                    .is_some_and(|t| t > 0)
            })
            .count();

        let density = contradiction_count as f32 / recent_memories.len() as f32;

        let multiplier = if density > 0.10 {
            0.5
        } else if recent_memories.len() < 5 {
            2.0
        } else {
            1.0
        };

        let interval = (base_interval_secs as f32 * multiplier) as u64;
        let interval = interval.clamp(
            cognition.adaptive_dream_min_interval_secs,
            cognition.adaptive_dream_max_interval_secs,
        );

        return Duration::from_secs(interval);
    }

    Duration::from_secs(base_interval_secs)
}
