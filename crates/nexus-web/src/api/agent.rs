//! Agent API endpoints for the always-on memory agent

use axum::{extract::State, Json};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;

use crate::error::{Result, WebError};
use crate::models::{
    AgentConsolidateResponse, AgentIngestRequest, AgentIngestResponse, AgentQueryRequest,
    AgentQueryResponse, AgentStatusResponse,
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
