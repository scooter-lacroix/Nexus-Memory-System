//! Shared blended ranking for working representations and recall output.

use std::cmp::Ordering;
use std::collections::HashMap;

use chrono::Utc;
use nexus_core::{
    perspective_from_metadata, CognitiveLevel, CognitiveMetadata, Memory, PerspectiveKey,
    WorkingRepresentation, WorkingRepresentationRequest,
};

use crate::types::MemoryBucket;

#[derive(Debug, Clone)]
pub(crate) struct BucketedMemory {
    pub(crate) memory: Memory,
    pub(crate) bucket: MemoryBucket,
    pub(crate) blended_score: f32,
}

pub(crate) fn flatten_ranked_representation(
    representation: WorkingRepresentation,
    request: &WorkingRepresentationRequest,
) -> Vec<BucketedMemory> {
    let mut best_by_id: HashMap<i64, BucketedMemory> = HashMap::new();

    for (bucket, memories) in [
        (MemoryBucket::Digests, representation.digests),
        (MemoryBucket::Derived, representation.derived),
        (MemoryBucket::Semantic, representation.semantic),
        (MemoryBucket::Recent, representation.recent),
        (MemoryBucket::Contradictions, representation.contradictions),
    ] {
        for memory in memories {
            let candidate = BucketedMemory {
                blended_score: blended_score(&memory, bucket, request),
                memory,
                bucket,
            };

            match best_by_id.get_mut(&candidate.memory.id) {
                Some(existing)
                    if compare_bucketed_memory(&candidate, existing) == Ordering::Greater =>
                {
                    *existing = candidate;
                }
                None => {
                    best_by_id.insert(candidate.memory.id, candidate);
                }
                _ => {}
            }
        }
    }

    let mut ranked: Vec<_> = best_by_id.into_values().collect();
    ranked.sort_by(|left, right| compare_bucketed_memory(left, right).reverse());
    ranked.truncate(request.max_items);
    ranked
}

fn compare_bucketed_memory(left: &BucketedMemory, right: &BucketedMemory) -> Ordering {
    left.blended_score
        .partial_cmp(&right.blended_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| left.memory.created_at.cmp(&right.memory.created_at))
        .then_with(|| bucket_priority(left.bucket).cmp(&bucket_priority(right.bucket)))
        .then_with(|| right.memory.id.cmp(&left.memory.id))
}

fn blended_score(
    memory: &Memory,
    bucket: MemoryBucket,
    request: &WorkingRepresentationRequest,
) -> f32 {
    if memory.is_archived {
        return -0.5;
    }

    if is_raw_activity(memory) {
        return if request.include_raw { -0.1 } else { -1.0 };
    }

    let cognitive = CognitiveMetadata::from_metadata(&memory.metadata);
    let confidence = cognitive
        .as_ref()
        .and_then(|value| value.confidence)
        .unwrap_or(0.75)
        .clamp(0.0, 1.0);
    let reinforcement_score = cognitive
        .as_ref()
        .map(|value| ((value.times_reinforced.max(0) as f32) / 5.0).min(1.0))
        .unwrap_or_default();
    let semantic_similarity = memory
        .relevance_score
        .or(memory.similarity_score)
        .unwrap_or_default()
        .clamp(0.0, 1.0);
    let recency_score = recency_weight(memory);
    let perspective_score = perspective_weight(memory, request.perspective.as_ref());
    let digest_or_derived_bonus = f32::from(matches!(
        cognitive.as_ref().map(|value| value.level),
        Some(CognitiveLevel::SummaryShort | CognitiveLevel::SummaryLong | CognitiveLevel::Derived)
    ));

    match bucket {
        MemoryBucket::Digests => {
            0.45 * confidence + 0.30 * recency_score + 0.25 * perspective_score + 0.05
        }
        MemoryBucket::Derived => {
            0.45 * confidence
                + 0.30 * reinforcement_score
                + 0.15 * recency_score
                + 0.10 * perspective_score
        }
        MemoryBucket::Semantic => {
            0.55 * semantic_similarity
                + 0.20 * reinforcement_score
                + 0.10 * recency_score
                + 0.10 * perspective_score
                + 0.05 * digest_or_derived_bonus
        }
        MemoryBucket::Recent => 0.50 * confidence + 0.30 * recency_score + 0.20 * perspective_score,
        MemoryBucket::Contradictions => {
            0.60 * confidence + 0.25 * recency_score + 0.15 * perspective_score
        }
    }
}

fn bucket_priority(bucket: MemoryBucket) -> u8 {
    match bucket {
        MemoryBucket::Digests => 5,
        MemoryBucket::Contradictions => 4,
        MemoryBucket::Derived => 3,
        MemoryBucket::Semantic => 2,
        MemoryBucket::Recent => 1,
    }
}

fn recency_weight(memory: &Memory) -> f32 {
    let age_hours = (Utc::now() - memory.created_at).num_hours();
    match age_hours {
        h if h <= 1 => 1.0,
        h if h <= 6 => 0.8,
        h if h <= 24 => 0.6,
        h if h <= 72 => 0.35,
        h if h <= 168 => 0.15,
        _ => 0.0,
    }
}

fn perspective_weight(memory: &Memory, request_perspective: Option<&PerspectiveKey>) -> f32 {
    let Some(request_perspective) = request_perspective else {
        return 0.5;
    };

    let Some(memory_perspective) = perspective_from_metadata(&memory.metadata) else {
        return 0.0;
    };

    match (
        memory_perspective.observer == request_perspective.observer,
        memory_perspective.subject == request_perspective.subject,
        memory_perspective.session_key.as_deref(),
        request_perspective.session_key.as_deref(),
    ) {
        (true, true, Some(left), Some(right)) if left == right => 1.0,
        (true, true, None, Some(_)) => 0.8,
        (true, true, Some(_), Some(_)) => 0.5,
        (true, true, _, None) => 0.8,
        _ => 0.0,
    }
}

fn is_raw_activity(memory: &Memory) -> bool {
    memory.labels.iter().any(|label| label == "raw-activity")
        || memory
            .metadata
            .get("raw_activity")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use nexus_core::{infer_perspective, PerspectiveSource};

    use super::*;

    fn memory(
        id: i64,
        content: &str,
        bucket: MemoryBucket,
        perspective: &PerspectiveKey,
        level: CognitiveLevel,
    ) -> BucketedMemory {
        let mut cognitive = CognitiveMetadata::new(
            level,
            perspective.observer.clone(),
            perspective.subject.clone(),
            perspective.session_key.clone(),
            "test",
        );
        cognitive.confidence = Some(0.8);

        BucketedMemory {
            memory: Memory {
                id,
                namespace_id: 1,
                content: content.to_string(),
                category: nexus_core::MemoryCategory::Facts,
                metadata: cognitive.merge_into(&serde_json::json!({})),
                created_at: Utc::now(),
                ..Memory::default()
            },
            bucket,
            blended_score: 0.0,
        }
    }

    #[test]
    fn test_flatten_ranked_representation_prefers_exact_perspective_match() {
        let request_perspective = infer_perspective(
            PerspectiveSource::Query,
            "claude-code",
            None,
            Some("session-a".to_string()),
        );
        let mismatched = infer_perspective(
            PerspectiveSource::Query,
            "codex",
            None,
            Some("session-b".to_string()),
        );

        let exact = memory(
            1,
            "exact perspective",
            MemoryBucket::Recent,
            &request_perspective,
            CognitiveLevel::Explicit,
        );
        let semantic_other = memory(
            2,
            "mismatched perspective",
            MemoryBucket::Semantic,
            &mismatched,
            CognitiveLevel::Explicit,
        );

        let ranked = flatten_ranked_representation(
            WorkingRepresentation {
                recent: vec![exact.memory],
                semantic: vec![semantic_other.memory],
                ..WorkingRepresentation::default()
            },
            &WorkingRepresentationRequest {
                perspective: Some(request_perspective),
                max_items: 10,
                ..WorkingRepresentationRequest::default()
            },
        );

        assert_eq!(ranked[0].memory.id, 1);
    }

    #[test]
    fn test_flatten_ranked_representation_prefers_reinforced_derived_memory() {
        let perspective = infer_perspective(
            PerspectiveSource::Query,
            "claude-code",
            None,
            Some("session-a".to_string()),
        );

        let mut derived = memory(
            1,
            "derived insight",
            MemoryBucket::Derived,
            &perspective,
            CognitiveLevel::Derived,
        );
        if let Some(cognitive) = derived.memory.metadata.get_mut("cognitive") {
            cognitive["times_reinforced"] = serde_json::json!(6);
        }

        let mut recent = memory(
            2,
            "recent note",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Explicit,
        );
        recent.memory.created_at = Utc::now() - Duration::hours(2);

        let ranked = flatten_ranked_representation(
            WorkingRepresentation {
                derived: vec![derived.memory],
                recent: vec![recent.memory],
                ..WorkingRepresentation::default()
            },
            &WorkingRepresentationRequest {
                perspective: Some(perspective),
                max_items: 10,
                ..WorkingRepresentationRequest::default()
            },
        );

        assert_eq!(ranked[0].memory.id, 1);
    }

    #[test]
    fn test_flatten_ranked_representation_demotes_raw_activity() {
        let perspective = infer_perspective(
            PerspectiveSource::Query,
            "claude-code",
            None,
            Some("session-a".to_string()),
        );

        let mut raw = memory(
            1,
            "raw payload",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Raw,
        );
        raw.memory.labels.push("raw-activity".to_string());
        raw.memory.metadata["raw_activity"] = serde_json::json!(true);

        let clean = memory(
            2,
            "clean observation",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Explicit,
        );

        let ranked = flatten_ranked_representation(
            WorkingRepresentation {
                recent: vec![raw.memory, clean.memory],
                ..WorkingRepresentation::default()
            },
            &WorkingRepresentationRequest {
                perspective: Some(perspective),
                max_items: 10,
                ..WorkingRepresentationRequest::default()
            },
        );

        assert_eq!(ranked[0].memory.id, 2);
    }
}
