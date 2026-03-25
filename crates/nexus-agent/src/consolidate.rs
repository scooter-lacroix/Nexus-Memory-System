//! Consolidate service - finds patterns across memories

use nexus_core::config::AgentConfig;
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, LlmClientJson};
use nexus_storage::repository::{MemoryRelationRepository, MemoryRepository};
use tracing::{debug, error, info};

use crate::error::AgentError;
use crate::prompts::{consolidate_user_prompt, CONSOLIDATE_SYSTEM_PROMPT};
use crate::types::ConsolidationResult;
use crate::util::extract_agent_summary;

pub struct ConsolidateService {
    llm: std::sync::Arc<dyn LlmClient>,
    config: AgentConfig,
}

impl ConsolidateService {
    pub fn new(llm: std::sync::Arc<dyn LlmClient>, config: AgentConfig) -> Self {
        Self { llm, config }
    }

    pub async fn consolidate(
        &self,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
        relation_repo: &MemoryRelationRepository<'_>,
    ) -> Result<Option<i64>, AgentError> {
        info!("Starting consolidation");

        // Step 1: Get unconsolidated memories
        let memories = memory_repo
            .get_unconsolidated(namespace_id, self.config.consolidation_batch_size as i32)
            .await
            .map_err(|e| {
                error!(error = %e, namespace_id, "Failed to get unconsolidated memories");
                AgentError::Storage(e.to_string())
            })?;

        if memories.len() < 2 {
            debug!("Not enough unconsolidated memories, skipping");
            return Ok(None);
        }

        info!(count = memories.len(), "Found memories to consolidate");

        // Step 2: Build summaries for LLM
        let summaries: Vec<(i64, String)> = memories
            .iter()
            .map(|m| {
                let summary = extract_agent_summary(&m.metadata, &m.content, 200);
                (m.id, summary)
            })
            .collect();

        // Step 3: Get consolidation from LLM
        let result = self.analyze(&summaries).await?;
        debug!(insight = %result.insight, "Generated consolidation insight");

        // Step 4: Store connections
        for conn in &result.connections {
            relation_repo
                .store(conn.from_id, conn.to_id, &conn.relationship, conn.strength)
                .await
                .map_err(|e| {
                    error!(error = %e, from_id = conn.from_id, to_id = conn.to_id, "Failed to store relation");
                    AgentError::Storage(e.to_string())
                })?;
        }

        // Step 5: Mark source memories as consolidated (batch update)
        let ids: Vec<i64> = memories.iter().map(|m| m.id).collect();
        memory_repo
            .mark_consolidated_batch(&ids)
            .await
            .map_err(|e| {
                error!(error = %e, count = ids.len(), "Failed to mark memories as consolidated");
                AgentError::Storage(e.to_string())
            })?;

        info!(
            connections = result.connections.len(),
            "Consolidation complete"
        );
        Ok(Some(memories.len() as i64))
    }

    async fn analyze(
        &self,
        summaries: &[(i64, String)],
    ) -> Result<ConsolidationResult, AgentError> {
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(CONSOLIDATE_SYSTEM_PROMPT),
                ChatMessage::user(consolidate_user_prompt(summaries)),
            ],
            max_tokens: 4096,
            temperature: 0.4,
            json_mode: true,
        };

        let result: ConsolidationResult = self
            .llm
            .generate_json(params)
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;

        Ok(result)
    }
}
