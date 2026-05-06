//! Agent API endpoints for the always-on memory agent

use axum::{extract::State, Json};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;

use crate::error::{Result, WebError};
use crate::models::{
    AgentBoostRequest, AgentBoostResponse, AgentConsolidateResponse, AgentIngestRequest,
    AgentIngestResponse, AgentQueryRequest, AgentQueryResponse, AgentStatusResponse,
};
use crate::state::AppState;

/// POST /api/agent/ingest — Ingest text with LLM enrichment
pub async fn agent_ingest(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(request): Json<AgentIngestRequest>,
) -> Result<Json<AgentIngestResponse>> {
    let state = state.read().await;

    let supervisor = state
        .agent_supervisor
        .as_ref()
        .ok_or_else(|| WebError::InvalidRequest("Agent is not enabled".to_string()))?;

    if request.text.trim().is_empty() {
        return Err(WebError::InvalidRequest("Text cannot be empty".to_string()));
    }

    let ingest_svc = supervisor.ingest_service();
    let namespace_id = supervisor.namespace_id();

    let memory_repo = nexus_storage::MemoryRepository::new(state.pool().clone());

    match ingest_svc
        .ingest(&request.text, &request.source, namespace_id, &memory_repo)
        .await
    {
        Ok(memory_id) => Ok(Json(AgentIngestResponse {
            success: true,
            memory_id: Some(memory_id),
            summary: None,
            error: None,
        })),
        Err(e) => {
            error!(error = %e, "Agent ingest failed");
            Ok(Json(AgentIngestResponse {
                success: false,
                memory_id: None,
                summary: None,
                error: Some(e.to_string()),
            }))
        }
    }
}

/// POST /api/agent/query — Query memory with LLM synthesis
pub async fn agent_query(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(request): Json<AgentQueryRequest>,
) -> Result<Json<AgentQueryResponse>> {
    let state = state.read().await;

    let supervisor = state
        .agent_supervisor
        .as_ref()
        .ok_or_else(|| WebError::InvalidRequest("Agent is not enabled".to_string()))?;

    if request.question.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "Question cannot be empty".to_string(),
        ));
    }

    let query_svc = supervisor.query_service();
    let namespace_id = supervisor.namespace_id();

    let memory_repo = nexus_storage::MemoryRepository::new(state.pool().clone());
    let relation_repo = nexus_storage::MemoryRelationRepository::new(state.pool());

    match query_svc
        .query(
            &request.question,
            namespace_id,
            &memory_repo,
            &relation_repo,
        )
        .await
    {
        Ok(answer) => {
            supervisor.increment_queries_answered().await;
            Ok(Json(AgentQueryResponse {
                success: true,
                question: request.question,
                answer: Some(answer.answer),
                error: None,
            }))
        }
        Err(e) => {
            error!(error = %e, "Agent query failed");
            Ok(Json(AgentQueryResponse {
                success: false,
                question: request.question,
                answer: None,
                error: Some(e.to_string()),
            }))
        }
    }
}

/// POST /api/agent/consolidate — Trigger manual consolidation
pub async fn agent_consolidate(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Result<Json<AgentConsolidateResponse>> {
    let state = state.read().await;

    let supervisor = state
        .agent_supervisor
        .as_ref()
        .ok_or_else(|| WebError::InvalidRequest("Agent is not enabled".to_string()))?;

    let namespace_id = supervisor.namespace_id();
    let config = nexus_core::Config::from_env().map_err(|e| WebError::Config(e.to_string()))?;

    let lease_owner = format!("web-agent-consolidate-{}", namespace_id);
    let embeddings = nexus_agent::create_embedding_service(&config).await;
    match nexus_agent::run_dream_cycle(
        state.pool().clone(),
        &config.cognition,
        &nexus_core::config::AgentConfig {
            namespace: supervisor.get_status().await.namespace,
            ..Default::default()
        },
        nexus_llm::create_client_auto_with_fallback()
            .map_err(|e| WebError::Config(format!("Failed to create LLM client: {}", e)))?,
        embeddings,
        nexus_agent::DreamCycleRequest {
            namespace_id,
            lease_owner: &lease_owner,
            perspective: None,
            session_key: None,
            reflect_reason: "web_manual_dream",
            digest_reason: "web_manual_digest",
        },
    )
    .await
    {
        Ok(processed) => Ok(Json(AgentConsolidateResponse {
            success: true,
            memories_processed: processed,
            error: None,
        })),
        Err(e) => {
            error!(error = %e, "Agent consolidation failed");
            Ok(Json(AgentConsolidateResponse {
                success: false,
                memories_processed: 0,
                error: Some(format!("{}", e)),
            }))
        }
    }
}

/// POST /api/agent/boost — Pin or boost a memory in cognitive cache
pub async fn agent_boost(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(request): Json<AgentBoostRequest>,
) -> Result<Json<AgentBoostResponse>> {
    let state = state.read().await;

    let _supervisor = state
        .agent_supervisor
        .as_ref()
        .ok_or_else(|| WebError::InvalidRequest("Agent is not enabled".to_string()))?;

    let memory_repo = nexus_storage::MemoryRepository::new(state.pool().clone());
    let memory = memory_repo
        .get_by_id(request.memory_id)
        .await
        .map_err(|e| WebError::Storage(e.to_string()))?
        .ok_or_else(|| WebError::NotFound(format!("Memory {} not found", request.memory_id)))?;

    // Verify the memory belongs to the active namespace
    if memory.namespace_id != _supervisor.namespace_id() {
        return Err(WebError::InvalidRequest(
            "Memory does not belong to the active namespace".to_string(),
        ));
    }

    // Resolve project root for cache path — explicit root_dir required for web API
    let cwd = request
        .root_dir
        .as_ref()
        .map(std::path::PathBuf::from)
        .ok_or_else(|| WebError::InvalidRequest("root_dir is required".to_string()))?;
    
    // Check for path traversal attempts
    if cwd.components().any(|comp| comp == std::path::Component::ParentDir) {
        return Err(WebError::InvalidRequest(
            "root_dir must not contain path traversal segments".to_string(),
        ));
    }
    
    // Canonicalize to resolve symlinks and reject nonexistent paths
    let cwd = cwd
        .canonicalize()
        .map_err(|e| WebError::Config(format!("Invalid root_dir: {}", e)))?;

    // Ensure the path is a directory
    if !cwd.is_dir() {
        return Err(WebError::InvalidRequest(
            "root_dir must be a directory".to_string(),
        ));
    }

    // Reject obviously unsafe paths (system pseudo-filesystems)
    let path_str = cwd.to_string_lossy();
    let is_pseudo_fs = ["/proc", "/sys", "/dev"]
        .iter()
        .any(|prefix| path_str == *prefix || path_str.starts_with(&format!("{}/", prefix)));
    if is_pseudo_fs {
        return Err(WebError::InvalidRequest(
            "root_dir must not point to a system pseudo-filesystem".to_string(),
        ));
    }
    let project_identity = nexus_core::ProjectIdentity::resolve(&cwd);
    let nexus_dir = project_identity.root_dir.join(".nexus");

    let mut cache = nexus_agent::CognitiveCache::load_or_init(&nexus_dir);

    let config = nexus_core::Config::from_env().unwrap_or_default();

    let relevance_score = request
        .boost_score
        .unwrap_or(memory.relevance_score.unwrap_or(0.85));
    let tier = nexus_agent::ConfidenceTier::from_score(relevance_score);

    let inserted = cache.hot_cache.promote(
        nexus_agent::HotCacheEntry {
            memory_id: memory.id,
            content: memory.content,
            relevance_score,
            tier,
            promoted_at: chrono::Utc::now(),
            last_surfaced: chrono::Utc::now(),
            hot_streak: 1,
            pinned: request.pin,
            source_agent: Some("web-ui".to_string()),
        },
        config.cognitive_system.hot_cache_max_entries,
    );

    if !inserted {
        return Ok(Json(AgentBoostResponse {
            success: false,
            error: Some("Cache at capacity with all entries pinned".to_string()),
        }));
    }

    cache
        .save(&nexus_dir)
        .map_err(|e| WebError::Storage(e.to_string()))?;

    Ok(Json(AgentBoostResponse {
        success: true,
        error: None,
    }))
}

/// GET /api/agent/status — Get agent status
pub async fn agent_status(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Result<Json<AgentStatusResponse>> {
    let state = state.read().await;

    match &state.agent_supervisor {
        Some(supervisor) => {
            let status = supervisor.get_status().await;
            Ok(Json(AgentStatusResponse {
                enabled: status.enabled,
                namespace: status.namespace,
                inbox_dir: status.inbox_dir,
                files_processed: status.files_processed,
                memories_consolidated: status.memories_consolidated,
                queries_answered: status.queries_answered,
                last_scan: status.last_scan.map(|d| d.to_rfc3339()),
                last_consolidation: status.last_consolidation.map(|d| d.to_rfc3339()),
                errors: status.errors,
                uptime_secs: state.uptime_seconds(),
            }))
        }
        None => Ok(Json(AgentStatusResponse {
            enabled: false,
            namespace: String::new(),
            inbox_dir: String::new(),
            files_processed: 0,
            memories_consolidated: 0,
            queries_answered: 0,
            last_scan: None,
            last_consolidation: None,
            errors: Vec::new(),
            uptime_secs: state.uptime_seconds(),
        })),
    }
}
