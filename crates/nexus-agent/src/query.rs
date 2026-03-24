//! Query service - answers questions using memory context

use nexus_core::config::AgentConfig;
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, LlmClientJson};
use nexus_storage::repository::{MemoryRelationRepository, MemoryRepository};
use tracing::{debug, info, warn};

use crate::error::AgentError;
use crate::prompts::{query_user_prompt, QUERY_SYSTEM_PROMPT};
use crate::types::QueryAnswer;

pub struct QueryService {
    llm: std::sync::Arc<dyn LlmClient>,
    config: AgentConfig,
}

impl QueryService {
    pub fn new(llm: std::sync::Arc<dyn LlmClient>, config: AgentConfig) -> Self {
        Self { llm, config }
    }

    pub async fn query(
        &self,
        question: &str,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
        _relation_repo: &MemoryRelationRepository<'_>,
    ) -> Result<QueryAnswer, AgentError> {
        info!(question = %question, "Processing query");

        // Step 1: Search for relevant memories
        let memories = memory_repo
            .search_by_text(
                namespace_id,
                question,
                self.config.query_context_limit as i32,
            )
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to search memories");
                AgentError::Storage(e.to_string())
            })?;

        debug!(count = memories.len(), "Found relevant memories");

        // Step 2: Build context from memories
        let context = self.build_context(&memories)?;

        // Step 3: Generate answer via LLM
        let answer = self.generate_answer(question, &context).await?;

        info!("Query answered successfully");
        Ok(answer)
    }

    fn build_context(
        &self,
        memories: &[nexus_storage::models::MemoryRow],
    ) -> Result<String, AgentError> {
        let mut context_parts = Vec::new();

        for memory in memories {
            let summary = serde_json::from_str::<serde_json::Value>(&memory.metadata)
                .ok()
                .and_then(|md| md.get("agent").cloned())
                .and_then(|a: serde_json::Value| a.get("summary").cloned())
                .and_then(|s: serde_json::Value| s.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| memory.content.chars().take(300).collect::<String>());

            context_parts.push(format!(
                "[Memory #{}] {}\nSummary: {}",
                memory.id,
                memory.content.chars().take(100).collect::<String>(),
                summary
            ));
        }

        Ok(context_parts.join("\n\n"))
    }

    async fn generate_answer(
        &self,
        question: &str,
        context: &str,
    ) -> Result<QueryAnswer, AgentError> {
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(QUERY_SYSTEM_PROMPT),
                ChatMessage::user(query_user_prompt(question, context)),
            ],
            max_tokens: 2048,
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
}
