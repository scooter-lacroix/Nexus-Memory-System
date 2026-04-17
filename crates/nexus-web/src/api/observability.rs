//! Observability API endpoints for jobs, digests, and runtime health.

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

use crate::error::{Result, WebError};
use crate::models::{
    AdaptiveDreamState, CognitionOverviewResponse, DashboardResponse, DigestEntry,
    DigestFreshnessState, DigestListResponse, DreamState, JobEntry, JobListResponse,
    JobSummaryResponse, QueryIntrospectionResponse, RecallComposition, ReflectionSampleEntry,
    ReflectionStateResponse, RuntimeResponse,
};
use crate::state::AppState;
use nexus_core::CognitiveLevel;

// ---- Query parameter structs ----

#[derive(Debug, Deserialize, Default)]
pub struct JobQueryParams {
    pub namespace: String,
    pub status: Option<String>,
    pub job_type: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Debug, Deserialize, Default)]
pub struct JobSummaryQueryParams {
    pub namespace: String,
    pub job_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DigestQueryParams {
    pub namespace: String,
    pub session_key: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

// ---- Handlers ----

/// GET /api/cognition/jobs — list memory jobs for a namespace.
pub async fn list_jobs(
    State(state): State<Arc<RwLock<AppState>>>,
    Query(params): Query<JobQueryParams>,
) -> Result<Json<JobListResponse>> {
    if params.namespace.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "namespace query parameter is required".to_string(),
        ));
    }

    let state = state.read().await;

    let namespace = state
        .namespace_repo
        .get_by_name(&params.namespace)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Namespace '{}' not found", params.namespace)))?;

    let limit = params.limit.clamp(1, 200);
    let offset = params.offset.max(0);

    let rows = state
        .memory_repo
        .list_jobs(
            namespace.id,
            params.job_type.as_deref(),
            params.status.as_deref(),
            limit,
            offset,
        )
        .await?;

    let total = state
        .memory_repo
        .count_jobs(
            namespace.id,
            params.job_type.as_deref(),
            params.status.as_deref(),
        )
        .await?;

    let jobs: Vec<JobEntry> = rows.into_iter().map(JobEntry::from).collect();

    Ok(Json(JobListResponse {
        success: true,
        namespace: params.namespace,
        jobs,
        total,
    }))
}

/// GET /api/cognition/jobs/summary — job counts grouped by status.
pub async fn job_summary(
    State(state): State<Arc<RwLock<AppState>>>,
    Query(params): Query<JobSummaryQueryParams>,
) -> Result<Json<JobSummaryResponse>> {
    if params.namespace.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "namespace query parameter is required".to_string(),
        ));
    }

    let state = state.read().await;

    let namespace = state
        .namespace_repo
        .get_by_name(&params.namespace)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Namespace '{}' not found", params.namespace)))?;

    let rows = state
        .memory_repo
        .count_jobs_by_status(namespace.id, params.job_type.as_deref())
        .await?;

    let counts = rows.into_iter().collect();

    Ok(Json(JobSummaryResponse {
        success: true,
        namespace: params.namespace,
        counts,
    }))
}

/// GET /api/cognition/digests — list session digests for a namespace.
pub async fn list_digests(
    State(state): State<Arc<RwLock<AppState>>>,
    Query(params): Query<DigestQueryParams>,
) -> Result<Json<DigestListResponse>> {
    if params.namespace.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "namespace query parameter is required".to_string(),
        ));
    }

    let state = state.read().await;

    let namespace = state
        .namespace_repo
        .get_by_name(&params.namespace)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Namespace '{}' not found", params.namespace)))?;

    let limit = params.limit.clamp(1, 200);
    let offset = params.offset.max(0);

    let rows = state
        .memory_repo
        .list_digests(namespace.id, params.session_key.as_deref(), limit, offset)
        .await?;

    let total = state
        .memory_repo
        .count_digests(namespace.id, params.session_key.as_deref())
        .await?;

    let digests: Vec<DigestEntry> = rows.into_iter().map(DigestEntry::from).collect();

    Ok(Json(DigestListResponse {
        success: true,
        namespace: params.namespace,
        digests,
        total,
    }))
}

/// GET /api/cognition/runtime — runtime health info from AppState.
pub async fn runtime_health(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Result<Json<RuntimeResponse>> {
    let state = state.read().await;

    // Probe the DB connection.
    let db_connected = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(state.pool())
        .await
        .is_ok();

    let agent_enabled = state.agent_supervisor.is_some();
    let active_sessions = state.orchestrator.active_session_count().await;

    Ok(Json(RuntimeResponse {
        success: true,
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.uptime_seconds(),
        db_connected,
        agent_enabled,
        active_sessions,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct OverviewQueryParams {
    pub namespace: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct ReflectionQueryParams {
    pub namespace: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

/// GET /api/cognition/overview — aggregated cognition state for a namespace.
pub async fn cognition_overview(
    State(state): State<Arc<RwLock<AppState>>>,
    Query(params): Query<OverviewQueryParams>,
) -> Result<Json<CognitionOverviewResponse>> {
    if params.namespace.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "namespace query parameter is required".to_string(),
        ));
    }

    let state = state.read().await;

    let namespace = state
        .namespace_repo
        .get_by_name(&params.namespace)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Namespace '{}' not found", params.namespace)))?;

    let status_rows = state
        .memory_repo
        .count_jobs_by_status(namespace.id, None)
        .await?;
    let jobs_by_status: std::collections::HashMap<String, i64> = status_rows.into_iter().collect();

    let digest_count = state.memory_repo.count_digests(namespace.id, None).await?;

    let evidence_count = state.memory_repo.count_evidence(namespace.id).await?;
    let stage_metrics = state
        .memory_repo
        .latest_metrics_for_namespace(namespace.id, Some("cognition."), 64)
        .await?
        .into_iter()
        .fold(
            std::collections::HashMap::new(),
            |mut acc: std::collections::HashMap<String, f64>, metric| {
                acc.entry(metric.metric_name).or_insert(metric.metric_value);
                acc
            },
        );

    Ok(Json(CognitionOverviewResponse {
        success: true,
        namespace: params.namespace,
        jobs_by_status,
        digest_count,
        evidence_count,
        stage_metrics,
    }))
}

/// GET /api/cognition/reflection — derived and contradiction observability for a namespace.
pub async fn reflection_state(
    State(state): State<Arc<RwLock<AppState>>>,
    Query(params): Query<ReflectionQueryParams>,
) -> Result<Json<ReflectionStateResponse>> {
    if params.namespace.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "namespace query parameter is required".to_string(),
        ));
    }

    let state = state.read().await;

    let namespace = state
        .namespace_repo
        .get_by_name(&params.namespace)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Namespace '{}' not found", params.namespace)))?;

    let limit = params.limit.clamp(1, 50);
    let contradiction_count = state
        .memory_repo
        .count_by_cognitive_level(namespace.id, CognitiveLevel::Contradiction)
        .await?;
    let derived_count = state
        .memory_repo
        .count_by_cognitive_level(namespace.id, CognitiveLevel::Derived)
        .await?;
    let recent_contradictions = state
        .memory_repo
        .get_by_cognitive_level(namespace.id, CognitiveLevel::Contradiction, limit)
        .await?
        .into_iter()
        .map(ReflectionSampleEntry::from)
        .collect();
    let recent_derived = state
        .memory_repo
        .get_by_cognitive_level(namespace.id, CognitiveLevel::Derived, limit)
        .await?
        .into_iter()
        .map(ReflectionSampleEntry::from)
        .collect();

    Ok(Json(ReflectionStateResponse {
        success: true,
        namespace: params.namespace,
        contradiction_count,
        derived_count,
        recent_contradictions,
        recent_derived,
    }))
}

// ---- Query Introspection ----

#[derive(Debug, Deserialize, Default)]
pub struct QueryIntrospectionQueryParams {
    pub namespace: String,
    pub question: String,
}

/// GET /api/cognition/query-introspection — ranking decision introspection.
///
/// Purely structural analysis (no LLM calls). Returns included/excluded
/// memories with per-memory signals, bucket stats, and relevant reflections.
/// Works without agent supervisor enabled.
pub async fn query_introspection(
    State(state): State<Arc<RwLock<AppState>>>,
    Query(params): Query<QueryIntrospectionQueryParams>,
) -> Result<Json<QueryIntrospectionResponse>> {
    if params.namespace.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "namespace query parameter is required".to_string(),
        ));
    }
    if params.question.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "question query parameter is required".to_string(),
        ));
    }

    let state = state.read().await;

    let namespace = state
        .namespace_repo
        .get_by_name(&params.namespace)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Namespace '{}' not found", params.namespace)))?;

    let query_context_limit = nexus_core::Config::from_env()
        .map(|config| config.agent.query_context_limit)
        .unwrap_or_else(|_| nexus_core::config::AgentConfig::default().query_context_limit);

    let request = nexus_core::WorkingRepresentationRequest {
        namespace_id: namespace.id,
        perspective: None,
        query: Some(params.question.clone()),
        max_items: query_context_limit,
        include_raw: false,
        ..nexus_core::WorkingRepresentationRequest::default()
    };

    let introspection =
        nexus_agent::introspect_query(&request, &params.question, &state.memory_repo)
            .await
            .map_err(|e| WebError::Storage(format!("Introspection failed: {}", e)))?;

    Ok(Json(QueryIntrospectionResponse {
        success: true,
        namespace: params.namespace,
        question: params.question,
        introspection,
    }))
}

// ---- Operator Dashboard ----

#[derive(Debug, Deserialize, Default)]
pub struct DashboardQueryParams {
    pub namespace: String,
}

/// GET /api/cognition/dashboard — at-a-glance operator view of dream, digest, recall, and adaptive state.
pub async fn dashboard(
    State(state): State<Arc<RwLock<AppState>>>,
    Query(params): Query<DashboardQueryParams>,
) -> Result<Json<DashboardResponse>> {
    if params.namespace.trim().is_empty() {
        return Err(WebError::InvalidRequest(
            "namespace query parameter is required".to_string(),
        ));
    }

    let state = state.read().await;

    let namespace = state
        .namespace_repo
        .get_by_name(&params.namespace)
        .await?
        .ok_or_else(|| WebError::NotFound(format!("Namespace '{}' not found", params.namespace)))?;

    // --- Dream throughput ---
    let completed_reflections = state
        .memory_repo
        .count_jobs(namespace.id, Some("reflect_namespace"), Some("completed"))
        .await?
        + state
            .memory_repo
            .count_jobs(namespace.id, Some("reflect_perspective"), Some("completed"))
            .await?;
    let completed_digests = state
        .memory_repo
        .count_jobs(namespace.id, Some("digest_session"), Some("completed"))
        .await?;
    let failed_jobs = state
        .memory_repo
        .count_jobs(namespace.id, None, Some("failed"))
        .await?;
    let pending_jobs = state
        .memory_repo
        .count_jobs(namespace.id, None, Some("enqueued"))
        .await?;

    // Most recent completed dream job (reflection or digest).
    let last_dream_at = {
        let reflect_jobs = state
            .memory_repo
            .list_jobs(
                namespace.id,
                Some("reflect_namespace"),
                Some("completed"),
                1,
                0,
            )
            .await
            .unwrap_or_else(|e| {
                warn!(error = %e, "Failed to list reflection jobs for dashboard");
                Vec::new()
            });
        let digest_jobs = state
            .memory_repo
            .list_jobs(
                namespace.id,
                Some("digest_session"),
                Some("completed"),
                1,
                0,
            )
            .await
            .unwrap_or_else(|e| {
                warn!(error = %e, "Failed to list digest jobs for dashboard");
                Vec::new()
            });
        let most_recent = reflect_jobs
            .iter()
            .chain(digest_jobs.iter())
            .max_by_key(|j| j.updated_at.as_str());
        most_recent.map(|j| j.updated_at.clone())
    };

    // --- Digest freshness ---
    let total_digests = state.memory_repo.count_digests(namespace.id, None).await?;
    let sessions_with_cognition = state
        .memory_repo
        .count_distinct_session_keys_with_cognition(namespace.id)
        .await?;

    let (latest_digest_at, latest_digest_age_seconds) = {
        let recent = state
            .memory_repo
            .list_digests(namespace.id, None, 1, 0)
            .await
            .unwrap_or_else(|e| {
                warn!(error = %e, "Failed to list digests for dashboard");
                Vec::new()
            });
        match recent.into_iter().next() {
            Some(d) => match parse_timestamp(&d.created_at) {
                Some(dt) => {
                    let age = chrono::Utc::now()
                        .signed_duration_since(dt)
                        .num_seconds()
                        .max(0);
                    (Some(d.created_at), Some(age))
                }
                None => {
                    warn!(
                        created_at = %d.created_at,
                        "Malformed digest timestamp; returning None for age"
                    );
                    (None, None)
                }
            },
            None => (None, None),
        }
    };

    // --- Recall composition ---
    let raw = state
        .memory_repo
        .count_by_cognitive_level(namespace.id, CognitiveLevel::Raw)
        .await?;
    let explicit = state
        .memory_repo
        .count_by_cognitive_level(namespace.id, CognitiveLevel::Explicit)
        .await?;
    let derived = state
        .memory_repo
        .count_by_cognitive_level(namespace.id, CognitiveLevel::Derived)
        .await?;
    let summary_short = state
        .memory_repo
        .count_by_cognitive_level(namespace.id, CognitiveLevel::SummaryShort)
        .await?;
    let summary_long = state
        .memory_repo
        .count_by_cognitive_level(namespace.id, CognitiveLevel::SummaryLong)
        .await?;
    let contradiction = state
        .memory_repo
        .count_by_cognitive_level(namespace.id, CognitiveLevel::Contradiction)
        .await?;
    let total = raw + explicit + derived + summary_short + summary_long + contradiction;

    // --- Adaptive dream state ---
    let cognition_config = match nexus_core::Config::from_env() {
        Ok(c) => c.cognition,
        Err(e) => {
            warn!(error = %e, "Failed to load cognition config for dashboard; using defaults");
            nexus_core::config::CognitionConfig::default()
        }
    };
    let contradiction_density = if total > 0 {
        contradiction as f64 / total as f64
    } else {
        0.0
    };

    let base_interval = cognition_config.adaptive_dream_max_interval_secs;
    let factor = 1.0 - ((contradiction as f32 * 0.10).min(0.9));
    let adapted = (base_interval as f32 * factor) as u64;
    let current_interval_secs = adapted.clamp(
        cognition_config.adaptive_dream_min_interval_secs,
        cognition_config.adaptive_dream_max_interval_secs,
    );

    Ok(Json(DashboardResponse {
        success: true,
        namespace: params.namespace,
        dream: DreamState {
            completed_reflections,
            completed_digests,
            failed_jobs,
            pending_jobs,
            last_dream_at,
        },
        digest: DigestFreshnessState {
            total_digests,
            sessions_with_cognition,
            latest_digest_age_seconds,
            latest_digest_at,
        },
        recall: RecallComposition {
            raw,
            explicit,
            derived,
            summary_short,
            summary_long,
            contradiction,
            total,
        },
        adaptive: AdaptiveDreamState {
            enabled: cognition_config.adaptive_dream_enabled,
            current_interval_secs,
            min_interval_secs: cognition_config.adaptive_dream_min_interval_secs,
            max_interval_secs: cognition_config.adaptive_dream_max_interval_secs,
            contradiction_count: contradiction,
            contradiction_density,
        },
    }))
}

/// Parse a timestamp string that may be in RFC3339 or SQLite `datetime('now')` format.
///
/// SQLite `datetime('now')` produces `YYYY-MM-DD HH:MM:SS` (no timezone suffix),
/// which is assumed to be UTC.  RFC3339 timestamps (if any exist) are tried first.
fn parse_timestamp(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    // Try RFC3339 first (explicitly timezone-annotated timestamps).
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // Fall back to SQLite datetime('now') format: "YYYY-MM-DD HH:MM:SS"
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(naive.and_utc());
    }
    // Try with fractional seconds: "YYYY-MM-DD HH:MM:SS.fff"
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(naive.and_utc());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use nexus_orchestrator::Orchestrator;
    use serde_json::Value;
    use std::sync::Arc;
    use tower::ServiceExt;

    struct TestApp {
        app: Router,
        state: Arc<RwLock<crate::state::AppState>>,
    }

    async fn test_app() -> TestApp {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect to in-memory db");
        nexus_storage::migrations::run_migrations(&pool)
            .await
            .expect("run migrations");

        let mut storage = nexus_storage::StorageManager::new(pool.clone());
        storage.initialize().await.expect("initialize storage");

        let orchestrator = Orchestrator::default();
        let state = Arc::new(RwLock::new(
            crate::state::AppState::new(storage, orchestrator)
                .await
                .expect("create app state"),
        ));

        let app = Router::new()
            .route("/api/cognition/jobs", get(list_jobs))
            .route("/api/cognition/jobs/summary", get(job_summary))
            .route("/api/cognition/digests", get(list_digests))
            .route("/api/cognition/overview", get(cognition_overview))
            .route("/api/cognition/reflection", get(reflection_state))
            .route("/api/cognition/runtime", get(runtime_health))
            .route(
                "/api/cognition/query-introspection",
                get(query_introspection),
            )
            .route("/api/cognition/dashboard", get(dashboard))
            .with_state(state.clone());

        TestApp { app, state }
    }

    /// Helper: create a namespace via the shared state.
    async fn create_namespace_in_test(state: &Arc<RwLock<crate::state::AppState>>, name: &str) {
        let s = state.read().await;
        s.namespace_repo
            .get_or_create(name, "test-agent")
            .await
            .expect("create namespace");
    }

    /// Helper: parse response body into JSON Value.
    fn body_to_json(body: axum::body::Bytes) -> Value {
        serde_json::from_slice(&body).expect("valid JSON")
    }

    #[tokio::test]
    async fn test_runtime_returns_honest_fields() {
        let test = test_app().await;
        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/runtime")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert!(!json["version"].as_str().unwrap().is_empty());
        assert!(json["uptime_seconds"].as_u64().is_some());
        assert!(json["db_connected"].is_boolean());
        assert!(json["agent_enabled"].is_boolean());
    }

    #[tokio::test]
    async fn test_query_introspection_missing_question_returns_400() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "intro-missing").await;

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/query-introspection?namespace=intro-missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_query_introspection_returns_structured_payload() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "intro-ns").await;

        {
            let state = test.state.read().await;
            let namespace = state
                .namespace_repo
                .get_by_name("intro-ns")
                .await
                .unwrap()
                .unwrap();

            state
                .memory_repo
                .store(nexus_storage::StoreMemoryParams {
                    namespace_id: namespace.id,
                    content: "Authentication now uses session cookies with http-only flags.",
                    category: &nexus_core::MemoryCategory::Facts,
                    memory_lane_type: None,
                    labels: &[],
                    metadata: &serde_json::json!({
                        "cognitive": {
                            "level": "explicit",
                            "observer": "claude-code",
                            "subject": "claude-code",
                            "generated_by": "test_fixture",
                            "confidence": 0.92
                        }
                    }),
                    embedding: None,
                    embedding_model: None,
                })
                .await
                .unwrap();
        }

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/query-introspection?namespace=intro-ns&question=session%20cookies")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["namespace"], "intro-ns");
        assert_eq!(json["question"], "session cookies");
        assert!(json["introspection"]["included"].is_array());
        assert!(!json["introspection"]["included"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(json["introspection"]["bucket_stats"].is_array());
    }

    #[tokio::test]
    async fn test_jobs_missing_namespace_returns_400() {
        let test = test_app().await;
        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/jobs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_jobs_unknown_namespace_returns_404() {
        let test = test_app().await;
        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/jobs?namespace=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_reflection_missing_namespace_returns_400() {
        let test = test_app().await;
        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/reflection")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_reflection_returns_counts_and_samples() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "reflect-ns").await;

        {
            let state = test.state.read().await;
            let namespace = state
                .namespace_repo
                .get_by_name("reflect-ns")
                .await
                .unwrap()
                .unwrap();

            for (content, level) in [
                ("derived insight", CognitiveLevel::Derived),
                ("contradiction note", CognitiveLevel::Contradiction),
            ] {
                state
                    .memory_repo
                    .store(nexus_storage::repository::StoreMemoryParams {
                        namespace_id: namespace.id,
                        content,
                        category: &nexus_core::MemoryCategory::Facts,
                        memory_lane_type: None,
                        labels: &[],
                        metadata: &serde_json::json!({
                            "cognitive": {
                                "level": level.as_str(),
                                "observer": "claude-code",
                                "subject": "claude-code",
                                "confidence": 0.9,
                                "generated_by": "test"
                            }
                        }),
                        embedding: None,
                        embedding_model: None,
                    })
                    .await
                    .unwrap();
            }
        }

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/reflection?namespace=reflect-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["derived_count"], 1);
        assert_eq!(json["contradiction_count"], 1);
        assert_eq!(json["recent_derived"][0]["content"], "derived insight");
        assert_eq!(
            json["recent_contradictions"][0]["content"],
            "contradiction note"
        );
    }

    #[tokio::test]
    async fn test_overview_returns_latest_stage_metrics() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "overview-ns").await;

        {
            let state = test.state.read().await;
            let namespace = state
                .namespace_repo
                .get_by_name("overview-ns")
                .await
                .unwrap()
                .unwrap();

            state
                .memory_repo
                .record_metric(
                    "cognition.query.total_ms",
                    11.0,
                    &serde_json::json!({"namespace_id": namespace.id, "stage": "total", "unit": "ms"}),
                )
                .await
                .unwrap();
            state
                .memory_repo
                .record_metric(
                    "cognition.query.total_ms",
                    15.5,
                    &serde_json::json!({"namespace_id": namespace.id, "stage": "total", "unit": "ms"}),
                )
                .await
                .unwrap();
            state
                .memory_repo
                .record_metric(
                    "cognition.dream.total_ms",
                    44.0,
                    &serde_json::json!({"namespace_id": namespace.id, "stage": "total", "unit": "ms"}),
                )
                .await
                .unwrap();
        }

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/overview?namespace=overview-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["stage_metrics"]["cognition.query.total_ms"], 15.5);
        assert_eq!(json["stage_metrics"]["cognition.dream.total_ms"], 44.0);
    }

    #[tokio::test]
    async fn test_digests_missing_namespace_returns_400() {
        let test = test_app().await;
        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/digests")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_overview_unknown_namespace_returns_404() {
        let test = test_app().await;
        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/overview?namespace=nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_jobs_with_existing_namespace_returns_empty_list() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "jobs-test-ns").await;

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/jobs?namespace=jobs-test-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["namespace"], "jobs-test-ns");
        assert_eq!(json["jobs"], Value::Array(vec![]));
        assert_eq!(json["total"], 0);
    }

    #[tokio::test]
    async fn test_jobs_reports_total_matching_rows_not_page_len() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "jobs-page-ns").await;

        {
            let state = test.state.read().await;
            let namespace = state
                .namespace_repo
                .get_by_name("jobs-page-ns")
                .await
                .unwrap()
                .unwrap();

            for idx in 0..3 {
                state
                    .memory_repo
                    .enqueue_job(nexus_storage::EnqueueJobParams {
                        namespace_id: namespace.id,
                        job_type: "derive",
                        priority: 10 - idx,
                        perspective: None,
                        payload: &serde_json::json!({ "idx": idx }),
                    })
                    .await
                    .unwrap();
            }
        }

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/jobs?namespace=jobs-page-ns&limit=2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["jobs"].as_array().unwrap().len(), 2);
        assert_eq!(json["total"], 3);
    }

    #[tokio::test]
    async fn test_overview_with_existing_namespace() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "overview-test-ns").await;

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/overview?namespace=overview-test-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["namespace"], "overview-test-ns");
        assert_eq!(json["digest_count"], 0);
        assert_eq!(json["evidence_count"], 0);
    }

    #[tokio::test]
    async fn test_job_summary_with_existing_namespace() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "summary-test-ns").await;

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/jobs/summary?namespace=summary-test-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["namespace"], "summary-test-ns");
        assert!(json["counts"].is_object());
    }

    #[tokio::test]
    async fn test_job_summary_returns_real_counts() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "summary-data-ns").await;

        {
            let state = test.state.read().await;
            let namespace = state
                .namespace_repo
                .get_by_name("summary-data-ns")
                .await
                .unwrap()
                .unwrap();

            state
                .memory_repo
                .enqueue_job(nexus_storage::EnqueueJobParams {
                    namespace_id: namespace.id,
                    job_type: "derive",
                    priority: 10,
                    perspective: None,
                    payload: &serde_json::json!({ "idx": 1 }),
                })
                .await
                .unwrap();
            state
                .memory_repo
                .enqueue_job(nexus_storage::EnqueueJobParams {
                    namespace_id: namespace.id,
                    job_type: "digest",
                    priority: 5,
                    perspective: None,
                    payload: &serde_json::json!({ "idx": 2 }),
                })
                .await
                .unwrap();
        }

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/jobs/summary?namespace=summary-data-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["counts"]["pending"], 2);
    }

    #[tokio::test]
    async fn test_digests_with_existing_namespace() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "digest-test-ns").await;

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/digests?namespace=digest-test-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["namespace"], "digest-test-ns");
        assert_eq!(json["total"], 0);
    }

    #[tokio::test]
    async fn test_digests_support_pagination_and_total() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "digest-page-ns").await;

        {
            let state = test.state.read().await;
            let namespace = state
                .namespace_repo
                .get_by_name("digest-page-ns")
                .await
                .unwrap()
                .unwrap();

            for idx in 0..3 {
                let content = format!("digest memory {idx}");
                let memory = state
                    .memory_repo
                    .store(nexus_storage::StoreMemoryParams {
                        namespace_id: namespace.id,
                        content: &content,
                        category: &nexus_core::MemoryCategory::Session,
                        memory_lane_type: None,
                        labels: &[],
                        metadata: &serde_json::json!({}),
                        embedding: None,
                        embedding_model: None,
                    })
                    .await
                    .unwrap();

                state
                    .memory_repo
                    .store_digest(nexus_storage::StoreDigestParams {
                        namespace_id: namespace.id,
                        session_key: "digest-session",
                        digest_kind: if idx % 2 == 0 {
                            "summary_short"
                        } else {
                            "summary_long"
                        },
                        memory_id: memory.id,
                        start_memory_id: Some(memory.id),
                        end_memory_id: Some(memory.id),
                        token_count: 100 + idx,
                    })
                    .await
                    .unwrap();
            }
        }

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/digests?namespace=digest-page-ns&limit=2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["digests"].as_array().unwrap().len(), 2);
        assert_eq!(json["total"], 3);
    }

    #[test]
    fn test_job_entry_serialization() {
        let job = JobEntry {
            id: 1,
            job_type: "derive".to_string(),
            status: "pending".to_string(),
            priority: 10,
            attempts: 0,
            last_error: None,
            lease_owner: Some("worker-1".to_string()),
            lease_expires_at: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_value(&job).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["job_type"], "derive");
        assert_eq!(json["lease_owner"], "worker-1");
        assert!(json["last_error"].is_null());
    }

    #[test]
    fn test_digest_entry_serialization() {
        let digest = DigestEntry {
            id: 1,
            session_key: "sess-1".to_string(),
            digest_kind: "summary_short".to_string(),
            memory_id: 42,
            start_memory_id: Some(1),
            end_memory_id: Some(10),
            token_count: 200,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_value(&digest).unwrap();
        assert_eq!(json["session_key"], "sess-1");
        assert_eq!(json["memory_id"], 42);
        assert_eq!(json["token_count"], 200);
    }

    #[tokio::test]
    async fn test_overview_with_enqueued_job_and_evidence() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "data-test-ns").await;

        {
            let s = test.state.read().await;
            let ns = s
                .namespace_repo
                .get_by_name("data-test-ns")
                .await
                .unwrap()
                .expect("namespace exists");

            let mem_id = s
                .memory_repo
                .store(nexus_storage::StoreMemoryParams {
                    namespace_id: ns.id,
                    content: "test memory for evidence",
                    category: &nexus_core::MemoryCategory::Session,
                    memory_lane_type: None,
                    labels: &[],
                    metadata: &serde_json::json!({}),
                    embedding: None,
                    embedding_model: None,
                })
                .await
                .unwrap();

            s.memory_repo
                .enqueue_job(nexus_storage::EnqueueJobParams {
                    namespace_id: ns.id,
                    job_type: "derive",
                    priority: 5,
                    perspective: None,
                    payload: &serde_json::json!({"test": true}),
                })
                .await
                .unwrap();

            s.memory_repo
                .store_with_lineage(nexus_storage::StoreMemoryWithLineageParams {
                    store: nexus_storage::StoreMemoryParams {
                        namespace_id: ns.id,
                        content: "derived from evidence",
                        category: &nexus_core::MemoryCategory::Facts,
                        memory_lane_type: None,
                        labels: &[],
                        metadata: &serde_json::json!({"cognitive": {"level": "derived"}}),
                        embedding: None,
                        embedding_model: None,
                    },
                    source_memory_ids: &[mem_id.id],
                    evidence_role: "source",
                })
                .await
                .unwrap();

            let _ = mem_id;
        }

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/overview?namespace=data-test-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["jobs_by_status"]["pending"], 1);
        assert!(json["evidence_count"].as_i64().unwrap() >= 1);
    }

    // ---- Dashboard tests ----

    #[tokio::test]
    async fn test_dashboard_missing_namespace_returns_400() {
        let test = test_app().await;
        let resp = test
            .app
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
    async fn test_dashboard_unknown_namespace_returns_404() {
        let test = test_app().await;
        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/dashboard?namespace=nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_dashboard_returns_all_sections() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "dash-empty-ns").await;

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/dashboard?namespace=dash-empty-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["success"], true);
        assert_eq!(json["namespace"], "dash-empty-ns");

        // Dream section
        assert_eq!(json["dream"]["completed_reflections"], 0);
        assert_eq!(json["dream"]["completed_digests"], 0);
        assert_eq!(json["dream"]["failed_jobs"], 0);
        assert_eq!(json["dream"]["pending_jobs"], 0);
        assert!(json["dream"]["last_dream_at"].is_null());

        // Digest section
        assert_eq!(json["digest"]["total_digests"], 0);
        assert_eq!(json["digest"]["sessions_with_cognition"], 0);
        assert!(json["digest"]["latest_digest_at"].is_null());
        assert!(json["digest"]["latest_digest_age_seconds"].is_null());

        // Recall section
        assert_eq!(json["recall"]["raw"], 0);
        assert_eq!(json["recall"]["explicit"], 0);
        assert_eq!(json["recall"]["contradiction"], 0);
        assert_eq!(json["recall"]["total"], 0);

        // Adaptive section
        assert!(json["adaptive"]["enabled"].is_boolean());
        assert!(json["adaptive"]["current_interval_secs"].as_u64().is_some());
        assert!(json["adaptive"]["contradiction_density"].is_number());
    }

    #[tokio::test]
    async fn test_dashboard_populates_recall_and_dream_from_data() {
        let test = test_app().await;
        create_namespace_in_test(&test.state, "dash-data-ns").await;

        {
            let state = test.state.read().await;
            let namespace = state
                .namespace_repo
                .get_by_name("dash-data-ns")
                .await
                .unwrap()
                .unwrap();

            // Store memories at different cognitive levels.
            for (content, level) in [
                ("raw event", CognitiveLevel::Raw),
                ("explicit fact", CognitiveLevel::Explicit),
                ("derived insight", CognitiveLevel::Derived),
                ("contradiction note", CognitiveLevel::Contradiction),
            ] {
                state
                    .memory_repo
                    .store(nexus_storage::repository::StoreMemoryParams {
                        namespace_id: namespace.id,
                        content,
                        category: &nexus_core::MemoryCategory::Facts,
                        memory_lane_type: None,
                        labels: &[],
                        metadata: &serde_json::json!({
                            "cognitive": {
                                "level": level.as_str(),
                                "observer": "claude-code",
                                "subject": "claude-code",
                                "confidence": 0.9,
                                "generated_by": "test"
                            }
                        }),
                        embedding: None,
                        embedding_model: None,
                    })
                    .await
                    .unwrap();
            }
        }

        let resp = test
            .app
            .oneshot(
                Request::builder()
                    .uri("/api/cognition/dashboard?namespace=dash-data-ns")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
            .await
            .unwrap();
        let json = body_to_json(body);

        assert_eq!(json["recall"]["raw"], 1);
        assert_eq!(json["recall"]["explicit"], 1);
        assert_eq!(json["recall"]["derived"], 1);
        assert_eq!(json["recall"]["contradiction"], 1);
        assert_eq!(json["recall"]["total"], 4);
        // contradiction_density = 1/4 = 0.25
        assert!((json["adaptive"]["contradiction_density"].as_f64().unwrap() - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_dashboard_response_serialization_roundtrip() {
        let dash = DashboardResponse {
            success: true,
            namespace: "test".to_string(),
            dream: DreamState {
                completed_reflections: 5,
                completed_digests: 3,
                failed_jobs: 1,
                pending_jobs: 2,
                last_dream_at: Some("2026-03-27T12:00:00Z".to_string()),
            },
            digest: DigestFreshnessState {
                total_digests: 10,
                sessions_with_cognition: 4,
                latest_digest_age_seconds: Some(3600),
                latest_digest_at: Some("2026-03-27T11:00:00Z".to_string()),
            },
            recall: RecallComposition {
                raw: 50,
                explicit: 30,
                derived: 10,
                summary_short: 5,
                summary_long: 3,
                contradiction: 2,
                total: 100,
            },
            adaptive: AdaptiveDreamState {
                enabled: true,
                current_interval_secs: 120,
                min_interval_secs: 60,
                max_interval_secs: 600,
                contradiction_count: 2,
                contradiction_density: 0.02,
            },
        };
        let json = serde_json::to_value(&dash).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["dream"]["completed_reflections"], 5);
        assert_eq!(json["recall"]["total"], 100);
        assert_eq!(json["adaptive"]["enabled"], true);
        assert_eq!(json["adaptive"]["current_interval_secs"], 120);

        // Round-trip
        let deserialized: DashboardResponse = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.dream.completed_reflections, 5);
        assert_eq!(deserialized.recall.total, 100);
    }
}
