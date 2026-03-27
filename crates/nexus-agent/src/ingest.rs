//! Ingest service - extracts structured info from raw text

use std::sync::Arc;

use nexus_core::config::AgentConfig;
use nexus_core::{
    infer_perspective, CognitiveLevel, CognitiveMetadata, MemoryCategory, PerspectiveSource,
};
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, LlmClientJson};
use nexus_storage::models::EnqueueJobParams;
use nexus_storage::repository::{MemoryRepository, StoreMemoryParams};
use tracing::{debug, error, info};

// Default max tokens for LLM enrichment responses
const INGEST_MAX_TOKENS: u32 = 8192;
const DERIVE_MEMORY_JOB: &str = "derive_memory";

use crate::error::AgentError;
use crate::prompts::{ingest_user_prompt, INGEST_SYSTEM_PROMPT};
use crate::types::IngestExtraction;

#[derive(Debug, Clone, Default)]
pub struct IngestContext {
    pub session_key: Option<String>,
    pub cwd: Option<String>,
}

pub struct IngestService {
    llm: Arc<dyn LlmClient>,
    config: AgentConfig,
}

impl IngestService {
    pub fn new(llm: Arc<dyn LlmClient>, config: AgentConfig) -> Self {
        Self { llm, config }
    }

    pub async fn ingest(
        &self,
        content: &str,
        source: &str,
        namespace_id: i64,
        repo: &MemoryRepository,
    ) -> Result<i64, AgentError> {
        self.ingest_with_context(
            content,
            source,
            namespace_id,
            repo,
            IngestContext::default(),
        )
        .await
    }

    pub async fn ingest_with_context(
        &self,
        content: &str,
        source: &str,
        namespace_id: i64,
        repo: &MemoryRepository,
        context: IngestContext,
    ) -> Result<i64, AgentError> {
        info!(source = %source, "Ingesting content");

        // Step 1: Extract structured info via LLM
        let extraction = self.extract(content, source).await?;
        debug!(summary = %extraction.summary, "Extracted content info");

        // Step 2: Build labels from entities and topics
        let labels: Vec<String> = extraction
            .entities
            .iter()
            .chain(extraction.topics.iter())
            .cloned()
            .collect();

        let perspective = infer_perspective(
            PerspectiveSource::HookIngest,
            self.config.namespace.clone(),
            None,
            context.session_key.clone(),
        );
        let mut cognitive = CognitiveMetadata::new(
            CognitiveLevel::Raw,
            perspective.observer.clone(),
            perspective.subject.clone(),
            perspective.session_key.clone(),
            "ingest_service",
        );
        cognitive.confidence = Some(extraction.importance_score);

        // Step 3: Build metadata with agent extraction info
        let metadata = cognitive.merge_into(&serde_json::json!({
            "agent": {
                "summary": extraction.summary,
                "entities": extraction.entities,
                "topics": extraction.topics,
                "importance_score": extraction.importance_score,
                "source": source,
                "generated_by": "ingest_agent"
            }
        }));

        // Step 4: Store memory using repository
        let _title = format!("Ingested: {}", source);
        let memory = repo
            .store(StoreMemoryParams {
                namespace_id,
                content,
                category: &MemoryCategory::General,
                memory_lane_type: None,
                labels: &labels,
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to store memory");
                AgentError::Storage(e.to_string())
            })?;

        let derive_payload = serde_json::json!({
            "memory_id": memory.id,
            "agent_namespace": self.config.namespace,
            "source": source,
            "session_key": context.session_key,
            "cwd": context.cwd,
        });
        let derive_perspective = serde_json::to_value(&perspective).ok();
        repo.enqueue_job(EnqueueJobParams {
            namespace_id,
            job_type: DERIVE_MEMORY_JOB,
            priority: 100,
            perspective: derive_perspective.as_ref(),
            payload: &derive_payload,
        })
        .await
        .map_err(|e| {
            error!(error = %e, memory_id = memory.id, "Failed to enqueue derive job");
            AgentError::Storage(e.to_string())
        })?;

        info!(memory_id = memory.id, "Memory stored successfully");
        Ok(memory.id)
    }

    async fn extract(&self, content: &str, source: &str) -> Result<IngestExtraction, AgentError> {
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(INGEST_SYSTEM_PROMPT),
                ChatMessage::user(ingest_user_prompt(content, source)),
            ],
            max_tokens: INGEST_MAX_TOKENS,
            temperature: 0.3,
            json_mode: true,
        };

        let extraction: IngestExtraction = self
            .llm
            .generate_json(params)
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;

        Ok(extraction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use nexus_llm::GenerateResponse;
    use nexus_storage::repository::NamespaceRepository;
    use sqlx::sqlite::SqlitePoolOptions;

    use crate::types::IngestExtraction;
    use nexus_core::cognitive_level_from_metadata;

    struct MockLlmClient {
        responses: Mutex<VecDeque<nexus_llm::Result<GenerateResponse>>>,
    }

    impl MockLlmClient {
        fn new(responses: Vec<nexus_llm::Result<GenerateResponse>>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
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
                .expect("mock response missing")
        }

        fn provider_name(&self) -> String {
            "mock".to_string()
        }

        fn model_name(&self) -> String {
            "mock-model".to_string()
        }
    }

    async fn setup_repo() -> (sqlx::SqlitePool, MemoryRepository, i64) {
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
            .get_or_create("ingest-test", "ingest-test")
            .await
            .unwrap();
        (pool.clone(), MemoryRepository::new(pool), namespace.id)
    }

    fn extraction_response() -> GenerateResponse {
        let extraction = IngestExtraction {
            summary: "Captured a durable implementation update.".to_string(),
            entities: vec!["query".to_string()],
            topics: vec!["pagination".to_string()],
            importance_score: 0.88,
        };
        GenerateResponse {
            content: serde_json::to_string(&extraction).unwrap(),
            model: "mock-model".to_string(),
            usage: None,
        }
    }

    #[tokio::test]
    async fn test_ingest_stores_raw_cognitive_metadata() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        let service = IngestService::new(
            Arc::new(MockLlmClient::new(vec![Ok(extraction_response())])),
            AgentConfig {
                enabled: true,
                namespace: "claude-code".to_string(),
                ..AgentConfig::default()
            },
        );

        let memory_id = service
            .ingest_with_context(
                "Implemented working-set retrieval.",
                "unit-test",
                namespace_id,
                &repo,
                IngestContext::default(),
            )
            .await
            .unwrap();

        let memory = repo.get_by_id(memory_id).await.unwrap().unwrap();
        assert_eq!(
            cognitive_level_from_metadata(&memory.metadata),
            CognitiveLevel::Raw
        );
        assert_eq!(
            memory.metadata["cognitive"]["generated_by"],
            serde_json::Value::String("ingest_service".to_string())
        );
    }

    #[tokio::test]
    async fn test_ingest_enqueues_derive_job() {
        let (pool, repo, namespace_id) = setup_repo().await;
        let service = IngestService::new(
            Arc::new(MockLlmClient::new(vec![Ok(extraction_response())])),
            AgentConfig {
                enabled: true,
                namespace: "claude-code".to_string(),
                ..AgentConfig::default()
            },
        );

        let memory_id = service
            .ingest_with_context(
                "Implemented derive service foundation.",
                "unit-test",
                namespace_id,
                &repo,
                IngestContext {
                    session_key: Some("session-ctx".to_string()),
                    cwd: Some("/tmp/project".to_string()),
                },
            )
            .await
            .unwrap();

        let job_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memory_jobs WHERE job_type = ?")
                .bind(DERIVE_MEMORY_JOB)
                .fetch_one(&pool)
                .await
                .unwrap();
        let job_memory_id: i64 =
            sqlx::query_scalar("SELECT json_extract(payload_json, '$.memory_id') FROM memory_jobs WHERE job_type = ? LIMIT 1")
                .bind(DERIVE_MEMORY_JOB)
                .fetch_one(&pool)
                .await
                .unwrap();
        let job_session_key: String =
            sqlx::query_scalar("SELECT json_extract(payload_json, '$.session_key') FROM memory_jobs WHERE job_type = ? LIMIT 1")
                .bind(DERIVE_MEMORY_JOB)
                .fetch_one(&pool)
                .await
                .unwrap();
        let job_perspective_session_key: String =
            sqlx::query_scalar("SELECT json_extract(perspective_json, '$.session_key') FROM memory_jobs WHERE job_type = ? LIMIT 1")
                .bind(DERIVE_MEMORY_JOB)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(job_count, 1);
        assert_eq!(job_memory_id, memory_id);
        assert_eq!(job_session_key, "session-ctx");
        assert_eq!(job_perspective_session_key, "session-ctx");
    }
}
