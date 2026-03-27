use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use nexus_core::config::AgentConfig;
use nexus_core::{
    CognitiveLevel, CognitiveMetadata, MemoryCategory, PerspectiveKey, WorkingRepresentationRequest,
};
use nexus_llm::{GenerateParams, GenerateResponse, LlmClient};
use nexus_memory_agent::{QueryService, ReflectService, RepresentationService};
use nexus_storage::repository::{
    MemoryRelationRepository, MemoryRepository, NamespaceRepository, StoreDigestParams,
    StoreMemoryParams,
};
use sqlx::sqlite::SqlitePoolOptions;

struct MockLlmClient {
    responses: Mutex<VecDeque<nexus_llm::Result<GenerateResponse>>>,
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

async fn setup_repo(
    total_memories: usize,
) -> (
    sqlx::SqlitePool,
    MemoryRepository,
    i64,
    PerspectiveKey,
    WorkingRepresentationRequest,
) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
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
    };

    (pool, repo, namespace.id, perspective, request)
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

fn bench_representation_build(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (_pool, repo, _namespace_id, _perspective, request) = rt.block_on(setup_repo(80));
    let service = RepresentationService::new();

    c.bench_function("cognition_representation_build_80", |b| {
        b.iter(|| {
            rt.block_on(service.build(black_box(&request), black_box(&repo)))
                .unwrap()
        });
    });
}

fn bench_query_with_representation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (pool, repo, _namespace_id, _perspective, request) = rt.block_on(setup_repo(80));
    let relation_repo = MemoryRelationRepository::new(&pool);
    let llm = Arc::new(MockLlmClient::new(
        r#"{"answer":"Bench answer","citations":[{"memory_id":1,"title":"digest","excerpt":"Short digest"}],"confidence":0.92,"lineages":[]}"#,
        10_000,
    ));
    let service = QueryService::new(llm, AgentConfig::default());

    c.bench_function("cognition_query_with_representation_80", |b| {
        b.iter(|| {
            rt.block_on(service.query_with_representation(
                black_box("What happened in the benchmark session?"),
                black_box(request.clone()),
                black_box(&repo),
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
            || rt.block_on(setup_repo(40)),
            |(_pool, repo, namespace_id, _perspective, _request)| {
                let service = ReflectService::new(AgentConfig::default());
                rt.block_on(service.reflect_cycle(black_box(namespace_id), black_box(&repo)))
                    .unwrap()
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(5))
        .sample_size(50);
    targets = bench_representation_build, bench_query_with_representation, bench_reflect_cycle
}

criterion_main!(benches);
