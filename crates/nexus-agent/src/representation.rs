//! Representation service - assembles bounded working-memory context.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use nexus_core::{
    config::Config, traits::EmbeddingService, CognitiveLevel, Memory, WorkingRepresentation,
    WorkingRepresentationRequest,
};
use nexus_storage::repository::{MemoryRepository, SemanticCandidateParams};
use nexus_vectors::{SearchOptions, SemanticSearch, VectorEntry};
use tokio::sync::OnceCell;
use tracing::{debug, warn};

use crate::error::AgentError;
use crate::ranking::flatten_ranked_representation;
use crate::util::{flush_metric_samples, stage_metric_sample, CognitionSnapshot};

const SEMANTIC_VECTOR_OVERFETCH_MULTIPLIER: usize = 8;
const SEMANTIC_VECTOR_THRESHOLD: f32 = 0.58;

enum EmbedderProvider {
    Disabled,
    Static(Arc<dyn EmbeddingService>),
    Auto(Arc<OnceCell<Option<Arc<dyn EmbeddingService>>>>),
}

pub struct RepresentationService {
    embedder: EmbedderProvider,
}

impl EmbedderProvider {
    async fn resolve(&self) -> Option<Arc<dyn EmbeddingService>> {
        match self {
            Self::Disabled => None,
            Self::Static(embedder) => Some(embedder.clone()),
            Self::Auto(cell) => {
                let embedder = cell
                    .get_or_init(|| async {
                        let config = match Config::from_env() {
                            Ok(config) => config,
                            Err(error) => {
                                warn!(
                                    error = %error,
                                    "Failed to load Nexus config for semantic embeddings; semantic retrieval will fall back to text"
                                );
                                return None;
                            }
                        };
                        match nexus_embeddings::create_service(&config).await {
                            Ok(Some(service)) => Some(service),
                            Ok(None) => None,
                            Err(error) => {
                                warn!(
                                    error = %error,
                                    "Failed to initialize embedding service; semantic retrieval will fall back to text. Configure a remote embedding provider, local OpenAI-compatible runtime, or set NEXUS_EMBEDDINGS_ENABLED=false"
                                );
                                None
                            }
                        }
                    })
                    .await;
                embedder.clone()
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BucketLimits {
    digests: i64,
    contradictions: i64,
    derived: i64,
    semantic: i64,
    recent: i64,
}

fn bucket_limits(max_items: usize) -> BucketLimits {
    let max_items = max_items.max(1);
    let mut digests = max_items.min(2);
    let mut contradictions = max_items.saturating_div(10).min(2);
    let remaining_after_fixed = max_items.saturating_sub(digests + contradictions);

    let mut derived_target = if max_items >= 16 {
        remaining_after_fixed.min(max_items.saturating_div(4).max(4))
    } else {
        remaining_after_fixed.min(max_items.saturating_div(4).max(1))
    };
    let remaining_after_derived = remaining_after_fixed.saturating_sub(derived_target);

    let mut semantic_target = if max_items >= 16 {
        remaining_after_derived.min(((max_items * 3) / 10).max(4))
    } else {
        remaining_after_derived.min(max_items.saturating_div(3).max(1))
    };
    let mut recent =
        max_items.saturating_sub(digests + contradictions + derived_target + semantic_target);

    if recent == 0 && max_items > 0 {
        if semantic_target > 0 {
            semantic_target = semantic_target.saturating_sub(1);
        } else if derived_target > 0 {
            derived_target = derived_target.saturating_sub(1);
        } else if digests > 0 {
            digests = digests.saturating_sub(1);
        } else if contradictions > 0 {
            contradictions = contradictions.saturating_sub(1);
        }
        recent = 1;
    }

    BucketLimits {
        digests: digests as i64,
        contradictions: contradictions as i64,
        derived: derived_target as i64,
        semantic: semantic_target as i64,
        recent: recent as i64,
    }
}

fn overfetch_limit(bucket_limit: i64, max_items: usize) -> i64 {
    let bucket_limit = bucket_limit.max(1) as usize;
    max_items.max(bucket_limit * 3) as i64
}

fn include_recent_memory(memory: &Memory, include_raw: bool) -> bool {
    let snapshot = CognitionSnapshot::from_memory(memory);
    match snapshot.level {
        CognitiveLevel::Raw => include_raw,
        CognitiveLevel::Explicit => snapshot.confidence_meets_threshold(),
        CognitiveLevel::Derived
        | CognitiveLevel::Contradiction
        | CognitiveLevel::SummaryShort
        | CognitiveLevel::SummaryLong => false,
    }
}

fn include_semantic_memory(memory: &Memory, include_raw: bool) -> bool {
    let snapshot = CognitionSnapshot::from_memory(memory);
    match snapshot.level {
        CognitiveLevel::Raw => include_raw,
        CognitiveLevel::SummaryShort | CognitiveLevel::SummaryLong => false,
        CognitiveLevel::Explicit | CognitiveLevel::Derived | CognitiveLevel::Contradiction => {
            snapshot.confidence_meets_threshold()
        }
    }
}

fn include_derived_memory(memory: &Memory) -> bool {
    let snapshot = CognitionSnapshot::from_memory(memory);
    snapshot.level == CognitiveLevel::Derived && snapshot.confidence_meets_threshold()
}

fn include_contradiction_memory(memory: &Memory) -> bool {
    let snapshot = CognitionSnapshot::from_memory(memory);
    snapshot.level == CognitiveLevel::Contradiction && snapshot.confidence_meets_threshold()
}

impl RepresentationService {
    pub fn new() -> Self {
        let auto_embedder = Config::from_env()
            .map(|config| config.embedding.enabled)
            .unwrap_or(false);

        let embedder = if auto_embedder {
            EmbedderProvider::Auto(Arc::new(OnceCell::new()))
        } else {
            EmbedderProvider::Disabled
        };

        Self { embedder }
    }

    pub fn without_embedder() -> Self {
        Self {
            embedder: EmbedderProvider::Disabled,
        }
    }

    pub fn with_embedder(embedder: Arc<dyn EmbeddingService>) -> Self {
        Self {
            embedder: EmbedderProvider::Static(embedder),
        }
    }

    async fn build_semantic_bucket(
        &self,
        request: &WorkingRepresentationRequest,
        repo: &MemoryRepository,
        limit: i64,
    ) -> Result<Vec<Memory>, AgentError> {
        let Some(query) = request.query.as_deref() else {
            return Ok(Vec::new());
        };

        let semantic_limit = limit.max(1) as usize;
        let vector_fetch_limit = (overfetch_limit(limit, request.max_items) as usize
            * SEMANTIC_VECTOR_OVERFETCH_MULTIPLIER) as i64;

        let mut ranked = Vec::new();
        let mut seen = HashSet::new();

        if let Some(embedder) = self.embedder.resolve().await {
            match embedder.embed(query).await {
                Ok(query_embedding) => {
                    let candidates = repo
                        .get_semantic_candidates(SemanticCandidateParams {
                            namespace_id: request.namespace_id,
                            perspective: request.perspective.as_ref(),
                            limit: vector_fetch_limit,
                            include_raw: request.include_raw,
                        })
                        .await
                        .map_err(storage_err)?;

                    let mut by_id = HashMap::new();
                    let vectors: Vec<VectorEntry> = candidates
                        .into_iter()
                        .filter(|memory| include_semantic_memory(memory, request.include_raw))
                        .filter_map(|memory| {
                            let embedding = memory.content_embedding.clone()?;
                            by_id.insert(memory.id, memory.clone());
                            Some(
                                VectorEntry::new(
                                    memory.id,
                                    embedding,
                                    memory.category.to_string(),
                                    memory.namespace_id,
                                )
                                .with_memory_lane_type(
                                    memory
                                        .memory_lane_type
                                        .as_ref()
                                        .map(|lane| lane.to_string())
                                        .unwrap_or_else(|| "none".to_string()),
                                ),
                            )
                        })
                        .collect();

                    if !vectors.is_empty() {
                        let search = SemanticSearch::new();
                        let options = SearchOptions::with_limit(semantic_limit)
                            .with_namespace(request.namespace_id)
                            .with_threshold(SEMANTIC_VECTOR_THRESHOLD);

                        match search.search(&query_embedding, &vectors, &options) {
                            Ok((results, _latency)) => {
                                for result in results {
                                    if let Some(memory) = by_id.remove(&result.id) {
                                        seen.insert(memory.id);
                                        ranked.push(memory);
                                    }
                                }
                            }
                            Err(error) => {
                                warn!(error = %error, "Vector semantic search failed; falling back to text search");
                            }
                        }
                    }
                }
                Err(error) => {
                    warn!(error = %error, "Query embedding failed; falling back to text search");
                }
            }
        }

        if ranked.len() < semantic_limit {
            let top_up_limit = overfetch_limit(limit, request.max_items) as i32;
            let fallback = repo
                .search_by_text_memories(
                    request.namespace_id,
                    query,
                    top_up_limit,
                    request.include_raw,
                )
                .await
                .map_err(storage_err)?;

            for memory in fallback {
                if include_semantic_memory(&memory, request.include_raw) && seen.insert(memory.id) {
                    ranked.push(memory);
                    if ranked.len() >= semantic_limit {
                        break;
                    }
                }
            }
        }

        ranked.truncate(semantic_limit);
        Ok(ranked)
    }

    pub async fn build(
        &self,
        request: &WorkingRepresentationRequest,
        repo: &MemoryRepository,
    ) -> Result<WorkingRepresentation, AgentError> {
        let total_started = Instant::now();
        let limits = bucket_limits(request.max_items);
        let mut representation = WorkingRepresentation::default();
        let mut metrics = Vec::new();

        if request.include_digests && limits.digests > 0 {
            let started = Instant::now();
            if let Some(perspective) = request.perspective.as_ref() {
                if let Some(session_key) = perspective.session_key.as_deref() {
                    if let Some(short) = repo
                        .latest_digest_for_session(request.namespace_id, session_key, "short")
                        .await
                        .map_err(storage_err)?
                    {
                        representation.digests.push(short);
                    }
                    if (representation.digests.len() as i64) < limits.digests {
                        if let Some(long) = repo
                            .latest_digest_for_session(request.namespace_id, session_key, "long")
                            .await
                            .map_err(storage_err)?
                        {
                            representation.digests.push(long);
                        }
                    }
                }
            }
            metrics.push(stage_metric_sample(
                request.namespace_id,
                "cognition.representation.digests_ms",
                started.elapsed().as_secs_f64() * 1000.0,
                "digests",
            ));
        }

        // Cross-namespace digests: fetch bounded digests from related namespaces
        // (e.g. alias namespaces for the same canonical agent).
        if request.include_digests
            && !request.cross_namespace_ids.is_empty()
            && (representation.digests.len() as i64) < limits.digests
        {
            let started = Instant::now();
            for &cross_ns_id in &request.cross_namespace_ids {
                if cross_ns_id == request.namespace_id {
                    continue;
                }
                if representation.digests.len() as i64 >= limits.digests {
                    break;
                }
                let cross_digest = if let Some(session_key) = request
                    .perspective
                    .as_ref()
                    .and_then(|p| p.session_key.as_deref())
                {
                    repo.latest_digest_for_session(cross_ns_id, session_key, "short")
                        .await
                        .ok()
                        .flatten()
                } else {
                    repo.latest_digest_for_namespace(cross_ns_id, "short")
                        .await
                        .ok()
                        .flatten()
                };

                if let Some(digest) = cross_digest {
                    representation.digests.push(digest);
                }
                // Only take one cross-namespace digest per namespace to stay bounded.
            }
            debug!(
                cross_namespaces = request.cross_namespace_ids.len(),
                cross_digests_added = representation.digests.len(),
                "Cross-namespace digest fetch complete"
            );
            metrics.push(stage_metric_sample(
                request.namespace_id,
                "cognition.representation.cross_namespace_ms",
                started.elapsed().as_secs_f64() * 1000.0,
                "cross_namespace",
            ));
        }

        if request.include_recent {
            let started = Instant::now();
            let fetch_limit = overfetch_limit(limits.recent, request.max_items);
            representation.recent = if let Some(perspective) = request.perspective.as_ref() {
                repo.get_recent_by_perspective_opts(
                    request.namespace_id,
                    perspective,
                    fetch_limit,
                    request.include_raw,
                )
                .await
                .map_err(storage_err)?
            } else {
                repo.list_filtered(
                    request.namespace_id,
                    nexus_storage::repository::ListMemoryFilters {
                        category: None,
                        since: None,
                        until: None,
                        content_like: None,
                        include_raw: request.include_raw,
                        limit: fetch_limit,
                        offset: 0,
                    },
                )
                .await
                .map_err(storage_err)?
            };
            representation
                .recent
                .retain(|memory| include_recent_memory(memory, request.include_raw));
            representation
                .recent
                .truncate(limits.recent.max(0) as usize);
            metrics.push(stage_metric_sample(
                request.namespace_id,
                "cognition.representation.recent_ms",
                started.elapsed().as_secs_f64() * 1000.0,
                "recent",
            ));
        }

        if request.include_semantic && limits.semantic > 0 {
            let started = Instant::now();
            representation.semantic = self
                .build_semantic_bucket(request, repo, limits.semantic)
                .await?;
            metrics.push(stage_metric_sample(
                request.namespace_id,
                "cognition.representation.semantic_ms",
                started.elapsed().as_secs_f64() * 1000.0,
                "semantic",
            ));
        }

        if let Some(perspective) = request.perspective.as_ref() {
            if request.include_derived && limits.derived > 0 {
                let started = Instant::now();
                let fetch_limit = overfetch_limit(limits.derived, request.max_items);
                representation.derived = repo
                    .get_most_reinforced_by_perspective_opts(
                        request.namespace_id,
                        perspective,
                        fetch_limit,
                        request.include_raw,
                    )
                    .await
                    .map_err(storage_err)?
                    .into_iter()
                    .filter(include_derived_memory)
                    .collect();
                representation.derived.truncate(limits.derived as usize);
                metrics.push(stage_metric_sample(
                    request.namespace_id,
                    "cognition.representation.derived_ms",
                    started.elapsed().as_secs_f64() * 1000.0,
                    "derived",
                ));
            }

            if request.include_contradictions && limits.contradictions > 0 {
                let started = Instant::now();
                let fetch_limit = overfetch_limit(limits.contradictions, request.max_items);
                representation.contradictions = repo
                    .get_contradictions_by_perspective_opts(
                        request.namespace_id,
                        perspective,
                        fetch_limit,
                        request.include_raw,
                    )
                    .await
                    .map_err(storage_err)?
                    .into_iter()
                    .filter(include_contradiction_memory)
                    .collect();
                representation
                    .contradictions
                    .truncate(limits.contradictions as usize);
                metrics.push(stage_metric_sample(
                    request.namespace_id,
                    "cognition.representation.contradictions_ms",
                    started.elapsed().as_secs_f64() * 1000.0,
                    "contradictions",
                ));
            }
        } else {
            if request.include_derived && limits.derived > 0 {
                let started = Instant::now();
                let fetch_limit = overfetch_limit(limits.derived, request.max_items);
                representation.derived = repo
                    .get_most_reinforced_by_namespace(
                        request.namespace_id,
                        fetch_limit,
                        request.include_raw,
                    )
                    .await
                    .map_err(storage_err)?
                    .into_iter()
                    .filter(include_derived_memory)
                    .collect();
                representation.derived.truncate(limits.derived as usize);
                metrics.push(stage_metric_sample(
                    request.namespace_id,
                    "cognition.representation.derived_ms",
                    started.elapsed().as_secs_f64() * 1000.0,
                    "derived",
                ));
            }

            if request.include_contradictions && limits.contradictions > 0 {
                let started = Instant::now();
                let fetch_limit = overfetch_limit(limits.contradictions, request.max_items);
                representation.contradictions = repo
                    .get_contradictions_by_namespace(
                        request.namespace_id,
                        fetch_limit,
                        request.include_raw,
                    )
                    .await
                    .map_err(storage_err)?
                    .into_iter()
                    .filter(include_contradiction_memory)
                    .collect();
                representation
                    .contradictions
                    .truncate(limits.contradictions as usize);
                metrics.push(stage_metric_sample(
                    request.namespace_id,
                    "cognition.representation.contradictions_ms",
                    started.elapsed().as_secs_f64() * 1000.0,
                    "contradictions",
                ));
            }
        }

        metrics.push(stage_metric_sample(
            request.namespace_id,
            "cognition.representation.total_ms",
            total_started.elapsed().as_secs_f64() * 1000.0,
            "total",
        ));
        flush_metric_samples(repo, &metrics).await;

        Ok(representation)
    }

    pub async fn flat_working_set(
        &self,
        request: &WorkingRepresentationRequest,
        repo: &MemoryRepository,
    ) -> Result<Vec<Memory>, AgentError> {
        let representation = self.build(request, repo).await?;
        let flat: Vec<Memory> = flatten_ranked_representation(representation, request)
            .into_iter()
            .map(|bucketed| bucketed.memory)
            .collect();

        // Fall back to storage primitive when the representation yields nothing.
        // This covers cases where include_* flags are all false or perspective
        // queries return empty but data exists in the namespace.
        if flat.is_empty()
            && !request.include_raw
            && !request.include_digests
            && !request.include_recent
            && !request.include_semantic
            && !request.include_derived
            && !request.include_contradictions
        {
            let limit = request.max_items.max(1) as i64;
            return repo
                .list_filtered(
                    request.namespace_id,
                    nexus_storage::repository::ListMemoryFilters {
                        category: None,
                        since: None,
                        until: None,
                        content_like: None,
                        include_raw: request.include_raw,
                        limit,
                        offset: 0,
                    },
                )
                .await
                .map_err(|e| AgentError::Storage(e.to_string()));
        }

        Ok(flat)
    }
}

fn storage_err(error: nexus_core::NexusError) -> AgentError {
    AgentError::Storage(error.to_string())
}

impl Default for RepresentationService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use nexus_core::traits::EmbeddingService;
    use nexus_core::{
        cognitive_level_from_metadata, CognitiveMetadata, MemoryCategory, PerspectiveKey,
    };
    use nexus_embeddings::MockEmbeddingService;
    use nexus_storage::repository::{
        MemoryRepository, NamespaceRepository, StoreDigestParams, StoreMemoryParams,
    };
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_repo() -> (MemoryRepository, i64, PerspectiveKey) {
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
            .get_or_create("representation-test", "representation-test")
            .await
            .unwrap();
        let perspective =
            PerspectiveKey::new("claude-code", "claude-code", Some("session-1".to_string()));
        (MemoryRepository::new(pool), namespace.id, perspective)
    }

    fn metadata(level: CognitiveLevel, perspective: &PerspectiveKey) -> serde_json::Value {
        let mut cognitive = CognitiveMetadata::new(
            level,
            perspective.observer.clone(),
            perspective.subject.clone(),
            perspective.session_key.clone(),
            "test",
        );
        cognitive.confidence = Some(0.9);
        cognitive.merge_into(&serde_json::json!({}))
    }

    async fn store_memory(
        repo: &MemoryRepository,
        namespace_id: i64,
        content: &str,
        level: CognitiveLevel,
        perspective: &PerspectiveKey,
    ) -> Memory {
        repo.store(StoreMemoryParams {
            namespace_id,
            content,
            category: &MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &metadata(level, perspective),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap()
    }

    async fn store_memory_with_embedding(
        repo: &MemoryRepository,
        namespace_id: i64,
        content: &str,
        level: CognitiveLevel,
        perspective: &PerspectiveKey,
        embedding: &[f32],
    ) -> Memory {
        repo.store(StoreMemoryParams {
            namespace_id,
            content,
            category: &MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &metadata(level, perspective),
            embedding: Some(embedding),
            embedding_model: Some("mock-embedding"),
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_build_representation_groups_buckets() {
        let (repo, namespace_id, perspective) = setup_repo().await;
        let digest = store_memory(
            &repo,
            namespace_id,
            "short digest",
            CognitiveLevel::SummaryShort,
            &perspective,
        )
        .await;
        repo.store_digest(StoreDigestParams {
            namespace_id,
            session_key: "session-1",
            digest_kind: "short",
            memory_id: digest.id,
            start_memory_id: Some(1),
            end_memory_id: Some(2),
            token_count: 32,
        })
        .await
        .unwrap();

        store_memory(
            &repo,
            namespace_id,
            "recent explicit observation",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "derived insight",
            CognitiveLevel::Derived,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "contradiction note",
            CognitiveLevel::Contradiction,
            &perspective,
        )
        .await;

        let service = RepresentationService::new();
        let representation = service
            .build(
                &WorkingRepresentationRequest {
                    namespace_id,
                    perspective: Some(perspective),
                    query: Some("recent".to_string()),
                    max_items: 12,
                    include_raw: false,
                    include_recent: true,
                    include_semantic: true,
                    include_derived: true,
                    include_digests: true,
                    include_contradictions: true,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
            )
            .await
            .unwrap();

        assert_eq!(representation.digests.len(), 1);
        assert_eq!(representation.derived.len(), 1);
        assert_eq!(representation.contradictions.len(), 1);
        assert!(!representation.recent.is_empty());
        assert!(!representation.semantic.is_empty());
    }

    #[tokio::test]
    async fn test_flat_working_set_uses_storage_primitive() {
        let (repo, namespace_id, perspective) = setup_repo().await;
        store_memory(
            &repo,
            namespace_id,
            "derived insight",
            CognitiveLevel::Derived,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "recent explicit observation",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;

        let service = RepresentationService::new();
        let flat = service
            .flat_working_set(
                &WorkingRepresentationRequest {
                    namespace_id,
                    perspective: Some(perspective),
                    max_items: 4,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
            )
            .await
            .unwrap();

        assert!(!flat.is_empty());
        assert!(flat.len() <= 4);
    }

    #[tokio::test]
    async fn test_build_representation_without_perspective_excludes_raw_noise() {
        let (repo, namespace_id, perspective) = setup_repo().await;
        store_memory(
            &repo,
            namespace_id,
            "recent explicit observation",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;

        repo.store(StoreMemoryParams {
            namespace_id,
            content: "raw hook payload",
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": true,
                "cognitive": {
                    "level": "raw",
                    "observer": "claude-code",
                    "subject": "claude-code",
                    "session_key": "session-1",
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let representation = RepresentationService::new()
            .build(
                &WorkingRepresentationRequest {
                    namespace_id,
                    perspective: None,
                    query: None,
                    max_items: 10,
                    include_raw: false,
                    include_recent: true,
                    include_semantic: false,
                    include_derived: false,
                    include_digests: false,
                    include_contradictions: false,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
            )
            .await
            .unwrap();

        assert_eq!(representation.recent.len(), 1);
        assert_eq!(
            representation.recent[0].content,
            "recent explicit observation"
        );
    }

    #[tokio::test]
    async fn test_build_representation_without_perspective_includes_cognition_outputs() {
        let (repo, namespace_id, perspective) = setup_repo().await;
        store_memory(
            &repo,
            namespace_id,
            "derived insight",
            CognitiveLevel::Derived,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "contradiction note",
            CognitiveLevel::Contradiction,
            &perspective,
        )
        .await;

        let representation = RepresentationService::new()
            .build(
                &WorkingRepresentationRequest {
                    namespace_id,
                    perspective: None,
                    query: None,
                    max_items: 10,
                    include_raw: false,
                    include_recent: false,
                    include_semantic: false,
                    include_derived: true,
                    include_digests: false,
                    include_contradictions: true,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
            )
            .await
            .unwrap();

        assert_eq!(representation.derived.len(), 1);
        assert_eq!(representation.derived[0].content, "derived insight");
        assert_eq!(representation.contradictions.len(), 1);
        assert_eq!(
            representation.contradictions[0].content,
            "contradiction note"
        );
    }

    #[tokio::test]
    async fn test_build_representation_without_perspective_can_include_raw_noise() {
        let (repo, namespace_id, perspective) = setup_repo().await;
        store_memory(
            &repo,
            namespace_id,
            "recent explicit observation",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;

        repo.store(StoreMemoryParams {
            namespace_id,
            content: "raw hook payload",
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": true,
                "cognitive": {
                    "level": "raw",
                    "observer": "claude-code",
                    "subject": "claude-code",
                    "session_key": "session-1",
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let representation = RepresentationService::new()
            .build(
                &WorkingRepresentationRequest {
                    namespace_id,
                    perspective: None,
                    query: None,
                    max_items: 10,
                    include_raw: true,
                    include_recent: true,
                    include_semantic: false,
                    include_derived: false,
                    include_digests: false,
                    include_contradictions: false,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
            )
            .await
            .unwrap();

        assert_eq!(representation.recent.len(), 2);
        assert!(representation
            .recent
            .iter()
            .any(|memory| memory.content == "raw hook payload"));
    }

    #[test]
    fn test_bucket_limits_match_locked_default_allocation() {
        let limits = bucket_limits(24);
        assert_eq!(limits.digests, 2);
        assert_eq!(limits.contradictions, 2);
        assert_eq!(limits.derived, 6);
        assert_eq!(limits.semantic, 7);
        assert_eq!(limits.recent, 7);
    }

    #[test]
    fn test_bucket_limits_preserve_recent_slot_for_tiny_requests() {
        let four = bucket_limits(4);
        assert!(four.recent >= 1);

        let single = bucket_limits(1);
        assert_eq!(single.recent, 1);
    }

    #[tokio::test]
    async fn test_build_representation_filters_below_confidence_thresholds() {
        let (repo, namespace_id, perspective) = setup_repo().await;

        let mut low_derived = metadata(CognitiveLevel::Derived, &perspective);
        low_derived["cognitive"]["confidence"] = serde_json::json!(0.60);
        repo.store(StoreMemoryParams {
            namespace_id,
            content: "low confidence derived",
            category: &MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &low_derived,
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let mut high_derived = metadata(CognitiveLevel::Derived, &perspective);
        high_derived["cognitive"]["confidence"] = serde_json::json!(0.90);
        repo.store(StoreMemoryParams {
            namespace_id,
            content: "high confidence derived",
            category: &MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &high_derived,
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let representation = RepresentationService::new()
            .build(
                &WorkingRepresentationRequest {
                    namespace_id,
                    perspective: Some(perspective),
                    query: None,
                    max_items: 10,
                    include_raw: false,
                    include_recent: false,
                    include_semantic: false,
                    include_derived: true,
                    include_digests: false,
                    include_contradictions: false,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
            )
            .await
            .unwrap();

        assert_eq!(representation.derived.len(), 1);
        assert_eq!(representation.derived[0].content, "high confidence derived");
    }

    #[tokio::test]
    async fn test_build_representation_prefers_vector_semantic_matches() {
        let (repo, namespace_id, perspective) = setup_repo().await;
        let embedder = Arc::new(MockEmbeddingService::new());
        let semantic_query = "provider switch rollout";
        let vector = embedder.embed(semantic_query).await.unwrap();

        store_memory_with_embedding(
            &repo,
            namespace_id,
            "migration timeline digest summary",
            CognitiveLevel::Derived,
            &perspective,
            &vector,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "provider switch rollout plain text fallback",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;

        let representation = RepresentationService::with_embedder(embedder)
            .build(
                &WorkingRepresentationRequest {
                    namespace_id,
                    perspective: Some(perspective),
                    query: Some(semantic_query.to_string()),
                    max_items: 12,
                    include_raw: false,
                    include_recent: false,
                    include_semantic: true,
                    include_derived: false,
                    include_digests: false,
                    include_contradictions: false,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
            )
            .await
            .unwrap();

        assert_eq!(representation.semantic.len(), 2);
        assert_eq!(
            representation.semantic[0].content,
            "migration timeline digest summary"
        );
        assert_eq!(
            representation.semantic[1].content,
            "provider switch rollout plain text fallback"
        );
    }

    #[tokio::test]
    async fn test_build_representation_text_fallback_overfetches_after_filtering() {
        let (repo, namespace_id, perspective) = setup_repo().await;
        let embedder = Arc::new(MockEmbeddingService::new());
        let query = "provider switch rollout";
        let vector = embedder.embed(query).await.unwrap();

        store_memory_with_embedding(
            &repo,
            namespace_id,
            "vector-only semantic memory",
            CognitiveLevel::Derived,
            &perspective,
            &vector,
        )
        .await;

        let mut low_derived = metadata(CognitiveLevel::Derived, &perspective);
        low_derived["cognitive"]["confidence"] = serde_json::json!(0.60);
        repo.store(StoreMemoryParams {
            namespace_id,
            content: "provider switch rollout low confidence derived",
            category: &MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &low_derived,
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let mut low_explicit = metadata(CognitiveLevel::Explicit, &perspective);
        low_explicit["cognitive"]["confidence"] = serde_json::json!(0.60);
        repo.store(StoreMemoryParams {
            namespace_id,
            content: "provider switch rollout low confidence explicit",
            category: &MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &low_explicit,
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        store_memory(
            &repo,
            namespace_id,
            "provider switch rollout strong explicit",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;

        let text_hits = repo
            .search_by_text_memories(namespace_id, query, 4, false)
            .await
            .unwrap();
        assert_eq!(text_hits.len(), 3);

        let representation = RepresentationService::with_embedder(embedder)
            .build(
                &WorkingRepresentationRequest {
                    namespace_id,
                    perspective: Some(perspective),
                    query: Some(query.to_string()),
                    max_items: 6,
                    include_raw: false,
                    include_recent: false,
                    include_semantic: true,
                    include_derived: false,
                    include_digests: false,
                    include_contradictions: false,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
            )
            .await
            .unwrap();

        assert_eq!(representation.semantic.len(), 2);
        assert_eq!(
            representation.semantic[0].content,
            "vector-only semantic memory"
        );
        assert_eq!(
            representation.semantic[1].content,
            "provider switch rollout strong explicit"
        );
        assert_eq!(
            cognitive_level_from_metadata(&representation.semantic[1].metadata),
            CognitiveLevel::Explicit
        );
    }
}
