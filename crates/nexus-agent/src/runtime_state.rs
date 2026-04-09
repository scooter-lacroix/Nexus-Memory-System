//! Runtime state persistence, session key derivation, and helper types.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use nexus_core::{CognitiveLevel, CognitiveMetadata, MemoryCategory};
use nexus_storage::repository::{MemoryRepository, NamespaceRepository, StoreMemoryParams};
use nexus_storage::StorageManager;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::AgentError;

// ── Public enums ──────────────────────────────────────────────────────

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

// ── Internal serialisation types ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RuntimeState {
    pub agent_type: String,
    pub session_key: String,
    pub mode: RuntimeModeSerde,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub(crate) struct RuntimeMarker<'a> {
    pub agent_type: &'a str,
    pub session_key: Option<&'a str>,
    pub cwd: Option<&'a str>,
    pub event: &'a str,
    pub detail: &'a str,
    pub agent_namespace: &'a str,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeModeSerde {
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

// ── State persistence helpers ─────────────────────────────────────────

pub(crate) fn state_root() -> PathBuf {
    if let Some(dir) = dirs::state_dir() {
        dir.join("nexus-memory-system").join("runtime")
    } else {
        std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".local/state/nexus-memory-system/runtime"))
            .unwrap_or_else(|_| PathBuf::from(".nexus-runtime"))
    }
}

pub(crate) fn state_file_path(agent_type: &str, session_key: &str) -> Result<PathBuf, AgentError> {
    let root = state_root().join("sessions");
    std::fs::create_dir_all(&root)?;
    Ok(root.join(format!(
        "{}__{}.json",
        sanitize_component(agent_type),
        sanitize_component(session_key)
    )))
}

pub(crate) fn read_runtime_state(path: &Path) -> Result<Option<RuntimeState>, AgentError> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(path)?;
    let state =
        serde_json::from_str(&contents).map_err(|e| AgentError::Supervisor(e.to_string()))?;
    Ok(Some(state))
}

pub(crate) fn write_runtime_state(path: &Path, state: &RuntimeState) -> Result<(), AgentError> {
    let contents =
        serde_json::to_string_pretty(state).map_err(|e| AgentError::Supervisor(e.to_string()))?;
    std::fs::write(path, contents)?;
    Ok(())
}

// ── Session key derivation ────────────────────────────────────────────

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
    format!(
        "derived-{}-{}",
        sanitize_component(&canonical_agent),
        sanitize_component(&fallback_scope)
    )
}

pub(crate) fn sanitize_component(value: &str) -> String {
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

pub(crate) fn runtime_reason_label(reason: RuntimeShutdownReason) -> &'static str {
    match reason {
        RuntimeShutdownReason::SessionEnded => "session-ended",
        RuntimeShutdownReason::IdleTimeout => "idle-timeout",
        RuntimeShutdownReason::Manual => "manual",
    }
}

pub(crate) async fn namespace_id_for(
    agent_type: &str,
    storage: &StorageManager,
) -> Result<i64, AgentError> {
    let canonical = nexus_core::canonicalize_agent_type(agent_type);
    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    namespace_repo
        .get_or_create(&canonical, &canonical)
        .await
        .map(|namespace| namespace.id)
        .map_err(|error| AgentError::Storage(error.to_string()))
}

pub(crate) async fn store_runtime_marker(
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
