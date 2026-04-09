//! Dream cycle orchestration — signal collection, adaptive scheduling,
//! job enqueue, and cognition draining.

use std::sync::Arc;
use std::time::Instant;

use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::traits::EmbeddingService;
use nexus_core::{CognitiveLevel, CognitiveMetadata, Memory, PerspectiveKey};
use nexus_llm::LlmClient;
use nexus_storage::models::EnqueueJobParams;
use nexus_storage::repository::MemoryRepository;
use serde_json::json;

use crate::distill;
use crate::error::AgentError;
use crate::job_processor;
use crate::runtime_state::RuntimeShutdownReason;
use crate::util::{flush_metric_samples, stage_metric_sample};

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

// ── Adaptive dream scheduling ─────────────────────────────────────────

pub(crate) fn choose_dream_schedule(
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
