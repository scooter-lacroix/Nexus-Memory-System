//! Digest service - produces short and long session summaries.

use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use nexus_core::config::AgentConfig;
use nexus_core::traits::EmbeddingService;
use nexus_core::{
    cognitive_level_from_metadata, CognitiveLevel, CognitiveMetadata, Memory, MemoryCategory,
    MemoryLaneCognitiveType, MemoryLaneType, PerspectiveKey,
};
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, TokenUsage};
use nexus_storage::repository::{
    MemoryRepository, StoreDigestParams, StoreMemoryParams, StoreMemoryWithLineageParams,
};
use tracing::{debug, info, warn};

use crate::error::AgentError;
use crate::prompts::{digest_user_prompt, DIGEST_SYSTEM_PROMPT};
use crate::util::{
    flush_metric_samples, maybe_embed, parse_json_response, stage_metric_sample,
    token_usage_metric_samples,
};

const DIGEST_MAX_TOKENS: u32 = 4096;
const DIGEST_MAX_SOURCE_MEMORIES: i64 = 200;
const DIGEST_GENERATED_BY: &str = "digest_service";
const DIGEST_KIND_SHORT: &str = "short";
const DIGEST_KIND_LONG: &str = "long";
const SHORT_MAX_CHARS: usize = 4000;
const LONG_MAX_CHARS: usize = 12000;
const DIGESTED_FROM_ROLE: &str = "digested_from";

/// Result of a session digest operation.
#[derive(Debug, Clone)]
pub struct DigestResult {
    pub short_id: i64,
    pub long_id: i64,
    pub source_count: usize,
}

/// Parsed LLM digest output.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DigestEnvelope {
    short: String,
    long: String,
}

pub struct DigestService {
    config: AgentConfig,
    llm: Arc<dyn LlmClient>,
    embeddings: Option<Arc<dyn EmbeddingService>>,
}

impl DigestService {
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

    /// Produce short and long digests for a session.
    ///
    /// Idempotent: if digests already exist for the session and `force` is false,
    /// returns the existing digest IDs without regenerating.
    pub async fn digest_session(
        &self,
        namespace_id: i64,
        session_key: &str,
        repo: &MemoryRepository,
        force: bool,
    ) -> Result<DigestResult, AgentError> {
        let total_started = Instant::now();
        let mut metrics = Vec::new();
        // Idempotency: return existing digests if present
        if !force {
            if let Some(existing) = existing_digest_ids(repo, namespace_id, session_key).await? {
                debug!(
                    namespace_id,
                    session_key,
                    short_id = existing.short_id,
                    long_id = existing.long_id,
                    "Reusing existing session digests"
                );
                return Ok(existing);
            }
        }

        let gather_started = Instant::now();
        let memories = gather_session_memories(repo, namespace_id, session_key).await?;
        metrics.push(stage_metric_sample(
            namespace_id,
            "cognition.digest.gather_ms",
            gather_started.elapsed().as_secs_f64() * 1000.0,
            "gather",
        ));
        if memories.is_empty() {
            return Err(AgentError::Digest(format!(
                "No non-raw memories found for session \"{}\"",
                session_key
            )));
        }

        let source_count = memories.len();
        let source_ids: Vec<i64> = memories.iter().map(|m| m.id).collect();
        let min_id = source_ids.iter().copied().min().unwrap_or(0);
        let max_id = source_ids.iter().copied().max().unwrap_or(0);

        let produce_started = Instant::now();
        let (envelope, usage) = produce_digests(self.llm.as_ref(), session_key, &memories).await;
        metrics.push(stage_metric_sample(
            namespace_id,
            "cognition.digest.produce_ms",
            produce_started.elapsed().as_secs_f64() * 1000.0,
            "produce",
        ));
        metrics.extend(token_usage_metric_samples(
            namespace_id,
            "cognition.digest.produce",
            "produce",
            usage.as_ref(),
        ));
        let short_content = truncate(envelope.short.trim(), SHORT_MAX_CHARS);
        let long_content = truncate(envelope.long.trim(), LONG_MAX_CHARS);

        let perspective = PerspectiveKey {
            observer: self.config.namespace.clone(),
            subject: self.config.namespace.clone(),
            session_key: Some(session_key.to_string()),
        };

        let embeddings = self.embeddings.as_deref();
        let short_memory = store_digest_memory(
            repo,
            namespace_id,
            &short_content,
            CognitiveLevel::SummaryShort,
            &perspective,
            &source_ids,
            embeddings,
        )
        .await?;

        let long_memory = store_digest_memory(
            repo,
            namespace_id,
            &long_content,
            CognitiveLevel::SummaryLong,
            &perspective,
            &source_ids,
            embeddings,
        )
        .await?;

        let short_tokens = estimate_tokens(&short_content);
        let long_tokens = estimate_tokens(&long_content);

        let store_started = Instant::now();
        repo.store_digest(StoreDigestParams {
            namespace_id,
            session_key,
            digest_kind: DIGEST_KIND_SHORT,
            memory_id: short_memory.id,
            start_memory_id: Some(min_id),
            end_memory_id: Some(max_id),
            token_count: short_tokens,
        })
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

        repo.store_digest(StoreDigestParams {
            namespace_id,
            session_key,
            digest_kind: DIGEST_KIND_LONG,
            memory_id: long_memory.id,
            start_memory_id: Some(min_id),
            end_memory_id: Some(max_id),
            token_count: long_tokens,
        })
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;
        metrics.push(stage_metric_sample(
            namespace_id,
            "cognition.digest.store_ms",
            store_started.elapsed().as_secs_f64() * 1000.0,
            "store",
        ));

        info!(
            namespace_id,
            session_key,
            short_id = short_memory.id,
            long_id = long_memory.id,
            source_count,
            "Created session digests"
        );
        metrics.push(stage_metric_sample(
            namespace_id,
            "cognition.digest.total_ms",
            total_started.elapsed().as_secs_f64() * 1000.0,
            "total",
        ));
        flush_metric_samples(repo, &metrics).await;

        Ok(DigestResult {
            short_id: short_memory.id,
            long_id: long_memory.id,
            source_count,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn existing_digest_ids(
    repo: &MemoryRepository,
    namespace_id: i64,
    session_key: &str,
) -> Result<Option<DigestResult>, AgentError> {
    let short = repo
        .latest_digest_for_session(namespace_id, session_key, DIGEST_KIND_SHORT)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    let long = repo
        .latest_digest_for_session(namespace_id, session_key, DIGEST_KIND_LONG)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    match (short, long) {
        (Some(s), Some(l)) => Ok(Some(DigestResult {
            short_id: s.id,
            long_id: l.id,
            source_count: 0,
        })),
        _ => Ok(None),
    }
}

async fn gather_session_memories(
    repo: &MemoryRepository,
    namespace_id: i64,
    session_key: &str,
) -> Result<Vec<Memory>, AgentError> {
    let matching = repo
        .list_by_session_key(namespace_id, session_key, DIGEST_MAX_SOURCE_MEMORIES, false)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    Ok(matching
        .into_iter()
        .filter(|m| {
            let level = cognitive_level_from_metadata(&m.metadata);
            if matches!(
                level,
                CognitiveLevel::Raw | CognitiveLevel::SummaryShort | CognitiveLevel::SummaryLong
            ) {
                return false;
            }
            true
        })
        .collect())
}

async fn produce_digests(
    llm: &dyn LlmClient,
    session_key: &str,
    memories: &[Memory],
) -> (DigestEnvelope, Option<TokenUsage>) {
    let pairs: Vec<(i64, &str)> = memories
        .iter()
        .map(|m| (m.id, m.content.as_str()))
        .collect();
    let user_msg = digest_user_prompt(session_key, &pairs);

    let params = GenerateParams {
        messages: vec![
            ChatMessage::system(DIGEST_SYSTEM_PROMPT),
            ChatMessage::user(user_msg),
        ],
        max_tokens: DIGEST_MAX_TOKENS,
        temperature: 0.2,
        json_mode: true,
    };

    match llm.generate(params).await {
        Ok(response) => {
            let usage = response.usage.clone();
            match parse_json_response::<DigestEnvelope>(&response) {
                Ok(envelope)
                    if envelope.short.trim().is_empty() && envelope.long.trim().is_empty() =>
                {
                    warn!("LLM returned empty digest, using fallback");
                    (fallback_digest(memories), usage)
                }
                Ok(envelope) => (envelope, usage),
                Err(error) => {
                    warn!(%error, "LLM digest response was invalid JSON, using fallback");
                    (fallback_digest(memories), usage)
                }
            }
        }
        Err(error) => {
            warn!(%error, "LLM digest call failed, using fallback");
            (fallback_digest(memories), None)
        }
    }
}

fn fallback_digest(memories: &[Memory]) -> DigestEnvelope {
    let short = fallback_short(memories);
    let long = fallback_long(memories);
    DigestEnvelope { short, long }
}

fn fallback_short(memories: &[Memory]) -> String {
    let combined: String = memories
        .iter()
        .take(5)
        .map(|m| m.content.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    truncate(&combined, SHORT_MAX_CHARS)
}

fn fallback_long(memories: &[Memory]) -> String {
    let combined: String = memories
        .iter()
        .map(|m| m.content.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    truncate(&combined, LONG_MAX_CHARS)
}

async fn store_digest_memory(
    repo: &MemoryRepository,
    namespace_id: i64,
    content: &str,
    level: CognitiveLevel,
    perspective: &PerspectiveKey,
    source_ids: &[i64],
    embeddings: Option<&dyn EmbeddingService>,
) -> Result<Memory, AgentError> {
    let mut cognitive = CognitiveMetadata::new(
        level,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        DIGEST_GENERATED_BY,
    );
    cognitive.source_memory_ids = source_ids.to_vec();
    cognitive.confidence = Some(0.80);
    cognitive.times_reinforced = 0;
    cognitive.times_contradicted = 0;
    cognitive.derived_at = Some(Utc::now());
    cognitive.generated_by = Some(DIGEST_GENERATED_BY.to_string());

    let metadata = cognitive.merge_into(&serde_json::json!({}));
    let (embedding, embedding_model) = maybe_embed(embeddings, content).await;

    let memory = repo
        .store_with_lineage(StoreMemoryWithLineageParams {
            store: StoreMemoryParams {
                namespace_id,
                content,
                category: &MemoryCategory::Session,
                memory_lane_type: Some(&MemoryLaneType::Cognitive(
                    MemoryLaneCognitiveType::Explicit,
                )),
                labels: &["digest".to_string(), level.to_string().to_lowercase()],
                metadata: &metadata,
                embedding: embedding.as_deref(),
                embedding_model: embedding_model.as_deref(),
            },
            source_memory_ids: source_ids,
            evidence_role: DIGESTED_FROM_ROLE,
        })
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    Ok(memory)
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = s[..end].to_string();
    if let Some(last_space) = truncated.rfind(' ') {
        truncated.truncate(last_space);
    }
    truncated
}

fn estimate_tokens(s: &str) -> usize {
    s.split_whitespace().count()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::Utc;
    use nexus_core::MemoryLanePriorityType;
    use nexus_llm::GenerateResponse;
    use nexus_storage::repository::NamespaceRepository;
    use sqlx::sqlite::SqlitePoolOptions;

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
            .get_or_create("digest-test", "digest-test")
            .await
            .unwrap();
        let repo = MemoryRepository::new(pool.clone());
        (pool, repo, namespace.id)
    }

    fn session_metadata(session_key: &str, level: CognitiveLevel) -> serde_json::Value {
        let cognitive = CognitiveMetadata::new(
            level,
            "claude-code",
            "claude-code",
            Some(session_key.to_string()),
            "derive_service",
        );
        cognitive.merge_into(&serde_json::json!({}))
    }

    async fn store_session_memory(
        repo: &MemoryRepository,
        namespace_id: i64,
        content: &str,
        session_key: &str,
        level: CognitiveLevel,
    ) -> Memory {
        repo.store(StoreMemoryParams {
            namespace_id,
            content,
            category: &MemoryCategory::Session,
            memory_lane_type: Some(&MemoryLaneType::Priority(MemoryLanePriorityType::Decision)),
            labels: &["test".to_string()],
            metadata: &session_metadata(session_key, level),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap()
    }

    fn good_digest_response() -> GenerateResponse {
        GenerateResponse {
            content: r#"{"short":"Fixed query pagination and added working-set retrieval.","long":"Session focused on fixing query pagination behavior in the memory system. The team tightened the ranking logic and added bounded working-set retrieval. New digest infrastructure was introduced to track session summaries."}"#
                .to_string(),
            model: "mock-model".to_string(),
            usage: None,
        }
    }

    #[tokio::test]
    async fn test_digest_session_errors_on_empty_session() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(Vec::new())),
            None,
        );

        let result = service
            .digest_session(namespace_id, "empty-session", &repo, false)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No non-raw memories"));
    }

    #[tokio::test]
    async fn test_digest_session_creates_short_and_long() {
        let (pool, repo, namespace_id) = setup_repo().await;
        store_session_memory(
            &repo,
            namespace_id,
            "Fixed query pagination behavior.",
            "session-1",
            CognitiveLevel::Explicit,
        )
        .await;
        store_session_memory(
            &repo,
            namespace_id,
            "Added working-set retrieval primitives.",
            "session-1",
            CognitiveLevel::Explicit,
        )
        .await;

        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(vec![Ok(good_digest_response())])),
            None,
        );

        let result = service
            .digest_session(namespace_id, "session-1", &repo, false)
            .await
            .unwrap();

        assert_eq!(result.source_count, 2);
        assert!(result.short_id > 0);
        assert!(result.long_id > 0);

        let short = repo.get_by_id(result.short_id).await.unwrap().unwrap();
        assert_eq!(
            cognitive_level_from_metadata(&short.metadata),
            CognitiveLevel::SummaryShort
        );
        assert!(short.content.contains("pagination"));

        let long = repo.get_by_id(result.long_id).await.unwrap().unwrap();
        assert_eq!(
            cognitive_level_from_metadata(&long.metadata),
            CognitiveLevel::SummaryLong
        );
        assert!(long.content.contains("digest"));

        // Verify session_digests registrations
        let short_digests: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_digests WHERE digest_kind = ?")
                .bind(DIGEST_KIND_SHORT)
                .fetch_one(&pool)
                .await
                .unwrap();
        let long_digests: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_digests WHERE digest_kind = ?")
                .bind(DIGEST_KIND_LONG)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(short_digests, 1);
        assert_eq!(long_digests, 1);
    }

    #[tokio::test]
    async fn test_digest_session_falls_back_on_llm_failure() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        store_session_memory(
            &repo,
            namespace_id,
            "Implemented digest service with fallback behavior.",
            "session-2",
            CognitiveLevel::Explicit,
        )
        .await;

        let bad_response = GenerateResponse {
            content: "not valid json at all".to_string(),
            model: "mock-model".to_string(),
            usage: None,
        };
        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(vec![Ok(bad_response)])),
            None,
        );

        let result = service
            .digest_session(namespace_id, "session-2", &repo, false)
            .await
            .unwrap();

        assert_eq!(result.source_count, 1);

        let short = repo.get_by_id(result.short_id).await.unwrap().unwrap();
        assert_eq!(
            cognitive_level_from_metadata(&short.metadata),
            CognitiveLevel::SummaryShort
        );
        // Fallback content comes from source memories
        assert!(short.content.len() <= SHORT_MAX_CHARS);

        let long = repo.get_by_id(result.long_id).await.unwrap().unwrap();
        assert!(long.content.contains("digest service"));
    }

    #[tokio::test]
    async fn test_digest_session_is_idempotent() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        store_session_memory(
            &repo,
            namespace_id,
            "Refactored the memory consolidation pipeline.",
            "session-3",
            CognitiveLevel::Explicit,
        )
        .await;

        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(vec![Ok(good_digest_response())])),
            None,
        );

        let first = service
            .digest_session(namespace_id, "session-3", &repo, false)
            .await
            .unwrap();
        let second = service
            .digest_session(namespace_id, "session-3", &repo, false)
            .await
            .unwrap();

        assert_eq!(first.short_id, second.short_id);
        assert_eq!(first.long_id, second.long_id);
    }

    #[tokio::test]
    async fn test_digest_session_force_regenerates() {
        let (pool, repo, namespace_id) = setup_repo().await;
        store_session_memory(
            &repo,
            namespace_id,
            "Added token counting to digest service.",
            "session-4",
            CognitiveLevel::Explicit,
        )
        .await;

        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(vec![
                Ok(good_digest_response()),
                Ok(good_digest_response()),
            ])),
            None,
        );

        let first = service
            .digest_session(namespace_id, "session-4", &repo, false)
            .await
            .unwrap();
        let forced = service
            .digest_session(namespace_id, "session-4", &repo, true)
            .await
            .unwrap();

        // Force creates new memories with different IDs
        assert_ne!(first.short_id, forced.short_id);
        assert_ne!(first.long_id, forced.long_id);

        // Force regeneration replaces the existing digest pointers rather than
        // summarizing prior digests recursively.
        let total_digests: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM session_digests WHERE session_key = ?")
                .bind("session-4")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(total_digests, 2);

        let latest_short = repo
            .latest_digest_for_session(namespace_id, "session-4", DIGEST_KIND_SHORT)
            .await
            .unwrap()
            .unwrap();
        let latest_long = repo
            .latest_digest_for_session(namespace_id, "session-4", DIGEST_KIND_LONG)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest_short.id, forced.short_id);
        assert_eq!(latest_long.id, forced.long_id);
    }

    #[tokio::test]
    async fn test_digest_session_ignores_raw_memories() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        store_session_memory(
            &repo,
            namespace_id,
            "Raw noise from hook capture",
            "session-5",
            CognitiveLevel::Raw,
        )
        .await;

        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(Vec::new())),
            None,
        );

        let result = service
            .digest_session(namespace_id, "session-5", &repo, false)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_digest_session_ignores_other_sessions() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        store_session_memory(
            &repo,
            namespace_id,
            "Memory from a different session entirely.",
            "other-session",
            CognitiveLevel::Explicit,
        )
        .await;

        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(Vec::new())),
            None,
        );

        let result = service
            .digest_session(namespace_id, "session-6", &repo, false)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_digest_session_creates_evidence_lineage() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        let m1 = store_session_memory(
            &repo,
            namespace_id,
            "Source memory one.",
            "session-7",
            CognitiveLevel::Explicit,
        )
        .await;
        let m2 = store_session_memory(
            &repo,
            namespace_id,
            "Source memory two.",
            "session-7",
            CognitiveLevel::Explicit,
        )
        .await;

        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(vec![Ok(good_digest_response())])),
            None,
        );

        let result = service
            .digest_session(namespace_id, "session-7", &repo, false)
            .await
            .unwrap();

        let short_lineage = repo.load_lineage(result.short_id).await.unwrap();
        assert_eq!(short_lineage.len(), 2);
        let short_sources: Vec<i64> = short_lineage.iter().map(|e| e.source_memory_id).collect();
        assert!(short_sources.contains(&m1.id));
        assert!(short_sources.contains(&m2.id));
        assert_eq!(short_lineage[0].evidence_role, DIGESTED_FROM_ROLE);

        let long_lineage = repo.load_lineage(result.long_id).await.unwrap();
        assert_eq!(long_lineage.len(), 2);
    }

    #[test]
    fn test_truncate_within_limit() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_at_boundary() {
        let input = "a".repeat(50);
        assert_eq!(truncate(&input, 50), input);
        assert_eq!(truncate(&input, 30).len(), 30);
    }

    #[test]
    fn test_truncate_splits_at_word() {
        let input = "the quick brown fox jumps over the lazy dog";
        let result = truncate(input, 20);
        assert!(result.len() <= 20);
        assert!(!result.ends_with(' ') || result.is_empty());
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello world"), 2);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("  spaced  out  "), 2);
    }

    fn test_memory(id: i64, content: &str) -> Memory {
        Memory {
            id,
            namespace_id: 1,
            content: content.to_string(),
            category: MemoryCategory::Session,
            memory_lane_type: None,
            labels: vec![],
            metadata: serde_json::json!({}),
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

    #[tokio::test]
    async fn test_digest_memories_get_embeddings_when_service_provided() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        store_session_memory(
            &repo,
            namespace_id,
            "Fixed query pagination behavior.",
            "session-embed",
            CognitiveLevel::Explicit,
        )
        .await;
        store_session_memory(
            &repo,
            namespace_id,
            "Added working-set retrieval primitives.",
            "session-embed",
            CognitiveLevel::Explicit,
        )
        .await;

        let mock_embed = nexus_embeddings::MockEmbeddingService::new();
        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(vec![Ok(good_digest_response())])),
            Some(Arc::new(mock_embed)),
        );

        let result = service
            .digest_session(namespace_id, "session-embed", &repo, false)
            .await
            .unwrap();

        let short = repo.get_by_id(result.short_id).await.unwrap().unwrap();
        assert!(
            short.content_embedding.is_some(),
            "short digest should have an embedding when service is provided"
        );
        assert_eq!(
            short.content_embedding.as_ref().unwrap().len(),
            384,
            "short digest embedding dimension should be 384"
        );

        let long = repo.get_by_id(result.long_id).await.unwrap().unwrap();
        assert!(
            long.content_embedding.is_some(),
            "long digest should have an embedding when service is provided"
        );
        assert_eq!(
            long.content_embedding.as_ref().unwrap().len(),
            384,
            "long digest embedding dimension should be 384"
        );
    }

    #[tokio::test]
    async fn test_digest_memories_stored_without_embedding_when_service_absent() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        store_session_memory(
            &repo,
            namespace_id,
            "Fixed query pagination behavior.",
            "session-no-embed",
            CognitiveLevel::Explicit,
        )
        .await;
        store_session_memory(
            &repo,
            namespace_id,
            "Added working-set retrieval primitives.",
            "session-no-embed",
            CognitiveLevel::Explicit,
        )
        .await;

        let service = DigestService::new(
            AgentConfig::default(),
            Arc::new(MockLlmClient::new(vec![Ok(good_digest_response())])),
            None,
        );

        let result = service
            .digest_session(namespace_id, "session-no-embed", &repo, false)
            .await
            .unwrap();

        let short = repo.get_by_id(result.short_id).await.unwrap().unwrap();
        assert!(
            short.content_embedding.is_none(),
            "short digest should NOT have embedding when no service provided"
        );

        let long = repo.get_by_id(result.long_id).await.unwrap().unwrap();
        assert!(
            long.content_embedding.is_none(),
            "long digest should NOT have embedding when no service provided"
        );
    }

    #[test]
    fn test_fallback_short_uses_first_five_memories() {
        let memories = vec![
            test_memory(1, "First memory content here."),
            test_memory(2, "Second memory."),
        ];
        let result = fallback_short(&memories);
        assert!(result.contains("First memory"));
        assert!(result.contains("Second memory"));
    }

    #[test]
    fn test_fallback_long_concatenates_all_memories() {
        let memories = vec![
            test_memory(1, "Alpha."),
            test_memory(2, "Beta."),
            test_memory(3, "Gamma."),
        ];
        let result = fallback_long(&memories);
        assert!(result.contains("Alpha"));
        assert!(result.contains("Beta"));
        assert!(result.contains("Gamma"));
    }
}
