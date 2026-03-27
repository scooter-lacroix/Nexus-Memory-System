//! Session lifecycle commands for hook-driven runtime orchestration.

use crate::commands::ingest_hook_event::{process_normalized_event, IngestOutcome};
use anyhow::Result;
use chrono::Utc;
use nexus_agent::{derive_session_key, RuntimeController, RuntimeMode, RuntimeShutdownReason};
use nexus_core::{CognitiveLevel, CognitiveMetadata, Config, MemoryCategory};
use nexus_hooks::retry_buffer::RetryBuffer;
use nexus_storage::repository::{MemoryRepository, NamespaceRepository, StoreMemoryParams};
use nexus_storage::StorageManager;
use serde_json::{json, Value};
use std::io::IsTerminal;

fn parse_runtime_mode(mode: &str) -> RuntimeMode {
    match mode {
        "persistent" => RuntimeMode::Persistent,
        _ => RuntimeMode::SessionScoped,
    }
}

pub async fn execute_start(
    agent: String,
    session_key: Option<String>,
    cwd: Option<String>,
    mode: String,
) -> Result<()> {
    let config = Config::from_env()?;
    let controller = RuntimeController::new(config.cognition.clone(), config.agent.clone());
    controller
        .ensure_started(
            &agent,
            session_key.as_deref(),
            cwd.as_deref(),
            parse_runtime_mode(&mode),
        )
        .await?;

    let raw_payload = read_optional_stdin_json();
    let detail = format!("mode={mode}");
    store_session_memory(
        &config,
        &agent,
        session_key.as_deref(),
        cwd.as_deref(),
        "session_start",
        &detail,
        raw_payload.as_ref(),
    )
    .await?;

    println!(
        "Session runtime ready for {} ({})",
        agent,
        derive_session_key(&agent, session_key.as_deref(), cwd.as_deref())
    );
    Ok(())
}

pub async fn execute_event(
    agent: String,
    session_key: Option<String>,
    cwd: Option<String>,
    kind: String,
) -> Result<()> {
    let config = Config::from_env()?;
    let should_record = !matches!(kind.as_str(), "compact" | "checkpoint" | "completion")
        || config.cognition.checkpoint_flush_enabled;
    let controller = RuntimeController::new(config.cognition.clone(), config.agent.clone());
    controller
        .ensure_started(
            &agent,
            session_key.as_deref(),
            cwd.as_deref(),
            RuntimeMode::SessionScoped,
        )
        .await?;

    if should_record {
        let raw_payload = read_optional_stdin_json();
        store_session_memory(
            &config,
            &agent,
            session_key.as_deref(),
            cwd.as_deref(),
            &format!("session_{kind}"),
            &kind,
            raw_payload.as_ref(),
        )
        .await?;
    }

    if matches!(kind.as_str(), "compact" | "checkpoint" | "completion")
        && config.cognition.checkpoint_flush_enabled
    {
        drain_retry_buffer_for_session(&config, &agent, session_key.as_deref(), cwd.as_deref(), 2)
            .await?;
    }

    println!("Session event recorded for {}: {}", agent, kind);
    Ok(())
}

pub async fn execute_end(
    agent: String,
    session_key: Option<String>,
    cwd: Option<String>,
    reason: Option<String>,
) -> Result<()> {
    let config = Config::from_env()?;
    let reason = reason.unwrap_or_else(|| "session-ended".to_string());
    let raw_payload = read_optional_stdin_json();

    store_session_memory(
        &config,
        &agent,
        session_key.as_deref(),
        cwd.as_deref(),
        "session_end",
        &reason,
        raw_payload.as_ref(),
    )
    .await?;

    drain_retry_buffer_for_session(
        &config,
        &agent,
        session_key.as_deref(),
        cwd.as_deref(),
        config.cognition.retry_buffer_drain_limit,
    )
    .await?;

    let controller = RuntimeController::new(config.cognition.clone(), config.agent.clone());
    controller
        .flush_and_shutdown(
            &agent,
            session_key.as_deref(),
            cwd.as_deref(),
            RuntimeShutdownReason::SessionEnded,
        )
        .await?;

    println!(
        "Session finalized for {} ({})",
        agent,
        derive_session_key(&agent, session_key.as_deref(), cwd.as_deref())
    );
    Ok(())
}

async fn store_session_memory(
    config: &Config,
    agent: &str,
    session_key: Option<&str>,
    cwd: Option<&str>,
    event: &str,
    detail: &str,
    raw_payload: Option<&Value>,
) -> Result<()> {
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;
    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());
    let namespace = namespace_repo.get_or_create(agent, agent).await?;

    let derived_session_key = derive_session_key(agent, session_key, cwd);
    let content = format!(
        "Session lifecycle event {} for {} [session:{}] ({})",
        event.replace('_', " "),
        agent,
        derived_session_key,
        detail
    );
    let metadata = json!({
        "session_lifecycle": {
            "event": event,
            "detail": detail,
            "session_key": session_key,
            "derived_session_key": derived_session_key,
            "cwd": cwd,
            "captured_at": Utc::now(),
        },
        "raw_payload": raw_payload,
    });
    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::Explicit,
        agent,
        agent,
        Some(derived_session_key.clone()),
        "session_lifecycle",
    );
    cognitive.confidence = Some(1.0);
    let metadata = cognitive.merge_into(&metadata);
    let labels = vec![
        "session".to_string(),
        "runtime".to_string(),
        event.to_string(),
    ];

    memory_repo
        .store(StoreMemoryParams {
            namespace_id: namespace.id,
            content: &content,
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &labels,
            metadata: &metadata,
            embedding: None,
            embedding_model: None,
        })
        .await?;

    Ok(())
}

async fn drain_retry_buffer_for_session(
    config: &Config,
    agent: &str,
    session_key: Option<&str>,
    cwd: Option<&str>,
    max_artifacts: usize,
) -> Result<()> {
    if max_artifacts == 0 {
        return Ok(());
    }

    let effective_session_key = derive_session_key(agent, session_key, cwd);
    let retry_buffer = RetryBuffer::new();
    let pending = retry_buffer.list_pending()?;
    if pending.is_empty() {
        return Ok(());
    }

    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;
    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());
    let namespace = namespace_repo.get_or_create(agent, agent).await?;

    let mut processed = 0usize;
    for path in pending {
        if processed >= max_artifacts {
            break;
        }

        let artifact = match RetryBuffer::read_pending(&path) {
            Ok(artifact) => artifact,
            Err(_) => continue,
        };

        let artifact_session_key = derive_session_key(
            &artifact.normalized_event.agent,
            artifact.normalized_event.session_id.as_deref(),
            artifact.normalized_event.cwd.as_deref(),
        );
        if artifact.normalized_event.agent != agent || artifact_session_key != effective_session_key
        {
            continue;
        }

        let outcome = process_normalized_event(
            namespace.id,
            &memory_repo,
            config,
            &artifact.normalized_event,
            &artifact.candidates,
            false,
        )
        .await?;

        match outcome {
            IngestOutcome::Persisted { .. } | IngestOutcome::RawActivityOnly => {
                RetryBuffer::remove_pending(&path)?;
                processed += 1;
            }
            IngestOutcome::Deferred => continue,
        }
    }

    Ok(())
}

fn read_optional_stdin_json() -> Option<Value> {
    use std::io::Read;

    if std::io::stdin().is_terminal() {
        return None;
    }

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return None;
    }

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    serde_json::from_str(trimmed).ok()
}
