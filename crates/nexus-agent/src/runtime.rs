//! Session-scoped runtime controller for hook-driven cognition.

use std::collections::{BTreeSet, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::traits::EmbeddingService;
use nexus_core::{
    infer_perspective, CognitiveLevel, CognitiveMetadata, Memory, MemoryCategory,
    MemoryLaneCognitiveType, MemoryLaneType, PerspectiveKey, PerspectiveSource,
};
use nexus_llm::{
    create_client_auto_with_fallback, ChatMessage, GenerateParams, GenerateResponse, LlmClient,
    LlmClientJson,
};
use nexus_storage::models::{memory_job_status, EnqueueJobParams, MemoryJobRow};
use nexus_storage::repository::{MemoryRepository, NamespaceRepository, StoreMemoryParams};
use nexus_storage::StorageManager;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::derive::DeriveService;
use crate::digest::DigestService;
use crate::error::AgentError;
use crate::reflect::ReflectService;
use crate::util::{flush_metric_samples, stage_metric_sample};

const DERIVE_MEMORY_JOB: &str = "derive_memory";
const ACTIVITY_DISTILL_JOB: &str = "activity_distill";
const REFLECT_NAMESPACE_JOB: &str = "reflect_namespace";
const REFLECT_PERSPECTIVE_JOB: &str = "reflect_perspective";
const DIGEST_SESSION_JOB: &str = "digest_session";
const ACTIVITY_DISTILL_SYSTEM_PROMPT: &str = r#"You are distilling a batch of raw agent hook events into a meaningful session summary.

Given a set of raw hook events (JSON with timestamps, tool names, CWD, session IDs), produce a structured summary of what happened in the session.

Focus on:
- What the user/agent was working on (project, directory, task)
- Which tools were used and how often
- Key actions taken (tests run, files edited, commands executed)
- Any patterns (repeated test runs, debugging cycles, etc.)

Return strict JSON with these fields:
- summary: A 1-3 sentence human-readable summary of the session
- category: One of "session", "context", "facts"
- labels: 2-5 descriptive labels
- key_activities: List of notable activities
- files_touched: List of files/directories mentioned
- tools_used: List of unique tools used
- decisions_made: Any decisions evident from the event sequence

Return strict JSON only. No markdown fences."#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    SessionScoped,
    Persistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeShutdownReason {
    SessionEnded,
    IdleTimeout,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeState {
    agent_type: String,
    session_key: String,
    mode: RuntimeModeSerde,
    started_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct RuntimeMarker<'a> {
    agent_type: &'a str,
    session_key: Option<&'a str>,
    cwd: Option<&'a str>,
    event: &'a str,
    detail: &'a str,
    agent_namespace: &'a str,
}

#[derive(Debug, Clone)]
pub struct DreamCycleRequest<'a> {
    pub namespace_id: i64,
    pub lease_owner: &'a str,
    pub perspective: Option<&'a PerspectiveKey>,
    pub session_key: Option<&'a str>,
    pub reflect_reason: &'a str,
    pub digest_reason: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DreamScheduleAction {
    ImmediateBounded,
    DelayedEnqueue,
    DigestOnly,
    Skip,
}

#[derive(Debug, Clone)]
struct DreamSchedulePlan {
    action: DreamScheduleAction,
    reason: &'static str,
}

#[derive(Debug, Clone, Default)]
struct DreamSignals {
    raw_event_count: usize,
    explicit_count: usize,
    derived_count: usize,
    contradiction_count: usize,
    has_digest_gap: bool,
    /// Total non-raw memories in the session scope.
    total_non_raw_count: usize,
    /// Ratio of contradictions to total non-raw (0.0 when no memories).
    contradiction_density: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RuntimeModeSerde {
    SessionScoped,
    Persistent,
}

impl From<RuntimeMode> for RuntimeModeSerde {
    fn from(value: RuntimeMode) -> Self {
        match value {
            RuntimeMode::SessionScoped => Self::SessionScoped,
            RuntimeMode::Persistent => Self::Persistent,
        }
    }
}

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
        let path = self.state_file_path(agent_type, &session_key)?;
        let now = Utc::now();

        let mut state = read_runtime_state(&path)?.unwrap_or(RuntimeState {
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

        write_runtime_state(&path, &state)?;
        let llm = runtime_llm_client();
        let processed = drain_cognition_jobs(
            storage.pool().clone(),
            namespace_id_for(agent_type, &storage).await?,
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
        store_runtime_marker(
            &memory_repo,
            namespace.id,
            RuntimeMarker {
                agent_type,
                session_key,
                cwd,
                event: "runtime_session_end",
                detail: runtime_reason_label(reason),
                agent_namespace: self.agent.namespace.as_str(),
            },
        )
        .await?;

        let llm = runtime_llm_client();
        let processed = drain_cognition_jobs(
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
            let signals =
                collect_dream_signals(&memory_repo, namespace.id, &derived_session_key).await?;
            let plan = choose_dream_schedule(&self.cognition, &signals, reason);
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
                    run_dream_cycle(
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
                    let queued = enqueue_dream_jobs(
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
                    let queued = enqueue_digest_job_if_absent(
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
                            drain_cognition_jobs(
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

        let path = self.state_file_path(agent_type, &derived_session_key)?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }

        Ok(())
    }

    pub fn state_root() -> PathBuf {
        if let Some(dir) = dirs::state_dir() {
            dir.join("nexus-memory-system").join("runtime")
        } else {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".local/state/nexus-memory-system/runtime"))
                .unwrap_or_else(|_| PathBuf::from(".nexus-runtime"))
        }
    }

    fn state_file_path(&self, agent_type: &str, session_key: &str) -> Result<PathBuf, AgentError> {
        let root = Self::state_root().join("sessions");
        std::fs::create_dir_all(&root)?;
        Ok(root.join(format!(
            "{}__{}.json",
            sanitize_component(agent_type),
            sanitize_component(session_key)
        )))
    }
}

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
            progressed += process_activity_distill_jobs(
                &repo,
                namespace_id,
                cognition,
                llm.clone(),
                lease_owner,
            )
            .await?;
        }
        if cognition.derive_enabled {
            progressed += process_derive_jobs(
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
            progressed += process_reflect_jobs(
                &repo,
                namespace_id,
                cognition,
                agent,
                embeddings.clone(),
                lease_owner,
            )
            .await?;
            progressed += process_reflect_namespace_jobs(
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
            progressed += process_digest_jobs(
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
        if enqueue_job_if_absent(
            repo,
            EnqueueJobParams {
                namespace_id,
                job_type: REFLECT_PERSPECTIVE_JOB,
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
        if enqueue_job_if_absent(
            repo,
            EnqueueJobParams {
                namespace_id,
                job_type: REFLECT_NAMESPACE_JOB,
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
        if enqueue_job_if_absent(
            repo,
            EnqueueJobParams {
                namespace_id,
                job_type: DIGEST_SESSION_JOB,
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

async fn enqueue_digest_job_if_absent(
    repo: &MemoryRepository,
    namespace_id: i64,
    session_key: &str,
    digest_reason: &str,
) -> Result<bool, AgentError> {
    let payload = json!({
        "session_key": session_key,
        "reason": digest_reason,
    });
    enqueue_job_if_absent(
        repo,
        EnqueueJobParams {
            namespace_id,
            job_type: DIGEST_SESSION_JOB,
            priority: 110,
            perspective: None,
            payload: &payload,
        },
    )
    .await
}

async fn enqueue_job_if_absent(
    repo: &MemoryRepository,
    params: EnqueueJobParams<'_>,
) -> Result<bool, AgentError> {
    for status in [memory_job_status::PENDING, memory_job_status::RUNNING] {
        let jobs = repo
            .list_jobs(
                params.namespace_id,
                Some(params.job_type),
                Some(status),
                64,
                0,
            )
            .await
            .map_err(|error| AgentError::Storage(error.to_string()))?;
        if jobs
            .iter()
            .any(|row| queued_job_matches(row, params.perspective, params.payload))
        {
            return Ok(false);
        }
    }

    repo.enqueue_job(params)
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;
    Ok(true)
}

fn queued_job_matches(
    row: &MemoryJobRow,
    perspective: Option<&serde_json::Value>,
    payload: &serde_json::Value,
) -> bool {
    let row_payload: serde_json::Value = match serde_json::from_str(&row.payload_json) {
        Ok(value) => value,
        Err(_) => return false,
    };
    if &row_payload != payload {
        return false;
    }

    match (&row.perspective_json, perspective) {
        (None, None) => true,
        (Some(existing), Some(expected)) => serde_json::from_str::<serde_json::Value>(existing)
            .map(|value| value == *expected)
            .unwrap_or(false),
        _ => false,
    }
}

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

async fn collect_dream_signals(
    repo: &MemoryRepository,
    namespace_id: i64,
    session_key: &str,
) -> Result<DreamSignals, AgentError> {
    let memories = repo
        .list_filtered(
            namespace_id,
            nexus_storage::repository::ListMemoryFilters {
                category: None,
                since: None,
                until: None,
                content_like: None,
                include_raw: true,
                limit: 256,
                offset: 0,
            },
        )
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
        if memory.labels.iter().any(|label| label == "raw-activity")
            || memory.metadata.get("raw_activity").is_some()
        {
            signals.raw_event_count += 1;
        }
        if let Some(level) =
            CognitiveMetadata::from_metadata(&memory.metadata).map(|meta| meta.level)
        {
            match level {
                CognitiveLevel::Explicit => {
                    signals.explicit_count += 1;
                    signals.total_non_raw_count += 1;
                }
                CognitiveLevel::Derived => {
                    signals.derived_count += 1;
                    signals.total_non_raw_count += 1;
                }
                CognitiveLevel::Contradiction => {
                    signals.contradiction_count += 1;
                    signals.total_non_raw_count += 1;
                }
                _ => {}
            }
        }
    }
    signals.contradiction_density = if signals.total_non_raw_count > 0 {
        signals.contradiction_count as f32 / signals.total_non_raw_count as f32
    } else {
        0.0
    };
    Ok(signals)
}

fn memory_matches_session_key(memory: &Memory, session_key: &str) -> bool {
    let metadata = &memory.metadata;
    let matches_value = |value: Option<&serde_json::Value>| {
        value.and_then(serde_json::Value::as_str) == Some(session_key)
    };

    if matches_value(metadata.pointer("/cognitive/session_key"))
        || matches_value(metadata.pointer("/raw_activity/derived_session_key"))
        || matches_value(metadata.pointer("/runtime/derived_session_key"))
        || matches_value(metadata.pointer("/runtime/session_key"))
    {
        return true;
    }

    for pointer in [
        "/cognitive/session_keys",
        "/source/derived_session_keys",
        "/raw_activity/derived_session_keys",
    ] {
        if metadata
            .pointer(pointer)
            .and_then(serde_json::Value::as_array)
            .is_some_and(|values| {
                values
                    .iter()
                    .any(|value| value.as_str() == Some(session_key))
            })
        {
            return true;
        }
    }

    false
}

fn choose_dream_schedule(
    cognition: &CognitionConfig,
    signals: &DreamSignals,
    reason: RuntimeShutdownReason,
) -> DreamSchedulePlan {
    if signals.raw_event_count == 0
        && signals.explicit_count == 0
        && signals.derived_count == 0
        && signals.contradiction_count == 0
    {
        return DreamSchedulePlan {
            action: DreamScheduleAction::Skip,
            reason: "no session cognition signal",
        };
    }

    // High contradiction density (>20% of non-raw memories) always triggers immediate.
    if signals.contradiction_count > 0
        && signals.total_non_raw_count > 0
        && signals.contradiction_density > 0.20
    {
        return DreamSchedulePlan {
            action: DreamScheduleAction::ImmediateBounded,
            reason: "high contradiction density requires immediate reconciliation",
        };
    }

    // Moderate contradiction density (5-20%) on session end triggers immediate.
    if matches!(reason, RuntimeShutdownReason::SessionEnded)
        && signals.contradiction_density > 0.05
        && signals.contradiction_density <= 0.20
    {
        return DreamSchedulePlan {
            action: DreamScheduleAction::ImmediateBounded,
            reason: "moderate contradiction density at session end warrants reconciliation",
        };
    }

    if signals.contradiction_count > 0 && signals.contradiction_density <= 0.05 {
        return DreamSchedulePlan {
            action: DreamScheduleAction::ImmediateBounded,
            reason: "contradictions require immediate reconciliation",
        };
    }

    if signals.raw_event_count >= cognition.activity_distill_min_events {
        return DreamSchedulePlan {
            action: DreamScheduleAction::ImmediateBounded,
            reason: "high session activity warrants immediate dream pass",
        };
    }

    if matches!(reason, RuntimeShutdownReason::SessionEnded)
        && signals.explicit_count >= 2
        && signals.derived_count == 0
    {
        return DreamSchedulePlan {
            action: DreamScheduleAction::ImmediateBounded,
            reason: "session end flushes unreconciled explicit observations immediately",
        };
    }

    if signals.has_digest_gap
        && signals.explicit_count == 0
        && signals.derived_count == 0
        && signals.contradiction_count == 0
        && signals.raw_event_count <= (cognition.activity_distill_min_events / 2).max(1)
    {
        return DreamSchedulePlan {
            action: DreamScheduleAction::DigestOnly,
            reason: "light session activity with uncovered digest window",
        };
    }

    if matches!(reason, RuntimeShutdownReason::IdleTimeout)
        && (signals.raw_event_count > 0
            || signals.explicit_count >= 2
            || (signals.explicit_count > 0 && signals.derived_count == 0))
    {
        return DreamSchedulePlan {
            action: DreamScheduleAction::DelayedEnqueue,
            reason: "idle timeout defers medium-signal dreaming to background jobs",
        };
    }

    if signals.explicit_count >= 2 && signals.derived_count == 0 {
        return DreamSchedulePlan {
            action: DreamScheduleAction::DelayedEnqueue,
            reason: "explicit observations suggest deferred reflection opportunity",
        };
    }

    DreamSchedulePlan {
        action: DreamScheduleAction::Skip,
        reason: "insufficient signal for additional dream work",
    }
}

async fn store_runtime_marker(
    memory_repo: &MemoryRepository,
    namespace_id: i64,
    marker: RuntimeMarker<'_>,
) -> Result<(), AgentError> {
    let session_tag = derive_session_key(marker.agent_type, marker.session_key, marker.cwd);
    let content = format!(
        "Runtime {} for {} [session:{}] ({})",
        marker.event.replace('_', " "),
        marker.agent_type,
        session_tag,
        marker.detail
    );
    let metadata = json!({
        "runtime": {
            "event": marker.event,
            "detail": marker.detail,
            "session_key": marker.session_key,
            "derived_session_key": session_tag,
            "cwd": marker.cwd,
            "agent_type": marker.agent_type,
            "agent_namespace": marker.agent_namespace,
            "captured_at": Utc::now(),
        }
    });
    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::Explicit,
        marker.agent_type,
        marker.agent_type,
        Some(session_tag.clone()),
        "runtime_controller",
    );
    cognitive.confidence = Some(1.0);
    let metadata = cognitive.merge_into(&metadata);

    memory_repo
        .store(StoreMemoryParams {
            namespace_id,
            content: &content,
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &[
                "runtime".to_string(),
                "session".to_string(),
                marker.event.to_string(),
            ],
            metadata: &metadata,
            embedding: None,
            embedding_model: None,
        })
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    Ok(())
}

fn read_runtime_state(path: &Path) -> Result<Option<RuntimeState>, AgentError> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(path)?;
    let state =
        serde_json::from_str(&contents).map_err(|e| AgentError::Supervisor(e.to_string()))?;
    Ok(Some(state))
}

fn write_runtime_state(path: &Path, state: &RuntimeState) -> Result<(), AgentError> {
    let contents =
        serde_json::to_string_pretty(state).map_err(|e| AgentError::Supervisor(e.to_string()))?;
    std::fs::write(path, contents)?;
    Ok(())
}

pub fn derive_session_key(
    agent_type: &str,
    session_key: Option<&str>,
    cwd: Option<&str>,
) -> String {
    if let Some(value) = session_key.filter(|value| !value.trim().is_empty()) {
        return value.to_string();
    }

    let canonical_agent = nexus_core::canonicalize_agent_type(agent_type);
    let fallback_scope = cwd
        .filter(|value| !value.trim().is_empty())
        .map(nexus_core::normalize_project_path)
        .unwrap_or_else(|| "unknown-cwd".to_string());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical_agent.hash(&mut hasher);
    fallback_scope.hash(&mut hasher);
    format!("derived-{:016x}", hasher.finish())
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn runtime_reason_label(reason: RuntimeShutdownReason) -> &'static str {
    match reason {
        RuntimeShutdownReason::SessionEnded => "session-ended",
        RuntimeShutdownReason::IdleTimeout => "idle-timeout",
        RuntimeShutdownReason::Manual => "manual",
    }
}

async fn namespace_id_for(agent_type: &str, storage: &StorageManager) -> Result<i64, AgentError> {
    let canonical = nexus_core::canonicalize_agent_type(agent_type);
    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    namespace_repo
        .get_or_create(&canonical, &canonical)
        .await
        .map(|namespace| namespace.id)
        .map_err(|error| AgentError::Storage(error.to_string()))
}

async fn process_derive_jobs(
    repo: &MemoryRepository,
    namespace_id: i64,
    cognition: &CognitionConfig,
    agent: &AgentConfig,
    llm: Arc<dyn LlmClient>,
    embeddings: Option<Arc<dyn EmbeddingService>>,
    lease_owner: &str,
) -> Result<usize, AgentError> {
    let jobs = repo
        .claim_jobs(
            namespace_id,
            DERIVE_MEMORY_JOB,
            lease_owner,
            cognition.lease_ttl_secs,
            cognition.max_job_batch as i64,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    let service = DeriveService::new(agent.clone(), llm, embeddings);
    let mut processed = 0usize;
    for job in jobs {
        let memory_id = job
            .payload
            .get("memory_id")
            .and_then(serde_json::Value::as_i64);
        let outcome = async {
            let memory_id = memory_id.ok_or_else(|| {
                AgentError::Derivation("derive job missing memory_id".to_string())
            })?;
            let memory = repo
                .get_by_id(memory_id)
                .await
                .map_err(|error| AgentError::Storage(error.to_string()))?
                .ok_or_else(|| {
                    AgentError::Derivation(format!("derive source memory {memory_id} not found"))
                })?;
            service
                .derive_memory_with_perspective(&memory, job.perspective.as_ref(), repo)
                .await
                .map(|_| ())
        }
        .await;

        match outcome {
            Ok(()) => {
                repo.complete_job(&job)
                    .await
                    .map_err(|error| AgentError::Storage(error.to_string()))?;
                processed += 1;
            }
            Err(error) => {
                repo.fail_job(&job, &error.to_string())
                    .await
                    .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
            }
        }
    }

    Ok(processed)
}

async fn process_reflect_jobs(
    repo: &MemoryRepository,
    namespace_id: i64,
    cognition: &CognitionConfig,
    agent: &AgentConfig,
    embeddings: Option<Arc<dyn EmbeddingService>>,
    lease_owner: &str,
) -> Result<usize, AgentError> {
    let jobs = repo
        .claim_jobs(
            namespace_id,
            REFLECT_PERSPECTIVE_JOB,
            lease_owner,
            cognition.lease_ttl_secs,
            cognition.max_job_batch as i64,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    let service = ReflectService::new(agent.clone(), cognition.clone(), embeddings.clone());
    let mut processed = 0usize;
    for job in jobs {
        let outcome = async {
            let perspective = job.perspective.as_ref().ok_or_else(|| {
                AgentError::Reflection("reflect job missing perspective".to_string())
            })?;
            service
                .reflect_perspective_cycle(namespace_id, perspective, repo)
                .await
                .map(|_| ())
        }
        .await;

        match outcome {
            Ok(()) => {
                repo.complete_job(&job)
                    .await
                    .map_err(|error| AgentError::Storage(error.to_string()))?;
                processed += 1;
            }
            Err(error) => {
                repo.fail_job(&job, &error.to_string())
                    .await
                    .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
            }
        }
    }

    Ok(processed)
}

async fn process_reflect_namespace_jobs(
    repo: &MemoryRepository,
    namespace_id: i64,
    cognition: &CognitionConfig,
    agent: &AgentConfig,
    embeddings: Option<Arc<dyn EmbeddingService>>,
    lease_owner: &str,
) -> Result<usize, AgentError> {
    let jobs = repo
        .claim_jobs(
            namespace_id,
            REFLECT_NAMESPACE_JOB,
            lease_owner,
            cognition.lease_ttl_secs,
            cognition.max_job_batch as i64,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    let service = ReflectService::new(agent.clone(), cognition.clone(), embeddings.clone());
    let mut processed = 0usize;
    for job in jobs {
        let outcome = service.reflect_cycle(namespace_id, repo).await.map(|_| ());

        match outcome {
            Ok(()) => {
                repo.complete_job(&job)
                    .await
                    .map_err(|error| AgentError::Storage(error.to_string()))?;
                processed += 1;
            }
            Err(error) => {
                repo.fail_job(&job, &error.to_string())
                    .await
                    .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
            }
        }
    }

    Ok(processed)
}

async fn process_digest_jobs(
    repo: &MemoryRepository,
    namespace_id: i64,
    cognition: &CognitionConfig,
    agent: &AgentConfig,
    llm: Arc<dyn LlmClient>,
    embeddings: Option<Arc<dyn EmbeddingService>>,
    lease_owner: &str,
) -> Result<usize, AgentError> {
    let jobs = repo
        .claim_jobs(
            namespace_id,
            DIGEST_SESSION_JOB,
            lease_owner,
            cognition.lease_ttl_secs,
            cognition.max_job_batch as i64,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    let service = DigestService::new(agent.clone(), llm, embeddings);
    let mut processed = 0usize;
    let mut seen_sessions = HashSet::new();
    for job in jobs {
        let session_key = job
            .payload
            .get("session_key")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                job.perspective
                    .as_ref()
                    .and_then(|perspective| perspective.session_key.clone())
            })
            .ok_or_else(|| AgentError::Digest("digest job missing session_key".to_string()))?;
        let force = digest_job_is_forced(
            job.payload
                .get("reason")
                .and_then(serde_json::Value::as_str),
        );

        if !seen_sessions.insert(session_key.clone()) {
            repo.complete_job(&job)
                .await
                .map_err(|error| AgentError::Storage(error.to_string()))?;
            processed += 1;
            continue;
        }

        if !force
            && !should_run_incremental_digest(repo, namespace_id, &session_key, cognition).await?
        {
            debug!(
                namespace_id,
                session_key, "Skipping digest rollover below threshold"
            );
            repo.complete_job(&job)
                .await
                .map_err(|error| AgentError::Storage(error.to_string()))?;
            processed += 1;
            continue;
        }

        let outcome = async {
            service
                .digest_session(namespace_id, &session_key, repo, force)
                .await
                .map(|_| ())
        }
        .await;

        match outcome {
            Ok(()) => {
                repo.complete_job(&job)
                    .await
                    .map_err(|error| AgentError::Storage(error.to_string()))?;
                processed += 1;
            }
            Err(error) => {
                repo.fail_job(&job, &error.to_string())
                    .await
                    .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
            }
        }
    }

    Ok(processed)
}

fn digest_job_is_forced(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some("dream_digest" | "session_end" | "manual_digest" | "manual_rebuild")
    )
}

async fn should_run_incremental_digest(
    repo: &MemoryRepository,
    namespace_id: i64,
    session_key: &str,
    cognition: &CognitionConfig,
) -> Result<bool, AgentError> {
    let rollover = repo
        .session_digest_rollover(namespace_id, session_key)
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    if rollover.last_digest_end_memory_id.is_none() {
        return Ok(true);
    }

    Ok(
        rollover.new_memory_count >= cognition.activity_distill_min_events as i64
            || rollover.estimated_new_tokens >= cognition.digest_short_target_tokens as i64,
    )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DistilledSession {
    summary: String,
    category: String,
    labels: Vec<String>,
    key_activities: Vec<String>,
    files_touched: Vec<String>,
    tools_used: Vec<String>,
    decisions_made: Vec<String>,
}

#[derive(Debug, Clone)]
struct DistillEvent {
    memory_id: i64,
    created_at: DateTime<Utc>,
    session_key: String,
    event_name: String,
    cwd: Option<String>,
    raw_payload: serde_json::Value,
}

async fn process_activity_distill_jobs(
    repo: &MemoryRepository,
    namespace_id: i64,
    cognition: &CognitionConfig,
    llm: Arc<dyn LlmClient>,
    lease_owner: &str,
) -> Result<usize, AgentError> {
    let jobs = repo
        .claim_jobs(
            namespace_id,
            ACTIVITY_DISTILL_JOB,
            lease_owner,
            cognition.lease_ttl_secs,
            cognition.max_job_batch as i64,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    let mut processed = 0usize;
    let mut seen_sessions = HashSet::new();
    for job in jobs {
        let session_key = job
            .payload
            .get("session_key")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                job.perspective
                    .as_ref()
                    .and_then(|perspective| perspective.session_key.clone())
            })
            .ok_or_else(|| {
                AgentError::Ingest("activity_distill job missing session_key".to_string())
            })?;
        let agent_name = job
            .payload
            .get("agent")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown-agent")
            .to_string();

        if !seen_sessions.insert(session_key.clone()) {
            repo.complete_job(&job)
                .await
                .map_err(|error| AgentError::Storage(error.to_string()))?;
            continue;
        }

        match distill_raw_activity_session(
            repo,
            namespace_id,
            &agent_name,
            &session_key,
            cognition,
            llm.clone(),
        )
        .await
        {
            Ok(_) => {
                repo.complete_job(&job)
                    .await
                    .map_err(|error| AgentError::Storage(error.to_string()))?;
                processed += 1;
            }
            Err(error) => {
                repo.fail_job(&job, &error.to_string())
                    .await
                    .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
            }
        }
    }

    Ok(processed)
}

async fn distill_raw_activity_session(
    repo: &MemoryRepository,
    namespace_id: i64,
    agent: &str,
    session_key: &str,
    cognition: &CognitionConfig,
    llm: Arc<dyn LlmClient>,
) -> Result<Option<i64>, AgentError> {
    let events: Vec<DistillEvent> = repo
        .list_by_session_key(
            namespace_id,
            session_key,
            cognition.activity_distill_max_events as i64,
            true,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?
        .into_iter()
        .filter_map(distill_event_from_memory)
        .collect();

    if events.len() < cognition.activity_distill_min_events {
        return Ok(None);
    }

    let event_summaries: Vec<String> = events.iter().map(summarize_distill_event).collect();
    let source_ids: Vec<i64> = events.iter().map(|event| event.memory_id).collect();
    let distilled = distill_with_llm(&llm, session_key, event_summaries.as_slice())
        .await
        .unwrap_or_else(|_| fallback_distilled_session(&events));

    let category = MemoryCategory::parse(&distilled.category).unwrap_or(MemoryCategory::Session);
    let lane_type = MemoryLaneType::Cognitive(MemoryLaneCognitiveType::Explicit);
    let cognitive = build_distill_cognitive_metadata(agent, session_key, &source_ids);
    let metadata = cognitive.merge_into(&serde_json::json!({
        "distilled_from": events.len(),
        "session_id": session_key,
        "key_activities": distilled.key_activities,
        "files_touched": distilled.files_touched,
        "tools_used": distilled.tools_used,
        "decisions_made": distilled.decisions_made,
        "pipeline": "distill-v1",
    }));

    let memory = repo
        .store_distilled_summary(
            StoreMemoryParams {
                namespace_id,
                content: &distilled.summary,
                category: &category,
                memory_lane_type: Some(&lane_type),
                labels: &distilled.labels,
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            },
            &source_ids,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    Ok(Some(memory.id))
}

async fn distill_with_llm(
    llm: &Arc<dyn LlmClient>,
    session_key: &str,
    event_summaries: &[String],
) -> Result<DistilledSession, AgentError> {
    let user_prompt = format!(
        "Session ID: {}\nEvent count: {}\n\nEvents:\n{}",
        session_key,
        event_summaries.len(),
        event_summaries.join("\n")
    );
    llm.generate_json(GenerateParams {
        messages: vec![
            ChatMessage::system(ACTIVITY_DISTILL_SYSTEM_PROMPT),
            ChatMessage::user(user_prompt),
        ],
        max_tokens: 2048,
        temperature: 0.3,
        json_mode: true,
    })
    .await
    .map_err(|error| AgentError::Llm(error.to_string()))
}

fn distill_event_from_memory(memory: Memory) -> Option<DistillEvent> {
    let raw_activity = memory.metadata.get("raw_activity")?;
    if !raw_activity
        .get("distill_pending")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    let session_key = raw_activity
        .get("derived_session_key")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            memory
                .metadata
                .pointer("/cognitive/session_key")
                .and_then(serde_json::Value::as_str)
        })?
        .to_string();
    let event_name = raw_activity
        .get("event_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("hook_event")
        .to_string();
    let cwd = raw_activity
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string);
    let raw_payload = memory
        .metadata
        .get("raw_payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    Some(DistillEvent {
        memory_id: memory.id,
        created_at: memory.created_at,
        session_key,
        event_name,
        cwd,
        raw_payload,
    })
}

fn summarize_distill_event(event: &DistillEvent) -> String {
    let ts = event
        .raw_payload
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| event.created_at.to_rfc3339());
    let event_type = event
        .raw_payload
        .get("event")
        .or_else(|| event.raw_payload.get("hook_event_name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&event.event_name);
    let tool = event
        .raw_payload
        .get("tool")
        .or_else(|| event.raw_payload.get("tool_name"))
        .or_else(|| event.raw_payload.get("toolName"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("-");
    let cwd = event
        .raw_payload
        .get("cwd")
        .or_else(|| event.raw_payload.get("working_directory"))
        .and_then(serde_json::Value::as_str)
        .or(event.cwd.as_deref())
        .unwrap_or("-");
    format!("{ts} | {event_type} | tool={tool} | cwd={cwd}")
}

fn fallback_distilled_session(events: &[DistillEvent]) -> DistilledSession {
    let mut tools = BTreeSet::new();
    let mut workspaces = BTreeSet::new();
    let mut activities = Vec::new();

    for event in events {
        if let Some(tool) = event
            .raw_payload
            .get("tool")
            .or_else(|| event.raw_payload.get("tool_name"))
            .or_else(|| event.raw_payload.get("toolName"))
            .and_then(serde_json::Value::as_str)
        {
            tools.insert(tool.to_string());
        }
        if let Some(cwd) = event.cwd.as_deref() {
            workspaces.insert(cwd.to_string());
        }
        activities.push(event.event_name.clone());
    }

    DistilledSession {
        summary: format!(
            "Session {} produced {} low-signal hook events across {} tool(s), primarily in {}.",
            events
                .first()
                .map(|event| event.session_key.as_str())
                .unwrap_or("unknown-session"),
            events.len(),
            tools.len(),
            workspaces
                .iter()
                .next()
                .cloned()
                .unwrap_or_else(|| "an unknown workspace".to_string())
        ),
        category: "session".to_string(),
        labels: vec!["activity-summary".to_string(), "auto-distilled".to_string()],
        key_activities: activities.into_iter().take(5).collect(),
        files_touched: workspaces.into_iter().collect(),
        tools_used: tools.into_iter().collect(),
        decisions_made: Vec::new(),
    }
}

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
    use nexus_embeddings::{EmbeddingConfig as RuntimeEmbeddingConfig, OrtEmbeddingService};
    match OrtEmbeddingService::new(RuntimeEmbeddingConfig::from_env()).await {
        Ok(service) => Some(Arc::new(service)),
        Err(error) => {
            warn!(%error, "Failed to initialize embedding service, cognition will run without embeddings");
            None
        }
    }
}

fn build_distill_cognitive_metadata(
    agent: &str,
    session_key: &str,
    source_memory_ids: &[i64],
) -> CognitiveMetadata {
    let perspective = infer_perspective(
        PerspectiveSource::Digest,
        agent,
        None::<String>,
        Some(session_key.to_string()),
    );
    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::SummaryShort,
        perspective.observer,
        perspective.subject,
        perspective.session_key,
        "nexus:distill-v1",
    );
    cognitive.source_memory_ids = source_memory_ids.to_vec();
    cognitive.confidence = Some(0.8);
    cognitive
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

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

        let processed = run_dream_cycle(
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
        let perspective = PerspectiveKey {
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
