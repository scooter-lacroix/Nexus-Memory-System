//! Memory CRUD API endpoints

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use nexus_storage::repository::StoreMemoryParams;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::{
    error::{Result, WebError},
    models::{
        CreateMemoryRequest, MemoryCreateResponse, MemoryListResponse, MemoryResponse,
        SearchRequest, SearchResponse, UpdateMemoryRequest, WebSocketMessage,
    },
    state::AppState,
};

/// Query parameters for listing memories
#[derive(Debug, Deserialize)]
pub struct ListMemoriesQuery {
    #[serde(default = "default_agent_type")]
    pub agent_type: String,
    pub query: Option<String>,
    pub category: Option<String>,
    pub memory_lane_type: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_agent_type() -> String {
    "general".to_string()
}

fn default_limit() -> usize {
    20
}

/// List memories with optional filtering
pub async fn list_memories(
    State(state): State<Arc<RwLock<AppState>>>,
    Query(params): Query<ListMemoriesQuery>,
) -> Result<Json<MemoryListResponse>> {
    let state = state.read().await;

    // Get or create namespace
    let namespace = state
        .namespace_repo
        .get_or_create(&params.agent_type, &params.agent_type)
        .await?;

    // Search memories
    let memories = state
        .memory_repo
        .search_by_namespace(namespace.id, params.limit, params.offset)
        .await?;

    let total = state.memory_repo.count_by_namespace(namespace.id).await?;

    // Convert to response models
    let results: Vec<MemoryResponse> = memories.into_iter().map(MemoryResponse::from).collect();

    let filters = json!({
        "category": params.category,
        "memory_lane_type": params.memory_lane_type,
    });

    Ok(Json(MemoryListResponse {
        success: true,
        total,
        results,
        query: params.query,
        agent_type: params.agent_type,
        filters,
    }))
}

/// Create a new memory
pub async fn create_memory(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(request): Json<CreateMemoryRequest>,
) -> Result<(StatusCode, Json<MemoryCreateResponse>)> {
    let state = state.read().await;

    // Validate content
    if request.content.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "Content cannot be empty".to_string(),
        ));
    }

    // Get or create namespace
    let namespace = state
        .namespace_repo
        .get_or_create(&request.agent_type, &request.agent_type)
        .await?;

    // Store memory
    let memory = state
        .memory_repo
        .store(StoreMemoryParams {
            namespace_id: namespace.id,
            content: &request.content,
            category: &request.category,
            memory_lane_type: request.memory_lane_type.as_ref(),
            labels: &request.labels,
            metadata: &request.metadata,
            embedding: None,
            embedding_model: None,
        })
        .await?;

    // Broadcast to WebSocket clients
    let memory_response = MemoryResponse::from(memory.clone());
    let ws_msg = WebSocketMessage::memory_stored(&memory_response, &request.agent_type);
    let _ = state.broadcast_ws(ws_msg);

    info!(
        "Memory created: id={}, agent_type={}",
        memory.id, request.agent_type
    );

    Ok((
        StatusCode::CREATED,
        Json(MemoryCreateResponse {
            success: true,
            memory_id: Some(memory.id),
            agent_type: request.agent_type,
            category: request.category.to_string(),
            error: None,
        }),
    ))
}

/// Get a specific memory by ID
pub async fn get_memory(
    State(state): State<Arc<RwLock<AppState>>>,
    Path(id): Path<i64>,
) -> Result<Json<MemoryResponse>> {
    let state = state.read().await;

    let memory = state
        .memory_repo
        .get_by_id(id)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Memory {} not found", id)))?;

    // Update access count
    let _ = state.memory_repo.touch(id).await;

    Ok(Json(MemoryResponse::from(memory)))
}

/// Update an existing memory
pub async fn update_memory(
    State(state): State<Arc<RwLock<AppState>>>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateMemoryRequest>,
) -> Result<Json<MemoryResponse>> {
    let state = state.read().await;

    // Check if memory exists
    let existing = state
        .memory_repo
        .get_by_id(id)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Memory {} not found", id)))?;

    // Build update query dynamically
    let mut updates: Vec<String> = Vec::new();

    if let Some(content) = request.content {
        if !content.trim().is_empty() {
            updates.push(format!("content = '{}'", content.replace("'", "''")));
        }
    }

    if let Some(category) = request.category {
        updates.push(format!("category = '{}'", category));
    }

    if let Some(memory_lane_type) = request.memory_lane_type {
        updates.push(format!("memory_lane_type = '{}'", memory_lane_type));
    }

    if let Some(labels) = request.labels {
        match serde_json::to_string(&labels) {
            Ok(labels_json) => {
                updates.push(format!("labels = '{}'", labels_json.replace("'", "''")));
            }
            Err(e) => {
                warn!(error = %e, "Failed to serialize labels for memory update; labels omitted from SQL update");
                // Do NOT push an empty-string assignment — that would corrupt the row.
            }
        }
    }

    if let Some(metadata) = request.metadata {
        match serde_json::to_string(&metadata) {
            Ok(metadata_json) => {
                updates.push(format!("metadata = '{}'", metadata_json.replace("'", "''")));
            }
            Err(e) => {
                warn!(error = %e, "Failed to serialize metadata for memory update; metadata omitted from SQL update");
                // Do NOT push an empty-string assignment — that would corrupt the row.
            }
        }
    }

    if let Some(is_active) = request.is_active {
        updates.push(format!("is_active = {}", if is_active { 1 } else { 0 }));
    }

    if let Some(is_archived) = request.is_archived {
        updates.push(format!("is_archived = {}", if is_archived { 1 } else { 0 }));
    }

    if updates.is_empty() {
        return Ok(Json(MemoryResponse::from(existing)));
    }

    updates.push(format!("updated_at = '{}'", Utc::now().to_rfc3339()));

    let query = format!("UPDATE memories SET {} WHERE id = ?", updates.join(", "));

    sqlx::query(&query)
        .bind(id)
        .execute(state.pool())
        .await
        .map_err(|e| WebError::Storage(e.to_string()))?;

    // Fetch updated memory
    let updated = state
        .memory_repo
        .get_by_id(id)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Memory {} not found after update", id)))?;

    // Broadcast update
    let ws_msg = WebSocketMessage::memory_updated(id);
    let _ = state.broadcast_ws(ws_msg);

    info!("Memory updated: id={}", id);

    Ok(Json(MemoryResponse::from(updated)))
}

/// Delete a memory (soft delete)
pub async fn delete_memory(
    State(state): State<Arc<RwLock<AppState>>>,
    Path(id): Path<i64>,
) -> Result<StatusCode> {
    let state = state.read().await;

    // Check if memory exists
    let _ = state
        .memory_repo
        .get_by_id(id)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Memory {} not found", id)))?;

    // Soft delete: mark as inactive and archived
    sqlx::query("UPDATE memories SET is_active = 0, is_archived = 1, updated_at = ? WHERE id = ?")
        .bind(Utc::now())
        .bind(id)
        .execute(state.pool())
        .await
        .map_err(|e| WebError::Storage(e.to_string()))?;

    // Broadcast deletion
    let ws_msg = WebSocketMessage::memory_deleted(id);
    let _ = state.broadcast_ws(ws_msg);

    info!("Memory deleted: id={}", id);

    Ok(StatusCode::NO_CONTENT)
}

/// Search memories using semantic or text search
pub async fn search_memories(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<SearchResponse>> {
    let state = state.read().await;

    // Validate query
    if request.query.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "Query cannot be empty".to_string(),
        ));
    }

    // Get namespace
    let namespace = state
        .namespace_repo
        .get_or_create(&request.agent_type, &request.agent_type)
        .await?;

    // For now, use text-based search (semantic search would require embeddings)
    // Search in content using LIKE
    let search_pattern = format!(
        "%{}%",
        request.query.replace("%", "\\%").replace("_", "\\_")
    );

    let query_str = "SELECT * FROM memories WHERE namespace_id = ? AND is_active = 1 AND content LIKE ? ORDER BY created_at DESC LIMIT ? OFFSET ?".to_string();

    let rows: Vec<nexus_storage::models::MemoryRow> = sqlx::query_as(&query_str)
        .bind(namespace.id)
        .bind(&search_pattern)
        .bind(request.limit as i64)
        .bind(request.offset as i64)
        .fetch_all(state.pool())
        .await
        .map_err(|e| WebError::Storage(e.to_string()))?;

    // Convert rows to memories
    let memories: Vec<nexus_core::Memory> = rows
        .into_iter()
        .map(row_to_memory)
        .collect::<crate::error::Result<Vec<_>>>()?;

    let results: Vec<MemoryResponse> = memories.into_iter().map(MemoryResponse::from).collect();

    let total = results.len() as i64;

    let filters = json!({
        "category": request.category.map(|c| c.to_string()),
        "memory_lane_type": request.memory_lane_type.map(|t| t.to_string()),
        "threshold": request.threshold,
    });

    Ok(Json(SearchResponse {
        success: true,
        results,
        total,
        query: request.query,
        agent_type: request.agent_type,
        filters,
        error: None,
    }))
}

/// Convert a database row to a Memory
fn row_to_memory(
    row: nexus_storage::models::MemoryRow,
) -> crate::error::Result<nexus_core::Memory> {
    use nexus_core::{Memory, MemoryCategory, MemoryLaneType};

    let labels: Vec<String> = serde_json::from_str(&row.labels).map_err(|e| {
        crate::error::WebError::Storage(format!("corrupted labels JSON for memory {}: {e}", row.id))
    })?;
    let metadata: serde_json::Value = serde_json::from_str(&row.metadata).map_err(|e| {
        crate::error::WebError::Storage(format!(
            "corrupted metadata JSON for memory {}: {e}",
            row.id
        ))
    })?;
    let embedding: Option<Vec<f32>> = row
        .content_embedding
        .map(|e| {
            serde_json::from_str(&e).map_err(|err| {
                crate::error::WebError::Storage(format!(
                    "corrupted embedding JSON for memory {}: {err}",
                    row.id
                ))
            })
        })
        .transpose()?;

    Ok(Memory {
        id: row.id,
        namespace_id: row.namespace_id,
        content: row.content,
        category: MemoryCategory::parse(&row.category).ok_or_else(|| {
            WebError::Storage(format!(
                "Unknown memory category '{}' persisted in database; row may be corrupted",
                row.category
            ))
        })?,
        memory_lane_type: match &row.memory_lane_type {
            Some(s) => Some(MemoryLaneType::parse(s).ok_or_else(|| {
                WebError::Storage(format!(
                    "Unknown memory_lane_type '{}' persisted in database; row may be corrupted",
                    s
                ))
            })?),
            None => None,
        },
        labels,
        metadata,
        similarity_score: row.similarity_score,
        relevance_score: row.relevance_score,
        content_embedding: embedding,
        embedding_model: row.embedding_model,
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_accessed: row.last_accessed,
        is_active: row.is_active,
        is_archived: row.is_archived,
        access_count: row.access_count,
    })
}
