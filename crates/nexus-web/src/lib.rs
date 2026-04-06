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
use http::HeaderValue;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::info;
use url::Url;

pub use error::{Result, WebError};
pub use models::*;
pub use state::AppState;

use api::{
    agent_consolidate, agent_ingest, agent_query, agent_status, cognition_overview, create_memory,
    create_namespace, dashboard, delete_memory, get_agent_stats, get_memory, get_namespace,
    get_stats, health_check, job_summary, list_digests, list_jobs, list_memories, list_namespaces,
    query_introspection, reflection_state, runtime_health, search_memories, update_memory,
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
        // CORS Layer — restrict to exact local-only origins.
        // Parses the Origin header as a URL and compares host exactly
        // to prevent prefix-spoofing attacks (e.g. http://localhost.evil.com).
        let cors = CorsLayer::new()
            .allow_origin(AllowOrigin::predicate(
                |origin: &HeaderValue, _request: &http::request::Parts| {
                    let origin_str = origin.to_str().unwrap_or("");
                    match Url::parse(origin_str) {
                        Ok(url) => {
                            let host = url.host_str().unwrap_or("");
                            let scheme = url.scheme();
                            // Only allow exact localhost / 127.0.0.1 origins
                            (scheme == "http" || scheme == "https")
                                && (host == "127.0.0.1" || host == "localhost")
                        }
                        Err(_) => false, // Malformed origins are rejected
                    }
                },
            ))
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
            ])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::ACCEPT,
                axum::http::header::ORIGIN,
            ]);

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
            .route("/cognition/runtime", get(runtime_health))
            .route("/cognition/query-introspection", get(query_introspection))
            .route("/cognition/dashboard", get(dashboard));

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
    use axum::body::to_bytes;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use nexus_orchestrator::Orchestrator;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    fn body_to_json(body: axum::body::Bytes) -> Value {
        serde_json::from_slice(&body).expect("valid JSON")
    }

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

    #[tokio::test]
    async fn test_production_router_exposes_cognition_dashboard_route() {
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

        // Dashboard requires a namespace, so this will be 400 (missing namespace).
        let resp = dashboard
            .router
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_update_memory_persists_native_sql_values() {
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

        let memory_id = {
            let state = dashboard.state.read().await;
            let namespace = state
                .namespace_repo
                .get_or_create("web-update-test", "test-agent")
                .await
                .expect("create namespace");
            state
                .memory_repo
                .store(nexus_storage::StoreMemoryParams {
                    namespace_id: namespace.id,
                    content: "original content",
                    category: &nexus_core::MemoryCategory::General,
                    memory_lane_type: None,
                    labels: &["initial".to_string()],
                    metadata: &json!({"before": true}),
                    embedding: None,
                    embedding_model: None,
                })
                .await
                .expect("store memory")
                .id
        };

        let resp = dashboard
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/memories/{memory_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "content": "updated content",
                            "category": "facts",
                            "memory_lane_type": "decision",
                            "labels": ["updated", "native-bindings"],
                            "metadata": {"source": "test"},
                            "is_active": true,
                            "is_archived": false
                        }))
                        .expect("serialize request"),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = resp.status();
        let body = to_bytes(resp.into_body(), 1_000_000).await.unwrap();
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected response body: {}",
            String::from_utf8_lossy(&body)
        );
        let json = body_to_json(body);
        assert_eq!(json["content"], "updated content");
        assert_eq!(json["category"], "facts");
        assert_eq!(json["memory_lane_type"], "decision");
        assert_eq!(json["metadata"]["source"], "test");

        let row: (String, String, String, i64, i64) = sqlx::query_as(
            "SELECT category, memory_lane_type, metadata, is_active, is_archived FROM memories WHERE id = ?",
        )
        .bind(memory_id)
        .fetch_one(&pool)
        .await
        .expect("fetch updated row");

        assert_eq!(row.0, "facts");
        assert_eq!(row.1, "decision");
        assert_eq!(row.2, r#"{"source":"test"}"#);
        assert_eq!(row.3, 1);
        assert_eq!(row.4, 0);
    }
}
