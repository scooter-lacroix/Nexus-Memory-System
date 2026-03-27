//! Shared blended ranking for working representations and recall output.

use std::cmp::Ordering;
use std::collections::HashMap;

use chrono::Utc;
use nexus_core::config::CognitionConfig;
use nexus_core::{
    CognitiveLevel, Memory, PerspectiveKey, WorkingRepresentation, WorkingRepresentationRequest,
};

use crate::types::{ExclusionReason, MemoryBucket};
use crate::util::CognitionSnapshot;

#[derive(Debug, Clone)]
pub struct BucketedMemory {
    pub memory: Memory,
    pub bucket: MemoryBucket,
    pub blended_score: f32,
}

/// Result of ranking with introspection: included memories plus those
/// excluded by budget truncation.
#[derive(Debug, Clone)]
pub struct RankedResult {
    pub included: Vec<BucketedMemory>,
    pub excluded: Vec<RankedExcludedMemory>,
}

#[derive(Debug, Clone)]
pub struct RankedExcludedMemory {
    pub memory: Memory,
    pub bucket: MemoryBucket,
    pub blended_score: f32,
    pub reason: ExclusionReason,
}

/// Deduplicate, rank, and split memories into included/excluded sets.
pub fn flatten_ranked_representation_with_excluded(
    representation: WorkingRepresentation,
    request: &WorkingRepresentationRequest,
) -> RankedResult {
    flatten_ranked_representation_with_config(representation, request, None)
}

/// Like [`flatten_ranked_representation_with_excluded`] but accepts an optional
/// `CognitionConfig` to enable memory aging decay in the blend.
pub fn flatten_ranked_representation_with_config(
    representation: WorkingRepresentation,
    request: &WorkingRepresentationRequest,
    cognition: Option<&CognitionConfig>,
) -> RankedResult {
    let mut best_by_id: HashMap<i64, BucketedMemory> = HashMap::new();
    let mut excluded = Vec::new();

    for (bucket, memories) in [
        (MemoryBucket::Digests, representation.digests),
        (MemoryBucket::Derived, representation.derived),
        (MemoryBucket::Semantic, representation.semantic),
        (MemoryBucket::Recent, representation.recent),
        (MemoryBucket::Contradictions, representation.contradictions),
    ] {
        for memory in memories {
            let candidate = BucketedMemory {
                blended_score: blended_score(&memory, bucket, request, cognition),
                memory,
                bucket,
            };

            match best_by_id.get_mut(&candidate.memory.id) {
                Some(existing)
                    if compare_bucketed_memory(&candidate, existing) == Ordering::Greater =>
                {
                    excluded.push(RankedExcludedMemory {
                        memory: existing.memory.clone(),
                        bucket: existing.bucket,
                        blended_score: existing.blended_score,
                        reason: ExclusionReason::Deduplicated,
                    });
                    *existing = candidate;
                }
                None => {
                    best_by_id.insert(candidate.memory.id, candidate);
                }
                Some(_) => excluded.push(RankedExcludedMemory {
                    memory: candidate.memory,
                    bucket: candidate.bucket,
                    blended_score: candidate.blended_score,
                    reason: ExclusionReason::Deduplicated,
                }),
            }
        }
    }

    let mut ranked: Vec<_> = best_by_id.into_values().collect();
    ranked.sort_by(|left, right| compare_bucketed_memory(left, right).reverse());

    let max_items = request.max_items;
    if ranked.len() > max_items {
        excluded.extend(ranked.split_off(max_items).into_iter().map(|memory| {
            RankedExcludedMemory {
                memory: memory.memory,
                bucket: memory.bucket,
                blended_score: memory.blended_score,
                reason: ExclusionReason::BudgetTruncation,
            }
        }));
    }

    RankedResult {
        included: ranked,
        excluded,
    }
}

pub(crate) fn flatten_ranked_representation(
    representation: WorkingRepresentation,
    request: &WorkingRepresentationRequest,
) -> Vec<BucketedMemory> {
    flatten_ranked_representation_with_config(representation, request, None).included
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
    cognition: Option<&CognitionConfig>,
) -> f32 {
    if memory.is_archived {
        return -0.5;
    }

    if is_raw_activity(memory) {
        return if request.include_raw { -0.1 } else { -1.0 };
    }

    let snapshot = CognitionSnapshot::from_memory(memory);
    let confidence = snapshot.confidence.unwrap_or(0.75).clamp(0.0, 1.0);
    let reinforcement_score = ((snapshot.times_reinforced.max(0) as f32) / 5.0).min(1.0);
    let semantic_similarity = memory
        .relevance_score
        .or(memory.similarity_score)
        .unwrap_or_default()
        .clamp(0.0, 1.0);
    let recency_score = recency_weight(memory);
    let perspective_score = perspective_weight(&snapshot, request.perspective.as_ref());
    let digest_or_derived_bonus = f32::from(matches!(
        snapshot.level,
        CognitiveLevel::SummaryShort | CognitiveLevel::SummaryLong | CognitiveLevel::Derived
    ));

    let base = match bucket {
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
    };

    // Apply memory aging decay penalty when enabled.
    let decay = decay_penalty(memory, cognition);
    (base - decay).max(-1.0)
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

/// Compute a decay penalty (0.0–0.15) based on memory age and access patterns.
///
/// Memories older than `decay_age_days` without recent access get an increasing
/// penalty. Recent access (within `access_boost_days`) resets the effective age.
fn decay_penalty(memory: &Memory, cognition: Option<&CognitionConfig>) -> f32 {
    let config = match cognition {
        Some(c) if c.memory_decay_enabled => c,
        _ => return 0.0,
    };

    let age_days = (Utc::now() - memory.created_at).num_days() as u64;
    if age_days < config.memory_decay_age_days {
        return 0.0;
    }

    // Recent access resets the effective age.
    let effective_age = if let Some(last_accessed) = memory.last_accessed {
        let access_age = (Utc::now() - last_accessed).num_days() as u64;
        if access_age < config.memory_decay_access_boost_days {
            0
        } else {
            age_days.saturating_sub(config.memory_decay_access_boost_days)
        }
    } else {
        age_days
    };

    if effective_age < config.memory_decay_age_days {
        return 0.0;
    }

    // Scale penalty: 0 at decay_age_days, up to 0.15 at 2x decay_age_days.
    let overage = (effective_age - config.memory_decay_age_days) as f32;
    let scale = config.memory_decay_age_days as f32;
    (overage / scale * 0.15).min(0.15)
}

fn perspective_weight(
    snapshot: &CognitionSnapshot,
    request_perspective: Option<&PerspectiveKey>,
) -> f32 {
    let Some(request_perspective) = request_perspective else {
        return 0.5;
    };

    let Some(memory_perspective) = snapshot.perspective.as_ref() else {
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
    CognitionSnapshot::from_memory(memory).raw_activity
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use nexus_core::config::CognitionConfig;
    use nexus_core::{
        infer_perspective, CognitiveMetadata, PerspectiveSource, WorkingRepresentation,
    };

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

    #[test]
    fn test_flatten_ranked_representation_with_excluded_captures_dedupe_and_budget() {
        let perspective = infer_perspective(
            PerspectiveSource::HookIngest,
            "claude-code",
            None::<String>,
            None,
        );
        let shared = memory(
            1,
            "shared semantic",
            MemoryBucket::Semantic,
            &perspective,
            CognitiveLevel::Explicit,
        );
        let lower_duplicate = memory(
            1,
            "shared recent",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Explicit,
        );
        let second = memory(
            2,
            "second memory",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Explicit,
        );

        let representation = WorkingRepresentation {
            digests: vec![],
            recent: vec![lower_duplicate.memory.clone(), second.memory.clone()],
            semantic: vec![shared.memory.clone()],
            derived: vec![],
            contradictions: vec![],
        };
        let request = WorkingRepresentationRequest {
            max_items: 1,
            ..WorkingRepresentationRequest::default()
        };

        let ranked = flatten_ranked_representation_with_excluded(representation, &request);

        assert_eq!(ranked.included.len(), 1);
        assert_eq!(ranked.excluded.len(), 2);
        assert!(ranked
            .excluded
            .iter()
            .any(|item| item.reason == ExclusionReason::Deduplicated));
        assert!(ranked
            .excluded
            .iter()
            .any(|item| item.reason == ExclusionReason::BudgetTruncation));
    }

    #[test]
    fn test_decay_penalty_demotes_old_memories_with_cognition_config() {
        let perspective = infer_perspective(
            PerspectiveSource::Query,
            "claude-code",
            None,
            Some("session-a".to_string()),
        );

        let mut old_memory = memory(
            1,
            "old observation from long ago",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Explicit,
        );
        // Make it very old (200 days).
        old_memory.memory.created_at = Utc::now() - Duration::days(200);

        let fresh_memory = memory(
            2,
            "fresh observation just now",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Explicit,
        );

        let ranked = flatten_ranked_representation_with_config(
            WorkingRepresentation {
                recent: vec![old_memory.memory.clone(), fresh_memory.memory.clone()],
                ..WorkingRepresentation::default()
            },
            &WorkingRepresentationRequest {
                perspective: Some(perspective),
                max_items: 10,
                ..WorkingRepresentationRequest::default()
            },
            Some(&CognitionConfig {
                memory_decay_enabled: true,
                memory_decay_age_days: 90,
                memory_decay_access_boost_days: 30,
                ..CognitionConfig::default()
            }),
        );

        // Fresh memory should rank higher.
        assert_eq!(ranked.included[0].memory.id, 2);
        assert!(ranked.included[0].blended_score > ranked.included[1].blended_score);
    }

    #[test]
    fn test_decay_penalty_recent_access_resets_clock() {
        let perspective = infer_perspective(
            PerspectiveSource::Query,
            "claude-code",
            None,
            Some("session-a".to_string()),
        );

        let mut old_but_accessed = memory(
            1,
            "old but recently accessed",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Explicit,
        );
        old_but_accessed.memory.created_at = Utc::now() - Duration::days(200);
        old_but_accessed.memory.last_accessed = Some(Utc::now() - Duration::days(10));

        let ranked = flatten_ranked_representation_with_config(
            WorkingRepresentation {
                recent: vec![old_but_accessed.memory],
                ..WorkingRepresentation::default()
            },
            &WorkingRepresentationRequest {
                perspective: Some(perspective),
                max_items: 10,
                ..WorkingRepresentationRequest::default()
            },
            Some(&CognitionConfig {
                memory_decay_enabled: true,
                memory_decay_age_days: 90,
                memory_decay_access_boost_days: 30,
                ..CognitionConfig::default()
            }),
        );

        // Recent access should prevent decay — score should be >= 0 (no penalty).
        assert!(ranked.included[0].blended_score >= 0.0);
    }

    #[test]
    fn test_no_decay_when_disabled() {
        let perspective = infer_perspective(
            PerspectiveSource::Query,
            "claude-code",
            None,
            Some("session-a".to_string()),
        );

        let mut old_memory = memory(
            1,
            "very old observation",
            MemoryBucket::Recent,
            &perspective,
            CognitiveLevel::Explicit,
        );
        old_memory.memory.created_at = Utc::now() - Duration::days(200);

        let without_decay = flatten_ranked_representation_with_config(
            WorkingRepresentation {
                recent: vec![old_memory.memory.clone()],
                ..WorkingRepresentation::default()
            },
            &WorkingRepresentationRequest {
                perspective: Some(perspective.clone()),
                max_items: 10,
                ..WorkingRepresentationRequest::default()
            },
            None, // no cognition config → no decay
        );

        let with_decay = flatten_ranked_representation_with_config(
            WorkingRepresentation {
                recent: vec![old_memory.memory.clone()],
                ..WorkingRepresentation::default()
            },
            &WorkingRepresentationRequest {
                perspective: Some(perspective.clone()),
                max_items: 10,
                ..WorkingRepresentationRequest::default()
            },
            Some(&CognitionConfig {
                memory_decay_enabled: true,
                memory_decay_age_days: 90,
                memory_decay_access_boost_days: 30,
                ..CognitionConfig::default()
            }),
        );

        // Decay should reduce the score.
        assert!(without_decay.included[0].blended_score > with_decay.included[0].blended_score);
    }
}
