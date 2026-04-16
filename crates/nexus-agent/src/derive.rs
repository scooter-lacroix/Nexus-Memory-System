//! Derivation service - converts raw memories into explicit observations.

use std::collections::HashSet;
use std::sync::Arc;

use nexus_core::config::AgentConfig;
use nexus_core::traits::EmbeddingService;
use nexus_core::{
    cognitive_level_from_metadata, infer_perspective, perspective_from_metadata, CognitiveLevel,
    CognitiveMetadata, Memory, MemoryCategory, MemoryLaneType, PerspectiveKey, PerspectiveSource,
};
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, LlmClientJson};
use nexus_storage::models::EnqueueJobParams;
use nexus_storage::repository::{
    MemoryRepository, StoreMemoryParams, StoreMemoryWithLineageParams,
};
use serde_json::json;
use tracing::{debug, info, warn};

use crate::error::AgentError;
use crate::prompts::{derive_user_prompt, DERIVE_SYSTEM_PROMPT};

const DERIVE_MAX_TOKENS: u32 = 4096;
const REFLECT_PERSPECTIVE_JOB: &str = "reflect_perspective";
const DIGEST_SESSION_JOB: &str = "digest_session";
const DERIVE_GENERATED_BY: &str = "derive_service";
const DERIVED_FROM_ROLE: &str = "derived_from";
const RAW_ACTIVITY_LABEL: &str = "raw-activity";
const LOW_SIGNAL_LABEL: &str = "low-signal";

pub struct DeriveService {
    config: AgentConfig,
    llm: Arc<dyn LlmClient>,
    embeddings: Option<Arc<dyn EmbeddingService>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DerivedObservation {
    pub content: String,
    pub category: String,
    pub memory_lane_type: Option<String>,
    pub labels: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DerivedObservationEnvelope {
    observations: Vec<DerivedObservation>,
}

impl DeriveService {
    pub fn new(
        config: AgentConfig,
        llm: Arc<dyn LlmClient>,
        embeddings: Option<Arc<dyn EmbeddingService>>,
    ) -> Self {
        Self {
            config,
            llm,
            embeddings,
        }
    }

    pub async fn derive_memory(
        &self,
        memory: &Memory,
        repo: &MemoryRepository,
    ) -> Result<Vec<i64>, AgentError> {
        self.derive_memory_with_perspective(memory, None, repo)
            .await
    }

    pub async fn derive_memory_with_perspective(
        &self,
        memory: &Memory,
        queued_perspective: Option<&PerspectiveKey>,
        repo: &MemoryRepository,
    ) -> Result<Vec<i64>, AgentError> {
        if !is_derivable_source(memory) {
            return Ok(Vec::new());
        }

        let existing_ids = existing_derived_ids(repo, memory.id).await?;
        if !existing_ids.is_empty() {
            debug!(
                memory_id = memory.id,
                derived_count = existing_ids.len(),
                "Reusing existing derived observations"
            );
            return Ok(existing_ids);
        }

        let perspective = queued_perspective
            .cloned()
            .or_else(|| perspective_from_metadata(&memory.metadata))
            .unwrap_or_else(|| {
                infer_perspective(
                    PerspectiveSource::HookIngest,
                    self.config.namespace.clone(),
                    None,
                    None,
                )
            });

        let observations = match self.derive_with_llm(memory).await {
            Ok(observations) => observations,
            Err(error) => {
                warn!(memory_id = memory.id, %error, "LLM derivation failed, using fallback");
                fallback_observations(memory)
            }
        };

        let observations = normalize_observations(observations);
        if observations.is_empty() {
            debug!(memory_id = memory.id, "No explicit observations derived");
            return Ok(Vec::new());
        }

        // --- PHASE 10 OPTIMIZATION: BATCH EMBEDDING ---
        let mut derived_ids = Vec::with_capacity(observations.len());

        // Prepare batch for embedding
        let contents: Vec<String> = observations.iter().map(|o| o.content.clone()).collect();
        let mut embeddings_map: std::collections::HashMap<usize, (Vec<f32>, String)> =
            std::collections::HashMap::new();

        if let Some(service) = self.embeddings.as_deref() {
            match service.embed_batch(&contents).await {
                Ok(vecs) => {
                    for (i, vec) in vecs.into_iter().enumerate() {
                        embeddings_map.insert(i, (vec, service.model_name().to_string()));
                    }
                }
                Err(error) => {
                    warn!(%error, "Batch embedding failed, falling back to individual or no embeddings");
                }
            }
        }

        for (i, observation) in observations.into_iter().enumerate() {
            let category = MemoryCategory::parse(&observation.category).unwrap_or(memory.category);
            let memory_lane_type = observation
                .memory_lane_type
                .as_deref()
                .and_then(MemoryLaneType::parse);
            let metadata = derive_metadata(memory, &perspective, observation.confidence);

            // Check batch results
            let (embedding, embedding_model) = if let Some((vec, model)) = embeddings_map.get(&i) {
                (Some(vec.clone()), Some(model.clone()))
            } else {
                (None, None)
            };

            let derived = repo
                .store_with_lineage(StoreMemoryWithLineageParams {
                    store: StoreMemoryParams {
                        namespace_id: memory.namespace_id,
                        content: &observation.content,
                        category: &category,
                        memory_lane_type: memory_lane_type.as_ref(),
                        labels: &observation.labels,
                        metadata: &metadata,
                        embedding: embedding.as_deref(),
                        embedding_model: embedding_model.as_deref(),
                    },
                    source_memory_ids: &[memory.id],
                    evidence_role: DERIVED_FROM_ROLE,
                })
                .await
                .map_err(|error| AgentError::Storage(error.to_string()))?;
            derived_ids.push(derived.id);
        }

        enqueue_follow_up_jobs(repo, memory, &perspective, &derived_ids, &self.config).await?;

        info!(
            memory_id = memory.id,
            derived_count = derived_ids.len(),
            "Derived explicit observations from raw memory"
        );
        Ok(derived_ids)
    }

    async fn derive_with_llm(
        &self,
        memory: &Memory,
    ) -> Result<Vec<DerivedObservation>, AgentError> {
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(DERIVE_SYSTEM_PROMPT),
                ChatMessage::user(derive_user_prompt(memory)),
            ],
            max_tokens: DERIVE_MAX_TOKENS,
            temperature: 0.1,
            json_mode: true,
        };

        let envelope: DerivedObservationEnvelope = self
            .llm
            .generate_json(params)
            .await
            .map_err(|error| AgentError::Llm(error.to_string()))?;

        Ok(envelope.observations)
    }
}

fn derive_metadata(
    source: &Memory,
    perspective: &PerspectiveKey,
    confidence: f32,
) -> serde_json::Value {
    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::Explicit,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        DERIVE_GENERATED_BY,
    );
    cognitive.source_memory_ids = vec![source.id];
    cognitive.confidence = Some(confidence.max(0.70));
    cognitive.merge_into(&sanitized_source_metadata(source))
}

fn fallback_observations(memory: &Memory) -> Vec<DerivedObservation> {
    let summary = memory
        .metadata
        .get("agent")
        .and_then(|agent| agent.get("summary"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|summary| summary.len() >= 16 && !looks_like_noise(summary))
        .map(ToString::to_string);

    let content = memory
        .content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let candidate = summary.unwrap_or(content);
    if candidate.is_empty() || looks_like_noise(&candidate) {
        return Vec::new();
    }

    vec![DerivedObservation {
        content: candidate,
        category: memory.category.to_string(),
        memory_lane_type: memory.memory_lane_type.as_ref().map(ToString::to_string),
        labels: explicit_labels_from_source(memory),
        confidence: 0.70,
    }]
}

fn sanitized_source_metadata(source: &Memory) -> serde_json::Value {
    let mut sanitized = serde_json::Map::new();

    if let Some(agent) = source
        .metadata
        .get("agent")
        .and_then(serde_json::Value::as_object)
    {
        let mut agent_sanitized = serde_json::Map::new();
        for key in [
            "summary",
            "entities",
            "topics",
            "importance_score",
            "source",
            "generated_by",
        ] {
            if let Some(value) = agent.get(key) {
                agent_sanitized.insert(key.to_string(), value.clone());
            }
        }
        if !agent_sanitized.is_empty() {
            sanitized.insert(
                "agent".to_string(),
                serde_json::Value::Object(agent_sanitized),
            );
        }
    }

    serde_json::Value::Object(sanitized)
}

fn explicit_labels_from_source(source: &Memory) -> Vec<String> {
    let mut labels: Vec<String> = source
        .labels
        .iter()
        .filter(|label| {
            !label.eq_ignore_ascii_case(RAW_ACTIVITY_LABEL)
                && !label.eq_ignore_ascii_case(LOW_SIGNAL_LABEL)
        })
        .cloned()
        .collect();
    dedupe_labels(&mut labels);
    labels
}

fn normalize_observations(observations: Vec<DerivedObservation>) -> Vec<DerivedObservation> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for mut observation in observations {
        observation.content = observation.content.trim().to_string();
        if observation.content.is_empty() || observation.confidence < 0.70 {
            continue;
        }

        let fingerprint = observation.content.to_lowercase();
        if !seen.insert(fingerprint) {
            continue;
        }

        observation.labels.retain(|label| {
            !label.eq_ignore_ascii_case(RAW_ACTIVITY_LABEL)
                && !label.eq_ignore_ascii_case(LOW_SIGNAL_LABEL)
        });
        dedupe_labels(&mut observation.labels);
        normalized.push(observation);
    }

    normalized
}

fn dedupe_labels(labels: &mut Vec<String>) {
    let mut seen = HashSet::new();
    labels.retain(|label| seen.insert(label.to_lowercase()));
}

fn looks_like_noise(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_ascii_punctuation() || c.is_whitespace())
}

fn is_derivable_source(memory: &Memory) -> bool {
    memory.category == MemoryCategory::Session
        && memory.labels.iter().any(|l| l == RAW_ACTIVITY_LABEL)
        && cognitive_level_from_metadata(&memory.metadata) == CognitiveLevel::Raw
}

async fn existing_derived_ids(
    repo: &MemoryRepository,
    source_id: i64,
) -> Result<Vec<i64>, AgentError> {
    let lineage = repo
        .load_lineage(source_id)
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    Ok(lineage
        .into_iter()
        .filter(|entry| {
            entry.source_memory_id == source_id && entry.evidence_role == DERIVED_FROM_ROLE
        })
        .map(|entry| entry.derived_memory_id)
        .collect())
}

async fn enqueue_follow_up_jobs(
    repo: &MemoryRepository,
    source: &Memory,
    perspective: &PerspectiveKey,
    derived_ids: &[i64],
    _config: &AgentConfig,
) -> Result<(), AgentError> {
    let perspective_json = serde_json::to_value(perspective).ok();

    // Enqueue reflect jobs for new derived IDs
    for &id in derived_ids {
        let payload = json!({
            "memory_id": id,
            "source": "derive_follow_up",
            "reason": "new_explicit_observation",
        });
        repo.enqueue_job(EnqueueJobParams {
            namespace_id: source.namespace_id,
            job_type: REFLECT_PERSPECTIVE_JOB,
            priority: 110,
            perspective: perspective_json.as_ref(),
            payload: &payload,
        })
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;
    }

    // Always attempt a session digest rollover after derivation
    let payload = json!({
        "session_key": perspective.session_key,
        "reason": "post_derivation_rollover",
    });
    repo.enqueue_job(EnqueueJobParams {
        namespace_id: source.namespace_id,
        job_type: DIGEST_SESSION_JOB,
        priority: 120,
        perspective: perspective_json.as_ref(),
        payload: &payload,
    })
    .await
    .map_err(|error| AgentError::Storage(error.to_string()))?;

    Ok(())
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
            .get_or_create("derive-test", "derive-test")
            .await
            .unwrap();
        (pool.clone(), MemoryRepository::new(pool), namespace.id)
    }

    fn derive_response() -> GenerateResponse {
        let envelope = DerivedObservationEnvelope {
            observations: vec![DerivedObservation {
                content: "Explicit observation from derivation.".to_string(),
                category: "session".to_string(),
                memory_lane_type: Some("process".to_string()),
                labels: vec!["derived".to_string()],
                confidence: 0.95,
            }],
        };
        GenerateResponse {
            content: serde_json::to_string(&envelope).unwrap(),
            model: "mock-model".to_string(),
            usage: None,
        }
    }

    #[tokio::test]
    async fn test_derive_memory_persists_explicit_observations_and_jobs() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        let service = DeriveService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(vec![Ok(derive_response())])),
            None,
        );

        let metadata = json!({
            "cognitive": {
                "level": "raw",
                "observer": "claude",
                "subject": "claude",
                "session_key": "sess-1",
            }
        });
        let raw = repo
            .store(StoreMemoryParams {
                namespace_id,
                content: "Raw implementation log.",
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &["raw-activity".to_string()],
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let derived_ids = service.derive_memory(&raw, &repo).await.unwrap();
        assert_eq!(derived_ids.len(), 1);

        let derived = repo.get_by_id(derived_ids[0]).await.unwrap().unwrap();
        assert_eq!(
            cognitive_level_from_metadata(&derived.metadata),
            CognitiveLevel::Explicit
        );

        let jobs = repo
            .list_jobs(namespace_id, None, None, 10, 0)
            .await
            .unwrap();
        assert!(jobs.iter().any(|j| j.job_type == REFLECT_PERSPECTIVE_JOB));
        assert!(jobs.iter().any(|j| j.job_type == DIGEST_SESSION_JOB));
    }
}
