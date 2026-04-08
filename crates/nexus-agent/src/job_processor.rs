//! Job processing for derive, reflect, and digest cognition jobs.
//!
//! Each function claims a batch of pending jobs, dispatches them to the
//! appropriate service, and marks them complete or failed.

use std::collections::HashMap;
use std::sync::Arc;

use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::traits::EmbeddingService;
use nexus_llm::LlmClient;
use nexus_storage::models::ClaimedMemoryJob;
use nexus_storage::models::{memory_job_status, EnqueueJobParams, MemoryJobRow};
use nexus_storage::repository::MemoryRepository;
use serde_json::json;
use tracing::debug;

use crate::derive::DeriveService;
use crate::digest::DigestService;
use crate::error::AgentError;
use crate::reflect::ReflectService;

pub(crate) const DERIVE_MEMORY_JOB: &str = "derive_memory";
pub(crate) const REFLECT_NAMESPACE_JOB: &str = "reflect_namespace";
pub(crate) const REFLECT_PERSPECTIVE_JOB: &str = "reflect_perspective";
pub(crate) const DIGEST_SESSION_JOB: &str = "digest_session";

// ── Derive jobs ───────────────────────────────────────────────────────

pub(crate) async fn process_derive_jobs(
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

// ── Reflect jobs ──────────────────────────────────────────────────────

pub(crate) async fn process_reflect_jobs(
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

pub(crate) async fn process_reflect_namespace_jobs(
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

// ── Digest jobs ───────────────────────────────────────────────────────

pub(crate) async fn process_digest_jobs(
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

    // Group claimed jobs by session_key, OR-ing the forced flag across
    // duplicates so that a manual_digest/manual_rebuild is never silently
    // dropped when an earlier automatic job exists for the same session.
    let mut session_jobs: HashMap<String, Vec<(ClaimedMemoryJob, bool)>> = HashMap::new();
    for job in jobs {
        let session_key = match job
            .payload
            .get("session_key")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                job.perspective
                    .as_ref()
                    .and_then(|perspective| perspective.session_key.clone())
            }) {
            Some(key) => key,
            None => {
                let error = AgentError::Digest("digest job missing session_key".to_string());
                repo.fail_job(&job, &error.to_string())
                    .await
                    .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
                processed += 1;
                continue;
            }
        };
        let force = digest_job_is_forced(
            job.payload
                .get("reason")
                .and_then(serde_json::Value::as_str),
        );
        session_jobs
            .entry(session_key)
            .or_default()
            .push((job, force));
    }

    for (session_key, job_batch) in session_jobs {
        // OR forced flag across all jobs for this session.
        let force = job_batch.iter().any(|(_, f)| *f);

        if !force
            && !should_run_incremental_digest(repo, namespace_id, &session_key, cognition).await?
        {
            debug!(
                namespace_id,
                session_key, "Skipping digest rollover below threshold"
            );
            for (job, _) in &job_batch {
                repo.complete_job(job)
                    .await
                    .map_err(|error| AgentError::Storage(error.to_string()))?;
                processed += 1;
            }
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
                for (job, _) in &job_batch {
                    repo.complete_job(job)
                        .await
                        .map_err(|error| AgentError::Storage(error.to_string()))?;
                    processed += 1;
                }
            }
            Err(error) => {
                for (job, _) in &job_batch {
                    repo.fail_job(job, &error.to_string())
                        .await
                        .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
                }
            }
        }
    }

    Ok(processed)
}

pub(crate) fn digest_job_is_forced(reason: Option<&str>) -> bool {
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

// ── Job enqueue helpers ───────────────────────────────────────────────

pub(crate) async fn enqueue_digest_job_if_absent(
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

/// Enqueue a job only if no matching pending/running job already exists.
///
/// **Note:** The `list_jobs` → `enqueue_job` sequence is subject to a TOCTOU
/// race under high concurrency. A proper uniqueness guarantee should be
/// enforced at the database level (e.g. a unique index on
/// `(namespace_id, job_type, payload_hash)`). The current scan is a
/// best-effort pre-check, not a source of truth.
pub(crate) async fn enqueue_job_if_absent(
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
