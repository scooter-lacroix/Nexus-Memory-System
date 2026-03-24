//! Repository implementations for database operations

use crate::models::{AgentNamespaceRow, MemoryRow, ProcessedFileRow};
use crate::{db_error, Result};
use chrono::Utc;
use nexus_core::{AgentNamespace, Memory, MemoryCategory, MemoryLaneType};
use sqlx::SqlitePool;

/// Type alias for backward compatibility
type Category = MemoryCategory;

/// Parameters for storing a new memory
pub struct StoreMemoryParams<'a> {
    pub namespace_id: i64,
    pub content: &'a str,
    pub category: &'a Category,
    pub memory_lane_type: Option<&'a MemoryLaneType>,
    pub labels: &'a [String],
    pub metadata: &'a serde_json::Value,
    pub embedding: Option<&'a [f32]>,
    pub embedding_model: Option<&'a str>,
}

/// Repository for memory operations
pub struct MemoryRepository {
    pool: SqlitePool,
}

impl MemoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store a new memory
    pub async fn store(&self, params: StoreMemoryParams<'_>) -> Result<Memory> {
        let labels_json = serde_json::to_string(params.labels)?;
        let metadata_json = serde_json::to_string(params.metadata)?;
        let embedding_json = params.embedding.map(serde_json::to_string).transpose()?;

        let result = sqlx::query(
            r#"
            INSERT INTO memories (
                namespace_id, content, category, memory_lane_type, labels, metadata,
                content_embedding, embedding_model, created_at, is_active, access_count
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 0)
            "#,
        )
        .bind(params.namespace_id)
        .bind(params.content)
        .bind(params.category.to_string())
        .bind(params.memory_lane_type.map(|t| t.to_string()))
        .bind(&labels_json)
        .bind(&metadata_json)
        .bind(&embedding_json)
        .bind(params.embedding_model)
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
            .bind(params.namespace_id)
            .bind(params.content)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

            return row.map(|r| self.row_to_memory(r)).ok_or_else(|| {
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

    /// Get unconsolidated memories
    pub async fn get_unconsolidated(
        &self,
        namespace_id: i64,
        limit: i32,
    ) -> Result<Vec<MemoryRow>> {
        let rows = sqlx::query_as::<_, MemoryRow>(
            r#"
            SELECT * FROM memories 
            WHERE namespace_id = ? 
            AND (metadata IS NULL OR json_extract(metadata, '$.agent.consolidated') IS NULL)
            ORDER BY created_at ASC
            LIMIT ?
            "#,
        )
        .bind(namespace_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows)
    }

    /// Mark a memory as consolidated
    pub async fn mark_consolidated(&self, id: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE memories
            SET metadata = json_set(
                COALESCE(metadata, '{}'),
                '$.agent.consolidated',
                true,
                '$.agent.consolidated_at',
                datetime('now')
            ),
            updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    /// Search memories by text content (LIKE search)
    pub async fn search_by_text(
        &self,
        namespace_id: i64,
        query: &str,
        limit: i32,
    ) -> Result<Vec<MemoryRow>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, MemoryRow>(
            r#"
            SELECT * FROM memories
            WHERE namespace_id = ?
            AND content LIKE ?
            ORDER BY updated_at DESC
            LIMIT ?
            "#,
        )
        .bind(namespace_id)
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows)
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

/// Repository for processed file operations (inbox deduplication)
pub struct ProcessedFileRepository<'a> {
    pub pool: &'a SqlitePool,
}

impl<'a> ProcessedFileRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Check if a file has been processed
    pub async fn is_processed(&self, namespace_id: i64, path: &str) -> Result<bool> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM processed_files WHERE namespace_id = ? AND path = ?")
                .bind(namespace_id)
                .bind(path)
                .fetch_optional(self.pool)
                .await
                .map_err(db_error)?;

        Ok(row.is_some())
    }

    /// Mark a file as being processed
    pub async fn mark_processing(
        &self,
        namespace_id: i64,
        path: &str,
        content_hash: Option<&str>,
    ) -> Result<i64> {
        let id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO processed_files (namespace_id, path, content_hash, status, updated_at)
            VALUES (?, ?, ?, 'processing', datetime('now'))
            ON CONFLICT(namespace_id, path) DO UPDATE SET
                content_hash = excluded.content_hash,
                status = 'processing',
                updated_at = datetime('now')
            RETURNING id
            "#,
        )
        .bind(namespace_id)
        .bind(path)
        .bind(content_hash)
        .fetch_one(self.pool)
        .await
        .map_err(db_error)?;

        Ok(id)
    }

    /// Mark a file as successfully processed with memory reference
    pub async fn mark_processed(&self, id: i64, memory_id: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processed_files
            SET status = 'completed', memory_id = ?, processed_at = datetime('now'), updated_at = datetime('now')
            WHERE id = ?
            "#
        )
        .bind(memory_id)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    /// Mark a file as failed
    pub async fn mark_failed(&self, id: i64, error: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE processed_files
            SET status = 'failed', last_error = ?, updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(error)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    /// Get files pending processing
    pub async fn get_pending(
        &self,
        namespace_id: i64,
        limit: i32,
    ) -> Result<Vec<ProcessedFileRow>> {
        let rows = sqlx::query_as::<_, ProcessedFileRow>(
            r#"
            SELECT * FROM processed_files 
            WHERE namespace_id = ? AND status = 'pending'
            ORDER BY created_at ASC
            LIMIT ?
            "#,
        )
        .bind(namespace_id)
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows)
    }

    /// Clear all processed files for a namespace
    pub async fn clear_namespace(&self, namespace_id: i64) -> Result<u64> {
        let result = sqlx::query("DELETE FROM processed_files WHERE namespace_id = ?")
            .bind(namespace_id)
            .execute(self.pool)
            .await
            .map_err(db_error)?;

        Ok(result.rows_affected())
    }
}

/// Repository for memory relationship operations
pub struct MemoryRelationRepository<'a> {
    pub pool: &'a SqlitePool,
}

impl<'a> MemoryRelationRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Store a relationship between two memories
    pub async fn store(
        &self,
        source_id: i64,
        target_id: i64,
        relation_type: &str,
        strength: f32,
    ) -> Result<i64> {
        let id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO memory_relations (source_memory_id, target_memory_id, relation_type, strength, created_at)
            VALUES (?, ?, ?, ?, datetime('now'))
            ON CONFLICT(source_memory_id, target_memory_id, relation_type) DO UPDATE SET
                strength = excluded.strength,
                created_at = datetime('now')
            RETURNING id
            "#
        )
        .bind(source_id)
        .bind(target_id)
        .bind(relation_type)
        .bind(strength)
        .fetch_one(self.pool)
        .await
        .map_err(db_error)?;

        Ok(id)
    }

    /// Get all related memories for a given memory
    pub async fn get_related(&self, memory_id: i64) -> Result<Vec<(i64, String, f32)>> {
        let rows: Vec<(i64, String, f32)> = sqlx::query_as(
            r#"
            SELECT target_memory_id as memory_id, relation_type, strength 
            FROM memory_relations 
            WHERE source_memory_id = ?
            UNION
            SELECT source_memory_id as memory_id, relation_type, strength 
            FROM memory_relations 
            WHERE target_memory_id = ?
            ORDER BY strength DESC
            "#,
        )
        .bind(memory_id)
        .bind(memory_id)
        .fetch_all(self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows)
    }

    /// Delete all relations for a memory
    pub async fn delete_for_memory(&self, memory_id: i64) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM memory_relations WHERE source_memory_id = ? OR target_memory_id = ?",
        )
        .bind(memory_id)
        .bind(memory_id)
        .execute(self.pool)
        .await
        .map_err(db_error)?;

        Ok(result.rows_affected())
    }
}

fn parse_category(s: &str) -> Category {
    MemoryCategory::parse(s).unwrap_or(MemoryCategory::General)
}

fn parse_memory_lane_type(s: &str) -> Option<MemoryLaneType> {
    MemoryLaneType::parse(s)
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

        assert!(parse_memory_lane_type("unknown").is_none());
    }
}
