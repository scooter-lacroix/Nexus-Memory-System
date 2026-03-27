//! Nexus Web Dashboard - Axum-based web interface for Nexus Memory System
//!
//! This crate provides:
//! - REST API endpoints for memory CRUD operations
//! - WebSocket real-time updates
//! - Static file serving for the dashboard UI
//! - CORS and security middleware
//!
//! ## Example
//!
//! ```rust,ignore
//! use nexus_memory_web::WebDashboard;
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let dashboard = WebDashboard::new(manager).await?;
//!     let addr = SocketAddr::from(([0, 0, 0, 0], 8768));
//!     dashboard.serve(addr).await?;
//!     Ok(())
//! }
//! ```

pub mod api;
pub mod error;
pub mod models;
pub mod state;
pub mod websocket;

use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::info;

pub use error::{Result, WebError};
pub use models::*;
pub use state::AppState;

use api::{
    agent_consolidate, agent_ingest, agent_query, agent_status, cognition_overview, create_memory,
    create_namespace, delete_memory, get_agent_stats, get_memory, get_namespace, get_stats,
    health_check, job_summary, list_digests, list_jobs, list_memories, list_namespaces,
    reflection_state, runtime_health, search_memories, update_memory,
};
use websocket::websocket_handler;

/// Web Dashboard for Nexus Memory System
pub struct WebDashboard {
    router: Router,
    state: Arc<RwLock<AppState>>,
}

impl WebDashboard {
    /// Create a new web dashboard instance
    pub async fn new(
        storage: nexus_storage::StorageManager,
        orchestrator: nexus_orchestrator::Orchestrator,
    ) -> Result<Self> {
        let state = Arc::new(RwLock::new(AppState::new(storage, orchestrator).await?));
        let router = Self::build_router(state.clone());

        Ok(Self { router, state })
    }

    /// Build the Axum router with all routes
    fn build_router(state: Arc<RwLock<AppState>>) -> Router {
        // CORS layer
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        // API routes
        let api_routes = Router::new()
            // Memory endpoints
            .route("/memories", get(list_memories).post(create_memory))
            .route(
                "/memories/{id}",
                get(get_memory).put(update_memory).delete(delete_memory),
            )
            .route("/memories/search", post(search_memories))
            // Namespace endpoints
            .route("/namespaces", get(list_namespaces).post(create_namespace))
            .route("/namespaces/{id}", get(get_namespace))
            // Stats endpoints
            .route("/stats", get(get_stats))
            .route("/stats/{agent}", get(get_agent_stats))
            // Health check
            .route("/health", get(health_check))
            // Agent endpoints
            .route("/agent/ingest", post(agent_ingest))
            .route("/agent/query", post(agent_query))
            .route("/agent/consolidate", post(agent_consolidate))
            .route("/agent/status", get(agent_status))
            // Cognition observability endpoints
            .route("/cognition/jobs", get(list_jobs))
            .route("/cognition/jobs/summary", get(job_summary))
            .route("/cognition/digests", get(list_digests))
            .route("/cognition/overview", get(cognition_overview))
            .route("/cognition/reflection", get(reflection_state))
            .route("/cognition/runtime", get(runtime_health));

        // WebSocket route
        let ws_route = Router::new().route("/ws", get(websocket_handler));

        // Combine all routes
        Router::new()
            .nest("/api", api_routes)
            .merge(ws_route)
            // Serve static files from the static directory
            .fallback_service(ServeDir::new("src/static").append_index_html_on_directories(true))
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .with_state(state)
    }

    /// Serve the web dashboard on the specified address
    pub async fn serve(self, addr: SocketAddr) -> Result<()> {
        info!("Starting Nexus Web Dashboard on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| WebError::ServerStart(e.to_string()))?;

        info!("Web Dashboard listening on http://{}", addr);

        axum::serve(listener, self.router)
            .await
            .map_err(|e| WebError::ServerStart(e.to_string()))?;

        Ok(())
    }

    /// Get a clone of the state
    pub fn state(&self) -> Arc<RwLock<AppState>> {
        self.state.clone()
    }
}

/// Create a new web dashboard with the given storage and orchestrator
pub async fn create_dashboard(
    storage: nexus_storage::StorageManager,
    orchestrator: nexus_orchestrator::Orchestrator,
) -> Result<WebDashboard> {
    WebDashboard::new(storage, orchestrator).await
}

/// Run the web dashboard on the default port (8768)
pub async fn run_default(
    storage: nexus_storage::StorageManager,
    orchestrator: nexus_orchestrator::Orchestrator,
) -> Result<()> {
    let dashboard = WebDashboard::new(storage, orchestrator).await?;
    let addr = SocketAddr::from(([0, 0, 0, 0], 8768));
    dashboard.serve(addr).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use nexus_orchestrator::Orchestrator;
    use tower::ServiceExt;

    #[test]
    fn test_web_error_display() {
        let err = WebError::ServerStart("test error".to_string());
        assert!(err.to_string().contains("test error"));
    }

    #[tokio::test]
    async fn test_production_router_exposes_cognition_runtime_route() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect to in-memory db");
        nexus_storage::migrations::run_migrations(&pool)
            .await
            .expect("run migrations");

        let mut storage = nexus_storage::StorageManager::new(pool.clone());
        storage.initialize().await.expect("initialize storage");

        let dashboard = WebDashboard::new(storage, Orchestrator::default())
            .await
            .expect("create dashboard");

        let resp = dashboard
            .router
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/runtime")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }
}
