//! Query service - answers questions using memory context.
//!
//! The query pipeline is:
//! 1. Build a `WorkingRepresentation` via `RepresentationService` (bucketed memories).
//! 2. Flatten to deduplicated memories with bucket provenance.
//! 3. Run phase detection via `nexus_lephase::PhaseAnalyzer` (CPU-only, fast).
//! 4. Build a phase-grouped, lineage-annotated context string.
//! 5. Generate an answer via LLM, attaching lineage metadata.

use std::collections::HashMap;

use nexus_core::config::AgentConfig;
use nexus_core::{Memory, WorkingRepresentationRequest};
use nexus_lephase::{CompressionMode, LePhaseIntegration};
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, LlmClientJson};
use nexus_storage::repository::{MemoryRelationRepository, MemoryRepository};
use tracing::{debug, info, warn};

use crate::error::AgentError;
use crate::prompts::{
    query_refinement_user_prompt, query_user_prompt_with_lineage, QUERY_SYSTEM_PROMPT,
};
use crate::ranking::{flatten_ranked_representation, BucketedMemory};
use crate::representation::RepresentationService;
use crate::types::{MemoryLineage, QueryAnswer};
use crate::util::extract_agent_summary;

pub struct QueryService {
    llm: std::sync::Arc<dyn LlmClient>,
    config: AgentConfig,
}

/// Threshold below which the lightweight (non-phase-grouped) context builder is used.
const PHASE_GROUPING_THRESHOLD: usize = 3;

impl QueryService {
    pub fn new(llm: std::sync::Arc<dyn LlmClient>, config: AgentConfig) -> Self {
        Self { llm, config }
    }

    pub async fn query(
        &self,
        question: &str,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
        relation_repo: &MemoryRelationRepository<'_>,
    ) -> Result<QueryAnswer, AgentError> {
        let request = WorkingRepresentationRequest {
            namespace_id,
            perspective: None,
            query: Some(question.to_string()),
            max_items: self.config.query_context_limit,
            include_raw: false,
            ..WorkingRepresentationRequest::default()
        };

        self.query_with_representation(question, request, memory_repo, relation_repo)
            .await
    }

    pub async fn query_with_representation(
        &self,
        question: &str,
        request: WorkingRepresentationRequest,
        memory_repo: &MemoryRepository,
        _relation_repo: &MemoryRelationRepository<'_>,
    ) -> Result<QueryAnswer, AgentError> {
        info!(question = %question, "Processing query");

        let representation = RepresentationService::new()
            .build(&request, memory_repo)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to build working representation");
                AgentError::Storage(e.to_string())
            })?;

        let bucketed = flatten_ranked_representation(representation, &request);
        debug!(count = bucketed.len(), "Found relevant memories");

        if bucketed.is_empty() {
            let answer = self.generate_answer(question, "").await?;
            return Ok(answer);
        }

        // Build lineages (phase detection + relevance scoring).
        let lineages = build_lineages(&bucketed);

        // Build context: phase-grouped for larger sets, lightweight for small sets.
        let context = if bucketed.len() >= PHASE_GROUPING_THRESHOLD {
            self.build_phase_aware_context(&bucketed, &lineages)?
        } else {
            self.build_lightweight_context(&bucketed, &lineages)?
        };

        debug!(context_len = context.len(), "Built query context");

        let answer = self.generate_answer(question, &context).await?;
        let mut answer = if should_refine_answer(question, &answer, &bucketed) {
            let refined = self
                .generate_refined_answer(question, &context, &answer)
                .await?;
            select_better_answer(answer, refined)
        } else {
            answer
        };
        answer.lineages = lineages;

        info!("Query answered successfully");
        Ok(answer)
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
    ) -> Result<QueryAnswer, AgentError> {
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

        let answer: QueryAnswer = self
            .llm
            .generate_json(params)
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;

        Ok(answer)
    }

    async fn generate_refined_answer(
        &self,
        question: &str,
        context: &str,
        draft: &QueryAnswer,
    ) -> Result<QueryAnswer, AgentError> {
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

        self.llm
            .generate_json(params)
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))
    }
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryBucket;
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
            lineages: Vec::new(),
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
            lineages: Vec::new(),
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
            lineages: Vec::new(),
        };
        let refined = QueryAnswer {
            answer: "Longer answer with supporting detail and an explicit citation.".to_string(),
            citations: vec![crate::types::MemoryCitation {
                memory_id: 3,
                title: "Evidence".to_string(),
                excerpt: "Supporting excerpt".to_string(),
            }],
            confidence: 0.76,
            lineages: Vec::new(),
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
}
