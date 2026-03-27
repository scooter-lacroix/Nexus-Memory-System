//! Reflection service - deterministic reinforcement and contradiction detection.
//!
//! Implements a typed, deterministic reflection pipeline that scans perspective-aligned
//! memories for reinforcement patterns and simple contradiction cases. Outputs are
//! persisted as derived/contradiction memories with evidence lineage.
//!
//! Explicit seam: the `ReflectService` struct is designed to accept an optional LLM
//! client in future phases for deeper semantic reflection, but the current slice
//! operates entirely through deterministic content analysis.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::traits::EmbeddingService;
use nexus_core::{
    CognitiveLevel, CognitiveMetadata, Memory, MemoryCategory, MemoryLaneCognitiveType,
    MemoryLanePriorityType, MemoryLaneType, PerspectiveKey,
};
use nexus_storage::repository::{
    ListMemoryFilters, MemoryRepository, StoreMemoryParams, StoreMemoryWithLineageParams,
};
use tracing::{debug, info};

use crate::error::AgentError;
use crate::util::{flush_metric_samples, maybe_embed, stage_metric_sample, CognitionSnapshot};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const REFLECT_GENERATED_BY: &str = "reflect_service";
const REINFORCE_EVIDENCE_ROLE: &str = "reinforces";
const CONTRADICT_EVIDENCE_ROLE: &str = "contradicts";
const INSIGHT_EVIDENCE_ROLE: &str = "insight_support";
const MAX_CANDIDATES: i64 = 100;
const MIN_INSIGHT_COMPONENT_SIZE: usize = 3;
const MAX_INSIGHT_CONTENT_CHARS: usize = 180;

/// Word-level Jaccard similarity threshold for reinforcement detection.
const REINFORCE_SIMILARITY_THRESHOLD: f32 = 0.80;
const INSIGHT_SIMILARITY_THRESHOLD: f32 = 0.55;

/// Minimum topic overlap for contradiction candidate consideration.
const CONTRADICTION_MIN_TOPIC_OVERLAP: f32 = 0.30;

/// Negation words that signal contradiction when paired with an affirmative claim.
const NEGATION_WORDS: &[&str] = &[
    "not",
    "no",
    "never",
    "don't",
    "doesn't",
    "can't",
    "cannot",
    "won't",
    "isn't",
    "aren't",
    "wasn't",
    "weren't",
    "shouldn't",
    "wouldn't",
    "couldn't",
];

/// Stop words excluded from topic comparison.
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can", "to",
    "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through", "during",
    "before", "after", "above", "below", "between", "and", "but", "or", "nor", "if", "then",
    "that", "this", "these", "those", "it", "its", "we", "our", "they", "their", "he", "she",
    "his", "her", "my", "your",
];

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// What kind of reflection was detected between memories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReflectionCase {
    /// Memories reinforce the same observation (high content similarity, same level).
    Reinforcement,
    /// Memories assert contradictory claims about the same topic.
    Contradiction,
}

/// Output of a single reflection comparison between two memories.
#[derive(Debug, Clone)]
pub struct ReflectionOutput {
    pub case: ReflectionCase,
    pub left_id: i64,
    pub right_id: i64,
    pub similarity: f32,
}

/// Summary of a full reflection cycle.
#[derive(Debug, Clone, Default)]
pub struct ReflectionResult {
    pub memories_scanned: usize,
    pub pairs_compared: usize,
    pub reinforcements: usize,
    pub insights_created: usize,
    pub contradictions_created: usize,
    pub insight_ids: Vec<i64>,
    pub contradiction_ids: Vec<i64>,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Deterministic reflection engine for reinforcement and contradiction detection.
///
/// Operates over perspective-aligned Explicit and Derived memories within a single
/// namespace. Each cycle is bounded by `MAX_CANDIDATES` and idempotent — pairs that
/// already have evidence links are skipped.
pub struct ReflectService {
    _config: AgentConfig,
    cognition: CognitionConfig,
    embeddings: Option<Arc<dyn EmbeddingService>>,
}

impl ReflectService {
    pub fn new(
        config: AgentConfig,
        cognition: CognitionConfig,
        embeddings: Option<Arc<dyn EmbeddingService>>,
    ) -> Self {
        Self {
            _config: config,
            cognition,
            embeddings,
        }
    }

    /// Run a single bounded reflection cycle over the namespace's memories.
    ///
    /// Scans up to `MAX_CANDIDATES` non-raw memories, compares all eligible pairs,
    /// and persists reinforcement or contradiction outputs. Returns a summary of
    /// what was found and created.
    pub async fn reflect_cycle(
        &self,
        namespace_id: i64,
        repo: &MemoryRepository,
    ) -> Result<ReflectionResult, AgentError> {
        let started = Instant::now();
        let groups = gather_candidates(namespace_id, repo).await?;
        let scanned: usize = groups.values().map(Vec::len).sum();
        if scanned < 2 {
            debug!(namespace_id, "Not enough candidates for reflection");
            return Ok(ReflectionResult::default());
        }

        let mut result = ReflectionResult {
            memories_scanned: scanned,
            ..Default::default()
        };

        for (perspective, candidates) in groups {
            let group_result = run_reflection_group(
                candidates,
                &perspective,
                repo,
                self.embeddings.as_deref(),
                &self.cognition,
            )
            .await?;
            result.pairs_compared += group_result.pairs_compared;
            result.reinforcements += group_result.reinforcements;
            result.insights_created += group_result.insights_created;
            result.contradictions_created += group_result.contradictions_created;
            result.insight_ids.extend(group_result.insight_ids);
            result
                .contradiction_ids
                .extend(group_result.contradiction_ids);
        }

        info!(
            namespace_id,
            scanned,
            pairs = result.pairs_compared,
            reinforcements = result.reinforcements,
            contradictions = result.contradictions_created,
            "Reflection cycle complete"
        );
        flush_metric_samples(
            repo,
            &[stage_metric_sample(
                namespace_id,
                "cognition.reflect.total_ms",
                started.elapsed().as_secs_f64() * 1000.0,
                "reflect_total",
            )],
        )
        .await;

        Ok(result)
    }

    pub async fn reflect_perspective_cycle(
        &self,
        namespace_id: i64,
        perspective: &PerspectiveKey,
        repo: &MemoryRepository,
    ) -> Result<ReflectionResult, AgentError> {
        let groups = gather_candidates(namespace_id, repo).await?;
        let candidates = groups.get(perspective).cloned().unwrap_or_default();
        run_reflection_group(
            candidates,
            perspective,
            repo,
            self.embeddings.as_deref(),
            &self.cognition,
        )
        .await
    }

    /// Compare two memories and return the reflection case, if any.
    ///
    /// This is the pure deterministic comparison logic, exposed for testing and
    /// future LLM augmentation hooks.
    pub fn compare_pair(left: &Memory, right: &Memory) -> Option<ReflectionCase> {
        compare_pair(left, right)
    }
}

async fn run_reflection_group(
    candidates: Vec<Memory>,
    perspective: &PerspectiveKey,
    repo: &MemoryRepository,
    embeddings: Option<&dyn EmbeddingService>,
    cognition: &CognitionConfig,
) -> Result<ReflectionResult, AgentError> {
    let scanned = candidates.len();
    if scanned < 2 {
        return Ok(ReflectionResult {
            memories_scanned: scanned,
            ..Default::default()
        });
    }

    let existing_links = load_pair_evidence(repo, &candidates).await?;
    let mut result = ReflectionResult {
        memories_scanned: scanned,
        ..Default::default()
    };
    let mut reinforcement_pairs = Vec::new();

    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let left = &candidates[i];
            let right = &candidates[j];
            result.pairs_compared += 1;

            let pair_key = ordered_pair_key(left.id, right.id);
            if existing_links.contains(&pair_key) {
                continue;
            }

            match compare_pair(left, right) {
                Some(ReflectionCase::Reinforcement) => {
                    handle_reinforcement(left, right, perspective, repo, embeddings).await?;
                    reinforcement_pairs.push((left.id, right.id));
                    result.reinforcements += 1;
                }
                Some(ReflectionCase::Contradiction) => {
                    let contradiction_id =
                        handle_contradiction(left, right, perspective, repo, embeddings, cognition)
                            .await?;
                    result.contradiction_ids.push(contradiction_id);
                    result.contradictions_created += 1;
                }
                None => {}
            }
        }
    }

    let insight_ids = synthesize_reinforcement_insights(
        &candidates,
        &reinforcement_pairs,
        perspective,
        repo,
        embeddings,
    )
    .await?;
    result.insights_created = insight_ids.len();
    result.insight_ids = insight_ids;

    Ok(result)
}

// ---------------------------------------------------------------------------
// Candidate gathering
// ---------------------------------------------------------------------------

async fn gather_candidates(
    namespace_id: i64,
    repo: &MemoryRepository,
) -> Result<HashMap<PerspectiveKey, Vec<Memory>>, AgentError> {
    let all = repo
        .list_filtered(
            namespace_id,
            ListMemoryFilters {
                category: None,
                since: None,
                until: None,
                content_like: None,
                include_raw: false,
                limit: MAX_CANDIDATES,
                offset: 0,
            },
        )
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    // Only consider Explicit and Derived level memories for reflection.
    let mut candidates: HashMap<PerspectiveKey, Vec<Memory>> = HashMap::new();
    for memory in all {
        let snapshot = CognitionSnapshot::from_memory(&memory);
        if matches!(
            snapshot.level,
            CognitiveLevel::Explicit | CognitiveLevel::Derived
        ) && !is_reflection_generated(&snapshot, &memory)
        {
            if let Some(perspective) = snapshot.perspective {
                candidates.entry(perspective).or_default().push(memory);
            }
        }
    }

    Ok(candidates)
}

fn is_reflection_generated(snapshot: &CognitionSnapshot, memory: &Memory) -> bool {
    memory.labels.iter().any(|label| label == "reflection")
        || snapshot
            .generated_by
            .as_deref()
            .map(|generated_by| generated_by == REFLECT_GENERATED_BY)
            .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Idempotency: evidence-based dedup
// ---------------------------------------------------------------------------

type PairKey = (i64, i64);

fn ordered_pair_key(a: i64, b: i64) -> PairKey {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Load evidence-based pair tracking for idempotency.
///
/// Evidence rows created by `store_with_lineage` link a reflection memory (derived)
/// to each source individually. To detect that a pair of sources already shares a
/// reflection, we collect all reflection-memory → source links and group sources by
/// their shared reflection memory. Any two sources linked to the same reflection
/// memory are considered an already-processed pair.
async fn load_pair_evidence(
    repo: &MemoryRepository,
    candidates: &[Memory],
) -> Result<HashSet<PairKey>, AgentError> {
    let candidate_ids: Vec<i64> = candidates.iter().map(|memory| memory.id).collect();
    let lineage_by_memory = repo
        .load_lineage_batch(&candidate_ids)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    // Map: reflection_memory_id → Vec<source_memory_id>
    let mut reflection_to_sources: HashMap<i64, Vec<i64>> = HashMap::new();

    for mem in candidates {
        if let Some(lineage) = lineage_by_memory.get(&mem.id) {
            for entry in lineage {
                let role = entry.evidence_role.to_lowercase();
                if role == REINFORCE_EVIDENCE_ROLE || role == CONTRADICT_EVIDENCE_ROLE {
                    let reflection_id = entry.derived_memory_id;
                    let source_id = entry.source_memory_id;
                    // The reflection memory is derived_memory_id; the source is source_memory_id.
                    // But load_lineage returns rows where either column matches mem.id,
                    // so we need to figure out which is the reflection and which is the source.
                    // Convention: store_with_lineage puts the NEW memory as derived_memory_id
                    // and the ORIGINAL as source_memory_id.
                    // Since we're loading lineage for a candidate (original), the candidate
                    // will appear as source_memory_id in the evidence row.
                    reflection_to_sources
                        .entry(reflection_id)
                        .or_default()
                        .push(source_id);
                }
            }
        }
    }

    // Any two sources sharing the same reflection memory form a processed pair.
    let mut seen = HashSet::new();
    for sources in reflection_to_sources.values() {
        for i in 0..sources.len() {
            for j in (i + 1)..sources.len() {
                seen.insert(ordered_pair_key(sources[i], sources[j]));
            }
        }
    }
    Ok(seen)
}

// ---------------------------------------------------------------------------
// Deterministic comparison
// ---------------------------------------------------------------------------

fn compare_pair(left: &Memory, right: &Memory) -> Option<ReflectionCase> {
    let similarity = word_jaccard(&left.content, &right.content);

    // High similarity → reinforcement.
    if similarity >= REINFORCE_SIMILARITY_THRESHOLD {
        return Some(ReflectionCase::Reinforcement);
    }

    // Moderate topic overlap with negation pattern → contradiction.
    if similarity >= CONTRADICTION_MIN_TOPIC_OVERLAP
        && has_negation_contradiction(&left.content, &right.content)
    {
        return Some(ReflectionCase::Contradiction);
    }

    None
}

fn word_jaccard(a: &str, b: &str) -> f32 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }
    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }

    let intersection: usize = set_a.intersection(&set_b).count();
    let union: usize = set_a.union(&set_b).count();

    intersection as f32 / union as f32
}

fn has_negation_contradiction(a: &str, b: &str) -> bool {
    let words_a: Vec<&str> = a.split_whitespace().collect();
    let words_b: Vec<&str> = b.split_whitespace().collect();

    has_negation_in_other(&words_a, &words_b) || has_negation_in_other(&words_b, &words_a)
}

/// Check if `base_words` contains a negated version of a claim present in `other_words`.
///
/// Looks for patterns where `other` has a word/token and `base` has that same
/// word preceded by a negation word within a 2-word window.
fn has_negation_in_other(base_words: &[&str], other_words: &[&str]) -> bool {
    let negation_set: HashSet<&str> = NEGATION_WORDS.iter().copied().collect();
    let other_set: HashSet<&str> = other_words.iter().copied().collect();

    for (i, word) in base_words.iter().enumerate() {
        if negation_set.contains(word) {
            // Check the next 1-2 words for a content word also present in the other set.
            for offset in 1..=2 {
                if i + offset < base_words.len() {
                    let target = base_words[i + offset];
                    if !STOP_WORDS.contains(&target)
                        && !negation_set.contains(target)
                        && other_set.contains(target)
                    {
                        return true;
                    }
                }
            }
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Reinforcement handling
// ---------------------------------------------------------------------------

async fn handle_reinforcement(
    left: &Memory,
    right: &Memory,
    perspective: &PerspectiveKey,
    repo: &MemoryRepository,
    embeddings: Option<&dyn EmbeddingService>,
) -> Result<(), AgentError> {
    let content = format!("Reinforced observation ({}x): {}", 2, left.content.trim());

    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::Derived,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        REFLECT_GENERATED_BY,
    );
    cognitive.source_memory_ids = vec![left.id, right.id];
    cognitive.confidence = Some(0.75);
    cognitive.times_reinforced = 2;

    let metadata = cognitive.merge_into(&serde_json::json!({}));
    let (embedding, embedding_model) = maybe_embed(embeddings, &content).await;

    repo.store_with_lineage(StoreMemoryWithLineageParams {
        store: StoreMemoryParams {
            namespace_id: left.namespace_id,
            content: &content,
            category: &MemoryCategory::Facts,
            memory_lane_type: Some(&MemoryLaneType::Cognitive(
                MemoryLaneCognitiveType::Metamemory,
            )),
            labels: &[
                "reflection".to_string(),
                "reinforcement".to_string(),
                "auto".to_string(),
            ],
            metadata: &metadata,
            embedding: embedding.as_deref(),
            embedding_model: embedding_model.as_deref(),
        },
        source_memory_ids: &[left.id, right.id],
        evidence_role: REINFORCE_EVIDENCE_ROLE,
    })
    .await
    .map_err(|e| AgentError::Storage(e.to_string()))?;
    increment_cognitive_counter(repo, left.id, "times_reinforced").await?;
    increment_cognitive_counter(repo, right.id, "times_reinforced").await?;

    debug!(
        left_id = left.id,
        right_id = right.id,
        "Created reinforcement record"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Contradiction handling
// ---------------------------------------------------------------------------

async fn handle_contradiction(
    left: &Memory,
    right: &Memory,
    perspective: &PerspectiveKey,
    repo: &MemoryRepository,
    embeddings: Option<&dyn EmbeddingService>,
    cognition: &CognitionConfig,
) -> Result<i64, AgentError> {
    let content = format!(
        "Contradiction: \"{}\" vs \"{}\"",
        truncate_content(left.content.trim(), 200),
        truncate_content(right.content.trim(), 200),
    );

    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::Contradiction,
        perspective.observer.clone(),
        perspective.subject.clone(),
        perspective.session_key.clone(),
        REFLECT_GENERATED_BY,
    );
    cognitive.source_memory_ids = vec![left.id, right.id];
    cognitive.confidence = Some(0.70);
    cognitive.times_contradicted = 1;

    let metadata = cognitive.merge_into(&serde_json::json!({
        "contradiction_source_ids": [left.id, right.id],
    }));
    let (embedding, embedding_model) = maybe_embed(embeddings, &content).await;

    let memory = repo
        .store_with_lineage(StoreMemoryWithLineageParams {
            store: StoreMemoryParams {
                namespace_id: left.namespace_id,
                content: &content,
                category: &MemoryCategory::Facts,
                memory_lane_type: Some(&MemoryLaneType::Cognitive(
                    MemoryLaneCognitiveType::Metamemory,
                )),
                labels: &[
                    "reflection".to_string(),
                    "contradiction".to_string(),
                    "auto".to_string(),
                ],
                metadata: &metadata,
                embedding: embedding.as_deref(),
                embedding_model: embedding_model.as_deref(),
            },
            source_memory_ids: &[left.id, right.id],
            evidence_role: CONTRADICT_EVIDENCE_ROLE,
        })
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;
    // Combined increment + belief revision: one load, one parse, one write per memory.
    // Previous code called increment_cognitive_counter and apply_belief_revision
    // separately, causing 4 DB loads/writes for a single contradiction pair.
    if cognition.contradiction_belief_revision_enabled {
        increment_counter_and_revise_belief(
            repo,
            left.id,
            cognition.contradiction_confidence_penalty,
        )
        .await?;
        increment_counter_and_revise_belief(
            repo,
            right.id,
            cognition.contradiction_confidence_penalty,
        )
        .await?;
    } else {
        increment_cognitive_counter(repo, left.id, "times_contradicted").await?;
        increment_cognitive_counter(repo, right.id, "times_contradicted").await?;
    }

    debug!(
        left_id = left.id,
        right_id = right.id,
        contradiction_id = memory.id,
        "Created contradiction record"
    );

    Ok(memory.id)
}

async fn increment_cognitive_counter(
    repo: &MemoryRepository,
    memory_id: i64,
    counter_key: &str,
) -> Result<(), AgentError> {
    let Some(memory) = repo
        .get_by_id(memory_id)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?
    else {
        return Ok(());
    };

    // Parse cognitive metadata exactly once.  The snapshot call was redundant
    // here because we need the full CognitiveMetadata struct to merge_into
    // anyway; the snapshot only stores a subset of its fields.
    let mut cognitive =
        CognitiveMetadata::from_metadata(&memory.metadata).unwrap_or_else(|| CognitiveMetadata {
            level: CognitiveLevel::Raw,
            observer: "unknown".to_string(),
            subject: "unknown".to_string(),
            session_key: None,
            source_memory_ids: Vec::new(),
            confidence: None,
            times_reinforced: 0,
            times_contradicted: 0,
            generated_by: REFLECT_GENERATED_BY.to_string(),
            derived_at: None,
            last_belief_revision: None,
            resolution_status: None,
        });

    match counter_key {
        "times_reinforced" => {
            cognitive.times_reinforced = cognitive.times_reinforced.saturating_add(1);
        }
        "times_contradicted" => {
            cognitive.times_contradicted = cognitive.times_contradicted.saturating_add(1);
        }
        _ => return Ok(()),
    }

    let merged = cognitive.merge_into(&memory.metadata);
    repo.update_memory_metadata(memory_id, &merged)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;
    Ok(())
}

/// Combined increment + belief revision for contradiction handling.
///
/// Performs both `increment_cognitive_counter("times_contradicted")` and
/// `apply_belief_revision` in a single DB load/parse/write cycle, halving the
/// round-trips compared to calling them separately.
async fn increment_counter_and_revise_belief(
    repo: &MemoryRepository,
    memory_id: i64,
    penalty: f32,
) -> Result<(), AgentError> {
    let Some(memory) = repo
        .get_by_id(memory_id)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?
    else {
        return Ok(());
    };

    let mut cognitive = match CognitiveMetadata::from_metadata(&memory.metadata) {
        Some(c) => c,
        None => return Ok(()),
    };

    cognitive.times_contradicted = cognitive.times_contradicted.saturating_add(1);
    cognitive.confidence = Some((cognitive.confidence.unwrap_or(0.75) - penalty).max(0.0));
    cognitive.last_belief_revision = Some(Utc::now());
    cognitive.resolution_status = Some("revised".to_string());

    let merged = cognitive.merge_into(&memory.metadata);
    repo.update_memory_metadata(memory_id, &merged)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;
    Ok(())
}

async fn synthesize_reinforcement_insights(
    candidates: &[Memory],
    reinforcement_pairs: &[(i64, i64)],
    perspective: &PerspectiveKey,
    repo: &MemoryRepository,
    embeddings: Option<&dyn EmbeddingService>,
) -> Result<Vec<i64>, AgentError> {
    if reinforcement_pairs.is_empty() {
        return Ok(Vec::new());
    }

    let candidate_by_id: HashMap<i64, &Memory> = candidates.iter().map(|m| (m.id, m)).collect();
    let components = build_reinforcement_components(candidates, reinforcement_pairs);
    let mut insight_ids = Vec::new();

    for component in components {
        if component.len() < MIN_INSIGHT_COMPONENT_SIZE {
            continue;
        }

        let mut source_ids: Vec<i64> = component.into_iter().collect();
        source_ids.sort_unstable();

        if find_existing_component_memory(repo, &source_ids, INSIGHT_EVIDENCE_ROLE)
            .await?
            .is_some()
        {
            continue;
        }

        let component_memories: Vec<&Memory> = source_ids
            .iter()
            .filter_map(|id| candidate_by_id.get(id).copied())
            .collect();
        if component_memories.len() < MIN_INSIGHT_COMPONENT_SIZE {
            continue;
        }

        let content = build_insight_content(&component_memories);
        let mut cognitive = CognitiveMetadata::new(
            CognitiveLevel::Derived,
            perspective.observer.clone(),
            perspective.subject.clone(),
            perspective.session_key.clone(),
            REFLECT_GENERATED_BY,
        );
        cognitive.source_memory_ids = source_ids.clone();
        cognitive.confidence = Some(insight_confidence(component_memories.len()));
        cognitive.times_reinforced = component_memories.len() as i64;

        let metadata = cognitive.merge_into(&serde_json::json!({
            "reflection_kind": "insight",
        }));
        let (embedding, embedding_model) = maybe_embed(embeddings, &content).await;
        let memory = repo
            .store_with_lineage(StoreMemoryWithLineageParams {
                store: StoreMemoryParams {
                    namespace_id: component_memories[0].namespace_id,
                    content: &content,
                    category: &MemoryCategory::Facts,
                    memory_lane_type: Some(&MemoryLaneType::Priority(
                        MemoryLanePriorityType::Insight,
                    )),
                    labels: &[
                        "reflection".to_string(),
                        "insight".to_string(),
                        "auto".to_string(),
                    ],
                    metadata: &metadata,
                    embedding: embedding.as_deref(),
                    embedding_model: embedding_model.as_deref(),
                },
                source_memory_ids: &source_ids,
                evidence_role: INSIGHT_EVIDENCE_ROLE,
            })
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        insight_ids.push(memory.id);
    }

    Ok(insight_ids)
}

fn build_reinforcement_components(
    candidates: &[Memory],
    reinforcement_pairs: &[(i64, i64)],
) -> Vec<Vec<i64>> {
    let mut adjacency: HashMap<i64, Vec<i64>> = HashMap::new();
    for &(left, right) in reinforcement_pairs {
        adjacency.entry(left).or_default().push(right);
        adjacency.entry(right).or_default().push(left);
    }

    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            let left = &candidates[i];
            let right = &candidates[j];
            if matches!(
                compare_pair(left, right),
                Some(ReflectionCase::Contradiction)
            ) {
                continue;
            }

            if word_jaccard(&left.content, &right.content) >= INSIGHT_SIMILARITY_THRESHOLD {
                adjacency.entry(left.id).or_default().push(right.id);
                adjacency.entry(right.id).or_default().push(left.id);
            }
        }
    }

    let mut visited = HashSet::new();
    let mut components = Vec::new();
    for &node in adjacency.keys() {
        if !visited.insert(node) {
            continue;
        }

        let mut stack = vec![node];
        let mut component = vec![node];
        while let Some(current) = stack.pop() {
            if let Some(neighbors) = adjacency.get(&current) {
                for &neighbor in neighbors {
                    if visited.insert(neighbor) {
                        stack.push(neighbor);
                        component.push(neighbor);
                    }
                }
            }
        }

        component.sort_unstable();
        components.push(component);
    }

    components
}

async fn find_existing_component_memory(
    repo: &MemoryRepository,
    source_ids: &[i64],
    evidence_role: &str,
) -> Result<Option<i64>, AgentError> {
    let Some(&first_source_id) = source_ids.first() else {
        return Ok(None);
    };

    let lineage_by_source = repo
        .load_lineage_batch(source_ids)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;
    let lineage = lineage_by_source
        .get(&first_source_id)
        .cloned()
        .unwrap_or_default();
    let candidate_ids: Vec<i64> = lineage
        .into_iter()
        .filter(|entry| {
            entry.source_memory_id == first_source_id && entry.evidence_role == evidence_role
        })
        .map(|entry| entry.derived_memory_id)
        .collect();

    for derived_id in candidate_ids {
        let matches_all = source_ids.iter().all(|source_id| {
            lineage_by_source
                .get(source_id)
                .into_iter()
                .flat_map(|entries| entries.iter())
                .any(|entry| {
                    entry.derived_memory_id == derived_id
                        && entry.source_memory_id == *source_id
                        && entry.evidence_role == evidence_role
                })
        });

        if matches_all {
            return Ok(Some(derived_id));
        }
    }

    Ok(None)
}

fn build_insight_content(memories: &[&Memory]) -> String {
    let representative = memories
        .iter()
        .min_by_key(|memory| memory.id)
        .map(|memory| truncate_content(memory.content.trim(), MAX_INSIGHT_CONTENT_CHARS))
        .unwrap_or("repeated observations");

    format!(
        "Dream insight: repeated evidence indicates {}",
        representative
    )
}

fn insight_confidence(component_size: usize) -> f32 {
    (0.72 + ((component_size.saturating_sub(MIN_INSIGHT_COMPONENT_SIZE)) as f32 * 0.05)).min(0.92)
}

fn truncate_content(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        return s;
    }
    // Find a safe char boundary.
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    // Try to break at a word boundary.
    if let Some(space_pos) = s[..end].rfind(' ') {
        return &s[..space_pos];
    }
    &s[..end]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use nexus_core::{Category, MemoryLanePriorityType};
    use nexus_storage::repository::{NamespaceRepository, StoreMemoryParams};
    use sqlx::sqlite::SqlitePoolOptions;

    fn test_memory(id: i64, content: &str, metadata: serde_json::Value) -> Memory {
        Memory {
            id,
            namespace_id: 1,
            content: content.to_string(),
            category: Category::Facts,
            memory_lane_type: None,
            labels: vec![],
            metadata,
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

    // ---- Helpers ----

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
            .get_or_create("reflect-test", "reflect-test")
            .await
            .unwrap();
        let repo = MemoryRepository::new(pool.clone());
        (pool, repo, namespace.id)
    }

    fn explicit_metadata(observer: &str) -> serde_json::Value {
        let cognitive = CognitiveMetadata::new(
            CognitiveLevel::Explicit,
            observer,
            observer,
            None,
            "derive_service",
        );
        cognitive.merge_into(&serde_json::json!({}))
    }

    fn derived_metadata(observer: &str) -> serde_json::Value {
        let cognitive = CognitiveMetadata::new(
            CognitiveLevel::Derived,
            observer,
            observer,
            None,
            "derive_service",
        );
        cognitive.merge_into(&serde_json::json!({}))
    }

    fn raw_metadata() -> serde_json::Value {
        let cognitive = CognitiveMetadata::new(
            CognitiveLevel::Raw,
            "claude-code",
            "claude-code",
            None,
            "ingest_service",
        );
        cognitive.merge_into(&serde_json::json!({}))
    }

    async fn store_memory(
        repo: &MemoryRepository,
        namespace_id: i64,
        content: &str,
        metadata: &serde_json::Value,
    ) -> Memory {
        repo.store(StoreMemoryParams {
            namespace_id,
            content,
            category: &MemoryCategory::Facts,
            memory_lane_type: Some(&MemoryLaneType::Priority(MemoryLanePriorityType::Decision)),
            labels: &["test".to_string()],
            metadata,
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap()
    }

    // ---- Unit tests: comparison logic ----

    #[test]
    fn test_word_jaccard_identical() {
        assert!((word_jaccard("hello world", "hello world") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_word_jaccard_disjoint() {
        assert!((word_jaccard("alpha beta", "gamma delta")).abs() < f32::EPSILON);
    }

    #[test]
    fn test_word_jaccard_partial() {
        let j = word_jaccard(
            "the query service handles search",
            "the query service handles pagination",
        );
        assert!(j > 0.5, "expected partial overlap, got {}", j);
    }

    #[test]
    fn test_word_jaccard_empty_strings() {
        assert!((word_jaccard("", "") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_word_jaccard_one_empty() {
        assert!((word_jaccard("hello", "")).abs() < f32::EPSILON);
    }

    #[test]
    fn test_compare_pair_similar_content_reinforces() {
        let left = test_memory(
            1,
            "The query service handles search requests",
            explicit_metadata("claude-code"),
        );
        let right = test_memory(
            2,
            "The query service handles search requests efficiently",
            explicit_metadata("claude-code"),
        );

        assert_eq!(
            compare_pair(&left, &right),
            Some(ReflectionCase::Reinforcement)
        );
    }

    #[test]
    fn test_compare_pair_contradiction_pattern() {
        let left = test_memory(
            1,
            "The cache system is enabled and improves performance",
            explicit_metadata("claude-code"),
        );
        let right = test_memory(
            2,
            "The cache system is not enabled and degrades performance",
            explicit_metadata("claude-code"),
        );

        assert_eq!(
            compare_pair(&left, &right),
            Some(ReflectionCase::Contradiction)
        );
    }

    #[test]
    fn test_compare_pair_unrelated() {
        let left = test_memory(
            1,
            "Fixed pagination bug in search endpoint",
            explicit_metadata("claude-code"),
        );
        let right = test_memory(
            2,
            "Updated deployment configuration for staging",
            explicit_metadata("claude-code"),
        );

        assert_eq!(compare_pair(&left, &right), None);
    }

    #[test]
    fn test_has_negation_contradiction_detects_negation() {
        assert!(has_negation_contradiction(
            "the feature is not working correctly",
            "the feature is working correctly"
        ));
    }

    #[test]
    fn test_has_negation_contradiction_no_negation() {
        assert!(!has_negation_contradiction(
            "the feature works well",
            "the feature is fast"
        ));
    }

    // ---- Integration tests: full cycle ----

    #[tokio::test]
    async fn test_reflect_cycle_empty_namespace() {
        let (_pool, repo, namespace_id) = setup_repo().await;
        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);

        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();
        assert_eq!(result.memories_scanned, 0);
        assert_eq!(result.pairs_compared, 0);
        assert_eq!(result.reinforcements, 0);
        assert_eq!(result.insights_created, 0);
        assert_eq!(result.contradictions_created, 0);
    }

    #[tokio::test]
    async fn test_reflect_cycle_skips_raw_memories() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(&repo, namespace_id, "raw noise event", &raw_metadata()).await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.memories_scanned, 0);
    }

    #[tokio::test]
    async fn test_reflect_cycle_detects_reinforcement() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        let left = store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests",
            &explicit_metadata("claude-code"),
        )
        .await;
        let right = store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests efficiently",
            &explicit_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.memories_scanned, 2);
        assert!(
            result.reinforcements >= 1,
            "expected at least 1 reinforcement"
        );
        assert_eq!(result.contradictions_created, 0);

        let left = repo.get_by_id(left.id).await.unwrap().unwrap();
        let right = repo.get_by_id(right.id).await.unwrap().unwrap();
        let left_cognitive = CognitiveMetadata::from_metadata(&left.metadata).unwrap();
        let right_cognitive = CognitiveMetadata::from_metadata(&right.metadata).unwrap();
        assert_eq!(left_cognitive.times_reinforced, 1);
        assert_eq!(right_cognitive.times_reinforced, 1);
    }

    #[tokio::test]
    async fn test_reflect_cycle_detects_contradiction() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        let left = store_memory(
            &repo,
            namespace_id,
            "The cache system is enabled and improves performance",
            &explicit_metadata("claude-code"),
        )
        .await;
        let right = store_memory(
            &repo,
            namespace_id,
            "The cache system is not enabled and degrades performance",
            &explicit_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.memories_scanned, 2);
        assert_eq!(result.contradictions_created, 1);
        assert_eq!(result.contradiction_ids.len(), 1);

        let left = repo.get_by_id(left.id).await.unwrap().unwrap();
        let right = repo.get_by_id(right.id).await.unwrap().unwrap();
        let left_cognitive = CognitiveMetadata::from_metadata(&left.metadata).unwrap();
        let right_cognitive = CognitiveMetadata::from_metadata(&right.metadata).unwrap();
        assert_eq!(left_cognitive.times_contradicted, 1);
        assert_eq!(right_cognitive.times_contradicted, 1);
    }

    #[tokio::test]
    async fn test_reflect_cycle_is_idempotent() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests",
            &explicit_metadata("claude-code"),
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests efficiently",
            &explicit_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);

        // First pass.
        let result1 = service.reflect_cycle(namespace_id, &repo).await.unwrap();
        let first_reinforcements = result1.reinforcements;

        // Second pass — should be idempotent.
        let result2 = service.reflect_cycle(namespace_id, &repo).await.unwrap();
        assert_eq!(
            result2.reinforcements, 0,
            "second pass should not create duplicate reinforcements"
        );
        // First pass result is still valid.
        assert!(first_reinforcements >= 1);
    }

    #[tokio::test]
    async fn test_reinforcement_creates_evidence_links() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        let m1 = store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests",
            &explicit_metadata("claude-code"),
        )
        .await;
        let m2 = store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests efficiently",
            &explicit_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        service.reflect_cycle(namespace_id, &repo).await.unwrap();

        // Verify evidence links exist: both m1 and m2 should share a reinforcement
        // link to the same reflection memory.
        let lineage1 = repo.load_lineage(m1.id).await.unwrap();
        let reinforcement_ids: Vec<i64> = lineage1
            .iter()
            .filter(|e| e.evidence_role == REINFORCE_EVIDENCE_ROLE)
            .map(|e| e.derived_memory_id)
            .collect();

        let lineage2 = repo.load_lineage(m2.id).await.unwrap();
        let shared: Vec<i64> = lineage2
            .iter()
            .filter(|e| {
                e.evidence_role == REINFORCE_EVIDENCE_ROLE
                    && reinforcement_ids.contains(&e.derived_memory_id)
            })
            .map(|e| e.derived_memory_id)
            .collect();

        assert!(
            !shared.is_empty(),
            "expected shared reinforcement memory linking both sources"
        );
    }

    #[tokio::test]
    async fn test_contradiction_stores_with_correct_metadata() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "The cache system is enabled and improves performance",
            &explicit_metadata("claude-code"),
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "The cache system is not enabled and degrades performance",
            &explicit_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();
        assert_eq!(result.contradiction_ids.len(), 1);

        // Fetch the contradiction memory and verify its cognitive metadata.
        let contradiction_id = result.contradiction_ids[0];
        let memories = repo
            .list_filtered(
                namespace_id,
                ListMemoryFilters {
                    category: None,
                    since: None,
                    until: None,
                    content_like: Some("Contradiction"),
                    include_raw: false,
                    limit: 10,
                    offset: 0,
                },
            )
            .await
            .unwrap();

        let contradiction = memories
            .iter()
            .find(|m| m.id == contradiction_id)
            .expect("contradiction memory should be retrievable");

        let cognitive = CognitiveMetadata::from_metadata(&contradiction.metadata)
            .expect("contradiction memory should have cognitive metadata");
        assert_eq!(cognitive.level, CognitiveLevel::Contradiction);
        assert_eq!(cognitive.generated_by, REFLECT_GENERATED_BY);
        assert_eq!(cognitive.source_memory_ids.len(), 2);
        assert!(cognitive.confidence.is_some());
        assert!(cognitive.confidence.unwrap() > 0.0);

        // Verify evidence links.
        let lineage = repo.load_lineage(contradiction_id).await.unwrap();
        assert!(
            lineage
                .iter()
                .any(|e| e.evidence_role == CONTRADICT_EVIDENCE_ROLE),
            "contradiction memory should have contradicts evidence"
        );
    }

    #[tokio::test]
    async fn test_reflect_cycle_handles_derived_level() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests",
            &derived_metadata("claude-code"),
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests efficiently",
            &derived_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.memories_scanned, 2);
        assert!(result.reinforcements >= 1);
    }

    #[tokio::test]
    async fn test_reflect_cycle_creates_higher_order_insight() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        for content in [
            "The query service handles search requests",
            "The query service handles search requests efficiently",
            "The query service handles search requests reliably",
        ] {
            store_memory(
                &repo,
                namespace_id,
                content,
                &explicit_metadata("claude-code"),
            )
            .await;
        }

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.insights_created, 1);
        assert_eq!(result.insight_ids.len(), 1);

        let insight = repo
            .get_by_id(result.insight_ids[0])
            .await
            .unwrap()
            .unwrap();
        assert!(insight.content.starts_with("Dream insight:"));
        assert!(insight.labels.iter().any(|label| label == "insight"));
        let cognitive = CognitiveMetadata::from_metadata(&insight.metadata).unwrap();
        assert_eq!(cognitive.level, CognitiveLevel::Derived);
        assert_eq!(cognitive.source_memory_ids.len(), 3);
        assert_eq!(cognitive.times_reinforced, 3);

        let lineage = repo.load_lineage(insight.id).await.unwrap();
        let evidence_count = lineage
            .iter()
            .filter(|entry| entry.evidence_role == INSIGHT_EVIDENCE_ROLE)
            .count();
        assert_eq!(evidence_count, 3);
    }

    #[tokio::test]
    async fn test_reflect_cycle_contradiction_idempotent() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "The cache system is enabled and improves performance",
            &explicit_metadata("claude-code"),
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "The cache system is not enabled and degrades performance",
            &explicit_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);

        let result1 = service.reflect_cycle(namespace_id, &repo).await.unwrap();
        assert_eq!(result1.contradictions_created, 1);

        let result2 = service.reflect_cycle(namespace_id, &repo).await.unwrap();
        assert_eq!(
            result2.contradictions_created, 0,
            "second pass should not create duplicate contradictions"
        );
    }

    #[tokio::test]
    async fn test_reflect_cycle_insight_idempotent() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        for content in [
            "The query service handles search requests",
            "The query service handles search requests efficiently",
            "The query service handles search requests reliably",
        ] {
            store_memory(
                &repo,
                namespace_id,
                content,
                &explicit_metadata("claude-code"),
            )
            .await;
        }

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let first = service.reflect_cycle(namespace_id, &repo).await.unwrap();
        let second = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(first.insights_created, 1);
        assert_eq!(second.insights_created, 0);

        let insights = repo
            .list_filtered(
                namespace_id,
                ListMemoryFilters {
                    category: None,
                    since: None,
                    until: None,
                    content_like: Some("Dream insight:"),
                    include_raw: false,
                    limit: 10,
                    offset: 0,
                },
            )
            .await
            .unwrap();
        assert_eq!(insights.len(), 1);
    }

    #[tokio::test]
    async fn test_contradiction_gets_embedding_when_service_provided() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "The cache system is enabled and improves performance",
            &explicit_metadata("claude-code"),
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "The cache system is not enabled and degrades performance",
            &explicit_metadata("claude-code"),
        )
        .await;

        let mock_embed = nexus_embeddings::MockEmbeddingService::new();
        let service = ReflectService::new(
            AgentConfig::default(),
            CognitionConfig::default(),
            Some(Arc::new(mock_embed)),
        );
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.contradiction_ids.len(), 1);

        let contradiction = repo
            .get_by_id(result.contradiction_ids[0])
            .await
            .unwrap()
            .unwrap();
        assert!(
            contradiction.content_embedding.is_some(),
            "contradiction memory should have an embedding when service is provided"
        );
        assert_eq!(
            contradiction.content_embedding.as_ref().unwrap().len(),
            384,
            "embedding dimension should be 384"
        );
    }

    #[tokio::test]
    async fn test_insight_gets_embedding_when_service_provided() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        for content in [
            "The query service handles search requests",
            "The query service handles search requests efficiently",
            "The query service handles search requests reliably",
        ] {
            store_memory(
                &repo,
                namespace_id,
                content,
                &explicit_metadata("claude-code"),
            )
            .await;
        }

        let mock_embed = nexus_embeddings::MockEmbeddingService::new();
        let service = ReflectService::new(
            AgentConfig::default(),
            CognitionConfig::default(),
            Some(Arc::new(mock_embed)),
        );
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.insights_created, 1);

        let insight = repo
            .get_by_id(result.insight_ids[0])
            .await
            .unwrap()
            .unwrap();
        assert!(
            insight.content_embedding.is_some(),
            "insight memory should have an embedding when service is provided"
        );
        assert_eq!(
            insight.content_embedding.as_ref().unwrap().len(),
            384,
            "embedding dimension should be 384"
        );
    }

    #[tokio::test]
    async fn test_reflect_stores_without_embedding_when_service_absent() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "The cache system is enabled and improves performance",
            &explicit_metadata("claude-code"),
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "The cache system is not enabled and degrades performance",
            &explicit_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();
        assert_eq!(result.contradiction_ids.len(), 1);

        let contradiction = repo
            .get_by_id(result.contradiction_ids[0])
            .await
            .unwrap()
            .unwrap();
        assert!(
            contradiction.content_embedding.is_none(),
            "contradiction memory should NOT have embedding when no service provided"
        );
    }

    #[tokio::test]
    async fn test_reflect_cycle_does_not_cross_perspectives() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests",
            &explicit_metadata("claude-code"),
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "The query service handles search requests efficiently",
            &explicit_metadata("codex"),
        )
        .await;

        let service = ReflectService::new(AgentConfig::default(), CognitionConfig::default(), None);
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.memories_scanned, 2);
        assert_eq!(result.pairs_compared, 0);
        assert_eq!(result.reinforcements, 0);
        assert_eq!(result.contradictions_created, 0);
    }

    // ---- Belief revision tests (Phase 16) ----

    #[tokio::test]
    async fn test_belief_revision_reduces_confidence_on_contradiction() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        let left = store_memory(
            &repo,
            namespace_id,
            "The cache system is enabled and improves performance",
            &explicit_metadata("claude-code"),
        )
        .await;
        let right = store_memory(
            &repo,
            namespace_id,
            "The cache system is not enabled and degrades performance",
            &explicit_metadata("claude-code"),
        )
        .await;

        // Both start with default confidence (None → 0.75).
        let left_before = repo.get_by_id(left.id).await.unwrap().unwrap();
        let right_before = repo.get_by_id(right.id).await.unwrap().unwrap();
        let left_conf_before = CognitiveMetadata::from_metadata(&left_before.metadata)
            .unwrap()
            .confidence;
        let right_conf_before = CognitiveMetadata::from_metadata(&right_before.metadata)
            .unwrap()
            .confidence;

        let service = ReflectService::new(
            AgentConfig::default(),
            CognitionConfig {
                contradiction_belief_revision_enabled: true,
                contradiction_confidence_penalty: 0.15,
                ..CognitionConfig::default()
            },
            None,
        );
        let result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        assert_eq!(result.contradictions_created, 1);

        let left_after = repo.get_by_id(left.id).await.unwrap().unwrap();
        let right_after = repo.get_by_id(right.id).await.unwrap().unwrap();
        let left_cognitive = CognitiveMetadata::from_metadata(&left_after.metadata).unwrap();
        let right_cognitive = CognitiveMetadata::from_metadata(&right_after.metadata).unwrap();

        // Confidence should have decreased (use unwrap_or for Option comparison).
        assert!(
            left_cognitive.confidence.unwrap_or(0.75) < left_conf_before.unwrap_or(0.75),
            "left confidence should decrease after belief revision"
        );
        assert!(
            right_cognitive.confidence.unwrap_or(0.75) < right_conf_before.unwrap_or(0.75),
            "right confidence should decrease after belief revision"
        );
        // Belief revision timestamp should be set.
        assert!(left_cognitive.last_belief_revision.is_some());
        assert!(right_cognitive.last_belief_revision.is_some());
        // Resolution status should be "revised".
        assert_eq!(left_cognitive.resolution_status.as_deref(), Some("revised"));
        assert_eq!(
            right_cognitive.resolution_status.as_deref(),
            Some("revised")
        );
    }

    #[tokio::test]
    async fn test_belief_revision_disabled_does_not_modify_confidence() {
        let (_pool, repo, namespace_id) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "The cache system is enabled",
            &explicit_metadata("claude-code"),
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "The cache system is not enabled",
            &explicit_metadata("claude-code"),
        )
        .await;

        let service = ReflectService::new(
            AgentConfig::default(),
            CognitionConfig {
                contradiction_belief_revision_enabled: false,
                ..CognitionConfig::default()
            },
            None,
        );
        let _result = service.reflect_cycle(namespace_id, &repo).await.unwrap();

        // Check that no memories got belief revision stamps.
        let memories = repo
            .list_filtered(
                namespace_id,
                ListMemoryFilters {
                    category: None,
                    since: None,
                    until: None,
                    content_like: None,
                    include_raw: false,
                    limit: 10,
                    offset: 0,
                },
            )
            .await
            .unwrap();

        for memory in memories {
            if let Some(cognitive) = CognitiveMetadata::from_metadata(&memory.metadata) {
                assert!(
                    cognitive.last_belief_revision.is_none(),
                    "no belief revision should occur when disabled"
                );
                assert!(
                    cognitive.resolution_status.is_none(),
                    "no resolution status when belief revision disabled"
                );
            }
        }
    }
}
