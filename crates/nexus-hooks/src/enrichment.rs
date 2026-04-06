//! LLM enrichment for hook-derived memory candidates
//!
//! Uses the nexus-llm crate to categorize, rewrite, and comment on
//! memory candidates before persistence.

use std::sync::Arc;

use nexus_llm::{
    create_client_auto_with_fallback, ChatMessage, GenerateParams, LlmClient, LlmClientJson,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::candidate::MemoryCandidate;
use crate::claude_payload::NormalizedHookEvent;

/// Result of LLM enrichment for a single memory candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedMemory {
    /// Whether this memory should be stored
    pub store: bool,
    /// Nexus category for the memory
    pub category: String,
    /// Rewritten, standalone, retrieval-friendly content
    #[serde(alias = "memory")]
    pub memory_text: String,
    /// Labels for retrieval
    pub labels: Vec<String>,
    /// Optional Memory Lane type
    #[serde(rename = "memory_lane_type")]
    pub memory_lane_type: Option<String>,
    /// Model-authored comment explaining why this is worth keeping
    pub comment: String,
    /// Model confidence in the enrichment
    pub confidence: f32,
}

/// Result of enriching a batch of candidates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichmentBatchResult {
    #[serde(alias = "accepted_memories", default)]
    pub memories: Vec<EnrichedMemory>,
}

/// System prompt for enrichment LLM
const ENRICHMENT_SYSTEM_PROMPT: &str = r#"You are enriching agent hook events into durable memories for a retrieval system.

Decide whether each candidate is worth storing.

Only keep information that is durable, decision-relevant, preference-revealing, specification-bearing, contextual in a useful way, or session-significant.

Allowed categories:
- general
- facts
- preferences
- context
- specifications
- session

For each accepted memory:
- rewrite the memory into a standalone retrieval-friendly sentence or short paragraph
- assign exactly one allowed category
- produce 2-5 labels
- optionally assign a memory_lane_type from: correction, decision, commitment, insight, learning, confidence, pattern_seed, cross_agent, workflow_note, gap
- produce a comment explaining why the memory is worth keeping

The comment must be model-authored and should explain why the memory is worth keeping, what retrieval value it has, or how it should be interpreted later.

Reject low-signal operational noise.

Return strict JSON only. No markdown fences."#;

/// Enrichment service for memory candidates.
pub struct EnrichmentService {
    client: Arc<dyn LlmClient>,
    model_name: String,
}

impl EnrichmentService {
    /// Create a new enrichment service using the configured LLM client.
    pub fn new() -> anyhow::Result<Self> {
        let client = create_client_auto_with_fallback()?;
        let model_name = client.model_name();

        info!("EnrichmentService initialized with model: {}", model_name);

        Ok(Self { client, model_name })
    }

    /// Get the model name being used for enrichment.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Enrich a batch of memory candidates using the LLM.
    ///
    /// Sends all candidates to the LLM in a single call for efficiency,
    /// receiving structured enrichment decisions back.
    pub async fn enrich_candidates(
        &self,
        candidates: &[MemoryCandidate],
        event: &NormalizedHookEvent,
    ) -> anyhow::Result<EnrichmentBatchResult> {
        if candidates.is_empty() {
            debug!("No candidates to enrich");
            return Ok(EnrichmentBatchResult {
                memories: Vec::new(),
            });
        }

        let user_payload = self.build_user_payload(candidates, event);
        let user_message = serde_json::to_string_pretty(&user_payload)?;

        debug!(
            "Enriching {} candidates with event: {}",
            candidates.len(),
            event.event_name
        );

        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(ENRICHMENT_SYSTEM_PROMPT),
                ChatMessage::user(user_message),
            ],
            max_tokens: 4096,
            temperature: 0.3,
            json_mode: true,
        };

        let result: EnrichmentBatchResult = self.client.generate_json(params).await?;

        info!(
            "Enrichment complete: {} memories returned",
            result.memories.len()
        );

        Ok(result)
    }

    /// Build the user payload for the enrichment LLM call.
    fn build_user_payload(
        &self,
        candidates: &[MemoryCandidate],
        event: &NormalizedHookEvent,
    ) -> serde_json::Value {
        // Build event context
        let event_obj = serde_json::json!({
            "agent": event.agent,
            "event_name": event.event_name,
            "tool_name": event.tool_name,
            "session_id": event.session_id,
            "turn_id": event.turn_id,
        });

        // Build candidates array
        let candidates_array: Vec<serde_json::Value> = candidates
            .iter()
            .map(|c| {
                serde_json::json!({
                    "candidate_id": c.candidate_id,
                    "signal_score": c.signal_score,
                    "provisional_category": c.provisional_category,
                    "memory_text": c.memory_text,
                    "evidence": c.evidence,
                    "labels": c.labels,
                })
            })
            .collect();

        serde_json::json!({
            "event": event_obj,
            "candidates": candidates_array,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn test_enriched_memory_serialization() {
        let memory = EnrichedMemory {
            store: true,
            category: "preferences".to_string(),
            memory_text: "User prefers Rust over C++ for systems programming".to_string(),
            labels: vec![
                "rust".to_string(),
                "cpp".to_string(),
                "preferences".to_string(),
            ],
            memory_lane_type: Some("preference".to_string()),
            comment: "Clear preference statement affecting future tool selection".to_string(),
            confidence: 0.9,
        };

        let serialized = serde_json::to_string(&memory).unwrap();
        let deserialized: EnrichedMemory = serde_json::from_str(&serialized).unwrap();

        assert!(deserialized.store);
        assert_eq!(deserialized.category, "preferences");
        assert_eq!(deserialized.labels.len(), 3);
    }

    #[test]
    fn test_enrichment_batch_result_serialization() {
        let result = EnrichmentBatchResult {
            memories: vec![EnrichedMemory {
                store: true,
                category: "facts".to_string(),
                memory_text: "Project uses SQLite for persistence".to_string(),
                labels: vec!["sqlite".to_string(), "database".to_string()],
                memory_lane_type: None,
                comment: "Architecture fact".to_string(),
                confidence: 0.95,
            }],
        };

        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: EnrichmentBatchResult = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.memories.len(), 1);
        assert_eq!(deserialized.memories[0].category, "facts");
    }

    #[test]
    fn test_enrichment_batch_result_accepts_accepted_memories_alias() {
        let payload = json!({
            "accepted_memories": [
                {
                    "store": true,
                    "category": "facts",
                    "memory_text": "Project uses SQLite for persistence",
                    "labels": ["sqlite", "database"],
                    "memory_lane_type": null,
                    "comment": "Architecture fact",
                    "confidence": 0.95
                }
            ]
        });

        let deserialized: EnrichmentBatchResult =
            serde_json::from_value(payload).expect("deserialize alias payload");

        assert_eq!(deserialized.memories.len(), 1);
        assert_eq!(deserialized.memories[0].category, "facts");
    }

    #[test]
    fn test_build_user_payload() {
        let service = EnrichmentService {
            client: Arc::new(MockLlmClient::new()),
            model_name: "test-model".to_string(),
        };

        let candidates = vec![MemoryCandidate {
            candidate_id: "test-1".to_string(),
            source_event_name: "test_event".to_string(),
            source_agent: "test-agent".to_string(),
            signal_score: 0.8,
            provisional_category: Some("preferences".to_string()),
            memory_text: "Test memory".to_string(),
            evidence: json!({"key": "value"}),
            labels: vec!["test".to_string()],
        }];

        let event = NormalizedHookEvent {
            agent: "claude-code".to_string(),
            event_name: "test_event".to_string(),
            observed_at: Utc::now(),
            session_id: Some("session-123".to_string()),
            turn_id: Some("turn-456".to_string()),
            cwd: Some("/home/user/project".to_string()),
            tool_name: Some("test_tool".to_string()),
            tool_input: None,
            tool_response_text: None,
            assistant_message_text: None,
            user_message_text: None,
            raw_payload: json!({}),
        };

        let payload = service.build_user_payload(&candidates, &event);

        assert_eq!(payload["event"]["agent"], "claude-code");
        assert_eq!(payload["event"]["event_name"], "test_event");
        assert_eq!(payload["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(payload["candidates"][0]["candidate_id"], "test-1");
    }

    // Mock LLM client for testing
    struct MockLlmClient;

    impl MockLlmClient {
        fn new() -> Self {
            Self
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for MockLlmClient {
        async fn generate(
            &self,
            _params: GenerateParams,
        ) -> nexus_llm::Result<nexus_llm::GenerateResponse> {
            Ok(nexus_llm::GenerateResponse {
                content: "{}".to_string(),
                model: "mock-model".to_string(),
                usage: None,
            })
        }

        fn provider_name(&self) -> String {
            "mock".to_string()
        }

        fn model_name(&self) -> String {
            "mock-model".to_string()
        }
    }
}
