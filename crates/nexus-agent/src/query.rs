//! Query service - answers questions using memory context.
//!
//! The query pipeline is:
//! 1. Build a `WorkingRepresentation` via `RepresentationService` (bucketed memories).
//! 2. Flatten to deduplicated memories with bucket provenance.
//! 3. Run phase detection via `nexus_lephase::PhaseAnalyzer` (CPU-only, fast).
//! 4. Build a phase-grouped, lineage-annotated context string.
//! 5. Generate an answer via LLM, attaching lineage metadata.

use std::collections::HashMap;
use std::time::Instant;

use nexus_core::config::AgentConfig;
use nexus_core::traits::EmbeddingService;
use nexus_core::{Memory, WorkingRepresentationRequest};
use nexus_lephase::{CompressionMode, LePhaseIntegration};
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, TokenUsage};
use nexus_storage::repository::{MemoryRelationRepository, MemoryRepository, NamespaceRepository};
use tracing::{debug, info, warn};

use crate::error::AgentError;
use crate::identity::IdentityResolver;
use crate::prompts::{
    query_refinement_user_prompt, query_user_prompt_with_lineage, QUERY_SYSTEM_PROMPT,
};
use crate::ranking::{
    flatten_ranked_representation_with_excluded, BucketedMemory, RankedExcludedMemory, RankedResult,
};
use crate::representation::RepresentationService;
use crate::types::{
    BucketIntrospectionStats, ExcludedCandidate, InclusionReason, InclusionSignal, MemoryBucket,
    MemoryLineage, QueryAnswer, QueryIntrospection, RelevantReflection,
    RepresentationConfigSnapshot,
};
use crate::util::{
    extract_agent_summary, flush_metric_samples, parse_json_response, stage_metric_sample,
    token_usage_metric_samples, CognitionSnapshot,
};

pub struct QueryService {
    llm: std::sync::Arc<dyn LlmClient>,
    config: AgentConfig,
    representation: RepresentationService,
}

/// Threshold below which the lightweight (non-phase-grouped) context builder is used.
const PHASE_GROUPING_THRESHOLD: usize = 3;

impl QueryService {
    pub fn new(llm: std::sync::Arc<dyn LlmClient>, config: AgentConfig) -> Self {
        Self {
            llm,
            config,
            representation: RepresentationService::new(),
        }
    }

    pub fn with_embedder(
        llm: std::sync::Arc<dyn LlmClient>,
        config: AgentConfig,
        embedder: std::sync::Arc<dyn EmbeddingService>,
    ) -> Self {
        Self {
            llm,
            config,
            representation: RepresentationService::with_embedder(embedder),
        }
    }

    pub async fn query(
        &self,
        question: &str,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
        relation_repo: &MemoryRelationRepository<'_>,
    ) -> Result<QueryAnswer, AgentError> {
        // Build representation request with all bucket inclusion flags enabled.
        let request = WorkingRepresentationRequest {
            namespace_id,
            perspective: None,
            query: Some(question.to_string()),
            max_items: self.config.query_context_limit,
            include_raw: false,
            include_digests: true,
            include_recent: true,
            include_semantic: true,
            include_derived: true,
            include_contradictions: true,
            ..WorkingRepresentationRequest::default()
        };
        // Resolve cross-namespace aliases; if this fails, fall back.
        let request = match self.with_cross_namespace_ids(request, memory_repo).await {
            Ok(req) => req,
            Err(e) => {
                warn!(error = %e, "Failed to prepare representation request, falling back to legacy search");
                return self
                    .query_legacy(question, namespace_id, memory_repo, relation_repo)
                    .await;
            }
        };

        // Try the representation pipeline; on failure, fall back to legacy text search.
        match self
            .query_with_representation(question, request, memory_repo, relation_repo)
            .await
        {
            Ok(answer) => Ok(answer),
            Err(e) => {
                warn!(error = %e, "Representation pipeline failed, falling back to legacy search");
                self.query_legacy(question, namespace_id, memory_repo, relation_repo)
                    .await
            }
        }
    }

    pub async fn query_with_representation(
        &self,
        question: &str,
        request: WorkingRepresentationRequest,
        memory_repo: &MemoryRepository,
        _relation_repo: &MemoryRelationRepository<'_>,
    ) -> Result<QueryAnswer, AgentError> {
        info!(question = %question, "Processing query");
        let total_started = Instant::now();
        let mut metrics = Vec::new();

        let representation_started = Instant::now();
        let representation = self
            .representation
            .build(&request, memory_repo)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to build working representation");
                AgentError::Storage(e.to_string())
            })?;
        metrics.push(stage_metric_sample(
            request.namespace_id,
            "cognition.query.representation_ms",
            representation_started.elapsed().as_secs_f64() * 1000.0,
            "representation",
        ));

        let flatten_started = Instant::now();
        let ranked = flatten_ranked_representation_with_excluded(representation, &request);
        let bucketed = &ranked.included;
        metrics.push(stage_metric_sample(
            request.namespace_id,
            "cognition.query.flatten_ms",
            flatten_started.elapsed().as_secs_f64() * 1000.0,
            "flatten",
        ));
        debug!(count = bucketed.len(), "Found relevant memories");

        if bucketed.is_empty() {
            let answer_started = Instant::now();
            let (answer, usage) = self.generate_answer(question, "").await?;
            metrics.push(stage_metric_sample(
                request.namespace_id,
                "cognition.query.answer_ms",
                answer_started.elapsed().as_secs_f64() * 1000.0,
                "answer",
            ));
            metrics.extend(token_usage_metric_samples(
                request.namespace_id,
                "cognition.query.answer",
                "answer",
                usage.as_ref(),
            ));
            metrics.push(stage_metric_sample(
                request.namespace_id,
                "cognition.query.total_ms",
                total_started.elapsed().as_secs_f64() * 1000.0,
                "total",
            ));
            flush_metric_samples(memory_repo, &metrics).await;
            return Ok(answer);
        }

        // Build lineages (phase detection + relevance scoring).
        let lineages = build_lineages(bucketed);

        // Build context: phase-grouped for larger sets, lightweight for small sets.
        let context_started = Instant::now();
        let context = if bucketed.len() >= PHASE_GROUPING_THRESHOLD {
            self.build_phase_aware_context(bucketed, &lineages)?
        } else {
            self.build_lightweight_context(bucketed, &lineages)?
        };
        metrics.push(stage_metric_sample(
            request.namespace_id,
            "cognition.query.context_ms",
            context_started.elapsed().as_secs_f64() * 1000.0,
            "context",
        ));

        debug!(context_len = context.len(), "Built query context");

        let answer_started = Instant::now();
        let (answer, usage) = self.generate_answer(question, &context).await?;
        metrics.push(stage_metric_sample(
            request.namespace_id,
            "cognition.query.answer_ms",
            answer_started.elapsed().as_secs_f64() * 1000.0,
            "answer",
        ));
        metrics.extend(token_usage_metric_samples(
            request.namespace_id,
            "cognition.query.answer",
            "answer",
            usage.as_ref(),
        ));
        let mut answer = if should_refine_answer(question, &answer, bucketed) {
            let refine_started = Instant::now();
            let (refined, usage) = self
                .generate_refined_answer(question, &context, &answer)
                .await?;
            metrics.push(stage_metric_sample(
                request.namespace_id,
                "cognition.query.refine_ms",
                refine_started.elapsed().as_secs_f64() * 1000.0,
                "refine",
            ));
            metrics.extend(token_usage_metric_samples(
                request.namespace_id,
                "cognition.query.refine",
                "refine",
                usage.as_ref(),
            ));
            select_better_answer(answer, refined)
        } else {
            answer
        };
        answer.lineages = lineages;

        metrics.push(stage_metric_sample(
            request.namespace_id,
            "cognition.query.total_ms",
            total_started.elapsed().as_secs_f64() * 1000.0,
            "total",
        ));
        flush_metric_samples(memory_repo, &metrics).await;

        info!("Query answered successfully");
        Ok(answer)
    }

    /// Compute introspection for a query without calling the LLM.
    ///
    /// Builds the working representation, runs ranking with excluded-candidate
    /// capture, and fetches recent reflective inferences. Returns a
    /// `QueryIntrospection` suitable for observability surfaces.
    pub async fn query_introspection(
        &self,
        question: &str,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
    ) -> Result<QueryIntrospection, AgentError> {
        let request = WorkingRepresentationRequest {
            namespace_id,
            perspective: None,
            query: Some(question.to_string()),
            max_items: self.config.query_context_limit,
            include_raw: false,
            ..WorkingRepresentationRequest::default()
        };
        let request = self.with_cross_namespace_ids(request, memory_repo).await?;

        self.introspection_with_representation(&request, question, memory_repo)
            .await
    }

    /// Introspection with a custom representation request.
    pub async fn introspection_with_representation(
        &self,
        request: &WorkingRepresentationRequest,
        question: &str,
        memory_repo: &MemoryRepository,
    ) -> Result<QueryIntrospection, AgentError> {
        introspect_query_with_representation_service(
            &self.representation,
            request,
            question,
            memory_repo,
        )
        .await
    }

    async fn with_cross_namespace_ids(
        &self,
        mut request: WorkingRepresentationRequest,
        memory_repo: &MemoryRepository,
    ) -> Result<WorkingRepresentationRequest, AgentError> {
        if !request.cross_namespace_ids.is_empty() {
            return Ok(request);
        }

        let namespace_repo = NamespaceRepository::new(memory_repo.pool().clone());
        let namespaces = namespace_repo
            .list_all()
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        let Some(current) = namespaces
            .iter()
            .find(|namespace| namespace.id == request.namespace_id)
        else {
            return Ok(request);
        };

        request.cross_namespace_ids =
            IdentityResolver::related_namespace_ids(&namespace_repo, &current.name)
                .await
                .into_iter()
                .filter(|id| *id != request.namespace_id)
                .collect();

        Ok(request)
    }

    /// Lightweight context builder for small memory sets (fast path).
    ///
    /// Produces a flat list with per-memory bucket/phase annotations but no
    /// phase-grouped sections.
    fn build_lightweight_context(
        &self,
        bucketed: &[BucketedMemory],
        lineages: &[MemoryLineage],
    ) -> Result<String, AgentError> {
        let mut parts = Vec::with_capacity(bucketed.len());

        for (bm, lineage) in bucketed.iter().zip(lineages.iter()) {
            let summary = extract_agent_summary(
                &serde_json::to_string(&bm.memory.metadata).unwrap_or_else(|_| "{}".to_string()),
                &bm.memory.content,
                300,
            );

            let relevance = lineage
                .relevance_score
                .map_or(String::new(), |r| format!(", relevance: {:.2}", r));

            parts.push(format!(
                "[Memory #{}] {} (bucket: {}, phase: {}{})\nSummary: {}",
                bm.memory.id,
                bm.memory.content.chars().take(100).collect::<String>(),
                lineage.bucket,
                lineage.phase,
                relevance,
                summary,
            ));
        }

        Ok(parts.join("\n\n"))
    }

    /// Phase-aware context builder for larger memory sets.
    ///
    /// Groups memories by detected phase, ordered by phase priority, with
    /// per-memory bucket annotations. Uses `LePhaseIntegration` for the
    /// heavy formatting path.
    fn build_phase_aware_context(
        &self,
        bucketed: &[BucketedMemory],
        lineages: &[MemoryLineage],
    ) -> Result<String, AgentError> {
        let mut lephase = LePhaseIntegration::with_mode(CompressionMode::Balanced);

        // Register memories so lephase can detect phases internally.
        for bm in bucketed {
            lephase.register_memory(&bm.memory);
        }

        let memories: Vec<Memory> = bucketed.iter().map(|bm| bm.memory.clone()).collect();
        let formatted = lephase.format_for_model(&memories, None);

        // Build a lineage map keyed by memory id for fast lookup.
        let lineage_map: HashMap<i64, &MemoryLineage> =
            lineages.iter().map(|l| (l.memory_id, l)).collect();

        // Post-process: annotate each memory line with bucket provenance.
        let annotated = annotate_with_buckets(&formatted, &lineage_map);

        Ok(annotated)
    }

    async fn generate_answer(
        &self,
        question: &str,
        context: &str,
    ) -> Result<(QueryAnswer, Option<TokenUsage>), AgentError> {
        let user_msg = if context.is_empty() {
            query_user_prompt_with_lineage(question, "No relevant memories found.")
        } else {
            query_user_prompt_with_lineage(question, context)
        };

        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(QUERY_SYSTEM_PROMPT),
                ChatMessage::user(user_msg),
            ],
            max_tokens: 4096,
            temperature: 0.3,
            json_mode: true,
        };

        let response = self
            .llm
            .generate(params)
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;
        let usage = response.usage.clone();
        let answer: QueryAnswer =
            parse_json_response(&response).map_err(|e| AgentError::Llm(e.to_string()))?;

        Ok((answer, usage))
    }

    async fn generate_refined_answer(
        &self,
        question: &str,
        context: &str,
        draft: &QueryAnswer,
    ) -> Result<(QueryAnswer, Option<TokenUsage>), AgentError> {
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(QUERY_SYSTEM_PROMPT),
                ChatMessage::user(query_refinement_user_prompt(
                    question,
                    context,
                    &draft.answer,
                )),
            ],
            max_tokens: 4096,
            temperature: 0.2,
            json_mode: true,
        };

        let response = self
            .llm
            .generate(params)
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;
        let usage = response.usage.clone();
        let answer: QueryAnswer =
            parse_json_response(&response).map_err(|e| AgentError::Llm(e.to_string()))?;
        Ok((answer, usage))
    }

    /// Legacy fallback query using simple text search when representation fails.
    async fn query_legacy(
        &self,
        question: &str,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
        _relation_repo: &MemoryRelationRepository<'_>,
    ) -> Result<QueryAnswer, AgentError> {
        let limit = self.config.query_context_limit;
        let memories = memory_repo
            .search_by_text_memories(namespace_id, question, limit as i32, false)
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;

        let context = memories
            .iter()
            .map(|m| format!("[Memory #{}] {}", m.id, m.content))
            .collect::<Vec<String>>()
            .join("\n\n");

        let (answer, _) = self.generate_answer(question, &context).await?;
        Ok(answer)
    }
}

fn introspection_request(request: &WorkingRepresentationRequest) -> WorkingRepresentationRequest {
    let mut overfetch = request.clone();
    overfetch.max_items = request
        .max_items
        .saturating_mul(3)
        .max(request.max_items + 8);
    overfetch
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Build per-memory lineage records with phase detection and relevance scores.
fn build_lineages(bucketed: &[BucketedMemory]) -> Vec<MemoryLineage> {
    let analyzer = nexus_lephase::PhaseAnalyzer::new();

    bucketed
        .iter()
        .map(|bm| {
            let analysis = analyzer.analyze(&bm.memory);
            MemoryLineage {
                memory_id: bm.memory.id,
                bucket: bm.bucket,
                phase: analysis.phase.phase_type.to_string(),
                relevance_score: bm
                    .memory
                    .relevance_score
                    .or(bm.memory.similarity_score)
                    .or(Some((bm.blended_score / 100.0).clamp(0.0, 1.0))),
            }
        })
        .collect()
}

fn should_refine_answer(question: &str, answer: &QueryAnswer, bucketed: &[BucketedMemory]) -> bool {
    if bucketed.is_empty() {
        return false;
    }

    let lower_question = question.to_ascii_lowercase();
    let question_word_count = question.split_whitespace().count();
    let question_complex = question.len() > 120
        || question_word_count > 18
        || [
            "why",
            "how",
            "compare",
            "contrast",
            "tradeoff",
            "timeline",
            "relationship",
            "explain",
            "summarize",
        ]
        .iter()
        .any(|needle| lower_question.contains(needle));
    let weak_answer =
        answer.confidence < 0.72 || answer.citations.is_empty() || answer.answer.trim().len() < 40;
    let broad_context = bucketed.len() >= 6;
    let contradiction_present = bucketed
        .iter()
        .any(|memory| memory.bucket == crate::MemoryBucket::Contradictions);

    (weak_answer && (question_complex || broad_context || contradiction_present))
        || (question_complex && answer.confidence < 0.82 && broad_context)
}

fn select_better_answer(initial: QueryAnswer, refined: QueryAnswer) -> QueryAnswer {
    if answer_quality(&refined) >= answer_quality(&initial) {
        refined
    } else {
        initial
    }
}

fn answer_quality(answer: &QueryAnswer) -> f32 {
    let citation_bonus = (answer.citations.len().min(4) as f32) * 0.05;
    let answer_length_bonus = if answer.answer.trim().len() >= 40 {
        0.02
    } else {
        0.0
    };
    answer.confidence + citation_bonus + answer_length_bonus
}

/// Scan the lephase-formatted context and append bucket provenance to any
/// `[Memory #N]` line where we have lineage data.
fn annotate_with_buckets(formatted: &str, lineage_map: &HashMap<i64, &MemoryLineage>) -> String {
    let mut out = String::with_capacity(formatted.len() + 256);
    for line in formatted.lines() {
        out.push_str(line);
        if let Some(id_str) = line.strip_prefix("[Memory #") {
            if let Some(end) = id_str.find(']') {
                if let Ok(id) = id_str[..end].parse::<i64>() {
                    if let Some(lineage) = lineage_map.get(&id) {
                        out.push_str(&format!(" (bucket: {})", lineage.bucket));
                    }
                }
            }
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Introspection builder
// ---------------------------------------------------------------------------

const CONTENT_PREVIEW_LEN: usize = 80;
const MAX_EXCLUDED_CANDIDATES: usize = 20;
const MAX_RELEVANT_REFLECTIONS: usize = 10;

/// Build full introspection from ranked results.
///
/// This is a pure structural analysis — no LLM calls.
pub(crate) async fn build_introspection(
    ranked: &RankedResult,
    question: &str,
    request: &WorkingRepresentationRequest,
    memory_repo: &MemoryRepository,
    started: Instant,
) -> Result<QueryIntrospection, AgentError> {
    let included = build_inclusion_reasons(&ranked.included, request);
    let excluded = build_excluded_candidates(&ranked.excluded);
    let bucket_stats = build_bucket_stats(&ranked.included, &ranked.excluded);
    let relevant_reflections = fetch_relevant_reflections(question, request, memory_repo).await?;

    let pipeline_latency_ms = Some(started.elapsed().as_millis() as u64);
    let representation_config = Some(RepresentationConfigSnapshot {
        max_items: request.max_items,
        include_raw: request.include_raw,
        include_digests: request.include_digests,
        include_semantic: request.include_semantic,
        include_derived: request.include_derived,
        include_contradictions: request.include_contradictions,
    });

    Ok(QueryIntrospection {
        included,
        excluded_candidates: excluded,
        relevant_reflections,
        bucket_stats,
        pipeline_latency_ms,
        representation_config,
    })
}

fn build_inclusion_reasons(
    bucketed: &[BucketedMemory],
    request: &WorkingRepresentationRequest,
) -> Vec<InclusionReason> {
    let analyzer = nexus_lephase::PhaseAnalyzer::new();

    bucketed
        .iter()
        .map(|bm| {
            let analysis = analyzer.analyze(&bm.memory);
            let relevance = bm
                .memory
                .relevance_score
                .or(bm.memory.similarity_score)
                .or(Some((bm.blended_score / 100.0).clamp(0.0, 1.0)));
            let signals = extract_inclusion_signals(bm, request);

            InclusionReason {
                memory_id: bm.memory.id,
                bucket: bm.bucket,
                phase: analysis.phase.phase_type.to_string(),
                relevance_score: relevance,
                blended_score: bm.blended_score,
                reason: classify_inclusion_reason(bm, relevance),
                signals,
            }
        })
        .collect()
}

/// Extract structured signals explaining why a memory was included.
///
/// Decomposes the blended score factors into human-readable signals.
fn extract_inclusion_signals(
    bm: &BucketedMemory,
    request: &WorkingRepresentationRequest,
) -> Vec<InclusionSignal> {
    use crate::util::CognitionSnapshot;
    use nexus_core::CognitiveLevel;

    let memory = &bm.memory;
    let snapshot = CognitionSnapshot::from_memory(memory);
    let mut signals = Vec::new();

    // Recency signal
    let age_hours = (chrono::Utc::now() - memory.created_at).num_hours();
    let recency_desc = match age_hours {
        h if h <= 1 => "Created within the last hour".to_string(),
        h if h <= 6 => format!("Created {h}h ago"),
        h if h <= 24 => format!("Created {h}h ago"),
        h if h <= 72 => format!("Created {h}h ago"),
        h if h <= 168 => format!("Created {}d ago", h / 24),
        _ => format!("Created {}d ago", age_hours / 24),
    };
    let recency_weight = match age_hours {
        h if h <= 1 => 1.0,
        h if h <= 6 => 0.8,
        h if h <= 24 => 0.6,
        h if h <= 72 => 0.35,
        h if h <= 168 => 0.15,
        _ => 0.0,
    };
    signals.push(InclusionSignal {
        signal_type: "recency".to_string(),
        description: recency_desc,
        weight_contribution: recency_weight,
    });

    // Cognitive level signal
    let level_desc = match snapshot.level {
        CognitiveLevel::Explicit => "Explicit factual memory",
        CognitiveLevel::Derived => "System-derived insight",
        CognitiveLevel::Contradiction => "Detected contradiction",
        CognitiveLevel::SummaryShort => "Short-form session digest",
        CognitiveLevel::SummaryLong => "Long-form session digest",
        CognitiveLevel::Raw => "Raw activity record",
    };
    let confidence = snapshot.confidence.unwrap_or(0.75).clamp(0.0, 1.0);
    signals.push(InclusionSignal {
        signal_type: "cognitive_level".to_string(),
        description: format!("{level_desc} (confidence: {:.2})", confidence),
        weight_contribution: confidence,
    });

    // Semantic similarity signal (only for semantic bucket)
    if matches!(bm.bucket, MemoryBucket::Semantic) {
        let similarity = memory
            .relevance_score
            .or(memory.similarity_score)
            .unwrap_or_default();
        if similarity > 0.0 {
            signals.push(InclusionSignal {
                signal_type: "semantic_similarity".to_string(),
                description: format!("Embedding similarity score: {:.3}", similarity),
                weight_contribution: similarity,
            });
        }
    }

    // Perspective match signal
    if let Some(ref req_perspective) = request.perspective {
        if let Some(ref mem_perspective) = snapshot.perspective {
            let observer_match = mem_perspective.observer == req_perspective.observer;
            let subject_match = mem_perspective.subject == req_perspective.subject;
            if observer_match || subject_match {
                let match_kind = if observer_match && subject_match {
                    "full perspective match"
                } else if observer_match {
                    "observer match"
                } else {
                    "subject match"
                };
                let weight = if observer_match && subject_match {
                    1.0
                } else {
                    0.5
                };
                signals.push(InclusionSignal {
                    signal_type: "perspective_match".to_string(),
                    description: match_kind.to_string(),
                    weight_contribution: weight,
                });
            }
        }
    }

    // Reinforcement signal
    if snapshot.times_reinforced > 0 {
        let reinforced = ((snapshot.times_reinforced as f32) / 5.0).min(1.0);
        signals.push(InclusionSignal {
            signal_type: "reinforcement".to_string(),
            description: format!("Reinforced {} time(s)", snapshot.times_reinforced),
            weight_contribution: reinforced,
        });
    }

    // Bucket boost signal
    let bucket_desc = match bm.bucket {
        MemoryBucket::Digests => Some("Digest bucket — prioritized for context grounding"),
        MemoryBucket::Contradictions => {
            Some("Contradiction bucket — surfaced for conflict awareness")
        }
        MemoryBucket::Derived => Some("Derived bucket — system insight priority"),
        MemoryBucket::Semantic => None,
        MemoryBucket::Recent => None,
    };
    if let Some(desc) = bucket_desc {
        signals.push(InclusionSignal {
            signal_type: "bucket_boost".to_string(),
            description: desc.to_string(),
            weight_contribution: 0.0,
        });
    }

    signals
}

fn classify_inclusion_reason(bm: &BucketedMemory, relevance: Option<f32>) -> String {
    match bm.bucket {
        MemoryBucket::Semantic => {
            let score_str = relevance
                .map(|s| format!("{:.2}", s))
                .unwrap_or_else(|| "N/A".to_string());
            format!("Semantic match (score: {}) in semantic bucket", score_str)
        }
        MemoryBucket::Digests => "Session digest selected for context grounding".to_string(),
        MemoryBucket::Derived => {
            format!(
                "Reinforced derived insight (blended: {:.2}) in derived bucket",
                bm.blended_score
            )
        }
        MemoryBucket::Recent => {
            format!(
                "Recent memory (blended: {:.2}) in recent bucket",
                bm.blended_score
            )
        }
        MemoryBucket::Contradictions => {
            "Contradiction detected — surfaced for conflict awareness".to_string()
        }
    }
}

fn build_excluded_candidates(excluded: &[RankedExcludedMemory]) -> Vec<ExcludedCandidate> {
    excluded
        .iter()
        .take(MAX_EXCLUDED_CANDIDATES)
        .map(|excluded| ExcludedCandidate {
            memory_id: excluded.memory.id,
            bucket: excluded.bucket,
            blended_score: excluded.blended_score,
            reason: excluded.reason.clone(),
            content_preview: truncate_str(&excluded.memory.content, CONTENT_PREVIEW_LEN),
        })
        .collect()
}

fn build_bucket_stats(
    included: &[BucketedMemory],
    excluded: &[RankedExcludedMemory],
) -> Vec<BucketIntrospectionStats> {
    let mut all_buckets: Vec<(MemoryBucket, usize, usize)> = Vec::new();

    for bm in included {
        if let Some(entry) = all_buckets.iter_mut().find(|(b, _, _)| *b == bm.bucket) {
            entry.1 += 1;
        } else {
            all_buckets.push((bm.bucket, 1, 0));
        }
    }

    for excluded in excluded {
        if let Some(entry) = all_buckets
            .iter_mut()
            .find(|(bucket, _, _)| *bucket == excluded.bucket)
        {
            entry.2 += 1;
        } else {
            all_buckets.push((excluded.bucket, 0, 1));
        }
    }

    all_buckets
        .into_iter()
        .map(|(bucket, inc, exc)| BucketIntrospectionStats {
            bucket,
            fetched: inc + exc,
            included: inc,
            excluded: exc,
        })
        .collect()
}

async fn fetch_relevant_reflections(
    question: &str,
    request: &WorkingRepresentationRequest,
    memory_repo: &MemoryRepository,
) -> Result<Vec<RelevantReflection>, AgentError> {
    let query_terms = tokenize_query(question);
    let filters = nexus_storage::repository::ListMemoryFilters {
        category: None,
        since: None,
        until: None,
        content_like: None,
        include_raw: false,
        limit: (MAX_RELEVANT_REFLECTIONS as i64).max(10) * 8,
        offset: 0,
    };

    let all = memory_repo
        .list_filtered(request.namespace_id, filters)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    let mut ranked: Vec<(f32, Memory)> = all
        .into_iter()
        .filter(|m| {
            if !reflection_matches_request_scope(m, request) {
                return false;
            }
            let level = m
                .metadata
                .get("cognitive")
                .and_then(|c| c.get("level"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            level == "derived" || level == "contradiction"
        })
        .map(|m| {
            let content_terms = tokenize_query(&m.content);
            let overlap = if query_terms.is_empty() {
                0.0
            } else {
                let shared = query_terms.intersection(&content_terms).count() as f32;
                shared / query_terms.len() as f32
            };
            let confidence = m
                .metadata
                .get("cognitive")
                .and_then(|c| c.get("confidence"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.75) as f32;
            let recency = {
                let age_hours = (chrono::Utc::now() - m.created_at).num_hours();
                match age_hours {
                    h if h <= 1 => 1.0,
                    h if h <= 6 => 0.8,
                    h if h <= 24 => 0.6,
                    h if h <= 72 => 0.35,
                    h if h <= 168 => 0.15,
                    _ => 0.0,
                }
            };
            let score = if query_terms.is_empty() {
                0.5 * confidence + 0.5 * recency
            } else {
                0.65 * overlap + 0.20 * confidence + 0.15 * recency
            };
            (score, m)
        })
        .filter(|(score, _)| query_terms.is_empty() || *score > 0.0)
        .collect();
    ranked.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.1.created_at.cmp(&left.1.created_at))
    });

    let reflections: Vec<RelevantReflection> = ranked
        .into_iter()
        .take(MAX_RELEVANT_REFLECTIONS)
        .map(|(_, m)| {
            let reflection_type = m
                .metadata
                .get("cognitive")
                .and_then(|c| c.get("level"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let confidence = m
                .metadata
                .get("cognitive")
                .and_then(|c| c.get("confidence"))
                .and_then(|v| v.as_f64());

            RelevantReflection {
                memory_id: m.id,
                reflection_type,
                content_preview: truncate_str(&m.content, CONTENT_PREVIEW_LEN),
                confidence,
                created_at: m.created_at.to_rfc3339(),
            }
        })
        .collect();

    Ok(reflections)
}

fn reflection_matches_request_scope(
    memory: &Memory,
    request: &WorkingRepresentationRequest,
) -> bool {
    let Some(request_perspective) = request.perspective.as_ref() else {
        return true;
    };

    let snapshot = CognitionSnapshot::from_memory(memory);
    let Some(memory_perspective) = snapshot.perspective.as_ref() else {
        return false;
    };

    if memory_perspective.observer != request_perspective.observer
        || memory_perspective.subject != request_perspective.subject
    {
        return false;
    }

    match request_perspective.session_key.as_deref() {
        Some(session_key) => memory_perspective.session_key.as_deref() == Some(session_key),
        None => true,
    }
}

fn tokenize_query(text: &str) -> std::collections::BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter_map(|segment| {
            let term = segment.trim().to_ascii_lowercase();
            (term.len() >= 3).then_some(term)
        })
        .collect()
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

// ---------------------------------------------------------------------------
// Standalone introspection (no LLM required)
// ---------------------------------------------------------------------------

/// Introspect query ranking decisions without requiring an LLM client.
///
/// This is the recommended entry point for observability surfaces (web API,
/// CLI) that need introspection data but don't need to generate answers.
/// Builds the same overfetched representation used by service-backed
/// introspection so near-miss/excluded-candidate explanations are consistent
/// across CLI, web, and agent surfaces.
pub async fn introspect_query(
    request: &WorkingRepresentationRequest,
    question: &str,
    memory_repo: &MemoryRepository,
) -> Result<QueryIntrospection, AgentError> {
    introspect_query_with_representation_service(
        &RepresentationService::new(),
        request,
        question,
        memory_repo,
    )
    .await
}

async fn introspect_query_with_representation_service(
    representation: &RepresentationService,
    request: &WorkingRepresentationRequest,
    question: &str,
    memory_repo: &MemoryRepository,
) -> Result<QueryIntrospection, AgentError> {
    let started = Instant::now();
    let overfetch_request = introspection_request(request);
    let representation = representation
        .build(&overfetch_request, memory_repo)
        .await
        .map_err(|e| AgentError::Storage(e.to_string()))?;

    let ranked = flatten_ranked_representation_with_excluded(representation, request);

    build_introspection(&ranked, question, request, memory_repo, started).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExclusionReason, MemoryBucket};
    use async_trait::async_trait;
    use nexus_core::{CognitiveLevel, CognitiveMetadata, MemoryCategory, PerspectiveKey};
    use nexus_llm::GenerateResponse;
    use nexus_storage::repository::{
        MemoryRelationRepository, MemoryRepository, NamespaceRepository, StoreDigestParams,
        StoreMemoryParams,
    };
    use sqlx::sqlite::SqlitePoolOptions;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct MockLlmClient {
        responses: Mutex<VecDeque<nexus_llm::Result<GenerateResponse>>>,
        calls: Mutex<Vec<GenerateParams>>,
    }

    impl MockLlmClient {
        fn new(responses: Vec<nexus_llm::Result<GenerateResponse>>) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().expect("mock calls poisoned").len()
        }

        fn user_messages(&self) -> Vec<String> {
            self.calls
                .lock()
                .expect("mock calls poisoned")
                .iter()
                .flat_map(|params| params.messages.iter())
                .filter(|message| message.role == "user")
                .map(|message| message.content.clone())
                .collect()
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn generate(&self, params: GenerateParams) -> nexus_llm::Result<GenerateResponse> {
            self.calls.lock().expect("mock calls poisoned").push(params);
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

    async fn setup_repo() -> (sqlx::SqlitePool, MemoryRepository, i64, PerspectiveKey) {
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
            .get_or_create("query-test", "query-test")
            .await
            .unwrap();
        let perspective =
            PerspectiveKey::new("claude-code", "claude-code", Some("session-1".to_string()));
        (
            pool.clone(),
            MemoryRepository::new(pool),
            namespace.id,
            perspective,
        )
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

    async fn store_raw_memory(
        repo: &MemoryRepository,
        namespace_id: i64,
        content: &str,
        perspective: &PerspectiveKey,
    ) -> Memory {
        repo.store(StoreMemoryParams {
            namespace_id,
            content,
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": true,
                "cognitive": {
                    "level": "raw",
                    "observer": perspective.observer,
                    "subject": perspective.subject,
                    "session_key": perspective.session_key,
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap()
    }

    fn answer_response(answer: &str, confidence: f32, citations: &[i64]) -> GenerateResponse {
        let citations_json: Vec<serde_json::Value> = citations
            .iter()
            .map(|memory_id| {
                serde_json::json!({
                    "memory_id": memory_id,
                    "title": format!("Memory {}", memory_id),
                    "excerpt": format!("Excerpt {}", memory_id)
                })
            })
            .collect();
        GenerateResponse {
            content: serde_json::json!({
                "answer": answer,
                "citations": citations_json,
                "confidence": confidence
            })
            .to_string(),
            model: "mock-model".to_string(),
            usage: None,
        }
    }

    fn test_memory(id: i64, content: &str) -> Memory {
        Memory {
            id,
            namespace_id: 1,
            content: content.to_string(),
            category: nexus_core::MemoryCategory::Facts,
            labels: Vec::new(),
            metadata: serde_json::json!({}),
            ..Memory::default()
        }
    }

    fn test_memory_with_relevance(id: i64, content: &str, relevance: f32) -> Memory {
        Memory {
            id,
            namespace_id: 1,
            content: content.to_string(),
            category: nexus_core::MemoryCategory::Facts,
            labels: Vec::new(),
            metadata: serde_json::json!({}),
            relevance_score: Some(relevance),
            ..Memory::default()
        }
    }

    #[test]
    fn test_build_lineages_detects_phases() {
        let bucketed = vec![
            BucketedMemory {
                memory: test_memory(1, "Plan the next sprint tasks"),
                bucket: MemoryBucket::Recent,
                blended_score: 0.91,
            },
            BucketedMemory {
                memory: test_memory(2, "Implement the auth feature code"),
                bucket: MemoryBucket::Semantic,
                blended_score: 0.83,
            },
            BucketedMemory {
                memory: test_memory(3, "Fix the bug in error handling"),
                bucket: MemoryBucket::Derived,
                blended_score: 0.88,
            },
        ];

        let lineages = build_lineages(&bucketed);
        assert_eq!(lineages.len(), 3);

        assert_eq!(lineages[0].memory_id, 1);
        assert_eq!(lineages[0].bucket, MemoryBucket::Recent);
        assert_eq!(lineages[0].phase, "planning");

        assert_eq!(lineages[1].memory_id, 2);
        assert_eq!(lineages[1].bucket, MemoryBucket::Semantic);
        assert_eq!(lineages[1].phase, "execution");

        assert_eq!(lineages[2].memory_id, 3);
        assert_eq!(lineages[2].bucket, MemoryBucket::Derived);
        assert_eq!(lineages[2].phase, "debugging");
    }

    #[test]
    fn test_build_lineages_captures_relevance_scores() {
        let bucketed = vec![BucketedMemory {
            memory: test_memory_with_relevance(42, "test content", 0.87),
            bucket: MemoryBucket::Semantic,
            blended_score: 0.95,
        }];

        let lineages = build_lineages(&bucketed);
        assert_eq!(lineages[0].relevance_score, Some(0.87));
    }

    #[test]
    fn test_build_lineages_falls_back_to_similarity_score() {
        let bucketed = vec![BucketedMemory {
            memory: Memory {
                id: 1,
                similarity_score: Some(0.72),
                ..test_memory(1, "test")
            },
            bucket: MemoryBucket::Semantic,
            blended_score: 0.79,
        }];

        let lineages = build_lineages(&bucketed);
        assert_eq!(lineages[0].relevance_score, Some(0.72));
    }

    #[test]
    fn test_should_refine_answer_for_complex_low_confidence_answer() {
        let bucketed = vec![
            BucketedMemory {
                memory: test_memory(1, "Digest"),
                bucket: MemoryBucket::Digests,
                blended_score: 0.88,
            },
            BucketedMemory {
                memory: test_memory(2, "Contradiction"),
                bucket: MemoryBucket::Contradictions,
                blended_score: 0.74,
            },
        ];
        let answer = QueryAnswer {
            answer: "Maybe.".to_string(),
            citations: Vec::new(),
            confidence: 0.55,
            ..Default::default()
        };

        assert!(should_refine_answer(
            "Explain the tradeoff timeline and contradictions in this session",
            &answer,
            &bucketed,
        ));
    }

    #[test]
    fn test_should_not_refine_simple_high_confidence_answer() {
        let bucketed = vec![BucketedMemory {
            memory: test_memory(1, "Recent memory"),
            bucket: MemoryBucket::Recent,
            blended_score: 0.82,
        }];
        let answer = QueryAnswer {
            answer: "The active provider is Gemini and the setting is already applied.".to_string(),
            citations: vec![crate::types::MemoryCitation {
                memory_id: 1,
                title: "Provider".to_string(),
                excerpt: "Gemini is active".to_string(),
            }],
            confidence: 0.91,
            ..Default::default()
        };

        assert!(!should_refine_answer(
            "What is the active provider?",
            &answer,
            &bucketed,
        ));
    }

    #[test]
    fn test_select_better_answer_prefers_cited_refined_answer() {
        let initial = QueryAnswer {
            answer: "Short".to_string(),
            citations: Vec::new(),
            confidence: 0.78,
            ..Default::default()
        };
        let refined = QueryAnswer {
            answer: "Longer answer with supporting detail and an explicit citation.".to_string(),
            citations: vec![crate::types::MemoryCitation {
                memory_id: 3,
                title: "Evidence".to_string(),
                excerpt: "Supporting excerpt".to_string(),
            }],
            confidence: 0.76,
            ..Default::default()
        };

        let selected = select_better_answer(initial, refined);
        assert_eq!(selected.citations.len(), 1);
    }

    // --- annotate_with_buckets ---

    #[test]
    fn test_annotate_with_buckets_appends_provenance() {
        let lineage = MemoryLineage {
            memory_id: 1,
            bucket: MemoryBucket::Semantic,
            phase: "execution".to_string(),
            relevance_score: Some(0.9),
        };
        let mut lineage_map = HashMap::new();
        lineage_map.insert(1, &lineage);

        let formatted = "[Memory #1] Implement feature\nSome content\n";
        let annotated = annotate_with_buckets(formatted, &lineage_map);

        assert!(annotated.contains("[Memory #1] Implement feature (bucket: semantic)"));
        assert!(annotated.contains("Some content"));
        // Only the Memory line should get annotated, not "Some content".
        assert_eq!(
            annotated.matches("(bucket:").count(),
            1,
            "expected exactly one bucket annotation"
        );
    }

    #[test]
    fn test_annotate_skips_unknown_ids() {
        let lineage_map = HashMap::new();
        let formatted = "[Memory #1] Implement feature\n";
        let annotated = annotate_with_buckets(formatted, &lineage_map);
        // `lines()` strips the trailing newline, then we append one per line.
        assert_eq!(annotated, "[Memory #1] Implement feature\n");
    }

    // --- MemoryBucket Display ---

    #[test]
    fn test_memory_bucket_display() {
        assert_eq!(MemoryBucket::Digests.to_string(), "digests");
        assert_eq!(MemoryBucket::Recent.to_string(), "recent");
        assert_eq!(MemoryBucket::Semantic.to_string(), "semantic");
        assert_eq!(MemoryBucket::Derived.to_string(), "derived");
        assert_eq!(MemoryBucket::Contradictions.to_string(), "contradictions");
    }

    #[tokio::test]
    async fn test_query_service_empty_working_set_uses_no_relevant_memories_prompt() {
        let (pool, repo, namespace_id, _perspective) = setup_repo().await;
        let relation_repo = MemoryRelationRepository::new(&pool);
        let llm = Arc::new(MockLlmClient::new(vec![Ok(answer_response(
            "No memory matched.",
            0.92,
            &[],
        ))]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query("What happened?", namespace_id, &repo, &relation_repo)
            .await
            .unwrap();

        assert_eq!(answer.answer, "No memory matched.");
        assert!(answer.lineages.is_empty());
        assert_eq!(llm.call_count(), 1);
        let prompts = llm.user_messages();
        assert!(prompts
            .iter()
            .any(|prompt| prompt.contains("No relevant memories found.")));
    }

    #[tokio::test]
    async fn test_query_service_excludes_raw_noise_by_default_and_attaches_lineages() {
        let (pool, repo, namespace_id, perspective) = setup_repo().await;
        let relation_repo = MemoryRelationRepository::new(&pool);
        let explicit = store_memory(
            &repo,
            namespace_id,
            "Explicit observation about retrieval ranking.",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        let raw = store_raw_memory(
            &repo,
            namespace_id,
            "raw hook payload about retrieval ranking",
            &perspective,
        )
        .await;
        let llm = Arc::new(MockLlmClient::new(vec![Ok(answer_response(
            "Ranking was improved.",
            0.9,
            &[explicit.id],
        ))]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query(
                "What changed in retrieval ranking?",
                namespace_id,
                &repo,
                &relation_repo,
            )
            .await
            .unwrap();

        assert!(!answer.lineages.is_empty());
        assert!(answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == explicit.id));
        assert!(!answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == raw.id));
        let prompts = llm.user_messages();
        assert!(prompts
            .iter()
            .any(|prompt| prompt.contains("Explicit observation about retrieval ranking.")));
        assert!(!prompts
            .iter()
            .any(|prompt| prompt.contains("raw hook payload about retrieval ranking")));
    }

    #[tokio::test]
    async fn test_query_with_representation_can_include_raw_when_requested() {
        let (pool, repo, namespace_id, perspective) = setup_repo().await;
        let relation_repo = MemoryRelationRepository::new(&pool);
        store_memory(
            &repo,
            namespace_id,
            "Explicit observation about hook routing.",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        let raw = store_raw_memory(
            &repo,
            namespace_id,
            "raw hook payload about hook routing",
            &perspective,
        )
        .await;
        let llm = Arc::new(MockLlmClient::new(vec![Ok(answer_response(
            "The hook routing is visible through the explicit and raw activity records.",
            0.88,
            &[raw.id],
        ))]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query_with_representation(
                "What does the hook traffic show?",
                WorkingRepresentationRequest {
                    namespace_id,
                    perspective: None,
                    query: Some("hook routing".to_string()),
                    max_items: 10,
                    include_raw: true,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
                &relation_repo,
            )
            .await
            .unwrap();

        assert!(answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == raw.id));
        let prompts = llm.user_messages();
        assert!(prompts
            .iter()
            .any(|prompt| prompt.contains("raw hook payload about hook routing")));
    }

    #[tokio::test]
    async fn test_query_service_mixed_cognition_outputs_attach_multiple_lineages_and_phase_context()
    {
        let (pool, repo, namespace_id, perspective) = setup_repo().await;
        let relation_repo = MemoryRelationRepository::new(&pool);
        let digest = store_memory(
            &repo,
            namespace_id,
            "Digest summary of the session.",
            CognitiveLevel::SummaryShort,
            &perspective,
        )
        .await;
        repo.store_digest(StoreDigestParams {
            namespace_id,
            session_key: "session-1",
            digest_kind: "short",
            memory_id: digest.id,
            start_memory_id: Some(digest.id),
            end_memory_id: Some(digest.id),
            token_count: 42,
        })
        .await
        .unwrap();
        let derived = store_memory(
            &repo,
            namespace_id,
            "Derived insight about the refactor.",
            CognitiveLevel::Derived,
            &perspective,
        )
        .await;
        let contradiction = store_memory(
            &repo,
            namespace_id,
            "Contradiction between old and new recall paths.",
            CognitiveLevel::Contradiction,
            &perspective,
        )
        .await;
        let llm = Arc::new(MockLlmClient::new(vec![Ok(answer_response(
            "The session had a digest, an insight, and a contradiction.",
            0.86,
            &[digest.id, derived.id, contradiction.id],
        ))]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query_with_representation(
                "Explain the session timeline and contradictions in context.",
                WorkingRepresentationRequest {
                    namespace_id,
                    perspective: Some(perspective.clone()),
                    query: Some("timeline contradiction insight".to_string()),
                    max_items: 10,
                    include_raw: false,
                    include_recent: false,
                    include_semantic: false,
                    include_derived: true,
                    include_digests: true,
                    include_contradictions: true,
                    ..WorkingRepresentationRequest::default()
                },
                &repo,
                &relation_repo,
            )
            .await
            .unwrap();

        assert!(answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == digest.id));
        assert!(answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == derived.id));
        assert!(answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == contradiction.id));
        let prompts = llm.user_messages();
        assert!(prompts
            .iter()
            .any(|prompt| prompt.contains("Digest summary of the session.")));
        assert!(prompts
            .iter()
            .any(|prompt| prompt.contains("Derived insight about the refactor.")));
        assert!(prompts
            .iter()
            .any(|prompt| prompt.contains("Contradiction between old and new recall paths.")));
        assert!(!prompts.iter().any(|prompt| prompt.contains("Summary:")));
    }

    #[tokio::test]
    async fn test_query_service_representation_beats_old_like_recall_for_session_digest_context() {
        let (pool, repo, namespace_id, perspective) = setup_repo().await;
        let relation_repo = MemoryRelationRepository::new(&pool);

        store_memory(
            &repo,
            namespace_id,
            "Configured Gemini as the active provider and preserved installer env settings.",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        let digest = store_memory(
            &repo,
            namespace_id,
            "Digest summary: migration timeline of the provider switch, installer preservation, and bounded dreaming rollout.",
            CognitiveLevel::SummaryShort,
            &perspective,
        )
        .await;
        repo.store_digest(StoreDigestParams {
            namespace_id,
            session_key: perspective.session_key.as_deref().unwrap_or("session-1"),
            digest_kind: "short",
            memory_id: digest.id,
            start_memory_id: Some(digest.id),
            end_memory_id: Some(digest.id),
            token_count: 64,
        })
        .await
        .unwrap();
        let contradiction = store_memory(
            &repo,
            namespace_id,
            "Contradiction note: old recall missed the migration timeline while representation-first recall surfaced it.",
            CognitiveLevel::Contradiction,
            &perspective,
        )
        .await;

        let question =
            "What does the migration timeline summary say about the provider switch rollout?";
        let old_like_hits = repo
            .search_by_text(namespace_id, question, 10, false)
            .await
            .unwrap();
        assert!(
            old_like_hits.is_empty(),
            "legacy LIKE recall should miss the natural-language question when no memory contains the full string"
        );

        let llm = Arc::new(MockLlmClient::new(vec![Ok(answer_response(
            "The migration timeline shows a provider switch digest with rollout context and a contradiction note about the old recall path missing it.",
            0.9,
            &[digest.id, contradiction.id],
        ))]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query_with_representation(
                question,
                WorkingRepresentationRequest {
                    namespace_id,
                    perspective: Some(perspective.clone()),
                    query: Some(question.to_string()),
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
                &relation_repo,
            )
            .await
            .unwrap();

        assert!(answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == digest.id));
        assert!(answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == contradiction.id));

        let prompts = llm.user_messages();
        assert!(prompts.iter().any(|prompt| prompt.contains(
            "Digest summary: migration timeline of the provider switch, installer preservation, and bounded dreaming rollout."
        )));
        assert!(prompts.iter().any(|prompt| prompt.contains(
            "Contradiction note: old recall missed the migration timeline while representation-first recall surfaced it."
        )));
    }

    #[tokio::test]
    async fn test_query_service_refinement_triggers_second_llm_call() {
        let (pool, repo, namespace_id, perspective) = setup_repo().await;
        let relation_repo = MemoryRelationRepository::new(&pool);
        for idx in 0..6 {
            let content = format!("Execution detail {idx} for the migration timeline.");
            store_memory(
                &repo,
                namespace_id,
                &content,
                CognitiveLevel::Explicit,
                &perspective,
            )
            .await;
        }
        let llm = Arc::new(MockLlmClient::new(vec![
            Ok(answer_response("Maybe.", 0.55, &[])),
            Ok(answer_response(
                "The migration timeline shows several execution details with stronger support.",
                0.9,
                &[1],
            )),
        ]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query(
                "Explain the tradeoff timeline and relationship across the migration work.",
                namespace_id,
                &repo,
                &relation_repo,
            )
            .await
            .unwrap();

        assert_eq!(
            answer.answer,
            "The migration timeline shows several execution details with stronger support."
        );
        assert_eq!(llm.call_count(), 2);
    }

    #[tokio::test]
    async fn test_query_service_auto_includes_cross_namespace_alias_digest_without_perspective() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        nexus_storage::migrations::run_migrations(&pool)
            .await
            .unwrap();

        let namespace_repo = NamespaceRepository::new(pool.clone());
        let primary = namespace_repo
            .get_or_create("claude-code", "claude-code")
            .await
            .unwrap();
        let alias = namespace_repo
            .get_or_create("claude", "claude")
            .await
            .unwrap();
        let unrelated = namespace_repo
            .get_or_create("codex", "codex")
            .await
            .unwrap();

        let repo = MemoryRepository::new(pool.clone());
        let relation_repo = MemoryRelationRepository::new(&pool);
        let perspective =
            PerspectiveKey::new("claude-code", "claude-code", Some("session-1".to_string()));

        let alias_digest = store_memory(
            &repo,
            alias.id,
            "Alias digest summary: the claude namespace captured the provider rollout timeline.",
            CognitiveLevel::SummaryShort,
            &perspective,
        )
        .await;
        repo.store_digest(StoreDigestParams {
            namespace_id: alias.id,
            session_key: "session-1",
            digest_kind: "short",
            memory_id: alias_digest.id,
            start_memory_id: Some(alias_digest.id),
            end_memory_id: Some(alias_digest.id),
            token_count: 48,
        })
        .await
        .unwrap();

        let unrelated_digest = store_memory(
            &repo,
            unrelated.id,
            "Unrelated codex digest summary: refactor unrelated CLI parsing bug.",
            CognitiveLevel::SummaryShort,
            &perspective,
        )
        .await;
        repo.store_digest(StoreDigestParams {
            namespace_id: unrelated.id,
            session_key: "session-1",
            digest_kind: "short",
            memory_id: unrelated_digest.id,
            start_memory_id: Some(unrelated_digest.id),
            end_memory_id: Some(unrelated_digest.id),
            token_count: 41,
        })
        .await
        .unwrap();

        let llm = Arc::new(MockLlmClient::new(vec![Ok(answer_response(
            "The provider rollout timeline is preserved in the alias digest.",
            0.93,
            &[alias_digest.id],
        ))]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query(
                "What does the provider rollout timeline say?",
                primary.id,
                &repo,
                &relation_repo,
            )
            .await
            .unwrap();

        assert!(answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == alias_digest.id));
        assert!(!answer
            .lineages
            .iter()
            .any(|lineage| lineage.memory_id == unrelated_digest.id));

        let prompts = llm.user_messages();
        assert!(prompts.iter().any(|prompt| prompt.contains(
            "Alias digest summary: the claude namespace captured the provider rollout timeline."
        )));
        assert!(!prompts.iter().any(|prompt| prompt
            .contains("Unrelated codex digest summary: refactor unrelated CLI parsing bug.")));
    }

    #[tokio::test]
    async fn test_query_service_simple_answer_stays_single_call() {
        let (pool, repo, namespace_id, perspective) = setup_repo().await;
        let relation_repo = MemoryRelationRepository::new(&pool);
        let explicit = store_memory(
            &repo,
            namespace_id,
            "Explicit note about the active provider switch.",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        let llm = Arc::new(MockLlmClient::new(vec![Ok(answer_response(
            "The active provider is Gemini and the change is already applied with explicit support.",
            0.94,
            &[explicit.id],
        ))]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query(
                "What is the active provider?",
                namespace_id,
                &repo,
                &relation_repo,
            )
            .await
            .unwrap();

        assert_eq!(
            answer.answer,
            "The active provider is Gemini and the change is already applied with explicit support."
        );
        assert_eq!(llm.call_count(), 1);
    }

    #[tokio::test]
    async fn test_query_service_lightweight_context_below_phase_threshold() {
        let (pool, repo, namespace_id, perspective) = setup_repo().await;
        let relation_repo = MemoryRelationRepository::new(&pool);
        store_memory(
            &repo,
            namespace_id,
            "Short note about the provider switch.",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "Follow-up note about the provider switch.",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        let llm = Arc::new(MockLlmClient::new(vec![Ok(answer_response(
            "The provider changed.",
            0.93,
            &[1],
        ))]));
        let service = QueryService::new(llm.clone(), AgentConfig::default());

        let answer = service
            .query(
                "What changed with the provider?",
                namespace_id,
                &repo,
                &relation_repo,
            )
            .await
            .unwrap();

        assert!(!answer.lineages.is_empty());
        let prompts = llm.user_messages();
        assert!(prompts.iter().any(|prompt| prompt.contains("Summary:")));
    }

    // -----------------------------------------------------------------------
    // Introspection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_inclusion_reasons_produces_non_empty_reasons() {
        let bucketed = vec![
            BucketedMemory {
                memory: test_memory(1, "Fix the authentication bug"),
                bucket: MemoryBucket::Semantic,
                blended_score: 0.91,
            },
            BucketedMemory {
                memory: test_memory(2, "Session digest for sprint review"),
                bucket: MemoryBucket::Digests,
                blended_score: 0.85,
            },
            BucketedMemory {
                memory: test_memory(3, "Derived insight: auth patterns converge"),
                bucket: MemoryBucket::Derived,
                blended_score: 0.88,
            },
        ];

        let default_request = WorkingRepresentationRequest::default();
        let reasons = build_inclusion_reasons(&bucketed, &default_request);
        assert_eq!(reasons.len(), 3);

        assert_eq!(reasons[0].memory_id, 1);
        assert_eq!(reasons[0].bucket, MemoryBucket::Semantic);
        assert!(!reasons[0].reason.is_empty());
        assert!(reasons[0].reason.contains("Semantic match"));

        assert_eq!(reasons[1].memory_id, 2);
        assert_eq!(reasons[1].bucket, MemoryBucket::Digests);
        assert!(reasons[1].reason.contains("digest"));

        assert_eq!(reasons[2].memory_id, 3);
        assert_eq!(reasons[2].bucket, MemoryBucket::Derived);
        assert!(reasons[2].reason.contains("derived"));
    }

    #[test]
    fn test_build_excluded_candidates_classifies_exclusion_reasons() {
        let explicit_memory = Memory {
            id: 10,
            namespace_id: 1,
            content: "Low-scoring memory that got cut".to_string(),
            category: nexus_core::MemoryCategory::Facts,
            labels: Vec::new(),
            metadata: serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "confidence": 0.85,
                    "observer": "",
                    "subject": "",
                    "generated_by": ""
                }
            }),
            ..Memory::default()
        };
        let duplicate_memory = test_memory(11, "Another excluded memory with more text content");

        let excluded = vec![
            RankedExcludedMemory {
                memory: explicit_memory,
                bucket: MemoryBucket::Recent,
                blended_score: 0.30,
                reason: ExclusionReason::BudgetTruncation,
            },
            RankedExcludedMemory {
                memory: duplicate_memory,
                bucket: MemoryBucket::Semantic,
                blended_score: 0.25,
                reason: ExclusionReason::Deduplicated,
            },
        ];

        let candidates = build_excluded_candidates(&excluded);
        assert_eq!(candidates.len(), 2);

        assert_eq!(candidates[0].memory_id, 10);
        assert_eq!(candidates[0].reason, ExclusionReason::BudgetTruncation);
        assert!(!candidates[0].content_preview.is_empty());

        assert_eq!(candidates[1].memory_id, 11);
        assert_eq!(candidates[1].reason, ExclusionReason::Deduplicated);
    }

    #[test]
    fn test_build_bucket_stats_correct_counts() {
        let included = vec![
            BucketedMemory {
                memory: test_memory(1, "a"),
                bucket: MemoryBucket::Semantic,
                blended_score: 0.9,
            },
            BucketedMemory {
                memory: test_memory(2, "b"),
                bucket: MemoryBucket::Semantic,
                blended_score: 0.8,
            },
            BucketedMemory {
                memory: test_memory(3, "c"),
                bucket: MemoryBucket::Derived,
                blended_score: 0.7,
            },
        ];
        let excluded = vec![
            RankedExcludedMemory {
                memory: test_memory(4, "d"),
                bucket: MemoryBucket::Semantic,
                blended_score: 0.5,
                reason: ExclusionReason::BudgetTruncation,
            },
            RankedExcludedMemory {
                memory: test_memory(5, "e"),
                bucket: MemoryBucket::Recent,
                blended_score: 0.4,
                reason: ExclusionReason::BudgetTruncation,
            },
        ];

        let stats = build_bucket_stats(&included, &excluded);

        let semantic = stats
            .iter()
            .find(|s| s.bucket == MemoryBucket::Semantic)
            .unwrap();
        assert_eq!(semantic.fetched, 3);
        assert_eq!(semantic.included, 2);
        assert_eq!(semantic.excluded, 1);

        let derived = stats
            .iter()
            .find(|s| s.bucket == MemoryBucket::Derived)
            .unwrap();
        assert_eq!(derived.fetched, 1);
        assert_eq!(derived.included, 1);
        assert_eq!(derived.excluded, 0);

        let recent = stats
            .iter()
            .find(|s| s.bucket == MemoryBucket::Recent)
            .unwrap();
        assert_eq!(recent.fetched, 1);
        assert_eq!(recent.included, 0);
        assert_eq!(recent.excluded, 1);
    }

    #[test]
    fn test_query_introspection_serialization_contract() {
        let introspection = QueryIntrospection {
            included: vec![InclusionReason {
                memory_id: 1,
                bucket: MemoryBucket::Semantic,
                phase: "execution".to_string(),
                relevance_score: Some(0.87),
                blended_score: 0.91,
                reason: "Semantic match (score: 0.87) in semantic bucket".to_string(),
                signals: vec![InclusionSignal {
                    signal_type: "semantic_similarity".to_string(),
                    description: "Embedding similarity score: 0.870".to_string(),
                    weight_contribution: 0.87,
                }],
            }],
            excluded_candidates: vec![ExcludedCandidate {
                memory_id: 2,
                bucket: MemoryBucket::Recent,
                blended_score: 0.30,
                reason: ExclusionReason::BudgetTruncation,
                content_preview: "Some content...".to_string(),
            }],
            relevant_reflections: vec![RelevantReflection {
                memory_id: 3,
                reflection_type: "derived".to_string(),
                content_preview: "Auth patterns converge...".to_string(),
                confidence: Some(0.92),
                created_at: "2026-03-27T12:00:00Z".to_string(),
            }],
            bucket_stats: vec![BucketIntrospectionStats {
                bucket: MemoryBucket::Semantic,
                fetched: 2,
                included: 1,
                excluded: 1,
            }],
            pipeline_latency_ms: Some(12),
            representation_config: Some(RepresentationConfigSnapshot {
                max_items: 20,
                include_raw: false,
                include_digests: true,
                include_semantic: true,
                include_derived: true,
                include_contradictions: true,
            }),
        };

        let json = serde_json::to_string(&introspection).expect("serialize introspection");
        let parsed: QueryIntrospection =
            serde_json::from_str(&json).expect("deserialize introspection");

        assert_eq!(parsed.included.len(), 1);
        assert_eq!(parsed.excluded_candidates.len(), 1);
        assert_eq!(parsed.relevant_reflections.len(), 1);
        assert_eq!(parsed.bucket_stats.len(), 1);
        assert_eq!(
            parsed.excluded_candidates[0].reason,
            ExclusionReason::BudgetTruncation
        );
        assert_eq!(parsed.bucket_stats[0].fetched, 2);
        assert_eq!(parsed.pipeline_latency_ms, Some(12));
        assert!(parsed.representation_config.is_some());
        let config = parsed.representation_config.unwrap();
        assert_eq!(config.max_items, 20);
        assert!(!config.include_raw);
        assert!(config.include_digests);
        // Signals round-trip
        assert_eq!(parsed.included[0].signals.len(), 1);
        assert_eq!(
            parsed.included[0].signals[0].signal_type,
            "semantic_similarity"
        );
    }

    #[test]
    fn test_truncate_str_behavior() {
        assert_eq!(truncate_str("short", 80), "short");
        assert_eq!(truncate_str("hello world", 5), "hello...");
        assert_eq!(truncate_str("a longer string here", 10), "a longer s...");
    }

    #[tokio::test]
    async fn test_query_introspection_with_stored_memories() {
        let (_pool, repo, namespace_id, perspective) = setup_repo().await;

        // Store explicit and derived memories
        let explicit = store_memory(
            &repo,
            namespace_id,
            "The authentication module uses JWT tokens for session management",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        let _derived = store_memory(
            &repo,
            namespace_id,
            "Derived: JWT token patterns show convergence across microservices",
            CognitiveLevel::Derived,
            &perspective,
        )
        .await;

        let llm = Arc::new(MockLlmClient::new(vec![])); // No LLM calls needed
        let service = QueryService::new(llm, AgentConfig::default());

        let introspection = service
            .query_introspection("authentication tokens", namespace_id, &repo)
            .await
            .unwrap();

        // Should have at least the explicit memory included
        assert!(introspection
            .included
            .iter()
            .any(|i| i.memory_id == explicit.id));
        // All inclusion reasons should be non-empty and have signals
        for reason in &introspection.included {
            assert!(!reason.reason.is_empty());
            assert!(!reason.signals.is_empty());
        }
        // Pipeline latency should be populated
        assert!(introspection.pipeline_latency_ms.is_some());
        // Representation config should be populated
        assert!(introspection.representation_config.is_some());
        // Bucket stats should be present
        assert!(!introspection.bucket_stats.is_empty());
    }

    #[tokio::test]
    async fn test_query_introspection_surfaces_excluded_candidates_with_small_budget() {
        let (_pool, repo, namespace_id, perspective) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "Session cookies replaced JWT for auth",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "Derived auth insight from repeated login failures",
            CognitiveLevel::Derived,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "Recent authentication follow-up note",
            CognitiveLevel::Explicit,
            &perspective,
        )
        .await;

        let service =
            QueryService::new(Arc::new(MockLlmClient::new(vec![])), AgentConfig::default());
        let request = WorkingRepresentationRequest {
            namespace_id,
            perspective: Some(perspective.clone()),
            query: Some("authentication".to_string()),
            max_items: 1,
            include_raw: false,
            include_recent: true,
            include_semantic: true,
            include_derived: true,
            include_digests: true,
            include_contradictions: true,
            ..WorkingRepresentationRequest::default()
        };

        let introspection = service
            .introspection_with_representation(&request, "authentication", &repo)
            .await
            .unwrap();

        assert!(!introspection.excluded_candidates.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_relevant_reflections_uses_tokenized_matching() {
        let (_pool, repo, namespace_id, perspective) = setup_repo().await;

        store_memory(
            &repo,
            namespace_id,
            "Derived insight: session cookies replaced JWT after CSRF review",
            CognitiveLevel::Derived,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "Contradiction: older notes still mention JWT login flow",
            CognitiveLevel::Contradiction,
            &perspective,
        )
        .await;

        let request = WorkingRepresentationRequest {
            namespace_id,
            perspective: Some(perspective.clone()),
            query: Some("why did auth move away from jwt tokens".to_string()),
            ..WorkingRepresentationRequest::default()
        };

        let reflections =
            fetch_relevant_reflections("why did auth move away from jwt tokens", &request, &repo)
                .await
                .unwrap();

        assert!(!reflections.is_empty());
        assert!(reflections
            .iter()
            .any(|reflection| reflection.content_preview.to_lowercase().contains("jwt")));
    }

    #[tokio::test]
    async fn test_fetch_relevant_reflections_respects_request_scope() {
        let (_pool, repo, namespace_id, perspective) = setup_repo().await;
        let other_perspective = PerspectiveKey {
            observer: "codex".to_string(),
            subject: "codex".to_string(),
            session_key: Some("other-session".to_string()),
        };

        store_memory(
            &repo,
            namespace_id,
            "Derived auth insight from the active scoped session",
            CognitiveLevel::Derived,
            &perspective,
        )
        .await;
        store_memory(
            &repo,
            namespace_id,
            "Derived auth insight from an unrelated session",
            CognitiveLevel::Derived,
            &other_perspective,
        )
        .await;

        let request = WorkingRepresentationRequest {
            namespace_id,
            perspective: Some(perspective.clone()),
            query: Some("auth insight".to_string()),
            ..WorkingRepresentationRequest::default()
        };

        let reflections = fetch_relevant_reflections("auth insight", &request, &repo)
            .await
            .unwrap();

        assert_eq!(reflections.len(), 1);
        assert!(reflections[0]
            .content_preview
            .contains("active scoped session"));
    }

    #[tokio::test]
    async fn test_standalone_introspect_query_matches_service_introspection_contract() {
        let (_pool, repo, namespace_id, perspective) = setup_repo().await;

        for content in [
            "Session cookies replaced JWT for auth",
            "Derived auth insight from repeated login failures",
            "Recent authentication follow-up note",
        ] {
            store_memory(
                &repo,
                namespace_id,
                content,
                if content.starts_with("Derived") {
                    CognitiveLevel::Derived
                } else {
                    CognitiveLevel::Explicit
                },
                &perspective,
            )
            .await;
        }

        let request = WorkingRepresentationRequest {
            namespace_id,
            perspective: Some(perspective.clone()),
            query: Some("authentication".to_string()),
            max_items: 1,
            include_raw: false,
            include_recent: true,
            include_semantic: true,
            include_derived: true,
            include_digests: true,
            include_contradictions: true,
            ..WorkingRepresentationRequest::default()
        };

        let service =
            QueryService::new(Arc::new(MockLlmClient::new(vec![])), AgentConfig::default());
        let from_service = service
            .introspection_with_representation(&request, "authentication", &repo)
            .await
            .unwrap();
        let standalone = introspect_query(&request, "authentication", &repo)
            .await
            .unwrap();

        assert_eq!(
            standalone
                .excluded_candidates
                .iter()
                .map(|candidate| {
                    (
                        candidate.memory_id,
                        candidate.bucket,
                        candidate.reason.clone(),
                    )
                })
                .collect::<Vec<_>>(),
            from_service
                .excluded_candidates
                .iter()
                .map(|candidate| {
                    (
                        candidate.memory_id,
                        candidate.bucket,
                        candidate.reason.clone(),
                    )
                })
                .collect::<Vec<_>>(),
            "standalone introspection should expose the same excluded candidates as service introspection"
        );
        assert_eq!(
            standalone
                .bucket_stats
                .iter()
                .map(|stats| (stats.bucket, stats.fetched, stats.included, stats.excluded))
                .collect::<Vec<_>>(),
            from_service
                .bucket_stats
                .iter()
                .map(|stats| (stats.bucket, stats.fetched, stats.included, stats.excluded))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            standalone
                .included
                .iter()
                .map(|reason| (reason.memory_id, reason.bucket))
                .collect::<Vec<_>>(),
            from_service
                .included
                .iter()
                .map(|reason| (reason.memory_id, reason.bucket))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_query_answer_introspection_field_defaults_to_none() {
        let answer = QueryAnswer {
            answer: "test".to_string(),
            citations: vec![],
            confidence: 0.9,
            lineages: vec![],
            introspection: None,
        };

        let json = serde_json::to_string(&answer).unwrap();
        assert!(!json.contains("introspection"));
    }

    #[test]
    fn test_query_answer_introspection_serializes_when_present() {
        let answer = QueryAnswer {
            answer: "test".to_string(),
            citations: vec![],
            confidence: 0.9,
            lineages: vec![],
            introspection: Some(QueryIntrospection {
                included: vec![],
                excluded_candidates: vec![],
                relevant_reflections: vec![],
                bucket_stats: vec![],
                pipeline_latency_ms: None,
                representation_config: None,
            }),
        };

        let json = serde_json::to_string(&answer).unwrap();
        assert!(json.contains("introspection"));
        let parsed: QueryAnswer = serde_json::from_str(&json).unwrap();
        assert!(parsed.introspection.is_some());
    }

    // -----------------------------------------------------------------------
    // Phase 16 contract tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_inclusion_signals_populated_for_semantic_bucket() {
        let semantic_memory = Memory {
            id: 42,
            namespace_id: 1,
            content: "Authentication uses JWT tokens".to_string(),
            category: nexus_core::MemoryCategory::Facts,
            labels: Vec::new(),
            metadata: serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "confidence": 0.90,
                    "observer": "",
                    "subject": "",
                    "generated_by": ""
                }
            }),
            similarity_score: Some(0.85),
            ..Memory::default()
        };
        let bm = BucketedMemory {
            memory: semantic_memory,
            bucket: MemoryBucket::Semantic,
            blended_score: 0.92,
        };
        let default_request = WorkingRepresentationRequest::default();
        let reasons = build_inclusion_reasons(std::slice::from_ref(&bm), &default_request);
        assert_eq!(reasons.len(), 1);
        assert!(!reasons[0].signals.is_empty());
        // Should have at least recency, cognitive_level, and semantic_similarity
        let signal_types: Vec<&str> = reasons[0]
            .signals
            .iter()
            .map(|s| s.signal_type.as_str())
            .collect();
        assert!(signal_types.contains(&"recency"));
        assert!(signal_types.contains(&"cognitive_level"));
        assert!(signal_types.contains(&"semantic_similarity"));
    }

    #[test]
    fn test_inclusion_signals_populated_for_digest_bucket() {
        let digest_memory = Memory {
            id: 55,
            namespace_id: 1,
            content: "Session summary: worked on auth module".to_string(),
            category: nexus_core::MemoryCategory::Session,
            labels: Vec::new(),
            metadata: serde_json::json!({
                "cognitive": {
                    "level": "summary_short",
                    "confidence": 0.80,
                    "observer": "",
                    "subject": "",
                    "generated_by": ""
                }
            }),
            ..Memory::default()
        };
        let bm = BucketedMemory {
            memory: digest_memory,
            bucket: MemoryBucket::Digests,
            blended_score: 0.88,
        };
        let default_request = WorkingRepresentationRequest::default();
        let reasons = build_inclusion_reasons(&[bm], &default_request);
        assert_eq!(reasons.len(), 1);
        let signal_types: Vec<&str> = reasons[0]
            .signals
            .iter()
            .map(|s| s.signal_type.as_str())
            .collect();
        assert!(signal_types.contains(&"bucket_boost"));
    }

    #[test]
    fn test_exclusion_reason_variants_serialize_correctly() {
        for (variant, expected) in [
            (ExclusionReason::BudgetTruncation, "budget_truncation"),
            (
                ExclusionReason::ConfidenceBelowThreshold,
                "confidence_below_threshold",
            ),
            (ExclusionReason::Deduplicated, "deduplicated"),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let parsed: ExclusionReason = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn test_representation_config_snapshot_serialization() {
        let config = RepresentationConfigSnapshot {
            max_items: 30,
            include_raw: true,
            include_digests: true,
            include_semantic: true,
            include_derived: false,
            include_contradictions: false,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"max_items\":30"));
        let parsed: RepresentationConfigSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.max_items, 30);
        assert!(parsed.include_raw);
        assert!(!parsed.include_derived);
    }

    #[test]
    fn test_introspection_optional_fields_default_safely() {
        let intro = QueryIntrospection {
            included: vec![],
            excluded_candidates: vec![],
            relevant_reflections: vec![],
            bucket_stats: vec![],
            pipeline_latency_ms: None,
            representation_config: None,
        };
        let json = serde_json::to_string(&intro).unwrap();
        // Optional fields should not appear in JSON when None
        assert!(!json.contains("pipeline_latency_ms"));
        assert!(!json.contains("representation_config"));
        // Round-trip
        let parsed: QueryIntrospection = serde_json::from_str(&json).unwrap();
        assert!(parsed.pipeline_latency_ms.is_none());
        assert!(parsed.representation_config.is_none());
    }

    #[test]
    fn test_exclusion_reason_display_impl() {
        assert_eq!(
            format!("{}", ExclusionReason::BudgetTruncation),
            "budget_truncation"
        );
        assert_eq!(
            format!("{}", ExclusionReason::ConfidenceBelowThreshold),
            "confidence_below_threshold"
        );
        assert_eq!(format!("{}", ExclusionReason::Deduplicated), "deduplicated");
    }
}
