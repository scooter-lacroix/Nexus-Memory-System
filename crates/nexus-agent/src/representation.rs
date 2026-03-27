//! Representation service - assembles bounded working-memory context.

use nexus_core::{
    cognitive_level_from_metadata, CognitiveLevel, CognitiveMetadata, Memory,
    WorkingRepresentation, WorkingRepresentationRequest,
};
use nexus_storage::repository::MemoryRepository;

use crate::error::AgentError;
use crate::ranking::flatten_ranked_representation;

pub struct RepresentationService;

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

fn confidence_meets_threshold(memory: &Memory) -> bool {
    let Some(cognitive) = CognitiveMetadata::from_metadata(&memory.metadata) else {
        return true;
    };

    let confidence = cognitive.confidence.unwrap_or(1.0);
    match cognitive.level {
        CognitiveLevel::Explicit => confidence >= 0.70,
        CognitiveLevel::Derived => confidence >= 0.75,
        CognitiveLevel::Contradiction => confidence >= 0.80,
        CognitiveLevel::SummaryShort | CognitiveLevel::SummaryLong => true,
        CognitiveLevel::Raw => true,
    }
}

fn include_recent_memory(memory: &Memory, include_raw: bool) -> bool {
    match cognitive_level_from_metadata(&memory.metadata) {
        CognitiveLevel::Raw => include_raw,
        CognitiveLevel::Explicit => confidence_meets_threshold(memory),
        CognitiveLevel::Derived
        | CognitiveLevel::Contradiction
        | CognitiveLevel::SummaryShort
        | CognitiveLevel::SummaryLong => false,
    }
}

fn include_semantic_memory(memory: &Memory, include_raw: bool) -> bool {
    match cognitive_level_from_metadata(&memory.metadata) {
        CognitiveLevel::Raw => include_raw,
        CognitiveLevel::SummaryShort | CognitiveLevel::SummaryLong => false,
        CognitiveLevel::Explicit | CognitiveLevel::Derived | CognitiveLevel::Contradiction => {
            confidence_meets_threshold(memory)
        }
    }
}

fn include_derived_memory(memory: &Memory) -> bool {
    cognitive_level_from_metadata(&memory.metadata) == CognitiveLevel::Derived
        && confidence_meets_threshold(memory)
}

fn include_contradiction_memory(memory: &Memory) -> bool {
    cognitive_level_from_metadata(&memory.metadata) == CognitiveLevel::Contradiction
        && confidence_meets_threshold(memory)
}

impl RepresentationService {
    pub fn new() -> Self {
        Self
    }

    pub async fn build(
        &self,
        request: &WorkingRepresentationRequest,
        repo: &MemoryRepository,
    ) -> Result<WorkingRepresentation, AgentError> {
        let limits = bucket_limits(request.max_items);
        let mut representation = WorkingRepresentation::default();

        if request.include_digests && limits.digests > 0 {
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
        }

        if request.include_recent {
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
        }

        if request.include_semantic && limits.semantic > 0 {
            if let Some(query) = request.query.as_deref() {
                let fetch_limit = overfetch_limit(limits.semantic, request.max_items);
                representation.semantic = repo
                    .search_by_text(
                        request.namespace_id,
                        query,
                        fetch_limit as i32,
                        request.include_raw,
                    )
                    .await
                    .map_err(storage_err)?
                    .into_iter()
                    .map(|row| Memory {
                        id: row.id,
                        namespace_id: row.namespace_id,
                        content: row.content,
                        category: nexus_core::MemoryCategory::parse(&row.category)
                            .unwrap_or(nexus_core::MemoryCategory::General),
                        memory_lane_type: row
                            .memory_lane_type
                            .as_deref()
                            .and_then(nexus_core::MemoryLaneType::parse),
                        labels: serde_json::from_str(&row.labels).unwrap_or_default(),
                        metadata: serde_json::from_str(&row.metadata)
                            .unwrap_or(serde_json::Value::Null),
                        similarity_score: row.similarity_score,
                        relevance_score: row.relevance_score,
                        content_embedding: row
                            .content_embedding
                            .and_then(|embedding| serde_json::from_str(&embedding).ok()),
                        embedding_model: row.embedding_model,
                        created_at: row.created_at,
                        updated_at: row.updated_at,
                        last_accessed: row.last_accessed,
                        is_active: row.is_active,
                        is_archived: row.is_archived,
                        access_count: row.access_count,
                    })
                    .filter(|memory| include_semantic_memory(memory, request.include_raw))
                    .collect();
                representation.semantic.truncate(limits.semantic as usize);
            }
        }

        if let Some(perspective) = request.perspective.as_ref() {
            if request.include_derived && limits.derived > 0 {
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
            }

            if request.include_contradictions && limits.contradictions > 0 {
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
            }
        } else {
            if request.include_derived && limits.derived > 0 {
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
            }

            if request.include_contradictions && limits.contradictions > 0 {
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
            }
        }

        Ok(representation)
    }

    pub async fn flat_working_set(
        &self,
        request: &WorkingRepresentationRequest,
        repo: &MemoryRepository,
    ) -> Result<Vec<Memory>, AgentError> {
        let representation = self.build(request, repo).await?;
        Ok(flatten_ranked_representation(representation, request)
            .into_iter()
            .map(|bucketed| bucketed.memory)
            .collect())
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

    use nexus_core::{CognitiveMetadata, MemoryCategory, PerspectiveKey};
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
                },
                &repo,
            )
            .await
            .unwrap();

        assert_eq!(representation.derived.len(), 1);
        assert_eq!(representation.derived[0].content, "high confidence derived");
    }
}
