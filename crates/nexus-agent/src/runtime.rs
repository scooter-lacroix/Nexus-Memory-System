//! Session-scoped runtime controller for hook-driven cognition.
//!
//! This module owns the `RuntimeController` lifecycle (start / shutdown),
//! the embedding service factory, and the LLM client fallback.  All heavy
//! cognition logic is delegated to the sibling modules:
//!
//! - [`dream_cycle`]     — dream-cycle orchestration, signal collection, scheduling
//! - [`job_processor`]   — derive / reflect / digest job handlers
//! - [`distill`]         — activity distillation pipeline (LLM summarisation)
//! - [`runtime_state`]   — state persistence, session-key derivation, helper types

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::traits::EmbeddingService;
use nexus_core::PerspectiveKey;
use nexus_llm::{create_client_auto_with_fallback, GenerateParams, GenerateResponse, LlmClient};
use nexus_storage::repository::{MemoryRepository, NamespaceRepository};
use nexus_storage::StorageManager;
use tracing::{debug, info, warn};

use crate::dream_cycle::{self, DreamCycleRequest, DreamScheduleAction};
use crate::error::AgentError;
use crate::job_processor;
use crate::runtime_state::{self, RuntimeState};

// ── Re-exports (public API stability) ─────────────────────────────────
pub use crate::runtime_state::{derive_session_key, RuntimeMode, RuntimeShutdownReason};

// ── RuntimeController ─────────────────────────────────────────────────

pub struct RuntimeController {
    cognition: CognitionConfig,
    agent: AgentConfig,
    embeddings: Option<Arc<dyn EmbeddingService>>,
}

impl RuntimeController {
    pub fn new(
        cognition: CognitionConfig,
        agent: AgentConfig,
        embeddings: Option<Arc<dyn EmbeddingService>>,
    ) -> Self {
        Self {
            cognition,
            agent,
            embeddings,
        }
    }

    pub async fn ensure_started(
        &self,
        agent_type: &str,
        session_key: Option<&str>,
        cwd: Option<&str>,
        mode: RuntimeMode,
    ) -> Result<(), AgentError> {
        if !self.cognition.auto_runtime_enabled {
            return Ok(());
        }

        let config =
            nexus_core::Config::from_env().map_err(|e| AgentError::Config(e.to_string()))?;
        let mut storage = StorageManager::from_url(&config.database_url())
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        storage
            .initialize()
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;

        let session_key = derive_session_key(agent_type, session_key, cwd);
        let path = runtime_state::state_file_path(agent_type, &session_key)?;
        let now = Utc::now();

        let mut state = runtime_state::read_runtime_state(&path)?.unwrap_or(RuntimeState {
            agent_type: agent_type.to_string(),
            session_key: session_key.clone(),
            mode: mode.into(),
            started_at: now,
            updated_at: now,
        });

        if (now - state.updated_at).num_seconds() > self.cognition.runtime_idle_timeout_secs as i64
        {
            state.started_at = now;
        }
        state.updated_at = now;
        state.mode = mode.into();

        runtime_state::write_runtime_state(&path, &state)?;
        let llm = runtime_llm_client();
        let processed = dream_cycle::drain_cognition_jobs(
            storage.pool().clone(),
            runtime_state::namespace_id_for(agent_type, &storage).await?,
            &self.cognition,
            &self.agent,
            llm,
            self.embeddings.clone(),
            &format!("runtime-start-{agent_type}-{}", state.session_key),
        )
        .await?;
        debug!(
            agent_type,
            processed_jobs = processed,
            "Runtime startup cognition drain complete"
        );
        debug!(agent_type, session_key = %state.session_key, "Runtime state ensured");
        Ok(())
    }

    pub async fn flush_and_shutdown(
        &self,
        agent_type: &str,
        session_key: Option<&str>,
        cwd: Option<&str>,
        reason: RuntimeShutdownReason,
    ) -> Result<(), AgentError> {
        if !self.cognition.auto_runtime_enabled {
            return Ok(());
        }

        let config =
            nexus_core::Config::from_env().map_err(|e| AgentError::Config(e.to_string()))?;
        let mut storage = StorageManager::from_url(&config.database_url())
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        storage
            .initialize()
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;

        let namespace_repo = NamespaceRepository::new(storage.pool().clone());
        let namespace = namespace_repo
            .get_or_create(agent_type, agent_type)
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;

        let memory_repo = MemoryRepository::new(storage.pool().clone());
        let derived_session_key = derive_session_key(agent_type, session_key, cwd);
        runtime_state::store_runtime_marker(
            &memory_repo,
            namespace.id,
            runtime_state::RuntimeMarker {
                agent_type,
                session_key,
                cwd,
                event: "runtime_session_end",
                detail: runtime_state::runtime_reason_label(reason),
                agent_namespace: self.agent.namespace.as_str(),
            },
        )
        .await?;

        let llm = runtime_llm_client();
        let processed = dream_cycle::drain_cognition_jobs(
            storage.pool().clone(),
            namespace.id,
            &self.cognition,
            &self.agent,
            llm.clone(),
            self.embeddings.clone(),
            &format!("runtime-stop-{agent_type}-{derived_session_key}"),
        )
        .await?;
        debug!(
            agent_type,
            processed_jobs = processed,
            "Runtime shutdown cognition drain complete"
        );

        if self.cognition.dream_on_session_end {
            let lease_owner = format!("runtime-dream-{agent_type}-{derived_session_key}");
            let shutdown_perspective = PerspectiveKey {
                observer: agent_type.to_string(),
                subject: agent_type.to_string(),
                session_key: Some(derived_session_key.clone()),
            };
            let signals = dream_cycle::collect_dream_signals(
                &memory_repo,
                namespace.id,
                &derived_session_key,
            )
            .await?;
            let plan = dream_cycle::choose_dream_schedule(&self.cognition, &signals, reason);
            debug!(
                agent_type,
                session_key = ?session_key,
                action = ?plan.action,
                plan_reason = plan.reason,
                raw_event_count = signals.raw_event_count,
                contradiction_count = signals.contradiction_count,
                contradiction_density = signals.contradiction_density,
                total_non_raw = signals.total_non_raw_count,
                has_digest_gap = signals.has_digest_gap,
                "Selected adaptive dream schedule"
            );
            match plan.action {
                DreamScheduleAction::ImmediateBounded => match tokio::time::timeout(
                    std::time::Duration::from_secs(self.cognition.session_end_dream_timeout_secs),
                    dream_cycle::run_dream_cycle(
                        storage.pool().clone(),
                        &self.cognition,
                        &self.agent,
                        llm,
                        self.embeddings.clone(),
                        DreamCycleRequest {
                            namespace_id: namespace.id,
                            lease_owner: &lease_owner,
                            perspective: Some(&shutdown_perspective),
                            session_key: Some(derived_session_key.as_str()),
                            reflect_reason: "session_end_dream",
                            digest_reason: "session_end",
                        },
                    ),
                )
                .await
                {
                    Ok(Ok(processed)) if processed > 0 => {
                        info!(
                            agent_type,
                            session_key = ?session_key,
                            processed,
                            plan_reason = plan.reason,
                            "Dream pass completed through cognition jobs"
                        );
                    }
                    Ok(Ok(_)) => {
                        debug!(
                            agent_type,
                            session_key = ?session_key,
                            plan_reason = plan.reason,
                            "Dream pass skipped"
                        );
                    }
                    Ok(Err(error)) => {
                        warn!(%error, agent_type, session_key = ?session_key, "Dream pass failed");
                    }
                    Err(_) => {
                        warn!(
                            agent_type,
                            session_key = ?session_key,
                            timeout_secs = self.cognition.session_end_dream_timeout_secs,
                            "Dream pass timed out during shutdown"
                        );
                    }
                },
                DreamScheduleAction::DelayedEnqueue => {
                    let queued = dream_cycle::enqueue_dream_jobs(
                        &memory_repo,
                        namespace.id,
                        Some(&shutdown_perspective),
                        Some(derived_session_key.as_str()),
                        "session_end_delayed_dream",
                        "session_end",
                    )
                    .await?;
                    debug!(
                        agent_type,
                        session_key = ?session_key,
                        queued,
                        plan_reason = plan.reason,
                        "Queued delayed dream jobs without immediate drain"
                    );
                }
                DreamScheduleAction::DigestOnly => {
                    let queued = job_processor::enqueue_digest_job_if_absent(
                        &memory_repo,
                        namespace.id,
                        derived_session_key.as_str(),
                        "session_end_digest_only",
                    )
                    .await?;
                    debug!(
                        agent_type,
                        session_key = ?session_key,
                        queued,
                        plan_reason = plan.reason,
                        "Queued digest-only shutdown work"
                    );
                    if queued && matches!(reason, RuntimeShutdownReason::SessionEnded) {
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(
                                self.cognition.session_end_dream_timeout_secs,
                            ),
                            dream_cycle::drain_cognition_jobs(
                                storage.pool().clone(),
                                namespace.id,
                                &self.cognition,
                                &self.agent,
                                llm.clone(),
                                self.embeddings.clone(),
                                &format!("runtime-finalize-{agent_type}-{derived_session_key}"),
                            ),
                        )
                        .await
                        {
                            Ok(Ok(processed)) => {
                                debug!(
                                    agent_type,
                                    session_key = ?session_key,
                                    processed,
                                    plan_reason = plan.reason,
                                    "Drained digest-only shutdown work before runtime teardown"
                                );
                            }
                            Ok(Err(error)) => {
                                warn!(
                                    %error,
                                    agent_type,
                                    session_key = ?session_key,
                                    "Digest-only shutdown drain failed"
                                );
                            }
                            Err(_) => {
                                warn!(
                                    agent_type,
                                    session_key = ?session_key,
                                    timeout_secs = self.cognition.session_end_dream_timeout_secs,
                                    "Digest-only shutdown drain timed out"
                                );
                            }
                        }
                    }
                }
                DreamScheduleAction::Skip => {
                    debug!(
                        agent_type,
                        session_key = ?session_key,
                        plan_reason = plan.reason,
                        "Skipped shutdown dream work after adaptive planning"
                    );
                }
            }
        }

        let path = runtime_state::state_file_path(agent_type, &derived_session_key)?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }

        Ok(())
    }

    pub fn state_root() -> std::path::PathBuf {
        runtime_state::state_root()
    }
}

// ── Embedding service factory ─────────────────────────────────────────

/// Create an embedding service when `config.embedding.enabled` is true.
///
/// Returns `None` when embeddings are disabled or the service fails to
/// initialise — this preserves graceful degradation throughout the
/// cognition pipeline.
pub async fn create_embedding_service(
    config: &nexus_core::Config,
) -> Option<Arc<dyn EmbeddingService>> {
    if !config.embedding.enabled {
        return None;
    }
    match nexus_embeddings::create_service(config).await {
        Ok(Some(service)) => Some(service),
        Ok(None) => None,
        Err(error) => {
            warn!(
                %error,
                "Failed to initialize embedding service; configured LLM features remain available and cognition will run without semantic embeddings. Configure a remote embedding provider, local OpenAI-compatible runtime, or set NEXUS_EMBEDDINGS_ENABLED=false"
            );
            None
        }
    }
}

// ── LLM client fallback ──────────────────────────────────────────────

fn runtime_llm_client() -> Arc<dyn LlmClient> {
    match create_client_auto_with_fallback() {
        Ok(client) => client,
        Err(error) => {
            warn!(%error, "LLM unavailable, using deterministic cognition fallbacks");
            Arc::new(UnavailableLlmClient {
                message: error.to_string(),
            })
        }
    }
}

struct UnavailableLlmClient {
    message: String,
}

#[async_trait]
impl LlmClient for UnavailableLlmClient {
    async fn generate(&self, _params: GenerateParams) -> nexus_llm::Result<GenerateResponse> {
        Err(nexus_llm::LlmError::InvalidJsonResponse(
            self.message.clone(),
        ))
    }

    fn provider_name(&self) -> String {
        "unavailable".to_string()
    }

    fn model_name(&self) -> String {
        "deterministic-fallback".to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dream_cycle::{
        choose_dream_schedule, collect_dream_signals, enqueue_dream_jobs, DreamScheduleAction,
        DreamSignals,
    };
    use crate::job_processor::{
        digest_job_is_forced, process_digest_jobs, DIGEST_SESSION_JOB, REFLECT_NAMESPACE_JOB,
        REFLECT_PERSPECTIVE_JOB,
    };
    use crate::runtime_state::{derive_session_key, sanitize_component, RuntimeShutdownReason};
    use nexus_core::config::{AgentConfig, CognitionConfig};
    use nexus_core::{CognitiveLevel, MemoryCategory};
    use nexus_storage::models::memory_job_status;
    use nexus_storage::repository::{MemoryRepository, NamespaceRepository, StoreMemoryParams};
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;

    #[tokio::test]
    async fn run_dream_cycle_processes_namespace_jobs() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        nexus_storage::migrations::run_migrations(&pool)
            .await
            .unwrap();

        let namespace_repo = NamespaceRepository::new(pool.clone());
        let namespace = namespace_repo
            .get_or_create("runtime-dream-test", "runtime-dream-test")
            .await
            .unwrap();
        let repo = MemoryRepository::new(pool.clone());

        for content in ["feature enabled", "feature not enabled"] {
            repo.store(StoreMemoryParams {
                namespace_id: namespace.id,
                content,
                category: &MemoryCategory::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &json!({
                    "cognitive": {
                        "level": "explicit",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "generated_by": "test"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }

        let processed = dream_cycle::run_dream_cycle(
            pool.clone(),
            &CognitionConfig::default(),
            &AgentConfig::default(),
            Arc::new(UnavailableLlmClient {
                message: "offline".to_string(),
            }),
            None,
            DreamCycleRequest {
                namespace_id: namespace.id,
                lease_owner: "test-owner",
                perspective: None,
                session_key: None,
                reflect_reason: "namespace_dream",
                digest_reason: "dream_digest",
            },
        )
        .await
        .unwrap();

        assert!(processed >= 1);
        assert_eq!(
            repo.get_by_cognitive_level(namespace.id, CognitiveLevel::Contradiction, 10)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(repo
            .list_jobs(
                namespace.id,
                Some(REFLECT_NAMESPACE_JOB),
                Some("pending"),
                10,
                0
            )
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn process_digest_jobs_skips_below_rollover_threshold() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        nexus_storage::migrations::run_migrations(&pool)
            .await
            .unwrap();

        let namespace_repo = NamespaceRepository::new(pool.clone());
        let namespace = namespace_repo
            .get_or_create("runtime-digest-skip", "runtime-digest-skip")
            .await
            .unwrap();
        let repo = MemoryRepository::new(pool.clone());

        let source = repo
            .store(StoreMemoryParams {
                namespace_id: namespace.id,
                content: "Small explicit update.",
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({
                    "cognitive": {
                        "level": "explicit",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "digest-skip-session"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let prior_digest = repo
            .store(StoreMemoryParams {
                namespace_id: namespace.id,
                content: "Prior digest",
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({
                    "cognitive": {
                        "level": "summary_short",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "digest-skip-session"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        repo.store_digest(nexus_storage::repository::StoreDigestParams {
            namespace_id: namespace.id,
            session_key: "digest-skip-session",
            digest_kind: "short",
            memory_id: prior_digest.id,
            start_memory_id: Some(source.id),
            end_memory_id: Some(source.id),
            token_count: 12,
        })
        .await
        .unwrap();

        let follow_up = repo
            .store(StoreMemoryParams {
                namespace_id: namespace.id,
                content: "Tiny follow-up.",
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({
                    "cognitive": {
                        "level": "explicit",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "digest-skip-session"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        repo.enqueue_job(nexus_storage::EnqueueJobParams {
            namespace_id: namespace.id,
            job_type: DIGEST_SESSION_JOB,
            priority: 90,
            perspective: None,
            payload: &serde_json::json!({
                "session_key": "digest-skip-session",
                "source_memory_id": follow_up.id
            }),
        })
        .await
        .unwrap();

        assert_eq!(
            repo.list_jobs(
                namespace.id,
                Some(DIGEST_SESSION_JOB),
                Some("pending"),
                10,
                0
            )
            .await
            .unwrap()
            .len(),
            1
        );

        let processed = process_digest_jobs(
            &repo,
            namespace.id,
            &CognitionConfig::default(),
            &AgentConfig::default(),
            Arc::new(UnavailableLlmClient {
                message: "offline".to_string(),
            }),
            None,
            "digest-skip",
        )
        .await
        .unwrap();

        assert_eq!(processed, 1);
        let latest = repo
            .latest_digest_for_session(namespace.id, "digest-skip-session", "short")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.id, prior_digest.id);
        assert_eq!(
            repo.count_digests(namespace.id, Some("digest-skip-session"))
                .await
                .unwrap(),
            1
        );
    }

    #[test]
    fn digest_job_force_reason_matches_expected_inputs() {
        assert!(digest_job_is_forced(Some("dream_digest")));
        assert!(digest_job_is_forced(Some("session_end")));
        assert!(digest_job_is_forced(Some("manual_digest")));
        assert!(digest_job_is_forced(Some("manual_rebuild")));
        assert!(!digest_job_is_forced(Some("derive_follow_up")));
        assert!(!digest_job_is_forced(None));
    }

    #[test]
    fn sanitize_component_replaces_unsafe_chars() {
        assert_eq!(sanitize_component("claude/code:1"), "claude_code_1");
    }

    #[test]
    fn derive_session_key_prefers_explicit_key() {
        assert_eq!(
            derive_session_key("claude-code", Some("abc"), Some("/tmp/project")),
            "abc"
        );
    }

    #[test]
    fn derive_session_key_falls_back_to_stable_hash() {
        let first = derive_session_key("claude-code", None, Some("/tmp/project"));
        let second = derive_session_key("claude-code", Some(""), Some("/tmp/project"));
        assert_eq!(first, second);
        assert!(first.starts_with("derived-"));
    }

    #[test]
    fn choose_dream_schedule_immediate_for_contradictions() {
        let plan = choose_dream_schedule(
            &CognitionConfig::default(),
            &DreamSignals {
                contradiction_count: 1,
                raw_event_count: 1,
                ..DreamSignals::default()
            },
            RuntimeShutdownReason::SessionEnded,
        );

        assert_eq!(plan.action, DreamScheduleAction::ImmediateBounded);
    }

    #[test]
    fn choose_dream_schedule_digest_only_for_light_digest_gap() {
        let config = CognitionConfig::default();
        let plan = choose_dream_schedule(
            &config,
            &DreamSignals {
                raw_event_count: 1,
                has_digest_gap: true,
                ..DreamSignals::default()
            },
            RuntimeShutdownReason::SessionEnded,
        );

        assert_eq!(plan.action, DreamScheduleAction::DigestOnly);
    }

    #[test]
    fn choose_dream_schedule_delays_idle_medium_signal_sessions() {
        let plan = choose_dream_schedule(
            &CognitionConfig::default(),
            &DreamSignals {
                raw_event_count: 2,
                explicit_count: 2,
                derived_count: 0,
                ..DreamSignals::default()
            },
            RuntimeShutdownReason::IdleTimeout,
        );

        assert_eq!(plan.action, DreamScheduleAction::DelayedEnqueue);
    }

    #[test]
    fn choose_dream_schedule_session_end_flushes_explicit_reflection() {
        let plan = choose_dream_schedule(
            &CognitionConfig::default(),
            &DreamSignals {
                explicit_count: 2,
                has_digest_gap: true,
                ..DreamSignals::default()
            },
            RuntimeShutdownReason::SessionEnded,
        );

        assert_eq!(plan.action, DreamScheduleAction::ImmediateBounded);
    }

    #[tokio::test]
    async fn collect_dream_signals_counts_session_signal_types() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        nexus_storage::migrations::run_migrations(&pool)
            .await
            .unwrap();

        let namespace_repo = NamespaceRepository::new(pool.clone());
        let namespace = namespace_repo
            .get_or_create("runtime-dream-signals", "runtime-dream-signals")
            .await
            .unwrap();
        let repo = MemoryRepository::new(pool.clone());

        for (content, labels, metadata) in [
            (
                "tool event",
                vec!["raw-activity".to_string()],
                json!({
                    "raw_activity": true,
                    "cognitive": {
                        "level": "raw",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "signals-session"
                    }
                }),
            ),
            (
                "explicit note",
                vec![],
                json!({
                    "cognitive": {
                        "level": "explicit",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "signals-session"
                    }
                }),
            ),
            (
                "derived insight",
                vec![],
                json!({
                    "cognitive": {
                        "level": "derived",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "signals-session"
                    }
                }),
            ),
            (
                "contradiction record",
                vec![],
                json!({
                    "cognitive": {
                        "level": "contradiction",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "signals-session"
                    }
                }),
            ),
        ] {
            repo.store(StoreMemoryParams {
                namespace_id: namespace.id,
                content,
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &labels,
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }

        let signals = collect_dream_signals(&repo, namespace.id, "signals-session")
            .await
            .unwrap();

        assert_eq!(signals.raw_event_count, 1);
        assert_eq!(signals.explicit_count, 1);
        assert_eq!(signals.derived_count, 1);
        assert_eq!(signals.contradiction_count, 1);
        assert!(signals.has_digest_gap);
    }

    #[tokio::test]
    async fn enqueue_dream_jobs_coalesces_session_scoped_shutdown_work() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        nexus_storage::migrations::run_migrations(&pool)
            .await
            .unwrap();

        let namespace_repo = NamespaceRepository::new(pool.clone());
        let namespace = namespace_repo
            .get_or_create("runtime-dream-dedupe", "runtime-dream-dedupe")
            .await
            .unwrap();
        let repo = MemoryRepository::new(pool.clone());
        let perspective = nexus_core::PerspectiveKey {
            observer: "claude-code".to_string(),
            subject: "claude-code".to_string(),
            session_key: Some("session-123".to_string()),
        };

        let first = enqueue_dream_jobs(
            &repo,
            namespace.id,
            Some(&perspective),
            Some("session-123"),
            "session_end_dream",
            "session_end",
        )
        .await
        .unwrap();
        let second = enqueue_dream_jobs(
            &repo,
            namespace.id,
            Some(&perspective),
            Some("session-123"),
            "session_end_dream",
            "session_end",
        )
        .await
        .unwrap();

        assert_eq!(first, 2);
        assert_eq!(second, 0);
        assert_eq!(
            repo.list_jobs(
                namespace.id,
                Some(REFLECT_PERSPECTIVE_JOB),
                Some(memory_job_status::PENDING),
                10,
                0
            )
            .await
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            repo.list_jobs(
                namespace.id,
                Some(REFLECT_NAMESPACE_JOB),
                Some(memory_job_status::PENDING),
                10,
                0
            )
            .await
            .unwrap()
            .len(),
            0
        );
        assert_eq!(
            repo.list_jobs(
                namespace.id,
                Some(DIGEST_SESSION_JOB),
                Some(memory_job_status::PENDING),
                10,
                0
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }

    // ---- Adaptive dream scheduling tests (Phase 16) ----

    #[test]
    fn test_choose_dream_schedule_uses_contradiction_density() {
        let cognition = CognitionConfig::default();

        // No signals → skip.
        let plan = choose_dream_schedule(
            &cognition,
            &DreamSignals::default(),
            RuntimeShutdownReason::SessionEnded,
        );
        assert_eq!(plan.action, DreamScheduleAction::Skip);

        // High contradiction density → immediate.
        let high_density = DreamSignals {
            total_non_raw_count: 10,
            contradiction_count: 3,
            contradiction_density: 0.30,
            ..DreamSignals::default()
        };
        let plan = choose_dream_schedule(
            &cognition,
            &high_density,
            RuntimeShutdownReason::SessionEnded,
        );
        assert_eq!(plan.action, DreamScheduleAction::ImmediateBounded);
        assert!(plan.reason.contains("high contradiction density"));

        // Moderate contradiction density at session end → immediate.
        let moderate_density = DreamSignals {
            total_non_raw_count: 20,
            contradiction_count: 2,
            contradiction_density: 0.10,
            explicit_count: 5,
            ..DreamSignals::default()
        };
        let plan = choose_dream_schedule(
            &cognition,
            &moderate_density,
            RuntimeShutdownReason::SessionEnded,
        );
        assert_eq!(plan.action, DreamScheduleAction::ImmediateBounded);
        assert!(plan.reason.contains("moderate contradiction density"));

        // Moderate contradiction density at idle timeout → not immediate.
        let plan = choose_dream_schedule(
            &cognition,
            &moderate_density,
            RuntimeShutdownReason::IdleTimeout,
        );
        assert_ne!(plan.action, DreamScheduleAction::ImmediateBounded);
    }

    #[test]
    fn test_dream_signals_computes_contradiction_density() {
        let signals = DreamSignals {
            total_non_raw_count: 20,
            contradiction_count: 4,
            contradiction_density: 4.0 / 20.0,
            ..DreamSignals::default()
        };
        assert!((signals.contradiction_density - 0.20).abs() < f32::EPSILON);

        let empty = DreamSignals::default();
        assert!((empty.contradiction_density).abs() < f32::EPSILON);
    }
}
