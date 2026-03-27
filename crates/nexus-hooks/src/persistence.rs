//! Persistence adapter for enriched hook memories
//!
//! Builds rich metadata and persists memories via the storage layer.

use chrono::Utc;
use nexus_core::{
    infer_perspective, CognitiveLevel, CognitiveMetadata, MemoryCategory, MemoryLaneType,
    PerspectiveSource,
};
use nexus_storage::repository::MemoryRepository;
use serde_json::json;
use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::claude_payload::NormalizedHookEvent;
use crate::enrichment::EnrichmentBatchResult;

/// Result of persisting a batch of enriched memories.
#[derive(Debug, Clone, Default)]
pub struct PersistResult {
    /// Number of memories successfully stored
    pub stored: usize,
    /// Number of memories skipped (store=false)
    pub skipped: usize,
    /// Breakdown by category
    pub categories: HashMap<String, usize>,
    /// Stored memory IDs, in insertion order
    pub stored_memory_ids: Vec<i64>,
}

/// Persist enriched memories to the database with rich metadata.
///
/// For each `EnrichedMemory` where `store == true`:
/// 1. Parses the category string into a `MemoryCategory`
/// 2. Parses memory_lane_type (optional) into `MemoryLaneType`
/// 3. Builds rich metadata including source, evidence, ingestion, and LLM comment
/// 4. Stores via `MemoryRepository`
pub async fn persist_enriched_memories(
    namespace_id: i64,
    event: &NormalizedHookEvent,
    batch: &EnrichmentBatchResult,
    memory_repo: &MemoryRepository,
    model_name: &str,
) -> anyhow::Result<PersistResult> {
    let mut result = PersistResult::default();
    let derived_session_key = derive_session_key(
        &event.agent,
        event.session_id.as_deref(),
        event.cwd.as_deref(),
    );
    let perspective = infer_perspective(
        PerspectiveSource::HookIngest,
        event.agent.clone(),
        None::<String>,
        Some(derived_session_key.clone()),
    );

    for enriched in &batch.memories {
        if !enriched.store {
            result.skipped += 1;
            debug!("Skipping memory (store=false): {}", enriched.memory_text);
            continue;
        }

        // Parse category — skip invalid categories rather than aborting the batch
        let category = match MemoryCategory::parse(&enriched.category) {
            Some(c) => c,
            None => {
                warn!(
                    "Skipping memory with invalid category '{}': {}",
                    enriched.category,
                    enriched.memory_text.chars().take(50).collect::<String>()
                );
                result.skipped += 1;
                continue;
            }
        };

        // Parse memory lane type (optional)
        let memory_lane_type = enriched
            .memory_lane_type
            .as_ref()
            .and_then(|t| MemoryLaneType::parse(t));

        // Build evidence from event fields (excerpts for large text)
        let evidence = json!({
            "tool_name": event.tool_name,
            "tool_input": event.tool_input,
            "tool_response_excerpt": event.tool_response_text.as_ref()
                .map(|s| s.chars().take(200).collect::<String>()),
            "assistant_message_excerpt": event.assistant_message_text.as_ref()
                .map(|s| s.chars().take(200).collect::<String>()),
            "user_message_excerpt": event.user_message_text.as_ref()
                .map(|s| s.chars().take(200).collect::<String>()),
        });

        // Build rich metadata
        let mut cognitive = CognitiveMetadata::new(
            CognitiveLevel::Explicit,
            perspective.observer.clone(),
            perspective.subject.clone(),
            perspective.session_key.clone(),
            "hook_persistence",
        );
        cognitive.confidence = Some(enriched.confidence);

        let metadata = cognitive.merge_into(&json!({
            "source": {
                "agent": event.agent,
                "event_name": event.event_name,
                "session_id": event.session_id,
                "derived_session_key": derived_session_key,
                "turn_id": event.turn_id,
                "cwd": event.cwd,
            },
            "evidence": evidence,
            "ingestion": {
                "normalized_at": event.observed_at.to_rfc3339(),
                "signal_score": null, // Would come from original candidate if we had it
                "pipeline_version": "hook-ingest-v1",
            },
            "llm_comment": {
                "model": model_name,
                "generated_at": Utc::now().to_rfc3339(),
                "text": enriched.comment,
            },
            "confidence": enriched.confidence,
        }));

        // Store the memory
        let params = nexus_storage::repository::StoreMemoryParams {
            namespace_id,
            content: &enriched.memory_text,
            category: &category,
            memory_lane_type: memory_lane_type.as_ref(),
            labels: &enriched.labels,
            metadata: &metadata,
            embedding: None,
            embedding_model: None,
        };

        match memory_repo.store(params).await {
            Ok(memory) => {
                result.stored += 1;
                result.stored_memory_ids.push(memory.id);
                *result
                    .categories
                    .entry(enriched.category.clone())
                    .or_insert(0) += 1;
                debug!(
                    "Stored memory id={} category='{}': {}",
                    memory.id,
                    enriched.category,
                    enriched.memory_text.chars().take(50).collect::<String>()
                );
            }
            Err(e) => {
                warn!(
                    "Failed to store memory: {}. Content: {}",
                    e, enriched.memory_text
                );
                // Continue with other memories
            }
        }
    }

    info!(
        "Persistence complete: {} stored, {} skipped",
        result.stored, result.skipped
    );

    Ok(result)
}

fn derive_session_key(agent: &str, session_key: Option<&str>, cwd: Option<&str>) -> String {
    if let Some(value) = session_key.filter(|value| !value.trim().is_empty()) {
        return value.to_string();
    }

    let fallback_scope = cwd
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unknown-cwd");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    agent.hash(&mut hasher);
    fallback_scope.hash(&mut hasher);
    format!("derived-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enrichment::EnrichedMemory;
    use nexus_storage::repository::MemoryRepository;
    use serde_json::json;

    async fn create_test_repo() -> (MemoryRepository, sqlx::SqlitePool) {
        let pool = sqlx::SqlitePool::connect(":memory:").await.unwrap();

        // Run migrations matching the real schema
        sqlx::query(
            r#"
            CREATE TABLE agent_namespaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                agent_type TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME
            );
        "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                namespace_id INTEGER NOT NULL,
                content TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT 'general',
                memory_lane_type TEXT,
                labels TEXT DEFAULT '[]',
                metadata TEXT DEFAULT '{}',
                similarity_score REAL,
                relevance_score REAL,
                content_embedding TEXT,
                embedding_model TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME,
                last_accessed DATETIME,
                is_active BOOLEAN DEFAULT 1,
                is_archived BOOLEAN DEFAULT 0,
                access_count INTEGER DEFAULT 0,
                FOREIGN KEY (namespace_id) REFERENCES agent_namespaces(id)
            );
        "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let repo = MemoryRepository::new(pool.clone());
        (repo, pool)
    }

    async fn create_test_namespace(pool: &sqlx::SqlitePool) -> i64 {
        let result = sqlx::query(
            "INSERT INTO agent_namespaces (name, agent_type, created_at) VALUES (?, ?, ?)",
        )
        .bind("test-namespace")
        .bind("test-agent")
        .bind(Utc::now())
        .execute(pool)
        .await
        .unwrap();
        result.last_insert_rowid()
    }

    fn make_test_event() -> NormalizedHookEvent {
        NormalizedHookEvent {
            agent: "test-agent".to_string(),
            event_name: "test_event".to_string(),
            observed_at: Utc::now(),
            session_id: Some("session-123".to_string()),
            turn_id: Some("turn-456".to_string()),
            cwd: Some("/home/user/project".to_string()),
            tool_name: Some("test_tool".to_string()),
            tool_input: Some(json!({"arg": "value"})),
            tool_response_text: Some("Tool response text".to_string()),
            assistant_message_text: Some("Assistant message text that might be quite long and need truncation for the evidence excerpt".to_string()),
            user_message_text: Some("User message text that might also be quite long and need truncation for the evidence excerpt".to_string()),
            raw_payload: json!({}),
        }
    }

    fn make_test_batch() -> EnrichmentBatchResult {
        EnrichmentBatchResult {
            memories: vec![
                EnrichedMemory {
                    store: true,
                    category: "preferences".to_string(),
                    memory_text: "User prefers Rust for systems programming".to_string(),
                    labels: vec![
                        "rust".to_string(),
                        "systems".to_string(),
                        "preferences".to_string(),
                    ],
                    memory_lane_type: Some("preference".to_string()),
                    comment: "Clear preference statement".to_string(),
                    confidence: 0.9,
                },
                EnrichedMemory {
                    store: false, // Should be skipped
                    category: "general".to_string(),
                    memory_text: "Noise to skip".to_string(),
                    labels: vec!["noise".to_string()],
                    memory_lane_type: None,
                    comment: "Low signal".to_string(),
                    confidence: 0.3,
                },
            ],
        }
    }

    #[tokio::test]
    async fn test_persist_enriched_memories() {
        let (repo, pool) = create_test_repo().await;
        let namespace_id = create_test_namespace(&pool).await;
        let event = make_test_event();
        let batch = make_test_batch();

        let result = persist_enriched_memories(namespace_id, &event, &batch, &repo, "test-model")
            .await
            .unwrap();

        assert_eq!(result.stored, 1);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.categories.get("preferences"), Some(&1));
    }

    #[test]
    fn test_persist_result_default() {
        let result = PersistResult::default();
        assert_eq!(result.stored, 0);
        assert_eq!(result.skipped, 0);
        assert!(result.categories.is_empty());
    }
}
