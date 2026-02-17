//! Error types for the web dashboard

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Result type for web operations
pub type Result<T> = std::result::Result<T, WebError>;

/// Error types for the web dashboard
#[derive(Error, Debug)]
pub enum WebError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Server error: {0}")]
    ServerStart(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(String),
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            WebError::Storage(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            WebError::Orchestrator(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            WebError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            WebError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            WebError::ServerStart(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            WebError::Serialization(_) => (StatusCode::BAD_REQUEST, "Invalid JSON".to_string()),
            WebError::WebSocket(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
        };

        let body = Json(json!({
            "success": false,
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

impl From<nexus_core::NexusError> for WebError {
    fn from(err: nexus_core::NexusError) -> Self {
        match err {
            nexus_core::NexusError::Storage(msg) => WebError::Storage(msg),
            nexus_core::NexusError::MemoryNotFound(id) => {
                WebError::NotFound(format!("Memory not found: {}", id))
            }
            nexus_core::NexusError::NamespaceNotFound(name) => {
                WebError::NotFound(format!("Namespace not found: {}", name))
            }
            nexus_core::NexusError::InvalidInput(msg) => WebError::InvalidRequest(msg),
            nexus_core::NexusError::InvalidConfig(msg) => WebError::InvalidRequest(msg),
            _ => WebError::Storage(err.to_string()),
        }
    }
}

impl From<nexus_orchestrator::OrchestratorError> for WebError {
    fn from(err: nexus_orchestrator::OrchestratorError) -> Self {
        WebError::Orchestrator(err.to_string())
    }
}

impl From<sqlx::Error> for WebError {
    fn from(err: sqlx::Error) -> Self {
        WebError::Storage(err.to_string())
    }
}
