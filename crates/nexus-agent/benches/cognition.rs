use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::{
    CognitiveLevel, CognitiveMetadata, Memory, MemoryCategory, PerspectiveKey,
    WorkingRepresentationRequest,
};
use nexus_llm::{GenerateParams, GenerateResponse, LlmClient};
use nexus_memory_agent::{
    CognitionSnapshot, DigestService, QueryService, ReflectService, RepresentationService,
};
use nexus_storage::repository::{
    MemoryRelationRepository, MemoryRepository, NamespaceRepository, StoreDigestParams,
    StoreMemoryParams,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

struct MockLlmClient {
    responses: Mutex<VecDeque<nexus_llm::Result<GenerateResponse>>>,
}

enum FixtureMode {
    InMemory,
    OnDisk,
}

struct BenchFixture {
    _tempdir: Option<TempDir>,
    pool: sqlx::SqlitePool,
    repo: MemoryRepository,
    namespace_id: i64,
    request: WorkingRepresentationRequest,
}

impl MockLlmClient {
    fn new(json_content: &str, copies: usize) -> Self {
        let responses = (0..copies)
            .map(|_| {
                Ok(GenerateResponse {
                    content: json_content.to_string(),
                    model: "mock-model".to_string(),
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
impl LlmClient for MockLlmClient {
    async fn generate(&self, _params: GenerateParams) -> nexus_llm::Result<GenerateResponse> {
        self.responses
            .lock()
            .expect("mock responses poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Ok(GenerateResponse {
                    content:
                        r#"{"answer":"No answer","citations":[],"confidence":0.0,"lineages":[]}"#
                            .to_string(),
                    model: "mock-model".to_string(),
                    usage: None,
                })
            })
    }

    fn provider_name(&self) -> String {
        "mock".to_string()
    }

    fn model_name(&self) -> String {
        "mock-model".to_string()
    }
}

async fn setup_repo(total_memories: usize, mode: FixtureMode) -> BenchFixture {
    let (pool, tempdir) = match mode {
        FixtureMode::InMemory => (
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap(),
            None,
        ),
        FixtureMode::OnDisk => {
            let tempdir = tempfile::tempdir().unwrap();
            let db_path = tempdir.path().join("cognition-bench.db");
            let options = SqliteConnectOptions::new()
                .filename(&db_path)
                .create_if_missing(true);
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
                .unwrap();
            (pool, Some(tempdir))
        }
    };
    nexus_storage::migrations::run_migrations(&pool)
        .await
        .unwrap();

    let namespace_repo = NamespaceRepository::new(pool.clone());
    let namespace = namespace_repo
        .get_or_create("bench-agent", "bench-agent")
        .await
        .unwrap();
    let repo = MemoryRepository::new(pool.clone());
    let perspective = PerspectiveKey::new(
        "claude-code",
        "claude-code",
        Some("benchmark-session".to_string()),
    );

    let digest_memory = repo
        .store(StoreMemoryParams {
            namespace_id: namespace.id,
            content: "Short digest: completed cognition hardening and query improvements.",
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &[],
            metadata: &metadata(CognitiveLevel::SummaryShort, &perspective, 0),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();
    repo.store_digest(StoreDigestParams {
        namespace_id: namespace.id,
        session_key: "benchmark-session",
        digest_kind: "short",
        memory_id: digest_memory.id,
        start_memory_id: Some(digest_memory.id),
        end_memory_id: Some(digest_memory.id),
        token_count: 32,
    })
    .await
    .unwrap();

    for i in 0..total_memories {
        let level = match i % 4 {
            0 => CognitiveLevel::Explicit,
            1 => CognitiveLevel::Derived,
            2 => CognitiveLevel::Contradiction,
            _ => CognitiveLevel::Raw,
        };
        let labels = if level == CognitiveLevel::Raw {
            vec!["raw-activity".to_string()]
        } else {
            Vec::new()
        };
        let content = match level {
            CognitiveLevel::Explicit => format!(
                "Explicit observation {} about query orchestration and benchmark coverage.",
                i
            ),
            CognitiveLevel::Derived => {
                format!(
                    "Derived insight {} about repeated benchmark tuning patterns.",
                    i
                )
            }
            CognitiveLevel::Contradiction => format!(
                "Contradiction {} between prior benchmark assumptions and current measurements.",
                i
            ),
            CognitiveLevel::Raw => format!(
                "{{\"event\":\"tool\",\"tool\":\"cargo test\",\"ordinal\":{}}}",
                i
            ),
            _ => unreachable!(),
        };

        repo.store(StoreMemoryParams {
            namespace_id: namespace.id,
            content: &content,
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &labels,
            metadata: &metadata(level, &perspective, i as i64),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();
    }

    let request = WorkingRepresentationRequest {
        namespace_id: namespace.id,
        perspective: Some(perspective.clone()),
        query: Some("What happened in the benchmark session?".to_string()),
        max_items: 24,
        include_raw: false,
        include_recent: true,
        include_semantic: true,
        include_derived: true,
        include_digests: true,
        include_contradictions: true,
        ..WorkingRepresentationRequest::default()
    };

    BenchFixture {
        _tempdir: tempdir,
        pool,
        repo,
        namespace_id: namespace.id,
        request,
    }
}

fn metadata(
    level: CognitiveLevel,
    perspective: &PerspectiveKey,
    ordinal: i64,
) -> serde_json::Value {
    let mut cognitive = CognitiveMetadata::new(
        level,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        "benchmark",
    );
    cognitive.confidence = Some(0.85);
    cognitive.times_reinforced = if level == CognitiveLevel::Derived {
        ordinal % 5
    } else {
        0
    };

    let mut metadata = cognitive.merge_into(&serde_json::json!({}));
    if level == CognitiveLevel::Raw {
        metadata["raw_activity"] = serde_json::json!({
            "derived_session_key": perspective.session_key.clone().unwrap_or_default()
        });
    }
    metadata
}

// ---------------------------------------------------------------------------
// Digest fixture: multiple sessions × memories per session on on-disk DB
// ---------------------------------------------------------------------------

struct DigestFixture {
    _tempdir: TempDir,
    repo: MemoryRepository,
    namespace_id: i64,
    target_session_key: String,
}

/// Creates `num_sessions` sessions, each with `memories_per_session` non-raw
/// memories (Explicit/Derived alternating). Returns a fixture that holds the DB
/// and the *last* session key for benchmarking.
async fn setup_digest_fixture(num_sessions: usize, memories_per_session: usize) -> DigestFixture {
    let tempdir = tempfile::tempdir().unwrap();
    let db_path = tempdir.path().join("digest-bench.db");
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
        .unwrap();

    let namespace_repo = NamespaceRepository::new(pool.clone());
    let namespace = namespace_repo
        .get_or_create("bench-agent", "bench-agent")
        .await
        .unwrap();
    let repo = MemoryRepository::new(pool.clone());

    for session_idx in 0..num_sessions {
        let session_key = format!("digest-session-{}", session_idx);
        let perspective =
            PerspectiveKey::new("claude-code", "claude-code", Some(session_key.clone()));
        for i in 0..memories_per_session {
            let level = if i % 2 == 0 {
                CognitiveLevel::Explicit
            } else {
                CognitiveLevel::Derived
            };
            let content = match level {
                CognitiveLevel::Explicit => format!(
                    "Explicit observation in session {} #{}: agent completed task and stored result.",
                    session_idx, i
                ),
                CognitiveLevel::Derived => format!(
                    "Derived insight in session {} #{}: repeated task pattern detected with high frequency.",
                    session_idx, i
                ),
                _ => unreachable!(),
            };
            repo.store(StoreMemoryParams {
                namespace_id: namespace.id,
                content: &content,
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &metadata(
                    level,
                    &perspective,
                    (session_idx * memories_per_session + i) as i64,
                ),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }
    }

    let target_session_key = format!("digest-session-{}", num_sessions - 1);
    DigestFixture {
        _tempdir: tempdir,
        repo,
        namespace_id: namespace.id,
        target_session_key,
    }
}

// ---------------------------------------------------------------------------
// Scaling benchmarks — on-disk with larger fixtures
// ---------------------------------------------------------------------------

fn bench_representation_build_ondisk_200(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(setup_repo(200, FixtureMode::OnDisk));
    let service = RepresentationService::new();

    c.bench_function("cognition_representation_build_ondisk_200", |b| {
        b.iter(|| {
            rt.block_on(service.build(black_box(&fixture.request), black_box(&fixture.repo)))
                .unwrap()
        });
    });
}

fn bench_representation_build_ondisk_500(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(setup_repo(500, FixtureMode::OnDisk));
    let service = RepresentationService::new();

    c.bench_function("cognition_representation_build_ondisk_500", |b| {
        b.iter(|| {
            rt.block_on(service.build(black_box(&fixture.request), black_box(&fixture.repo)))
                .unwrap()
        });
    });
}

fn bench_query_with_representation_ondisk_200(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(setup_repo(200, FixtureMode::OnDisk));
    let relation_repo = MemoryRelationRepository::new(&fixture.pool);
    let llm = Arc::new(MockLlmClient::new(
        r#"{"answer":"Bench answer","citations":[{"memory_id":1,"title":"digest","excerpt":"Short digest"}],"confidence":0.92,"lineages":[]}"#,
        10_000,
    ));
    let service = QueryService::new(llm, AgentConfig::default());

    c.bench_function("cognition_query_with_representation_ondisk_200", |b| {
        b.iter(|| {
            rt.block_on(service.query_with_representation(
                black_box("What happened in the benchmark session?"),
                black_box(fixture.request.clone()),
                black_box(&fixture.repo),
                black_box(&relation_repo),
            ))
            .unwrap()
        });
    });
}

fn bench_query_with_representation_ondisk_500(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(setup_repo(500, FixtureMode::OnDisk));
    let relation_repo = MemoryRelationRepository::new(&fixture.pool);
    let llm = Arc::new(MockLlmClient::new(
        r#"{"answer":"Bench answer","citations":[{"memory_id":1,"title":"digest","excerpt":"Short digest"}],"confidence":0.92,"lineages":[]}"#,
        10_000,
    ));
    let service = QueryService::new(llm, AgentConfig::default());

    c.bench_function("cognition_query_with_representation_ondisk_500", |b| {
        b.iter(|| {
            rt.block_on(service.query_with_representation(
                black_box("What happened in the benchmark session?"),
                black_box(fixture.request.clone()),
                black_box(&fixture.repo),
                black_box(&relation_repo),
            ))
            .unwrap()
        });
    });
}

fn bench_reflect_cycle_ondisk_200(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("cognition_reflect_cycle_ondisk_200", |b| {
        b.iter_batched(
            || rt.block_on(setup_repo(200, FixtureMode::OnDisk)),
            |fixture| {
                let service =
                    ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
                rt.block_on(
                    service
                        .reflect_cycle(black_box(fixture.namespace_id), black_box(&fixture.repo)),
                )
                .unwrap()
            },
            BatchSize::SmallInput,
        );
    });
}

// ---------------------------------------------------------------------------
// Session digestion benchmark — repeated digestion over growing on-disk history
// ---------------------------------------------------------------------------

fn bench_digest_session_ondisk_multi(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("cognition_digest_session_ondisk_5_sessions", |b| {
        b.iter_batched(
            || {
                let fixture =
                    rt.block_on(setup_digest_fixture(5, 30));
                let llm = Arc::new(MockLlmClient::new(
                    r#"{"short":"Compressed session summary.","long":"Detailed session summary covering all key activities and observations."}"#,
                    10_000,
                ));
                let service = DigestService::new(AgentConfig::default(), llm, None);
                (fixture, service)
            },
            |(fixture, service)| {
                rt.block_on(service.digest_session(
                    black_box(fixture.namespace_id),
                    black_box(&fixture.target_session_key),
                    black_box(&fixture.repo),
                    true,
                ))
                .unwrap()
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_representation_build(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(setup_repo(80, FixtureMode::InMemory));
    let service = RepresentationService::new();

    c.bench_function("cognition_representation_build_80", |b| {
        b.iter(|| {
            rt.block_on(service.build(black_box(&fixture.request), black_box(&fixture.repo)))
                .unwrap()
        });
    });
}

fn bench_representation_build_ondisk(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(setup_repo(80, FixtureMode::OnDisk));
    let service = RepresentationService::new();

    c.bench_function("cognition_representation_build_ondisk_80", |b| {
        b.iter(|| {
            rt.block_on(service.build(black_box(&fixture.request), black_box(&fixture.repo)))
                .unwrap()
        });
    });
}

fn bench_query_with_representation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(setup_repo(80, FixtureMode::InMemory));
    let relation_repo = MemoryRelationRepository::new(&fixture.pool);
    let llm = Arc::new(MockLlmClient::new(
        r#"{"answer":"Bench answer","citations":[{"memory_id":1,"title":"digest","excerpt":"Short digest"}],"confidence":0.92,"lineages":[]}"#,
        10_000,
    ));
    let service = QueryService::new(llm, AgentConfig::default());

    c.bench_function("cognition_query_with_representation_80", |b| {
        b.iter(|| {
            rt.block_on(service.query_with_representation(
                black_box("What happened in the benchmark session?"),
                black_box(fixture.request.clone()),
                black_box(&fixture.repo),
                black_box(&relation_repo),
            ))
            .unwrap()
        });
    });
}

fn bench_query_with_representation_ondisk(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let fixture = rt.block_on(setup_repo(80, FixtureMode::OnDisk));
    let relation_repo = MemoryRelationRepository::new(&fixture.pool);
    let llm = Arc::new(MockLlmClient::new(
        r#"{"answer":"Bench answer","citations":[{"memory_id":1,"title":"digest","excerpt":"Short digest"}],"confidence":0.92,"lineages":[]}"#,
        10_000,
    ));
    let service = QueryService::new(llm, AgentConfig::default());

    c.bench_function("cognition_query_with_representation_ondisk_80", |b| {
        b.iter(|| {
            rt.block_on(service.query_with_representation(
                black_box("What happened in the benchmark session?"),
                black_box(fixture.request.clone()),
                black_box(&fixture.repo),
                black_box(&relation_repo),
            ))
            .unwrap()
        });
    });
}

fn bench_reflect_cycle(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("cognition_reflect_cycle_40", |b| {
        b.iter_batched(
            || rt.block_on(setup_repo(40, FixtureMode::InMemory)),
            |fixture| {
                let service =
                    ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
                rt.block_on(
                    service
                        .reflect_cycle(black_box(fixture.namespace_id), black_box(&fixture.repo)),
                )
                .unwrap()
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_reflect_cycle_ondisk(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("cognition_reflect_cycle_ondisk_40", |b| {
        b.iter_batched(
            || rt.block_on(setup_repo(40, FixtureMode::OnDisk)),
            |fixture| {
                let service =
                    ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
                rt.block_on(
                    service
                        .reflect_cycle(black_box(fixture.namespace_id), black_box(&fixture.repo)),
                )
                .unwrap()
            },
            BatchSize::SmallInput,
        );
    });
}

// ---------------------------------------------------------------------------
// Micro-benchmark: CognitionSnapshot::from_memory single-parse throughput
// ---------------------------------------------------------------------------

/// Build a realistic Memory with full cognitive metadata (the common case).
fn make_memory_with_cognitive(level: CognitiveLevel) -> Memory {
    let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("sess-1".into()));
    let mut cognitive = CognitiveMetadata::new(
        level,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        "reflect_service",
    );
    cognitive.confidence = Some(0.85);
    cognitive.times_reinforced = 3;
    let md = cognitive.merge_into(&serde_json::json!({}));

    Memory {
        id: 42,
        namespace_id: 1,
        content: "Explicit observation about query orchestration patterns.".into(),
        category: MemoryCategory::Session,
        memory_lane_type: None,
        labels: vec![],
        metadata: md,
        similarity_score: None,
        relevance_score: None,
        content_embedding: None,
        embedding_model: None,
        created_at: Utc::now(),
        updated_at: None,
        last_accessed: None,
        is_active: true,
        is_archived: false,
        access_count: 0,
    }
}

fn bench_snapshot_from_memory_explicit(c: &mut Criterion) {
    let mem = make_memory_with_cognitive(CognitiveLevel::Explicit);
    c.bench_function("cognition_snapshot_from_memory_explicit", |b| {
        b.iter(|| CognitionSnapshot::from_memory(black_box(&mem)));
    });
}

fn bench_snapshot_from_memory_raw(c: &mut Criterion) {
    let mem = make_memory_with_cognitive(CognitiveLevel::Raw);
    c.bench_function("cognition_snapshot_from_memory_raw", |b| {
        b.iter(|| CognitionSnapshot::from_memory(black_box(&mem)));
    });
}

fn bench_snapshot_from_memory_derived(c: &mut Criterion) {
    let mem = make_memory_with_cognitive(CognitiveLevel::Derived);
    c.bench_function("cognition_snapshot_from_memory_derived", |b| {
        b.iter(|| CognitionSnapshot::from_memory(black_box(&mem)));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(5))
        .sample_size(50);
    targets = bench_representation_build,
        bench_representation_build_ondisk,
        bench_representation_build_ondisk_200,
        bench_representation_build_ondisk_500,
        bench_query_with_representation,
        bench_query_with_representation_ondisk,
        bench_query_with_representation_ondisk_200,
        bench_query_with_representation_ondisk_500,
        bench_reflect_cycle,
        bench_reflect_cycle_ondisk,
        bench_reflect_cycle_ondisk_200,
        bench_digest_session_ondisk_multi,
        bench_snapshot_from_memory_explicit,
        bench_snapshot_from_memory_raw,
        bench_snapshot_from_memory_derived
}

criterion_main!(benches);
