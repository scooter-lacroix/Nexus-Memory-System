use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::{
    CognitiveLevel, CognitiveMetadata, MemoryCategory, PerspectiveKey, WorkingRepresentationRequest,
};
use nexus_llm::{GenerateParams, GenerateResponse, LlmClient};
use nexus_memory_agent::{DigestService, QueryService, ReflectService, RepresentationService};
use nexus_storage::repository::{
    MemoryRelationRepository, MemoryRepository, NamespaceRepository, StoreMemoryParams,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

struct MockLlmClient {
    responses: Mutex<VecDeque<nexus_llm::Result<GenerateResponse>>>,
}

impl MockLlmClient {
    fn new(json_responses: &[&str]) -> Self {
        let responses = json_responses
            .iter()
            .map(|content| {
                Ok(GenerateResponse {
                    content: (*content).to_string(),
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
                        r#"{"answer":"fallback","citations":[],"confidence":0.5,"lineages":[]}"#
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

fn metadata(
    level: CognitiveLevel,
    perspective: &PerspectiveKey,
    ordinal: usize,
) -> serde_json::Value {
    let mut cognitive = CognitiveMetadata::new(
        level,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        "resource-test",
    );
    cognitive.confidence = Some(0.85);
    if level == CognitiveLevel::Derived {
        cognitive.times_reinforced = (ordinal % 4) as i64;
    }

    let mut metadata = cognitive.merge_into(&serde_json::json!({}));
    if level == CognitiveLevel::Raw {
        metadata["raw_activity"] = serde_json::json!({
            "derived_session_key": perspective.session_key.clone().unwrap_or_default()
        });
    }
    metadata
}

#[tokio::test]
async fn cognition_fixture_stays_resource_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("resource-efficiency.db");
    let connect_options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect_options)
        .await
        .unwrap();
    nexus_storage::migrations::run_migrations(&pool)
        .await
        .unwrap();

    let namespace_repo = NamespaceRepository::new(pool.clone());
    let namespace = namespace_repo
        .get_or_create("resource-agent", "resource-agent")
        .await
        .unwrap();
    let repo = MemoryRepository::new(pool.clone());
    let relation_repo = MemoryRelationRepository::new(&pool);
    let perspective = PerspectiveKey::new(
        "claude-code",
        "claude-code",
        Some("resource-session".to_string()),
    );

    for i in 0..80 {
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
            CognitiveLevel::Explicit => format!("Explicit memory {} about current implementation progress and verification activity.", i),
            CognitiveLevel::Derived => format!("Derived insight {} summarizing repeated implementation patterns.", i),
            CognitiveLevel::Contradiction => format!("Contradiction {} between stale assumptions and current validated behavior.", i),
            CognitiveLevel::Raw => format!("{{\"event\":\"tool\",\"tool\":\"cargo test\",\"ordinal\":{}}}", i),
            _ => unreachable!(),
        };

        repo.store(StoreMemoryParams {
            namespace_id: namespace.id,
            content: &content,
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &labels,
            metadata: &metadata(level, &perspective, i),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();
    }

    let llm = Arc::new(MockLlmClient::new(&[
        r#"{"short":"Short digest of the resource session.","long":"Long digest of the resource session covering work completed, key decisions, and validation results."}"#,
        r#"{"answer":"The session completed cognition work and validation.","citations":[{"memory_id":1,"title":"Memory 1","excerpt":"Explicit memory"}],"confidence":0.9,"lineages":[]}"#,
    ]));

    let digest_service = DigestService::new(AgentConfig::default(), llm.clone(), None);
    let digest_result = digest_service
        .digest_session(namespace.id, "resource-session", &repo, true)
        .await
        .unwrap();
    assert!(digest_result.source_count > 0);

    let reflect_service =
        ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
    let reflect_result = reflect_service
        .reflect_cycle(namespace.id, &repo)
        .await
        .unwrap();
    assert!(reflect_result.memories_scanned > 0);

    let request = WorkingRepresentationRequest {
        namespace_id: namespace.id,
        perspective: Some(perspective),
        query: Some("What happened in this session?".to_string()),
        max_items: CognitionConfig::default().representation_max_items,
        include_raw: false,
        include_recent: true,
        include_semantic: true,
        include_derived: true,
        include_digests: true,
        include_contradictions: true,
        ..WorkingRepresentationRequest::default()
    };

    let representation = RepresentationService::new()
        .build(&request, &repo)
        .await
        .unwrap();
    let flat = RepresentationService::new()
        .flat_working_set(&request, &repo)
        .await
        .unwrap();
    assert!(flat.len() <= CognitionConfig::default().representation_max_items);
    assert!(!representation.digests.is_empty());

    let query_service = QueryService::new(llm, AgentConfig::default());
    let answer = query_service
        .query_with_representation(
            "What happened in this session?",
            request,
            &repo,
            &relation_repo,
        )
        .await
        .unwrap();
    assert!(!answer.answer.is_empty());

    let digests = repo
        .list_digests(namespace.id, Some("resource-session"), 10, 0)
        .await
        .unwrap();
    let cognition = CognitionConfig::default();
    for digest in digests {
        match digest.digest_kind.as_str() {
            "short" => assert!(digest.token_count as usize <= cognition.digest_short_target_tokens),
            "long" => assert!(digest.token_count as usize <= cognition.digest_long_target_tokens),
            other => panic!("unexpected digest kind: {other}"),
        }
    }

    // Keep the on-disk fixture compact for a representative cognition run.
    let db_size = std::fs::metadata(&db_path).unwrap().len();
    assert!(
        db_size <= 1_048_576,
        "resource fixture database should stay under 1 MiB, got {} bytes",
        db_size
    );
}
