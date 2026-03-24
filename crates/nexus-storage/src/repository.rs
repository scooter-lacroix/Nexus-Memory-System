//! Repository implementations for database operations

use crate::models::{AgentNamespaceRow, MemoryRow};
use crate::{db_error, Result};
use chrono::Utc;
use nexus_core::{AgentNamespace, Memory, MemoryCategory, MemoryLaneType};
use sqlx::SqlitePool;

/// Type alias for backward compatibility
type Category = MemoryCategory;

/// Repository for memory operations
pub struct MemoryRepository {
    pool: SqlitePool,
}

impl MemoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store a new memory
    pub async fn store(
        &self,
        namespace_id: i64,
        content: &str,
        category: &Category,
        memory_lane_type: Option<&MemoryLaneType>,
        labels: &[String],
        metadata: &serde_json::Value,
        embedding: Option<&[f32]>,
        embedding_model: Option<&str>,
    ) -> Result<Memory> {
        let labels_json = serde_json::to_string(labels)?;
        let metadata_json = serde_json::to_string(metadata)?;
        let embedding_json = embedding.map(|e| serde_json::to_string(e)).transpose()?;

        let result = sqlx::query(
            r#"
            INSERT INTO memories (
                namespace_id, content, category, memory_lane_type, labels, metadata,
                content_embedding, embedding_model, created_at, is_active, access_count
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0)
            "#,
        )
        .bind(namespace_id)
        .bind(content)
        .bind(category.to_string())
        .bind(memory_lane_type.map(|t| t.to_string()))
        .bind(&labels_json)
        .bind(&metadata_json)
        .bind(&embedding_json)
        .bind(embedding_model)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        let id = result.last_insert_rowid();

        // If last_insert_rowid() is 0, the BEFORE INSERT trigger
        // (trg_memories_same_namespace_merge) detected a duplicate and
        // raised IGNORE. The existing row was touched (access_count
        // incremented) — find it by content match.
        if id == 0 {
            let row: Option<MemoryRow> = sqlx::query_as(
                "SELECT * FROM memories WHERE namespace_id = ? AND LOWER(TRIM(content)) = LOWER(TRIM(?)) AND is_active = 1 ORDER BY created_at DESC LIMIT 1"
            )
            .bind(namespace_id)
            .bind(content)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

            return row
                .map(|r| self.row_to_memory(r))
                .ok_or_else(|| {
                    nexus_core::NexusError::Storage(
                        "Duplicate merged by trigger but matching row not found".to_string(),
                    )
                });
        }

        self.get_by_id(id).await?.ok_or_else(|| {
            nexus_core::NexusError::Storage(format!("Failed to retrieve memory with id {}", id))
        })
    }

    /// Get a memory by ID
    pub async fn get_by_id(&self, id: i64) -> Result<Option<Memory>> {
        let row: Option<MemoryRow> = sqlx::query_as("SELECT * FROM memories WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(row.map(|r| self.row_to_memory(r)))
    }

    /// Search memories by namespace
    pub async fn search_by_namespace(
        &self,
        namespace_id: i64,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Memory>> {
        let rows: Vec<MemoryRow> = sqlx::query_as(
            "SELECT * FROM memories WHERE namespace_id = ? AND is_active = 1 ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
        .bind(namespace_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows.into_iter().map(|r| self.row_to_memory(r)).collect())
    }

    /// Count memories in namespace
    pub async fn count_by_namespace(&self, namespace_id: i64) -> Result<i64> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memories WHERE namespace_id = ? AND is_active = 1",
        )
        .bind(namespace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(count.0)
    }

    /// Delete a memory
    pub async fn delete(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM memories WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(result.rows_affected() > 0)
    }

    /// Update access count
    pub async fn touch(&self, id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE memories SET access_count = access_count + 1, last_accessed = ? WHERE id = ?",
        )
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    fn row_to_memory(&self, row: MemoryRow) -> Memory {
        let labels: Vec<String> = serde_json::from_str(&row.labels).unwrap_or_default();
        let metadata: serde_json::Value =
            serde_json::from_str(&row.metadata).unwrap_or(serde_json::Value::Null);
        let embedding: Option<Vec<f32>> = row
            .content_embedding
            .and_then(|e| serde_json::from_str(&e).ok());

        Memory {
            id: row.id,
            namespace_id: row.namespace_id,
            content: row.content,
            category: parse_category(&row.category),
            memory_lane_type: row
                .memory_lane_type
                .as_deref()
                .and_then(parse_memory_lane_type),
            labels,
            metadata,
            similarity_score: row.similarity_score,
            relevance_score: row.relevance_score,
            content_embedding: embedding,
            embedding_model: row.embedding_model,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_accessed: row.last_accessed,
            is_active: row.is_active,
            is_archived: row.is_archived,
            access_count: row.access_count,
        }
    }
}

/// Repository for namespace operations
pub struct NamespaceRepository {
    pool: SqlitePool,
}

impl NamespaceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get or create a namespace
    pub async fn get_or_create(&self, name: &str, agent_type: &str) -> Result<AgentNamespace> {
        if let Some(ns) = self.get_by_name(name).await? {
            return Ok(ns);
        }

        let result = sqlx::query(
            "INSERT INTO agent_namespaces (name, agent_type, created_at) VALUES (?, ?, ?)",
        )
        .bind(name)
        .bind(agent_type)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        let id = result.last_insert_rowid();
        Ok(AgentNamespace {
            id,
            name: name.to_string(),
            description: None,
            agent_type: agent_type.to_string(),
            created_at: Utc::now(),
            updated_at: None,
        })
    }

    /// Get a namespace by name
    pub async fn get_by_name(&self, name: &str) -> Result<Option<AgentNamespace>> {
        let row: Option<AgentNamespaceRow> =
            sqlx::query_as("SELECT * FROM agent_namespaces WHERE name = ?")
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?;

        Ok(row.map(|r| AgentNamespace {
            id: r.id,
            name: r.name,
            description: r.description,
            agent_type: r.agent_type,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    /// List all namespaces
    pub async fn list_all(&self) -> Result<Vec<AgentNamespace>> {
        let rows: Vec<AgentNamespaceRow> =
            sqlx::query_as("SELECT * FROM agent_namespaces ORDER BY name")
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?;

        Ok(rows
            .into_iter()
            .map(|r| AgentNamespace {
                id: r.id,
                name: r.name,
                description: r.description,
                agent_type: r.agent_type,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}

fn parse_category(s: &str) -> Category {
    MemoryCategory::from_str(s).unwrap_or(MemoryCategory::General)
}

fn parse_memory_lane_type(s: &str) -> Option<MemoryLaneType> {
    MemoryLaneType::from_str(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::MemoryLanePriorityType;

    #[test]
    fn test_parse_category() {
        assert!(matches!(parse_category("facts"), Category::Facts));
        assert!(matches!(
            parse_category("preferences"),
            Category::Preferences
        ));
        assert!(matches!(parse_category("unknown"), Category::General));
    }

    #[test]
    fn test_parse_memory_lane_type() {
        let correction = parse_memory_lane_type("correction");
        assert!(matches!(
            correction,
            Some(MemoryLaneType::Priority(MemoryLanePriorityType::Correction))
        ));

        let pattern_seed = parse_memory_lane_type("pattern_seed");
        assert!(matches!(
            pattern_seed,
            Some(MemoryLaneType::Priority(
                MemoryLanePriorityType::PatternSeed
            ))
        ));

        assert!(matches!(parse_memory_lane_type("unknown"), None));
    }
}
