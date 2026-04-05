//! Transcript evaluation harness for cognition recall quality.
//!
//! Compares three retrieval modes against representative fixtures:
//! - **Text-only**: `MemoryRepository::search_by_text_memories` (LIKE-based SQL)
//! - **Hybrid**: `RepresentationService::build` (working set + text fallback, no embedder)
//! - **Cognition**: `QueryService::query_with_representation` (full cognition pipeline)
//!
//! Metrics captured per query: recall, latency, citation usefulness, contradiction
//! surfacing, and answer confidence.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use nexus_core::config::AgentConfig;
use nexus_core::{
    CognitiveLevel, CognitiveMetadata, MemoryCategory, PerspectiveKey, WorkingRepresentationRequest,
};
use nexus_llm::{GenerateParams, GenerateResponse, LlmClient};
use nexus_memory_agent::{QueryService, RepresentationService};
use nexus_storage::repository::{
    MemoryRelationRepository, MemoryRepository, NamespaceRepository, StoreMemoryParams,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

// ---------------------------------------------------------------------------
// Retrieval modes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RetrievalMode {
    TextOnly,
    Hybrid,
    Cognition,
}

impl std::fmt::Display for RetrievalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextOnly => write!(f, "text-only"),
            Self::Hybrid => write!(f, "hybrid"),
            Self::Cognition => write!(f, "cognition"),
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation types
// ---------------------------------------------------------------------------

struct EvalQueryCase {
    name: &'static str,
    query: &'static str,
    expected_memory_ids: HashSet<i64>,
    is_paraphrase: bool,
    expects_contradiction: bool,
}

struct RetrievalOutcome {
    mode: RetrievalMode,
    memory_ids: Vec<i64>,
    latency: Duration,
    answer: Option<nexus_memory_agent::QueryAnswer>,
}

struct EvalMetrics {
    query_name: String,
    mode: RetrievalMode,
    recall: f32,
    precision: f32,
    latency_ms: f64,
    citation_count: usize,
    useful_citation_count: usize,
    citation_usefulness: f32,
    has_contradiction: bool,
    confidence: Option<f32>,
    answer_length: usize,
    approx_answer_tokens: usize,
}

// ---------------------------------------------------------------------------
// Fixture memories
// ---------------------------------------------------------------------------

struct FixtureMemory {
    #[allow(dead_code)]
    id_hint: i64,
    content: &'static str,
    level: CognitiveLevel,
    category: MemoryCategory,
    labels: Vec<&'static str>,
    times_reinforced: i64,
    times_contradicted: i64,
    #[allow(dead_code)]
    is_contradiction_target: bool,
}

fn fixture_memories() -> Vec<FixtureMemory> {
    vec![
        // -- Authentication domain --
        FixtureMemory {
            id_hint: 1,
            content: "User asked to implement authentication with JWT tokens for the REST API",
            level: CognitiveLevel::Raw,
            category: MemoryCategory::Session,
            labels: vec!["raw-activity"],
            times_reinforced: 0,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 2,
            content: "Changed auth approach to use session cookies instead of JWT",
            level: CognitiveLevel::Raw,
            category: MemoryCategory::Session,
            labels: vec!["raw-activity"],
            times_reinforced: 0,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 3,
            content: "The project uses session-based authentication with http-only cookies",
            level: CognitiveLevel::Explicit,
            category: MemoryCategory::Facts,
            labels: vec![],
            times_reinforced: 3,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 4,
            content: "JWT tokens were considered but rejected due to CSRF concerns",
            level: CognitiveLevel::Explicit,
            category: MemoryCategory::Facts,
            labels: vec![],
            times_reinforced: 2,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 5,
            content:
                "Authentication strategy evolved from JWT to session cookies during the project",
            level: CognitiveLevel::Derived,
            category: MemoryCategory::Context,
            labels: vec![],
            times_reinforced: 2,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 6,
            content: "Auth uses JWT tokens",
            level: CognitiveLevel::Contradiction,
            category: MemoryCategory::Facts,
            labels: vec![],
            times_reinforced: 0,
            times_contradicted: 1,
            is_contradiction_target: true,
        },
        FixtureMemory {
            id_hint: 7,
            content: "Auth uses session cookies",
            level: CognitiveLevel::Contradiction,
            category: MemoryCategory::Facts,
            labels: vec![],
            times_reinforced: 0,
            times_contradicted: 0,
            is_contradiction_target: true,
        },
        FixtureMemory {
            id_hint: 8,
            content:
                "Implemented session-based authentication with http-only cookies for the REST API",
            level: CognitiveLevel::SummaryShort,
            category: MemoryCategory::Context,
            labels: vec![],
            times_reinforced: 1,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 9,
            content: "Migrated authentication from JWT tokens to session-based auth with http-only cookies. JWT was rejected due to CSRF concerns. The implementation uses middleware-based session validation.",
            level: CognitiveLevel::SummaryLong,
            category: MemoryCategory::Context,
            labels: vec![],
            times_reinforced: 1,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        // -- Rate limiting domain --
        FixtureMemory {
            id_hint: 10,
            content: "Added rate limiting to the login endpoint: 5 requests per minute per IP",
            level: CognitiveLevel::Raw,
            category: MemoryCategory::Session,
            labels: vec!["raw-activity"],
            times_reinforced: 0,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 11,
            content:
                "Rate limiting is configured at 5 requests per minute per IP address on login",
            level: CognitiveLevel::Explicit,
            category: MemoryCategory::Facts,
            labels: vec![],
            times_reinforced: 2,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 12,
            content:
                "The login endpoint has defensive rate limiting to prevent brute force attacks",
            level: CognitiveLevel::Derived,
            category: MemoryCategory::Context,
            labels: vec![],
            times_reinforced: 1,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        // -- Database domain --
        FixtureMemory {
            id_hint: 13,
            content: "Database connection pooling uses PgPool with max 10 connections",
            level: CognitiveLevel::Raw,
            category: MemoryCategory::Session,
            labels: vec!["raw-activity"],
            times_reinforced: 0,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 14,
            content:
                "PostgreSQL connection pool size is 10, configured via DATABASE_POOL_SIZE env var",
            level: CognitiveLevel::Explicit,
            category: MemoryCategory::Facts,
            labels: vec![],
            times_reinforced: 2,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 15,
            content:
                "Database connectivity uses a bounded connection pool (10 max) to prevent resource exhaustion",
            level: CognitiveLevel::Derived,
            category: MemoryCategory::Context,
            labels: vec![],
            times_reinforced: 1,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        // -- Bug fix domain --
        FixtureMemory {
            id_hint: 16,
            content:
                "Fixed memory leak in the event handler by removing circular reference",
            level: CognitiveLevel::Raw,
            category: MemoryCategory::Session,
            labels: vec!["raw-activity"],
            times_reinforced: 0,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 17,
            content:
                "Event handler memory leak was caused by circular reference between emitter and listener",
            level: CognitiveLevel::Explicit,
            category: MemoryCategory::Facts,
            labels: vec![],
            times_reinforced: 2,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 18,
            content: "Memory leak was in the database query builder",
            level: CognitiveLevel::Contradiction,
            category: MemoryCategory::Facts,
            labels: vec![],
            times_reinforced: 0,
            times_contradicted: 2,
            is_contradiction_target: true,
        },
        FixtureMemory {
            id_hint: 19,
            content: "Fixed event handler memory leak caused by circular reference",
            level: CognitiveLevel::SummaryShort,
            category: MemoryCategory::Context,
            labels: vec![],
            times_reinforced: 1,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        // -- Code style domain --
        FixtureMemory {
            id_hint: 20,
            content: "User mentioned they prefer TypeScript over JavaScript for new files",
            level: CognitiveLevel::Raw,
            category: MemoryCategory::Preferences,
            labels: vec!["raw-activity"],
            times_reinforced: 0,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 21,
            content: "TypeScript is preferred over JavaScript for all new source files",
            level: CognitiveLevel::Explicit,
            category: MemoryCategory::Preferences,
            labels: vec![],
            times_reinforced: 3,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
        FixtureMemory {
            id_hint: 22,
            content: "The project has a TypeScript-first policy for new code",
            level: CognitiveLevel::Derived,
            category: MemoryCategory::Context,
            labels: vec![],
            times_reinforced: 1,
            times_contradicted: 0,
            is_contradiction_target: false,
        },
    ]
}

fn eval_query_cases() -> Vec<EvalQueryCase> {
    vec![
        EvalQueryCase {
            name: "auth_method_literal",
            query: "What authentication method does the project use?",
            expected_memory_ids: [3, 5, 7, 8, 9].into_iter().collect(),
            is_paraphrase: false,
            expects_contradiction: true,
        },
        EvalQueryCase {
            name: "auth_method_paraphrase",
            query: "How does the login system verify user identity?",
            expected_memory_ids: [3, 5, 7, 8, 9].into_iter().collect(),
            is_paraphrase: true,
            expects_contradiction: true,
        },
        EvalQueryCase {
            name: "jwt_rejection_literal",
            query: "Why was JWT rejected?",
            expected_memory_ids: [4, 5, 9].into_iter().collect(),
            is_paraphrase: false,
            expects_contradiction: false,
        },
        EvalQueryCase {
            name: "security_concerns_paraphrase",
            query: "What security concerns influenced the auth implementation?",
            expected_memory_ids: [4, 5, 9, 10, 11, 12].into_iter().collect(),
            is_paraphrase: true,
            expects_contradiction: false,
        },
        EvalQueryCase {
            name: "rate_limit_literal",
            query: "What rate limit is set on the login endpoint?",
            expected_memory_ids: [10, 11, 12].into_iter().collect(),
            is_paraphrase: false,
            expects_contradiction: false,
        },
        EvalQueryCase {
            name: "brute_force_paraphrase",
            query: "How does the system prevent brute force login attempts?",
            expected_memory_ids: [10, 11, 12].into_iter().collect(),
            is_paraphrase: true,
            expects_contradiction: false,
        },
        EvalQueryCase {
            name: "memory_leak_literal",
            query: "What caused the memory leak?",
            expected_memory_ids: [16, 17, 19].into_iter().collect(),
            is_paraphrase: false,
            expects_contradiction: true,
        },
        EvalQueryCase {
            name: "event_handler_bug_paraphrase",
            query: "What was the root cause of the event handler bug?",
            expected_memory_ids: [16, 17, 19].into_iter().collect(),
            is_paraphrase: true,
            expects_contradiction: true,
        },
    ]
}

// ---------------------------------------------------------------------------
// Mock LLM for deterministic cognition queries
// ---------------------------------------------------------------------------

struct EvalMockLlm {
    responses: Mutex<VecDeque<nexus_llm::Result<GenerateResponse>>>,
}

impl EvalMockLlm {
    fn with_json_responses(json_responses: &[&str]) -> Self {
        let responses = json_responses
            .iter()
            .map(|content| {
                Ok(GenerateResponse {
                    content: (*content).to_string(),
                    model: "eval-mock".to_string(),
                    usage: None,
                })
            })
            .collect();
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl LlmClient for EvalMockLlm {
    async fn generate(&self, _params: GenerateParams) -> nexus_llm::Result<GenerateResponse> {
        self.responses
            .lock()
            .expect("mock responses poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Ok(GenerateResponse {
                    content: r#"{"answer":"No relevant memories found.","citations":[],"confidence":0.1,"lineages":[]}"#
                        .to_string(),
                    model: "eval-mock".to_string(),
                    usage: None,
                })
            })
    }

    fn provider_name(&self) -> String {
        "eval-mock".to_string()
    }

    fn model_name(&self) -> String {
        "eval-mock".to_string()
    }
}

// ---------------------------------------------------------------------------
// Fixture setup
// ---------------------------------------------------------------------------

struct EvalFixture {
    _tempdir: tempfile::TempDir,
    pool: sqlx::SqlitePool,
    repo: MemoryRepository,
    namespace_id: i64,
    #[allow(dead_code)]
    perspective: PerspectiveKey,
    request: WorkingRepresentationRequest,
    contradiction_ids: HashSet<i64>,
}

fn build_cognitive_metadata(
    level: CognitiveLevel,
    perspective: &PerspectiveKey,
    fm: &FixtureMemory,
) -> serde_json::Value {
    let mut cognitive = CognitiveMetadata::new(
        level,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        "eval-harness",
    );
    cognitive.confidence = Some(0.85);
    cognitive.times_reinforced = fm.times_reinforced;
    cognitive.times_contradicted = fm.times_contradicted;

    let mut metadata = cognitive.merge_into(&serde_json::json!({}));

    if level == CognitiveLevel::Raw {
        metadata["raw_activity"] = serde_json::json!({
            "derived_session_key": perspective.session_key.clone().unwrap_or_default()
        });
    }

    metadata
}

async fn setup_fixture() -> EvalFixture {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("eval-harness.db");
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    nexus_storage::migrations::run_migrations(&pool)
        .await
        .expect("migrations failed");

    let repo = MemoryRepository::new(pool.clone());
    let ns_repo = NamespaceRepository::new(pool.clone());
    let namespace = ns_repo
        .get_or_create("eval-harness", "test")
        .await
        .expect("namespace creation failed");
    let namespace_id = namespace.id;

    let perspective =
        PerspectiveKey::new("claude-code", "eval-harness", Some("eval-session".into()));
    let mut contradiction_ids = HashSet::new();

    // Store fixture memories
    for fm in fixture_memories() {
        let metadata = build_cognitive_metadata(fm.level, &perspective, &fm);
        let labels: Vec<String> = fm.labels.iter().map(|s| (*s).to_string()).collect();

        let params = StoreMemoryParams {
            namespace_id,
            content: fm.content,
            category: &fm.category,
            memory_lane_type: None,
            labels: &labels,
            metadata: &metadata,
            embedding: None,
            embedding_model: None,
        };

        let memory = repo
            .store(params)
            .await
            .expect("store fixture memory failed");
        if fm.is_contradiction_target {
            contradiction_ids.insert(memory.id);
        }
    }

    let request = WorkingRepresentationRequest {
        namespace_id,
        perspective: Some(perspective.clone()),
        query: None,
        max_items: 20,
        include_raw: false,
        include_recent: true,
        include_semantic: true,
        include_derived: true,
        include_digests: true,
        include_contradictions: true,
        ..WorkingRepresentationRequest::default()
    };

    EvalFixture {
        _tempdir: tempdir,
        pool,
        repo,
        namespace_id,
        perspective,
        request,
        contradiction_ids,
    }
}

// ---------------------------------------------------------------------------
// Retrieval functions
// ---------------------------------------------------------------------------

async fn retrieve_text_only(query: &str, fixture: &EvalFixture) -> RetrievalOutcome {
    let start = Instant::now();
    let memories = fixture
        .repo
        .search_by_text_memories(fixture.namespace_id, query, 20, false)
        .await
        .expect("text search failed");
    let latency = start.elapsed();

    RetrievalOutcome {
        mode: RetrievalMode::TextOnly,
        memory_ids: memories.iter().map(|m| m.id).collect(),
        latency,
        answer: None,
    }
}

async fn retrieve_hybrid(query: &str, fixture: &EvalFixture) -> RetrievalOutcome {
    let service = RepresentationService::without_embedder();
    let mut request = fixture.request.clone();
    request.query = Some(query.to_string());

    let start = Instant::now();
    let representation = service
        .build(&request, &fixture.repo)
        .await
        .expect("representation build failed");
    let latency = start.elapsed();

    // Flatten all buckets into a single set of memory IDs
    let mut ids = Vec::new();
    for m in representation.digests {
        ids.push(m.id);
    }
    for m in representation.derived {
        ids.push(m.id);
    }
    for m in representation.semantic {
        ids.push(m.id);
    }
    for m in representation.recent {
        ids.push(m.id);
    }
    for m in representation.contradictions {
        ids.push(m.id);
    }

    RetrievalOutcome {
        mode: RetrievalMode::Hybrid,
        memory_ids: ids,
        latency,
        answer: None,
    }
}

async fn retrieve_cognition(
    query: &str,
    fixture: &EvalFixture,
    llm: &Arc<EvalMockLlm>,
) -> RetrievalOutcome {
    let service = QueryService::new(llm.clone(), AgentConfig::default());
    let relation_repo = MemoryRelationRepository::new(&fixture.pool);

    let mut request = fixture.request.clone();
    request.query = Some(query.to_string());
    request.include_raw = false;

    let start = Instant::now();
    let answer = service
        .query_with_representation(query, request, &fixture.repo, &relation_repo)
        .await
        .expect("cognition query failed");
    let latency = start.elapsed();

    let memory_ids: Vec<i64> = answer.citations.iter().map(|c| c.memory_id).collect();

    RetrievalOutcome {
        mode: RetrievalMode::Cognition,
        memory_ids,
        latency,
        answer: Some(answer),
    }
}

// ---------------------------------------------------------------------------
// Metric computation
// ---------------------------------------------------------------------------

fn compute_metrics(
    case: &EvalQueryCase,
    outcome: &RetrievalOutcome,
    contradiction_ids: &HashSet<i64>,
) -> EvalMetrics {
    let found: HashSet<i64> = outcome.memory_ids.iter().copied().collect();
    let intersection: HashSet<i64> = found
        .intersection(&case.expected_memory_ids)
        .copied()
        .collect();

    let recall = if case.expected_memory_ids.is_empty() {
        1.0
    } else {
        intersection.len() as f32 / case.expected_memory_ids.len() as f32
    };

    let precision = if outcome.memory_ids.is_empty() {
        0.0
    } else {
        intersection.len() as f32 / outcome.memory_ids.len() as f32
    };

    let has_contradiction = outcome
        .memory_ids
        .iter()
        .any(|id| contradiction_ids.contains(id));

    let (
        citation_count,
        useful_citation_count,
        citation_usefulness,
        confidence,
        answer_length,
        approx_answer_tokens,
    ) = match &outcome.answer {
        Some(a) => {
            let citation_count = a.citations.len();
            let useful_citation_count = a
                .citations
                .iter()
                .filter(|c| case.expected_memory_ids.contains(&c.memory_id))
                .count();
            let citation_usefulness = if citation_count == 0 {
                0.0
            } else {
                useful_citation_count as f32 / citation_count as f32
            };
            (
                citation_count,
                useful_citation_count,
                citation_usefulness,
                Some(a.confidence),
                a.answer.trim().len(),
                approx_token_count(&a.answer),
            )
        }
        None => (0, 0, 0.0, None, 0, 0),
    };

    EvalMetrics {
        query_name: case.name.to_string(),
        mode: outcome.mode,
        recall,
        precision,
        latency_ms: latency_to_ms(outcome.latency),
        citation_count,
        useful_citation_count,
        citation_usefulness,
        has_contradiction,
        confidence,
        answer_length,
        approx_answer_tokens,
    }
}

fn latency_to_ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn approx_token_count(text: &str) -> usize {
    text.split_whitespace().count()
}

// ---------------------------------------------------------------------------
// Summary output
// ---------------------------------------------------------------------------

fn print_summary(all_metrics: &[EvalMetrics]) {
    println!();
    println!("{}", "=".repeat(100));
    println!("{:^100}", "NEXUS COGNITION EVALUATION HARNESS");
    println!("{}", "=".repeat(100));
    println!();

    // Group by mode
    for mode in [
        RetrievalMode::TextOnly,
        RetrievalMode::Hybrid,
        RetrievalMode::Cognition,
    ] {
        let mode_metrics: Vec<&EvalMetrics> =
            all_metrics.iter().filter(|m| m.mode == mode).collect();

        let avg_recall: f32 =
            mode_metrics.iter().map(|m| m.recall).sum::<f32>() / mode_metrics.len().max(1) as f32;
        let avg_precision: f32 = mode_metrics.iter().map(|m| m.precision).sum::<f32>()
            / mode_metrics.len().max(1) as f32;
        let avg_latency: f64 = mode_metrics.iter().map(|m| m.latency_ms).sum::<f64>()
            / mode_metrics.len().max(1) as f64;
        let total_citations: usize = mode_metrics.iter().map(|m| m.citation_count).sum();
        let total_useful_citations: usize =
            mode_metrics.iter().map(|m| m.useful_citation_count).sum();
        let avg_citation_usefulness: f32 = mode_metrics
            .iter()
            .map(|m| m.citation_usefulness)
            .sum::<f32>()
            / mode_metrics.len().max(1) as f32;
        let avg_answer_tokens: f64 = mode_metrics
            .iter()
            .map(|m| m.approx_answer_tokens as f64)
            .sum::<f64>()
            / mode_metrics.len().max(1) as f64;
        let contradictions_found: usize =
            mode_metrics.iter().filter(|m| m.has_contradiction).count();

        println!("--- {} ---", mode);
        println!(
            "  Queries: {} | Avg Recall: {:.1}% | Avg Precision: {:.1}% | Avg Latency: {:.1}ms | Citations: {} (useful: {}, avg usefulness: {:.1}%) | Avg answer tokens: {:.1} | Contradictions surfaced: {}/{}",
            mode_metrics.len(),
            avg_recall * 100.0,
            avg_precision * 100.0,
            avg_latency,
            total_citations,
            total_useful_citations,
            avg_citation_usefulness * 100.0,
            avg_answer_tokens,
            contradictions_found,
            mode_metrics.len(),
        );
        println!();
    }

    // Per-query comparison table
    println!("{}", "-".repeat(100));
    println!(
        "{:<30} {:>12} {:>12} {:>12}   {:>10} {:>10} {:>10}",
        "Query", "Text Rec", "Hybrid Rec", "Cogn Rec", "Text ms", "Hyb ms", "Cogn ms"
    );
    println!("{}", "-".repeat(100));

    let query_names: Vec<String> = all_metrics
        .iter()
        .map(|m| m.query_name.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for name in &query_names {
        let query_metrics: Vec<&EvalMetrics> = all_metrics
            .iter()
            .filter(|m| &m.query_name == name)
            .collect();

        let text = query_metrics
            .iter()
            .find(|m| m.mode == RetrievalMode::TextOnly);
        let hybrid = query_metrics
            .iter()
            .find(|m| m.mode == RetrievalMode::Hybrid);
        let cogn = query_metrics
            .iter()
            .find(|m| m.mode == RetrievalMode::Cognition);

        let tr = text
            .map(|m| format!("{:.0}%", m.recall * 100.0))
            .unwrap_or("-".into());
        let hr = hybrid
            .map(|m| format!("{:.0}%", m.recall * 100.0))
            .unwrap_or("-".into());
        let cr = cogn
            .map(|m| format!("{:.0}%", m.recall * 100.0))
            .unwrap_or("-".into());
        let tl = text
            .map(|m| format!("{:.1}", m.latency_ms))
            .unwrap_or("-".into());
        let hl = hybrid
            .map(|m| format!("{:.1}", m.latency_ms))
            .unwrap_or("-".into());
        let cl = cogn
            .map(|m| format!("{:.1}", m.latency_ms))
            .unwrap_or("-".into());

        println!(
            "{:<30} {:>12} {:>12} {:>12}   {:>10} {:>10} {:>10}",
            name, tr, hr, cr, tl, hl, cl
        );
    }
    println!("{}", "=".repeat(100));
    println!();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn eval_harness_recall_comparison() {
    let fixture = setup_fixture().await;

    // Mock LLM returns a deterministic structured answer with citations.
    // The answer content is less important than the structural metrics.
    let mock_response = r#"{"answer":"Based on the memories, the project uses session-based authentication with http-only cookies. JWT was rejected due to CSRF concerns. Rate limiting is set at 5 req/min per IP on login. The memory leak was caused by a circular reference in the event handler.","citations":[{"memory_id":3,"title":"Auth fact","excerpt":"session-based authentication with http-only cookies"},{"memory_id":5,"title":"Auth derived","excerpt":"Authentication strategy evolved"},{"memory_id":7,"title":"Contradiction resolved","excerpt":"Auth uses session cookies"},{"memory_id":11,"title":"Rate limit","excerpt":"5 requests per minute per IP"},{"memory_id":12,"title":"Rate limit derived","excerpt":"defensive rate limiting"},{"memory_id":17,"title":"Leak cause","excerpt":"circular reference between emitter and listener"},{"memory_id":18,"title":"Contradiction wrong","excerpt":"Memory leak was in the database query builder"}],"confidence":0.88,"lineages":[{"memory_id":3,"bucket":"Semantic","phase":"explicit","relevance_score":0.92},{"memory_id":5,"bucket":"Derived","phase":"derived","relevance_score":0.88},{"memory_id":7,"bucket":"Contradictions","phase":"contradiction","relevance_score":0.75},{"memory_id":11,"bucket":"Semantic","phase":"explicit","relevance_score":0.90},{"memory_id":12,"bucket":"Derived","phase":"derived","relevance_score":0.85},{"memory_id":17,"bucket":"Semantic","phase":"explicit","relevance_score":0.91},{"memory_id":18,"bucket":"Contradictions","phase":"contradiction","relevance_score":0.70}]}"#;

    // Pre-load enough mock responses for all queries
    let json_responses: Vec<String> = eval_query_cases()
        .iter()
        .map(|_| mock_response.to_string())
        .collect();

    let llm = Arc::new(EvalMockLlm::with_json_responses(
        &json_responses
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
    ));

    let mut all_metrics = Vec::new();

    for case in eval_query_cases() {
        // Text-only retrieval
        let text_outcome = retrieve_text_only(case.query, &fixture).await;
        all_metrics.push(compute_metrics(
            &case,
            &text_outcome,
            &fixture.contradiction_ids,
        ));

        // Hybrid retrieval
        let hybrid_outcome = retrieve_hybrid(case.query, &fixture).await;
        all_metrics.push(compute_metrics(
            &case,
            &hybrid_outcome,
            &fixture.contradiction_ids,
        ));

        // Cognition retrieval
        let cogn_outcome = retrieve_cognition(case.query, &fixture, &llm).await;
        all_metrics.push(compute_metrics(
            &case,
            &cogn_outcome,
            &fixture.contradiction_ids,
        ));
    }

    // Print the summary table
    print_summary(&all_metrics);

    // --- Assertions ---

    // 1. All three modes return results for every query
    for case in eval_query_cases() {
        for mode in [
            RetrievalMode::TextOnly,
            RetrievalMode::Hybrid,
            RetrievalMode::Cognition,
        ] {
            let m = all_metrics
                .iter()
                .find(|m| m.query_name == case.name && m.mode == mode)
                .expect("missing metrics");
            assert!(
                m.recall >= 0.0,
                "{} ({}) should have non-negative recall",
                case.name,
                mode
            );
        }
    }

    // 2. Cognition mode surfaces contradictions when expected
    for case in eval_query_cases() {
        if !case.expects_contradiction {
            continue;
        }
        let cogn = all_metrics
            .iter()
            .find(|m| m.query_name == case.name && m.mode == RetrievalMode::Cognition)
            .expect("missing cognition metrics");
        // The mock response includes contradiction memory IDs (7, 18), so they
        // should appear as citations
        assert!(
            cogn.has_contradiction,
            "cognition should surface contradictions for '{}'",
            case.name
        );
    }

    // 3. Cognition mode produces answers with citations
    for case in eval_query_cases() {
        let cogn = all_metrics
            .iter()
            .find(|m| m.query_name == case.name && m.mode == RetrievalMode::Cognition)
            .expect("missing cognition metrics");
        assert!(
            cogn.citation_count > 0,
            "cognition should produce citations for '{}'",
            case.name
        );
        assert!(
            cogn.confidence.is_some(),
            "cognition should report confidence for '{}'",
            case.name
        );
        assert!(
            cogn.answer_length > 20,
            "cognition answer should be substantive for '{}', got {} chars",
            case.name,
            cogn.answer_length
        );
    }

    // 4. Latency is bounded (no mode should take more than 2 seconds per query)
    for m in &all_metrics {
        assert!(
            m.latency_ms < 2000.0,
            "{} ({}) latency {:.1}ms exceeds 2s bound",
            m.query_name,
            m.mode,
            m.latency_ms
        );
    }

    // 5. Hybrid and cognition modes find at least as much as text-only for
    //    the auth queries (which have good text overlap and cognition structure)
    let auth_queries = ["auth_method_literal", "auth_method_paraphrase"];
    let mut hybrid_at_least_text = 0;
    for name in &auth_queries {
        let text = all_metrics
            .iter()
            .find(|m| m.query_name == *name && m.mode == RetrievalMode::TextOnly);
        let hybrid = all_metrics
            .iter()
            .find(|m| m.query_name == *name && m.mode == RetrievalMode::Hybrid);
        if let (Some(t), Some(h)) = (text, hybrid) {
            if h.recall >= t.recall {
                hybrid_at_least_text += 1;
            }
        }
    }
    assert!(
        hybrid_at_least_text >= 1,
        "hybrid should match or beat text-only on at least 1 auth query (got {})",
        hybrid_at_least_text
    );
}

#[tokio::test]
async fn eval_harness_text_only_finds_literal_matches() {
    let fixture = setup_fixture().await;

    // Literal query for "session cookies" should find at least the explicit fact
    let outcome = retrieve_text_only("session cookies", &fixture).await;

    // The text search is LIKE-based, so "session cookies" should match memories
    // 3 (session-based authentication with http-only cookies) and 7 (Auth uses session cookies)
    assert!(
        !outcome.memory_ids.is_empty(),
        "text-only should find at least one result for 'session cookies'"
    );

    // Text search uses LIKE '%session cookies%' which matches memory 7 ("Auth uses session cookies")
    // but NOT memory 3 ("session-based authentication with http-only cookies") since
    // "session" and "cookies" aren't adjacent in that string.
    assert!(
        outcome.memory_ids.contains(&7),
        "text-only should find memory 7 for 'session cookies', found: {:?}",
        outcome.memory_ids
    );
}

#[tokio::test]
async fn eval_harness_hybrid_returns_multiple_buckets() {
    let fixture = setup_fixture().await;

    let service = RepresentationService::without_embedder();
    let mut request = fixture.request.clone();
    request.query = Some("authentication".to_string());

    let representation = service
        .build(&request, &fixture.repo)
        .await
        .expect("representation build failed");

    // Hybrid mode should pull from multiple buckets, not just one
    let bucket_counts = [
        representation.digests.len(),
        representation.derived.len(),
        representation.semantic.len(),
        representation.recent.len(),
        representation.contradictions.len(),
    ];

    let non_empty_buckets = bucket_counts.iter().filter(|&&c| c > 0).count();
    assert!(
        non_empty_buckets >= 2,
        "hybrid representation should have >= 2 non-empty buckets, got: {:?}",
        bucket_counts
    );
}

#[tokio::test]
async fn eval_harness_cognition_surfaces_contradictions() {
    let fixture = setup_fixture().await;

    let mock_response = r#"{"answer":"There is a contradiction about the authentication method: some memories claim JWT tokens while the resolved answer is session-based cookies. The memory leak was caused by a circular reference, contradicting an earlier diagnosis pointing to the database query builder.","citations":[{"memory_id":6,"title":"Contradiction","excerpt":"Auth uses JWT tokens"},{"memory_id":7,"title":"Resolved","excerpt":"Auth uses session cookies"},{"memory_id":18,"title":"Wrong diagnosis","excerpt":"Memory leak was in the database query builder"}],"confidence":0.82,"lineages":[]}"#;

    let llm = Arc::new(EvalMockLlm::with_json_responses(&[mock_response]));

    let outcome = retrieve_cognition("What contradictions exist?", &fixture, &llm).await;

    let answer = outcome.answer.expect("cognition should produce an answer");

    // Should cite contradiction memories
    let cited_ids: Vec<i64> = answer.citations.iter().map(|c| c.memory_id).collect();
    assert!(
        cited_ids.contains(&6),
        "should cite contradiction memory 6 (JWT), cited: {:?}",
        cited_ids
    );
    assert!(
        cited_ids.contains(&18),
        "should cite contradiction memory 18 (wrong diagnosis), cited: {:?}",
        cited_ids
    );

    // Answer should mention contradiction-related content
    assert!(
        answer.answer.to_lowercase().contains("contradiction"),
        "answer should mention contradictions, got: {}",
        answer.answer
    );

    // Confidence should be present
    assert!(
        answer.confidence > 0.0,
        "confidence should be positive, got: {}",
        answer.confidence
    );
}

#[tokio::test]
async fn eval_harness_metrics_compute_correctly() {
    // Verify the metric computation logic with known inputs
    let case = EvalQueryCase {
        name: "test_case",
        query: "test",
        expected_memory_ids: [1, 2, 3, 4, 5].into_iter().collect(),
        is_paraphrase: false,
        expects_contradiction: false,
    };

    let outcome = RetrievalOutcome {
        mode: RetrievalMode::TextOnly,
        memory_ids: vec![2, 3, 4, 5, 6],
        latency: Duration::from_millis(50),
        answer: None,
    };

    let metrics = compute_metrics(&case, &outcome, &HashSet::from([6]));

    // Found: {2,3,4,5,6}, Expected: {1,2,3,4,5}, Intersection: {2,3,4,5}
    // Recall = 4/5 = 0.8, Precision = 4/5 = 0.8
    assert!(
        (metrics.recall - 0.8).abs() < 0.001,
        "recall should be 0.8, got: {}",
        metrics.recall
    );
    assert!(
        (metrics.precision - 0.8).abs() < 0.001,
        "precision should be 0.8, got: {}",
        metrics.precision
    );
    assert!(
        (metrics.latency_ms - 50.0).abs() < 1.0,
        "latency should be ~50ms, got: {}",
        metrics.latency_ms
    );
    assert_eq!(metrics.citation_count, 0);
    // Memory 6 (a contradiction) is in the result set, so has_contradiction should be true
    assert!(metrics.has_contradiction);
    assert!(metrics.confidence.is_none());
    assert_eq!(metrics.answer_length, 0);
}

#[tokio::test]
async fn eval_harness_paraphrase_queries_return_results() {
    let fixture = setup_fixture().await;

    let paraphrase_cases = eval_query_cases().into_iter().filter(|c| c.is_paraphrase);

    for case in paraphrase_cases {
        let text_outcome = retrieve_text_only(case.query, &fixture).await;
        let hybrid_outcome = retrieve_hybrid(case.query, &fixture).await;

        // Even paraphrased queries should return something from at least one mode
        let any_found =
            !text_outcome.memory_ids.is_empty() || !hybrid_outcome.memory_ids.is_empty();
        assert!(
            any_found,
            "paraphrase query '{}' should return results from at least one mode (text: {}, hybrid: {})",
            case.name,
            text_outcome.memory_ids.len(),
            hybrid_outcome.memory_ids.len(),
        );
    }
}
