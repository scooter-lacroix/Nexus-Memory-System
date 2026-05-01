//! Ingest hook event command implementation
//!
//! Reads raw JSON from stdin, normalizes, derives candidates,
//! enriches via LLM, and persists accepted memories.

use std::collections::HashSet;
use std::io::Read;

use anyhow::{Context, Result};
use chrono::Utc;
use nexus_agent::{create_embedding_service, RuntimeController, RuntimeMode};
use nexus_core::{
    infer_perspective, CognitiveLevel, CognitiveMetadata, Config, MemoryCategory, PerspectiveSource,
};
use nexus_hooks::candidate::derive_candidates;
use nexus_hooks::claude_payload::{normalize_payload, NormalizedHookEvent};
use nexus_hooks::enrichment::EnrichmentService;
use nexus_hooks::retry_buffer::RetryBuffer;
use nexus_storage::models::EnqueueJobParams;
use nexus_storage::repository::StoreMemoryParams;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};

const REFLECT_PERSPECTIVE_JOB: &str = "reflect_perspective";
const DIGEST_SESSION_JOB: &str = "digest_session";
const ACTIVITY_DISTILL_JOB: &str = "activity_distill";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IngestOutcome {
    RawActivityOnly,
    Deferred,
    Persisted { stored: usize, skipped: usize },
}

fn parse_stdin_json_or_empty(raw_input: &str) -> Result<serde_json::Value> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }

    serde_json::from_str(trimmed).context("Failed to parse stdin as JSON")
}

/// Execute the ingest-hook-event command
pub async fn execute(
    agent: String,
    event: String,
    format: String,
    session_key: Option<String>,
    cwd: Option<String>,
) -> Result<()> {
    // 1. Read raw JSON from stdin
    let mut raw_input = String::new();
    std::io::stdin()
        .read_to_string(&mut raw_input)
        .context("Failed to read stdin")?;

    let raw = parse_stdin_json_or_empty(&raw_input)?;

    tracing::info!(agent = %agent, event = %event, format = %format, "Processing hook event");

    // 2. Normalize the payload (unified dispatcher selects Claude vs generic normalizer)
    let mut normalized = normalize_payload(&agent, &event, &raw);
    if normalized.session_id.is_none() {
        normalized.session_id = session_key;
    }
    if normalized.cwd.is_none() {
        normalized.cwd = cwd;
    }
    tracing::debug!(tool_name = ?normalized.tool_name, "Normalized event");

    // 3. Initialize storage
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = namespace_repo.get_or_create(&agent, &agent).await?;

    // 4. Derive high-signal candidates
    let mut fingerprints = HashSet::new();
    let candidates = derive_candidates(&normalized, &mut fingerprints);

    if candidates.is_empty() {
        store_raw_activity_memory(namespace.id, &memory_repo, &normalized, &config).await?;
        let embeddings = create_embedding_service(&config).await;
        RuntimeController::new(config.cognition.clone(), config.agent.clone(), embeddings)
            .ensure_started(
                &agent,
                normalized.session_id.as_deref(),
                normalized.cwd.as_deref(),
                RuntimeMode::SessionScoped,
            )
            .await?;
        println!("No high-signal candidates derived from {}/{}", agent, event);
        return Ok(());
    }

    // 5. Enrich, persist, and enqueue downstream cognition work.
    let outcome = process_normalized_event(
        namespace.id,
        &memory_repo,
        &config,
        &normalized,
        &candidates,
        true,
    )
    .await?;

    // 7. Print summary
    match outcome {
        IngestOutcome::Persisted { stored, skipped } => {
            println!("Stored {} memories from {}/{}", stored, agent, event);
            if skipped > 0 {
                println!("- skipped: {} rejected by LLM", skipped);
            }
        }
        IngestOutcome::Deferred => {
            println!(
                "LLM enrichment unavailable. {} candidates buffered for retry.",
                candidates.len()
            );
            return Ok(());
        }
        IngestOutcome::RawActivityOnly => unreachable!("candidate path cannot return raw-only"),
    }

    RuntimeController::new(
        config.cognition.clone(),
        config.agent.clone(),
        create_embedding_service(&config).await,
    )
    .ensure_started(
        &agent,
        normalized.session_id.as_deref(),
        normalized.cwd.as_deref(),
        RuntimeMode::SessionScoped,
    )
    .await?;

    Ok(())
}

pub(crate) async fn process_normalized_event(
    namespace_id: i64,
    memory_repo: &MemoryRepository,
    config: &Config,
    normalized: &NormalizedHookEvent,
    candidates: &[nexus_hooks::candidate::MemoryCandidate],
    buffer_on_failure: bool,
) -> Result<IngestOutcome> {
    if candidates.is_empty() {
        store_raw_activity_memory(namespace_id, memory_repo, normalized, config).await?;
        return Ok(IngestOutcome::RawActivityOnly);
    }

    let enrichment_service: EnrichmentService = match EnrichmentService::new() {
        Ok(svc) => svc,
        Err(e) => {
            tracing::warn!(error = %e, "LLM enrichment unavailable");
            if buffer_on_failure {
                let retry_buffer = RetryBuffer::new();
                retry_buffer.write_failed(normalized, candidates, &e.to_string())?;
            }
            return Ok(IngestOutcome::Deferred);
        }
    };

    let batch = match enrichment_service
        .enrich_candidates(candidates, normalized)
        .await
    {
        Ok(batch) => batch,
        Err(e) => {
            tracing::warn!(error = %e, "LLM enrichment failed");
            if buffer_on_failure {
                let retry_buffer = RetryBuffer::new();
                retry_buffer.write_failed(normalized, candidates, &e.to_string())?;
            }
            return Ok(IngestOutcome::Deferred);
        }
    };

    let result = nexus_hooks::persistence::persist_enriched_memories(
        namespace_id,
        normalized,
        &batch,
        memory_repo,
        enrichment_service.model_name(),
    )
    .await
    .context("Failed to persist enriched memories")?;

    enqueue_enriched_cognition_jobs(
        namespace_id,
        normalized,
        &result.stored_memory_ids,
        memory_repo,
        config,
    )
    .await?;

    tracing::info!(
        stored = result.stored,
        skipped = result.skipped,
        "Ingestion complete"
    );

    Ok(IngestOutcome::Persisted {
        stored: result.stored,
        skipped: result.skipped,
    })
}

pub(crate) async fn store_raw_activity_memory(
    namespace_id: i64,
    memory_repo: &MemoryRepository,
    event: &NormalizedHookEvent,
    config: &Config,
) -> Result<()> {
    let derived_session_key = event
        .session_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            nexus_agent::derive_session_key(
                &event.agent,
                event.session_id.as_deref(),
                event.cwd.as_deref(),
            )
        });
    let perspective = infer_perspective(
        PerspectiveSource::HookIngest,
        event.agent.clone(),
        None::<String>,
        Some(derived_session_key.clone()),
    );
    let event_identity = event
        .turn_id
        .clone()
        .or_else(|| event.tool_name.clone())
        .unwrap_or_else(|| event.observed_at.timestamp_millis().to_string());
    let content = format!(
        "Observed raw activity {} for {} [session:{}] [event:{}] at {}",
        event.event_name,
        event.agent,
        derived_session_key,
        event_identity,
        event.observed_at.to_rfc3339()
    );
    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::Raw,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        "hook_raw_activity",
    );
    cognitive.confidence = Some(0.35);
    cognitive.times_reinforced = 0;
    cognitive.times_contradicted = 0;
    cognitive.derived_at = Some(Utc::now());
    cognitive.generated_by = Some("hook_raw_activity".to_string());
    let metadata = cognitive.merge_into(&serde_json::json!({
        "raw_activity": {
            "agent": event.agent,
            "event_name": event.event_name,
            "session_key": event.session_id,
            "derived_session_key": derived_session_key,
            "cwd": event.cwd,
            "captured_at": event.observed_at,
            "distill_pending": true,
        },
        "raw_payload": event.raw_payload,
    }));
    let labels = vec![
        "session".to_string(),
        "raw-activity".to_string(),
        "distill-pending".to_string(),
        event.event_name.clone(),
    ];

    let memory = memory_repo
        .store(StoreMemoryParams {
            namespace_id,
            content: &content,
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &labels,
            metadata: &metadata,
            embedding: None,
            embedding_model: None,
        })
        .await?;

    if config.cognition.activity_distill_enabled {
        let perspective_json = serde_json::to_value(&perspective).ok();
        let payload = serde_json::json!({
            "memory_id": memory.id,
            "session_key": derived_session_key,
            "agent": event.agent,
        });
        memory_repo
            .enqueue_job(EnqueueJobParams {
                namespace_id,
                job_type: ACTIVITY_DISTILL_JOB,
                priority: 70,
                perspective: perspective_json.as_ref(),
                payload: &payload,
            })
            .await?;
    }

    Ok(())
}

pub(crate) async fn enqueue_enriched_cognition_jobs(
    namespace_id: i64,
    event: &NormalizedHookEvent,
    memory_ids: &[i64],
    memory_repo: &MemoryRepository,
    config: &Config,
) -> Result<()> {
    if memory_ids.is_empty() {
        return Ok(());
    }

    let derived_session_key = event
        .session_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            nexus_agent::derive_session_key(
                &event.agent,
                event.session_id.as_deref(),
                event.cwd.as_deref(),
            )
        });
    let perspective = infer_perspective(
        PerspectiveSource::HookIngest,
        event.agent.clone(),
        None::<String>,
        Some(derived_session_key.clone()),
    );
    let perspective_json = serde_json::to_value(&perspective).ok();

    if config.cognition.reflect_enabled {
        let reflect_payload = serde_json::json!({
            "memory_ids": memory_ids,
            "session_key": derived_session_key,
            "agent": event.agent,
        });
        memory_repo
            .enqueue_job(EnqueueJobParams {
                namespace_id,
                job_type: REFLECT_PERSPECTIVE_JOB,
                priority: 85,
                perspective: perspective_json.as_ref(),
                payload: &reflect_payload,
            })
            .await?;
    }

    if config.cognition.digest_enabled {
        let digest_payload = serde_json::json!({
            "memory_ids": memory_ids,
            "session_key": derived_session_key,
            "agent": event.agent,
        });
        memory_repo
            .enqueue_job(EnqueueJobParams {
                namespace_id,
                job_type: DIGEST_SESSION_JOB,
                priority: 80,
                perspective: perspective_json.as_ref(),
                payload: &digest_payload,
            })
            .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_stdin_json_or_empty_treats_blank_input_as_empty_object() {
        let parsed = parse_stdin_json_or_empty("").expect("parse blank stdin");

        assert_eq!(parsed, json!({}));
    }
}
