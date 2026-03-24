//! Ingest service - extracts structured info from raw text

use nexus_core::config::AgentConfig;
use nexus_core::types::MemoryCategory;
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, LlmClientJson};
use nexus_storage::repository::{MemoryRepository, StoreMemoryParams};
use tracing::{debug, error, info};

use crate::error::AgentError;
use crate::prompts::{ingest_user_prompt, INGEST_SYSTEM_PROMPT};
use crate::types::IngestExtraction;

pub struct IngestService {
    llm: std::sync::Arc<dyn LlmClient>,
    config: AgentConfig,
}

impl IngestService {
    pub fn new(llm: std::sync::Arc<dyn LlmClient>, config: AgentConfig) -> Self {
        Self { llm, config }
    }

    pub async fn ingest(
        &self,
        content: &str,
        source: &str,
        namespace_id: i64,
        repo: &MemoryRepository,
    ) -> Result<i64, AgentError> {
        info!(source = %source, "Ingesting content");

        // Step 1: Extract structured info via LLM
        let extraction = self.extract(content, source).await?;
        debug!(summary = %extraction.summary, "Extracted content info");

        // Step 2: Build labels from entities and topics
        let labels: Vec<String> = extraction
            .entities
            .iter()
            .chain(extraction.topics.iter())
            .cloned()
            .collect();

        // Step 3: Build metadata with agent extraction info
        let metadata = serde_json::json!({
            "agent": {
                "summary": extraction.summary,
                "entities": extraction.entities,
                "topics": extraction.topics,
                "importance_score": extraction.importance_score,
                "source": source,
                "generated_by": "ingest_agent"
            }
        });

        // Step 4: Store memory using repository
        let _title = format!("Ingested: {}", source);
        let memory = repo
            .store(StoreMemoryParams {
                namespace_id,
                content,
                category: &MemoryCategory::General,
                memory_lane_type: None,
                labels: &labels,
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            })
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to store memory");
                AgentError::Storage(e.to_string())
            })?;

        info!(memory_id = memory.id, "Memory stored successfully");
        Ok(memory.id)
    }

    async fn extract(&self, content: &str, source: &str) -> Result<IngestExtraction, AgentError> {
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(INGEST_SYSTEM_PROMPT),
                ChatMessage::user(ingest_user_prompt(content, source)),
            ],
            max_tokens: self.config.query_context_limit as u32,
            temperature: 0.3,
            json_mode: true,
        };

        let extraction: IngestExtraction = self
            .llm
            .generate_json(params)
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;

        Ok(extraction)
    }
}
