//! Namespace API endpoints

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::{
    error::{Result, WebError},
    models::{CreateNamespaceRequest, NamespaceListResponse, NamespaceResponse},
    state::AppState,
};

/// List all namespaces
pub async fn list_namespaces(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Result<Json<NamespaceListResponse>> {
    let state = state.read().await;

    let namespaces = state.namespace_repo.list_all().await?;

    let namespaces: Vec<NamespaceResponse> = namespaces
        .into_iter()
        .map(NamespaceResponse::from)
        .collect();

    let total = namespaces.len();

    Ok(Json(NamespaceListResponse {
        success: true,
        namespaces,
        total,
    }))
}

/// Get a specific namespace by ID
pub async fn get_namespace(
    State(state): State<Arc<RwLock<AppState>>>,
    Path(id): Path<String>,
) -> Result<Json<NamespaceResponse>> {
    let state = state.read().await;

    // Try to parse as i64 first, otherwise treat as name
    let namespace = if let Ok(id_num) = id.parse::<i64>() {
        // Search by ID - we need to list all and find by ID
        let all = state.namespace_repo.list_all().await?;
        all.into_iter().find(|n| n.id == id_num)
    } else {
        // Search by name
        state.namespace_repo.get_by_name(&id).await?
    };

    let namespace =
        namespace.ok_or_else(|| WebError::NotFound(format!("Namespace '{}' not found", id)))?;

    Ok(Json(NamespaceResponse::from(namespace)))
}

/// Create a new namespace
pub async fn create_namespace(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(request): Json<CreateNamespaceRequest>,
) -> Result<(StatusCode, Json<NamespaceResponse>)> {
    let state = state.read().await;

    // Validate name
    if request.name.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "Namespace name cannot be empty".to_string(),
        ));
    }

    // Check if namespace already exists
    if let Some(existing) = state.namespace_repo.get_by_name(&request.name).await? {
        return Ok((StatusCode::OK, Json(NamespaceResponse::from(existing))));
    }

    // Create namespace
    let namespace = state
        .namespace_repo
        .get_or_create(&request.name, &request.agent_type)
        .await?;

    info!(
        "Namespace created: id={}, name={}",
        namespace.id, namespace.name
    );

    Ok((
        StatusCode::CREATED,
        Json(NamespaceResponse::from(namespace)),
    ))
}
