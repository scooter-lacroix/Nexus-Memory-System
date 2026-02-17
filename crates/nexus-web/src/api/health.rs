//! Health check endpoint

use axum::{extract::State, Json};
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{error::Result, models::HealthResponse, state::AppState};

/// Health check endpoint
pub async fn health_check(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Result<Json<HealthResponse>> {
    let state = state.read().await;
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(state.pool())
        .await
        .map_err(|e| {
            crate::error::WebError::Storage(format!("Health check database probe failed: {}", e))
        })?;

    Ok(Json(HealthResponse {
        status: "healthy".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}
