//! Repository implementations for database operations

use std::collections::HashMap;

use crate::models::{
    memory_job_status, AgentNamespaceRow, ClaimedMemoryJob, EnqueueJobParams, MemoryEvidenceRow,
    MemoryJobRow, MemoryLineageEntry, MemoryRow, ProcessedFileRow, SessionDigestRow,
    SystemMetricRow,
};
use crate::{db_error, Result};
use chrono::{DateTime, Utc};
use nexus_core::{
    AgentNamespace, CognitiveLevel, Memory, MemoryCategory, MemoryLaneType, PerspectiveKey,
};
use sqlx::SqlitePool;
use tracing::warn;

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

/// Parameters for storing a memory with evidence lineage.
pub struct StoreMemoryWithLineageParams<'a> {
    pub store: StoreMemoryParams<'a>,
    pub source_memory_ids: &'a [i64],
    pub evidence_role: &'a str,
}

/// Parameters for storing or updating a session digest registration.
pub struct StoreDigestParams<'a> {
    pub namespace_id: i64,
    pub session_key: &'a str,
    pub digest_kind: &'a str,
    pub memory_id: i64,
    pub start_memory_id: Option<i64>,
    pub end_memory_id: Option<i64>,
    pub token_count: usize,
}

pub struct SessionDigestRollover {
    pub last_digest_end_memory_id: Option<i64>,
    pub new_memory_count: i64,
    pub estimated_new_tokens: i64,
}

pub struct ListMemoryFilters<'a> {
    pub category: Option<&'a str>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub content_like: Option<&'a str>,
    pub include_raw: bool,
    pub limit: i64,
    pub offset: i64,
}

/// Parameters for [`MemoryRepository::search_working_set`].
pub struct WorkingSetParams<'a> {
    /// Namespace to search within.
    pub namespace_id: i64,
    /// Optional perspective lens. When `None`, queries fall back to
    /// namespace-only retrieval without observer/subject filtering.
    pub perspective: Option<&'a PerspectiveKey>,
    /// Maximum number of memories to return after deduplication.
    pub max_items: usize,
    /// When `false` (default), raw-activity hook noise is excluded from
    /// every bucket.
    pub include_raw: bool,
}

/// Parameters for embedding-backed semantic candidate retrieval.
pub struct SemanticCandidateParams<'a> {
    /// Namespace to search within.
    pub namespace_id: i64,
    /// Optional perspective lens for observer/subject/session scoping.
    pub perspective: Option<&'a PerspectiveKey>,
    /// Maximum number of candidate memories to return before vector ranking.
    pub limit: i64,
    /// When `false`, raw-activity hook noise is excluded.
    pub include_raw: bool,
}

/// A persisted system metric sample.
#[derive(Debug, Clone)]
pub struct MetricSample {
    pub metric_name: String,
    pub metric_value: f64,
    pub labels: serde_json::Value,
}

/// Maximum number of retry attempts before a job is permanently failed.
const MAX_JOB_ATTEMPTS: i64 = 5;
const RAW_ACTIVITY_FILTER_SQL: &str =
    "labels NOT LIKE '%raw-activity%' AND json_extract(COALESCE(metadata, '{}'), '$.raw_activity') IS NULL";

/// Repository for memory operations
pub struct MemoryRepository {
    pool: SqlitePool,
}

impl MemoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
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
            // Try to find existing row by content match (case-insensitive, trimmed)
            let row: Option<MemoryRow> = sqlx::query_as(
                "SELECT * FROM memories WHERE namespace_id = ? AND LOWER(TRIM(content)) = LOWER(TRIM(?)) AND is_active = 1 ORDER BY created_at DESC LIMIT 1"
            )
            .bind(params.namespace_id)
            .bind(params.content)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

            if let Some(existing) = row {
                self.merge_duplicate_memory_context(existing.id, params)
                    .await?;
                return self.get_by_id(existing.id).await?.ok_or_else(|| {
                    nexus_core::NexusError::Storage(format!(
                        "Duplicate merged row {} could not be reloaded",
                        existing.id
                    ))
                });
            }

            // If no duplicate found, this is unexpected - log warning but don't fail
            // This can happen if trigger fires but content doesn't match exactly
            tracing::warn!(
                namespace_id = params.namespace_id,
                content_length = params.content.len(),
                "Insert returned id 0 but no matching duplicate found - treating as successful insert"
            );
            
            // Return success anyway - the insert did happen, we just can't track the id
            // This prevents hook failures due to this edge case
            return self.get_by_content(params.namespace_id, params.content).await;
        }

        self.get_by_id(id).await?.ok_or_else(|| {
            nexus_core::NexusError::Storage(format!("Failed to retrieve memory with id {}", id))
        })
    }

    async fn merge_duplicate_memory_context(
        &self,
        existing_id: i64,
        params: StoreMemoryParams<'_>,
    ) -> Result<()> {
        let current = self.get_by_id(existing_id).await?.ok_or_else(|| {
            nexus_core::NexusError::Storage(format!(
                "Failed to load duplicate-merged memory {}",
                existing_id
            ))
        })?;

        let merged_labels = merge_labels(&current.labels, params.labels);
        let merged_metadata = merge_duplicate_metadata(&current.metadata, params.metadata);
        let labels_json = serde_json::to_string(&merged_labels)?;
        let metadata_json = serde_json::to_string(&merged_metadata)?;

        sqlx::query(
            r#"
            UPDATE memories
            SET labels = ?, metadata = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&labels_json)
        .bind(&metadata_json)
        .bind(Utc::now())
        .bind(existing_id)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    /// Store a memory and write evidence lineage rows atomically.
    pub async fn store_with_lineage(
        &self,
        params: StoreMemoryWithLineageParams<'_>,
    ) -> Result<Memory> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let memory_id = insert_memory_tx(&mut tx, &params.store).await?;
        for &source_id in params.source_memory_ids {
            insert_evidence_tx(&mut tx, memory_id, source_id, params.evidence_role).await?;
        }
        tx.commit().await.map_err(db_error)?;

        self.get_by_id(memory_id).await?.ok_or_else(|| {
            nexus_core::NexusError::Storage(format!(
                "Failed to retrieve memory with id {} after lineage store",
                memory_id
            ))
        })
    }

    /// Enqueue a new cognitive job.
    pub async fn enqueue_job(&self, params: EnqueueJobParams<'_>) -> Result<i64> {
        let perspective_json = params.perspective.map(serde_json::to_string).transpose()?;
        let payload_json = serde_json::to_string(params.payload)?;

        let id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO memory_jobs (namespace_id, job_type, status, priority, perspective_json, payload_json, created_at, updated_at)
            VALUES (?, ?, 'pending', ?, ?, ?, datetime('now'), datetime('now'))
            RETURNING id
            "#,
        )
        .bind(params.namespace_id)
        .bind(params.job_type)
        .bind(params.priority)
        .bind(&perspective_json)
        .bind(&payload_json)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(id)
    }

    /// Claim up to `limit` pending (or stale running) jobs for a given type and namespace.
    ///
    /// Stale running jobs are those whose lease has expired (`lease_expires_at < now`).
    /// Claimed jobs are transitioned to `running` with a lease owner and TTL.
    pub async fn claim_jobs(
        &self,
        namespace_id: i64,
        job_type: &str,
        lease_owner: &str,
        lease_ttl_secs: u64,
        limit: i64,
    ) -> Result<Vec<ClaimedMemoryJob>> {
        let claim_token = new_claim_token(lease_owner);
        let rows: Vec<MemoryJobRow> = sqlx::query_as::<_, MemoryJobRow>(
            r#"
            WITH candidates AS (
                SELECT id
                FROM memory_jobs
                WHERE namespace_id = ? AND job_type = ? AND (
                    status = ?
                    OR (status = ? AND lease_expires_at IS NOT NULL AND lease_expires_at < datetime('now'))
                )
                ORDER BY priority DESC, created_at ASC
                LIMIT ?
            )
            UPDATE memory_jobs
            SET status = ?,
                lease_owner = ?,
                claim_token = ?,
                lease_expires_at = datetime('now', '+' || ? || ' seconds'),
                attempts = attempts + 1,
                updated_at = datetime('now')
            WHERE id IN (SELECT id FROM candidates)
            RETURNING *
            "#,
        )
        .bind(namespace_id)
        .bind(job_type)
        .bind(memory_job_status::PENDING)
        .bind(memory_job_status::RUNNING)
        .bind(limit)
        .bind(memory_job_status::RUNNING)
        .bind(lease_owner)
        .bind(&claim_token)
        .bind(lease_ttl_secs as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        let mut rows = rows;
        rows.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.created_at.cmp(&right.created_at))
        });

        let mut claimed = Vec::with_capacity(rows.len());
        for row in rows {
            let perspective = match row.perspective_json.as_deref() {
                Some(s) => Some(serde_json::from_str(s).map_err(|e| {
                    nexus_core::NexusError::Storage(format!(
                        "corrupted perspective JSON for job {}: {e}",
                        row.id
                    ))
                })?),
                None => None,
            };
            let payload: serde_json::Value =
                serde_json::from_str(&row.payload_json).map_err(|e| {
                    nexus_core::NexusError::Storage(format!(
                        "corrupted payload JSON for job {}: {e}",
                        row.id
                    ))
                })?;
            claimed.push(ClaimedMemoryJob {
                row,
                perspective,
                payload,
            });
        }

        Ok(claimed)
    }

    /// Mark a job as completed.
    pub async fn complete_job(&self, job: &ClaimedMemoryJob) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE memory_jobs
            SET status = ?, lease_owner = NULL, claim_token = NULL, lease_expires_at = NULL, updated_at = datetime('now')
            WHERE id = ?
              AND lease_owner = ?
              AND claim_token = ?
            "#,
        )
        .bind(memory_job_status::COMPLETED)
        .bind(job.row.id)
        .bind(job.row.lease_owner.as_deref())
        .bind(job.row.claim_token.as_deref())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        if result.rows_affected() == 0 {
            return Err(nexus_core::NexusError::Storage(format!(
                "Memory job {} completion lost lease ownership",
                job.row.id
            )));
        }

        Ok(())
    }

    /// Mark a job as failed. If attempts < MAX_JOB_ATTEMPTS, requeue it as pending.
    /// Otherwise, mark it permanently failed.
    pub async fn fail_job(&self, job: &ClaimedMemoryJob, error: &str) -> Result<()> {
        let row: Option<MemoryJobRow> = sqlx::query_as("SELECT * FROM memory_jobs WHERE id = ?")
            .bind(job.row.id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

        let row = row.ok_or_else(|| {
            nexus_core::NexusError::Storage(format!("Memory job {} not found", job.row.id))
        })?;

        let lease_matches =
            row.lease_owner == job.row.lease_owner && row.claim_token == job.row.claim_token;
        if !lease_matches {
            return Err(nexus_core::NexusError::Storage(format!(
                "Memory job {} failure lost lease ownership",
                job.row.id
            )));
        }

        if row.attempts >= MAX_JOB_ATTEMPTS {
            // Permanently failed.
            let result = sqlx::query(
                r#"
                UPDATE memory_jobs
                SET status = ?, last_error = ?, lease_owner = NULL, claim_token = NULL, lease_expires_at = NULL, updated_at = datetime('now')
                WHERE id = ? AND lease_owner = ? AND claim_token = ?
                "#,
            )
            .bind(memory_job_status::FAILED)
            .bind(error)
            .bind(job.row.id)
            .bind(job.row.lease_owner.as_deref())
            .bind(job.row.claim_token.as_deref())
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
            if result.rows_affected() == 0 {
                return Err(nexus_core::NexusError::Storage(format!(
                    "Memory job {} failure lost lease ownership",
                    job.row.id
                )));
            }
        } else {
            // Requeue for retry.
            let result = sqlx::query(
                r#"
                UPDATE memory_jobs
                SET status = ?, last_error = ?, lease_owner = NULL, claim_token = NULL, lease_expires_at = NULL, updated_at = datetime('now')
                WHERE id = ? AND lease_owner = ? AND claim_token = ?
                "#,
            )
            .bind(memory_job_status::PENDING)
            .bind(error)
            .bind(job.row.id)
            .bind(job.row.lease_owner.as_deref())
            .bind(job.row.claim_token.as_deref())
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
            if result.rows_affected() == 0 {
                return Err(nexus_core::NexusError::Storage(format!(
                    "Memory job {} failure lost lease ownership",
                    job.row.id
                )));
            }
        }

        Ok(())
    }

    pub async fn get_most_reinforced_by_namespace(
        &self,
        namespace_id: i64,
        limit: i64,
        include_raw: bool,
    ) -> Result<Vec<Memory>> {
        let noise_sql = if include_raw {
            String::new()
        } else {
            format!("AND {}", RAW_ACTIVITY_FILTER_SQL)
        };
        let rows = sqlx::query_as::<_, MemoryRow>(&format!(
            r#"
            SELECT * FROM memories
            WHERE namespace_id = ?
              AND is_active = 1
              AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level') = ?
              {noise_sql}
            ORDER BY COALESCE(json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.times_reinforced'), 0) DESC,
                     created_at DESC
            LIMIT ?
            "#
        ))
        .bind(namespace_id)
        .bind(CognitiveLevel::Derived.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    pub async fn get_contradictions_by_namespace(
        &self,
        namespace_id: i64,
        limit: i64,
        include_raw: bool,
    ) -> Result<Vec<Memory>> {
        let noise_sql = if include_raw {
            String::new()
        } else {
            format!("AND {}", RAW_ACTIVITY_FILTER_SQL)
        };
        let rows = sqlx::query_as::<_, MemoryRow>(&format!(
            r#"
            SELECT * FROM memories
            WHERE namespace_id = ?
              AND is_active = 1
              AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level') = ?
              {noise_sql}
            ORDER BY COALESCE(json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.times_contradicted'), 0) DESC,
                     created_at DESC
            LIMIT ?
            "#
        ))
        .bind(namespace_id)
        .bind(CognitiveLevel::Contradiction.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    pub async fn list_by_session_key(
        &self,
        namespace_id: i64,
        session_key: &str,
        limit: i64,
        include_raw: bool,
    ) -> Result<Vec<Memory>> {
        let noise_sql = if include_raw {
            String::new()
        } else {
            format!("AND {}", RAW_ACTIVITY_FILTER_SQL)
        };
        let rows = sqlx::query_as::<_, MemoryRow>(&format!(
            r#"
            SELECT * FROM memories
            WHERE namespace_id = ?
              AND is_active = 1
              AND (
                  json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.session_key') = ?
                  OR EXISTS (
                      SELECT 1
                      FROM json_each(COALESCE(json_extract(metadata, '$.cognitive.session_keys'), '[]'))
                      WHERE value = ?
                  )
              )
              {noise_sql}
            ORDER BY created_at DESC
            LIMIT ?
            "#
        ))
        .bind(namespace_id)
        .bind(session_key)
        .bind(session_key)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    /// Register a session digest for a memory.
    ///
    /// Returns the digest row ID. Uses the unique index on
    /// `(namespace_id, session_key, digest_kind, end_memory_id)` to prevent
    /// duplicate registrations.
    pub async fn store_digest(&self, params: StoreDigestParams<'_>) -> Result<i64> {
        let id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO session_digests (namespace_id, session_key, digest_kind, memory_id, start_memory_id, end_memory_id, token_count, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'))
            ON CONFLICT(namespace_id, session_key, digest_kind, end_memory_id) DO UPDATE SET
                memory_id = excluded.memory_id,
                token_count = excluded.token_count
            RETURNING id
            "#,
        )
        .bind(params.namespace_id)
        .bind(params.session_key)
        .bind(params.digest_kind)
        .bind(params.memory_id)
        .bind(params.start_memory_id)
        .bind(params.end_memory_id)
        .bind(params.token_count as i64)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(id)
    }

    /// Get the latest digest memory for a session and digest kind.
    /// Returns the `Memory` row that the digest points to, or None.
    pub async fn latest_digest_for_session(
        &self,
        namespace_id: i64,
        session_key: &str,
        digest_kind: &str,
    ) -> Result<Option<Memory>> {
        let digest: Option<SessionDigestRow> = sqlx::query_as::<_, SessionDigestRow>(
            "SELECT * FROM session_digests WHERE namespace_id = ? AND session_key = ? AND digest_kind = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(namespace_id)
        .bind(session_key)
        .bind(digest_kind)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        match digest {
            Some(d) => self.get_by_id(d.memory_id).await,
            None => Ok(None),
        }
    }

    /// Get the latest digest memory for a namespace and digest kind, regardless
    /// of session key. Returns the `Memory` row that the digest points to, or
    /// None if no digest exists.
    pub async fn latest_digest_for_namespace(
        &self,
        namespace_id: i64,
        digest_kind: &str,
    ) -> Result<Option<Memory>> {
        let digest: Option<SessionDigestRow> = sqlx::query_as::<_, SessionDigestRow>(
            "SELECT * FROM session_digests WHERE namespace_id = ? AND digest_kind = ? ORDER BY created_at DESC LIMIT 1",
        )
        .bind(namespace_id)
        .bind(digest_kind)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        match digest {
            Some(d) => self.get_by_id(d.memory_id).await,
            None => Ok(None),
        }
    }

    /// Return how much fresh non-raw, non-digest session content has accumulated
    /// since the latest covered digest window.
    pub async fn session_digest_rollover(
        &self,
        namespace_id: i64,
        session_key: &str,
    ) -> Result<SessionDigestRollover> {
        let last_digest_end_memory_id: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(end_memory_id) FROM session_digests WHERE namespace_id = ? AND session_key = ?",
        )
        .bind(namespace_id)
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;

        let (new_memory_count, estimated_new_tokens): (i64, i64) = sqlx::query_as(&format!(
            r#"
            SELECT
                COUNT(*) as new_memory_count,
                COALESCE(SUM((LENGTH(content) + 3) / 4), 0) as estimated_new_tokens
            FROM memories
            WHERE namespace_id = ?
              AND is_active = 1
              AND id > ?
              AND (
                  json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.session_key') = ?
                  OR EXISTS (
                      SELECT 1
                      FROM json_each(COALESCE(json_extract(metadata, '$.cognitive.session_keys'), '[]'))
                      WHERE value = ?
                  )
              )
              AND {RAW_ACTIVITY_FILTER_SQL}
              AND COALESCE(json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level'), '') NOT IN ('raw', 'summary_short', 'summary_long')
            "#,
        ))
        .bind(namespace_id)
        .bind(last_digest_end_memory_id.unwrap_or(0))
        .bind(session_key)
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(SessionDigestRollover {
            last_digest_end_memory_id,
            new_memory_count,
            estimated_new_tokens,
        })
    }

    /// Get recent memories scoped to a perspective, optionally narrowed to a session.
    ///
    /// By default, raw-activity noise is excluded. Set `include_raw` to `true` to
    /// include operational hook event payloads.
    pub async fn get_recent_by_perspective(
        &self,
        namespace_id: i64,
        perspective: &PerspectiveKey,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        self.get_recent_by_perspective_opts(namespace_id, perspective, limit, false)
            .await
    }

    /// Like [`Self::get_recent_by_perspective`] but allows opting into raw-activity
    /// noise inclusion.
    pub async fn get_recent_by_perspective_opts(
        &self,
        namespace_id: i64,
        perspective: &PerspectiveKey,
        limit: i64,
        include_raw: bool,
    ) -> Result<Vec<Memory>> {
        let noise_sql = if include_raw {
            String::new()
        } else {
            format!("AND {}", RAW_ACTIVITY_FILTER_SQL)
        };

        let sql = if perspective.session_key.is_some() {
            format!(
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                  AND (
                      json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.session_key') = ?
                      OR EXISTS (
                          SELECT 1
                          FROM json_each(COALESCE(json_extract(metadata, '$.cognitive.session_keys'), '[]'))
                          WHERE value = ?
                      )
                  )
                  {noise_sql}
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
        } else {
            format!(
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                  {noise_sql}
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
        };

        let mut query = sqlx::query_as::<_, MemoryRow>(&sql)
            .bind(namespace_id)
            .bind(&perspective.observer)
            .bind(&perspective.subject);

        if let Some(session_key) = &perspective.session_key {
            query = query.bind(session_key);
            query = query.bind(session_key);
        }

        let rows = query
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    /// Get active memories by cognitive level ordered from newest to oldest.
    pub async fn get_by_cognitive_level(
        &self,
        namespace_id: i64,
        level: CognitiveLevel,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        let rows = sqlx::query_as::<_, MemoryRow>(
            r#"
            SELECT * FROM memories
            WHERE namespace_id = ?
              AND is_active = 1
              AND json_extract(COALESCE(metadata, '{}'), '$.cognitive.level') = ?
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(namespace_id)
        .bind(level.as_str())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    /// Get active memories by cognitive level and perspective, ordered from newest to oldest.
    ///
    /// Unlike [`Self::get_by_cognitive_level`], this method applies perspective filtering
    /// (observer, subject, session_key, and session_keys arrays) in the SQL query BEFORE
    /// the LIMIT is applied, ensuring the caller receives up to `limit` matching results.
    pub async fn get_by_cognitive_level_with_perspective(
        &self,
        namespace_id: i64,
        level: CognitiveLevel,
        perspective: &PerspectiveKey,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        let sql = if perspective.session_key.is_some() {
            format!(
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                  AND (
                      json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.session_key') = ?
                      OR EXISTS (
                          SELECT 1
                          FROM json_each(COALESCE(json_extract(metadata, '$.cognitive.session_keys'), '[]'))
                          WHERE value = ?
                      )
                  )
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
        } else {
            format!(
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
        };

        let mut query = sqlx::query_as::<_, MemoryRow>(&sql)
            .bind(namespace_id)
            .bind(level.as_str())
            .bind(&perspective.observer)
            .bind(&perspective.subject);

        if let Some(session_key) = &perspective.session_key {
            query = query.bind(session_key);
            query = query.bind(session_key);
        }

        let rows = query
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    /// Get the most reinforced perspective-aligned memories first.
    ///
    /// By default, raw-activity noise and contradiction memories are excluded.
    pub async fn get_most_reinforced_by_perspective(
        &self,
        namespace_id: i64,
        perspective: &PerspectiveKey,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        self.get_most_reinforced_by_perspective_opts(namespace_id, perspective, limit, false)
            .await
    }

    /// Like [`Self::get_most_reinforced_by_perspective`] but allows opting into
    /// raw-activity noise inclusion.
    pub async fn get_most_reinforced_by_perspective_opts(
        &self,
        namespace_id: i64,
        perspective: &PerspectiveKey,
        limit: i64,
        include_raw: bool,
    ) -> Result<Vec<Memory>> {
        let noise_sql = if include_raw {
            String::new()
        } else {
            format!("AND {}", RAW_ACTIVITY_FILTER_SQL)
        };

        let sql = if perspective.session_key.is_some() {
            format!(
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                  AND (
                      json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.session_key') = ?
                      OR EXISTS (
                          SELECT 1
                          FROM json_each(COALESCE(json_extract(metadata, '$.cognitive.session_keys'), '[]'))
                          WHERE value = ?
                      )
                  )
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level') != ?
                  {noise_sql}
                ORDER BY COALESCE(json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.times_reinforced'), 0) DESC,
                         created_at DESC
                LIMIT ?
                "#,
            )
        } else {
            format!(
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level') != ?
                  {noise_sql}
                ORDER BY COALESCE(json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.times_reinforced'), 0) DESC,
                         created_at DESC
                LIMIT ?
                "#,
            )
        };

        let mut query = sqlx::query_as::<_, MemoryRow>(&sql)
            .bind(namespace_id)
            .bind(&perspective.observer)
            .bind(&perspective.subject);

        if let Some(session_key) = &perspective.session_key {
            query = query.bind(session_key);
            query = query.bind(session_key);
        }

        let rows = query
            .bind(CognitiveLevel::Contradiction.as_str())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    /// Get contradiction memories for the requested perspective.
    ///
    /// By default, raw-activity noise is excluded.
    pub async fn get_contradictions_by_perspective(
        &self,
        namespace_id: i64,
        perspective: &PerspectiveKey,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        self.get_contradictions_by_perspective_opts(namespace_id, perspective, limit, false)
            .await
    }

    /// Like [`Self::get_contradictions_by_perspective`] but allows opting into
    /// raw-activity noise inclusion.
    pub async fn get_contradictions_by_perspective_opts(
        &self,
        namespace_id: i64,
        perspective: &PerspectiveKey,
        limit: i64,
        include_raw: bool,
    ) -> Result<Vec<Memory>> {
        let noise_sql = if include_raw {
            String::new()
        } else {
            format!("AND {}", RAW_ACTIVITY_FILTER_SQL)
        };

        let sql = if perspective.session_key.is_some() {
            format!(
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                  AND (
                      json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.session_key') = ?
                      OR EXISTS (
                          SELECT 1
                          FROM json_each(COALESCE(json_extract(metadata, '$.cognitive.session_keys'), '[]'))
                          WHERE value = ?
                      )
                  )
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level') = ?
                  {noise_sql}
                ORDER BY COALESCE(json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.times_contradicted'), 0) DESC,
                         created_at DESC
                LIMIT ?
                "#,
            )
        } else {
            format!(
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                  AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.level') = ?
                  {noise_sql}
                ORDER BY COALESCE(json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.times_contradicted'), 0) DESC,
                         created_at DESC
                LIMIT ?
                "#,
            )
        };

        let mut query = sqlx::query_as::<_, MemoryRow>(&sql)
            .bind(namespace_id)
            .bind(&perspective.observer)
            .bind(&perspective.subject);

        if let Some(session_key) = &perspective.session_key {
            query = query.bind(session_key);
            query = query.bind(session_key);
        }

        let rows = query
            .bind(CognitiveLevel::Contradiction.as_str())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    // ---------------------------------------------------------------------------
    // Working-set assembly
    // ---------------------------------------------------------------------------

    /// Assemble a bounded working set from multiple retrieval buckets.
    ///
    /// This is the low-level repository primitive used by higher-level services
    /// (e.g. `RepresentationService`) to build a [`WorkingRepresentation`].
    ///
    /// The method queries four buckets in sequence:
    ///
    /// 1. **Digests** — latest short + long session digests
    /// 2. **Reinforced** — highest reinforcement-scored memories
    /// 3. **Recent** — newest perspective-aligned memories
    /// 4. **Contradictions** — highest-confidence contradiction records
    ///
    /// Results are deduplicated by memory ID (first-seen-wins in bucket order)
    /// and truncated to `max_items`.
    pub async fn search_working_set(&self, params: WorkingSetParams<'_>) -> Result<Vec<Memory>> {
        let WorkingSetParams {
            namespace_id,
            perspective,
            max_items,
            include_raw,
        } = params;

        // Each sub-query requests more than max_items so dedup still has enough
        // material after filtering.
        let per_bucket = (max_items as i64).max(8);

        // 1. Digests
        let session = perspective
            .and_then(|p| p.session_key.as_deref())
            .unwrap_or("");
        let mut digests = Vec::new();
        if let Some(short) = self
            .latest_digest_for_session(namespace_id, session, "short")
            .await?
        {
            digests.push(short);
        }
        if let Some(long) = self
            .latest_digest_for_session(namespace_id, session, "long")
            .await?
        {
            digests.push(long);
        }

        // 2-4. Perspective or namespace-level queries
        let reinforced = if let Some(persp) = perspective {
            self.get_most_reinforced_by_perspective_opts(
                namespace_id,
                persp,
                per_bucket,
                include_raw,
            )
            .await?
        } else {
            self.get_most_reinforced_by_namespace(namespace_id, per_bucket, include_raw)
                .await?
        };

        let recent = if let Some(persp) = perspective {
            self.get_recent_by_perspective_opts(namespace_id, persp, per_bucket, include_raw)
                .await?
        } else {
            let sql = if include_raw {
                "SELECT * FROM memories WHERE namespace_id = ? AND is_active = 1 ORDER BY created_at DESC LIMIT ?"
                    .to_string()
            } else {
                format!(
                    "SELECT * FROM memories WHERE namespace_id = ? AND is_active = 1 AND {} ORDER BY created_at DESC LIMIT ?",
                    RAW_ACTIVITY_FILTER_SQL,
                )
            };
            let rows: Vec<MemoryRow> = sqlx::query_as(&sql)
                .bind(namespace_id)
                .bind(per_bucket)
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?;
            rows.into_iter()
                .map(|r| self.row_to_memory(r))
                .collect::<Result<Vec<_>>>()?
        };

        let contradictions = if let Some(persp) = perspective {
            self.get_contradictions_by_perspective_opts(
                namespace_id,
                persp,
                per_bucket,
                include_raw,
            )
            .await?
        } else {
            self.get_contradictions_by_namespace(namespace_id, per_bucket, include_raw)
                .await?
        };

        // Merge: digests → reinforced → recent → contradictions, dedupe by ID
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::with_capacity(max_items);

        for memory in digests
            .into_iter()
            .chain(reinforced)
            .chain(recent)
            .chain(contradictions)
        {
            if seen.insert(memory.id) {
                result.push(memory);
                if result.len() >= max_items {
                    break;
                }
            }
        }

        Ok(result)
    }

    /// Load all evidence lineage entries for a given memory (as derived or source).
    pub async fn load_lineage(&self, memory_id: i64) -> Result<Vec<MemoryLineageEntry>> {
        let rows: Vec<MemoryEvidenceRow> = sqlx::query_as::<_, MemoryEvidenceRow>(
            r#"
            SELECT * FROM memory_evidence
            WHERE derived_memory_id = ? OR source_memory_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(memory_id)
        .bind(memory_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows
            .into_iter()
            .map(|r| MemoryLineageEntry {
                derived_memory_id: r.derived_memory_id,
                source_memory_id: r.source_memory_id,
                evidence_role: r.evidence_role,
            })
            .collect())
    }

    /// Load lineage rows for many source/derived memory IDs in one query.
    pub async fn load_lineage_batch(
        &self,
        memory_ids: &[i64],
    ) -> Result<HashMap<i64, Vec<MemoryLineageEntry>>> {
        if memory_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = memory_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            r#"
            SELECT * FROM memory_evidence
            WHERE derived_memory_id IN ({placeholders})
               OR source_memory_id IN ({placeholders})
            ORDER BY created_at ASC
            "#
        );

        let mut query = sqlx::query_as::<_, MemoryEvidenceRow>(&sql);
        for id in memory_ids {
            query = query.bind(*id);
        }
        for id in memory_ids {
            query = query.bind(*id);
        }

        let rows = query.fetch_all(&self.pool).await.map_err(db_error)?;
        let mut grouped: HashMap<i64, Vec<MemoryLineageEntry>> = HashMap::new();

        for row in rows {
            let entry = MemoryLineageEntry {
                derived_memory_id: row.derived_memory_id,
                source_memory_id: row.source_memory_id,
                evidence_role: row.evidence_role,
            };

            if memory_ids.contains(&entry.derived_memory_id) {
                grouped
                    .entry(entry.derived_memory_id)
                    .or_default()
                    .push(entry.clone());
            }
            if memory_ids.contains(&entry.source_memory_id) {
                grouped
                    .entry(entry.source_memory_id)
                    .or_default()
                    .push(entry);
            }
        }

        Ok(grouped)
    }

    /// Get a memory by ID
    pub async fn get_by_id(&self, id: i64) -> Result<Option<Memory>> {
        let row: Option<MemoryRow> = sqlx::query_as("SELECT * FROM memories WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;

        row.map(|r| self.row_to_memory(r)).transpose()
    }

    /// Get a memory by namespace and content (fallback for id 0 edge case)
    pub async fn get_by_content(&self, namespace_id: i64, content: &str) -> Result<Memory> {
        let row: Option<MemoryRow> = sqlx::query_as(
            "SELECT * FROM memories WHERE namespace_id = ? AND LOWER(TRIM(content)) = LOWER(TRIM(?)) AND is_active = 1 ORDER BY created_at DESC LIMIT 1"
        )
        .bind(namespace_id)
        .bind(content)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        row.map(|r| self.row_to_memory(r))
            .transpose()?
            .ok_or_else(|| {
                nexus_core::NexusError::Storage(
                    "No memories found in namespace after insert".to_string(),
                )
            })
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

        rows.into_iter().map(|r| self.row_to_memory(r)).collect()
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

    /// Count all memories in namespace (including inactive/archived)
    pub async fn count_all_by_namespace(&self, namespace_id: i64) -> Result<i64> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memories WHERE namespace_id = ?")
            .bind(namespace_id)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(count.0)
    }

    /// Count archived memories in namespace
    pub async fn count_archived_by_namespace(&self, namespace_id: i64) -> Result<i64> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memories WHERE namespace_id = ? AND is_archived = 1",
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
            AND is_active = 1
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

    /// Mark multiple memories as consolidated in a single query
    pub async fn mark_consolidated_batch(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let query = format!(
            r#"
            UPDATE memories
            SET metadata = json_set(
                COALESCE(metadata, '{{}}'),
                '$.agent.consolidated',
                true,
                '$.agent.consolidated_at',
                datetime('now')
            ),
            updated_at = datetime('now')
            WHERE id IN ({})
            "#,
            placeholders
        );
        let mut q = sqlx::query(&query);
        for id in ids {
            q = q.bind(*id);
        }
        q.execute(&self.pool).await.map_err(db_error)?;
        Ok(())
    }

    /// Search memories by text content (LIKE search)
    pub async fn search_by_text(
        &self,
        namespace_id: i64,
        query: &str,
        limit: i32,
        include_raw: bool,
    ) -> Result<Vec<MemoryRow>> {
        let pattern = format!("%{}%", query);
        let raw_clause = if include_raw {
            String::new()
        } else {
            format!("AND {RAW_ACTIVITY_FILTER_SQL}")
        };
        let rows = sqlx::query_as::<_, MemoryRow>(&format!(
            r#"
            SELECT * FROM memories
            WHERE namespace_id = ?
            AND is_active = 1
            AND content LIKE ?
            {}
            ORDER BY updated_at DESC
            LIMIT ?
            "#,
            raw_clause
        ))
        .bind(namespace_id)
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows)
    }

    /// Search memories by text content and return domain memories.
    pub async fn search_by_text_memories(
        &self,
        namespace_id: i64,
        query: &str,
        limit: i32,
        include_raw: bool,
    ) -> Result<Vec<Memory>> {
        let rows = self
            .search_by_text(namespace_id, query, limit, include_raw)
            .await?;
        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    /// Fetch recent, embedding-bearing cognition memories for vector-first semantic recall.
    pub async fn get_semantic_candidates(
        &self,
        params: SemanticCandidateParams<'_>,
    ) -> Result<Vec<Memory>> {
        let SemanticCandidateParams {
            namespace_id,
            perspective,
            limit,
            include_raw,
        } = params;

        let noise_sql = if include_raw {
            String::new()
        } else {
            format!("AND {}", RAW_ACTIVITY_FILTER_SQL)
        };

        let rows = if let Some(perspective) = perspective {
            let sql = if perspective.session_key.is_some() {
                format!(
                    r#"
                    SELECT * FROM memories
                    WHERE namespace_id = ?
                      AND is_active = 1
                      AND content_embedding IS NOT NULL
                      AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                      AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                      AND (
                          json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.session_key') = ?
                          OR EXISTS (
                              SELECT 1
                              FROM json_each(COALESCE(json_extract(metadata, '$.cognitive.session_keys'), '[]'))
                              WHERE value = ?
                          )
                      )
                      {noise_sql}
                    ORDER BY updated_at DESC, created_at DESC
                    LIMIT ?
                    "#
                )
            } else {
                format!(
                    r#"
                    SELECT * FROM memories
                    WHERE namespace_id = ?
                      AND is_active = 1
                      AND content_embedding IS NOT NULL
                      AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.observer') = ?
                      AND json_extract(COALESCE(metadata, '{{}}'), '$.cognitive.subject') = ?
                      {noise_sql}
                    ORDER BY updated_at DESC, created_at DESC
                    LIMIT ?
                    "#
                )
            };

            let mut query = sqlx::query_as::<_, MemoryRow>(&sql)
                .bind(namespace_id)
                .bind(&perspective.observer)
                .bind(&perspective.subject);

            if let Some(session_key) = &perspective.session_key {
                query = query.bind(session_key);
                query = query.bind(session_key);
            }

            query
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?
        } else {
            let sql = if include_raw {
                r#"
                SELECT * FROM memories
                WHERE namespace_id = ?
                  AND is_active = 1
                  AND content_embedding IS NOT NULL
                ORDER BY updated_at DESC, created_at DESC
                LIMIT ?
                "#
                .to_string()
            } else {
                format!(
                    r#"
                    SELECT * FROM memories
                    WHERE namespace_id = ?
                      AND is_active = 1
                      AND content_embedding IS NOT NULL
                      AND {}
                    ORDER BY updated_at DESC, created_at DESC
                    LIMIT ?
                    "#,
                    RAW_ACTIVITY_FILTER_SQL,
                )
            };

            sqlx::query_as::<_, MemoryRow>(&sql)
                .bind(namespace_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?
        };

        rows.into_iter()
            .map(|row| self.row_to_memory(row))
            .collect()
    }

    /// List memories with optional filters
    pub async fn list_filtered(
        &self,
        namespace_id: i64,
        filters: ListMemoryFilters<'_>,
    ) -> Result<Vec<Memory>> {
        // Build WHERE clause dynamically
        let mut conditions = vec!["namespace_id = ?1".to_string(), "is_active = 1".to_string()];
        let mut param_idx = 2u32;

        if filters.category.is_some() {
            conditions.push(format!("category = ?{}", param_idx));
            param_idx += 1;
        }
        if filters.since.is_some() {
            conditions.push(format!("created_at >= ?{}", param_idx));
            param_idx += 1;
        }
        if filters.until.is_some() {
            conditions.push(format!("created_at <= ?{}", param_idx));
            param_idx += 1;
        }
        if filters.content_like.is_some() {
            conditions.push(format!("content LIKE ?{}", param_idx));
            param_idx += 1;
        }
        if !filters.include_raw {
            conditions.push(RAW_ACTIVITY_FILTER_SQL.to_string());
        }

        let sql = format!(
            "SELECT * FROM memories WHERE {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
            conditions.join(" AND "),
            param_idx,
            param_idx + 1,
        );

        let mut query = sqlx::query_as::<_, MemoryRow>(&sql).bind(namespace_id);

        if let Some(cat) = filters.category {
            query = query.bind(cat.to_string());
        }
        if let Some(s) = filters.since {
            query = query.bind(s);
        }
        if let Some(u) = filters.until {
            query = query.bind(u);
        }
        if let Some(search) = filters.content_like {
            query = query.bind(format!("%{}%", search));
        }

        let rows: Vec<MemoryRow> = query
            .bind(filters.limit)
            .bind(filters.offset)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;

        rows.into_iter().map(|r| self.row_to_memory(r)).collect()
    }

    /// List memories that still need cognition metadata backfilled.
    pub async fn list_missing_cognitive_metadata(
        &self,
        namespace_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Memory>> {
        let rows: Vec<MemoryRow> = sqlx::query_as(
            r#"
            SELECT * FROM memories
            WHERE namespace_id = ?
            AND json_extract(COALESCE(metadata, '{}'), '$.cognitive.level') IS NULL
            ORDER BY id ASC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(namespace_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        rows.into_iter().map(|r| self.row_to_memory(r)).collect()
    }

    /// Count memories that still need cognition metadata backfilled.
    pub async fn count_missing_cognitive_metadata(&self, namespace_id: i64) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM memories
            WHERE namespace_id = ?
            AND json_extract(COALESCE(metadata, '{}'), '$.cognitive.level') IS NULL
            "#,
        )
        .bind(namespace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(count)
    }

    /// Replace the metadata blob for a single memory.
    pub async fn update_memory_metadata(
        &self,
        memory_id: i64,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        let metadata_json = serde_json::to_string(metadata)?;
        sqlx::query(
            r#"
            UPDATE memories
            SET metadata = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(metadata_json)
        .bind(Utc::now())
        .bind(memory_id)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    /// List distinct session keys that have cognitive metadata but no digest coverage yet.
    pub async fn list_session_keys_without_digests(
        &self,
        namespace_id: i64,
        limit: i64,
    ) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT json_extract(COALESCE(m.metadata, '{}'), '$.cognitive.session_key') AS session_key
            FROM memories m
            WHERE m.namespace_id = ?
            AND json_extract(COALESCE(m.metadata, '{}'), '$.cognitive.session_key') IS NOT NULL
            AND TRIM(json_extract(COALESCE(m.metadata, '{}'), '$.cognitive.session_key')) <> ''
            AND NOT EXISTS (
                SELECT 1 FROM session_digests sd
                WHERE sd.namespace_id = m.namespace_id
                AND sd.session_key = json_extract(COALESCE(m.metadata, '{}'), '$.cognitive.session_key')
            )
            ORDER BY session_key ASC
            LIMIT ?
            "#,
        )
        .bind(namespace_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows.into_iter().map(|(session_key,)| session_key).collect())
    }

    /// Count distinct non-empty cognitive session keys present in active memories.
    pub async fn count_distinct_session_keys_with_cognition(
        &self,
        namespace_id: i64,
    ) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(DISTINCT json_extract(COALESCE(metadata, '{}'), '$.cognitive.session_key'))
            FROM memories
            WHERE namespace_id = ?
              AND is_active = 1
              AND json_extract(COALESCE(metadata, '{}'), '$.cognitive.session_key') IS NOT NULL
              AND TRIM(json_extract(COALESCE(metadata, '{}'), '$.cognitive.session_key')) <> ''
            "#,
        )
        .bind(namespace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(count)
    }

    /// List lineage-backed archived raw-activity memories that are safe to prune.
    pub async fn list_archived_raw_cleanup_candidates(
        &self,
        namespace_id: i64,
        older_than: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        let rows: Vec<MemoryRow> = sqlx::query_as(
            r#"
            SELECT * FROM memories
            WHERE namespace_id = ?
            AND is_active = 0
            AND is_archived = 1
            AND (
                labels LIKE '%raw-activity%'
                OR json_extract(COALESCE(metadata, '{}'), '$.raw_activity') = 1
            )
            AND json_extract(COALESCE(metadata, '{}'), '$.distillation.summary_memory_id') IS NOT NULL
            AND created_at <= ?
            ORDER BY created_at ASC
            LIMIT ?
            "#,
        )
        .bind(namespace_id)
        .bind(older_than)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        rows.into_iter().map(|r| self.row_to_memory(r)).collect()
    }

    /// Count lineage-backed archived raw-activity memories that are safe to prune.
    pub async fn count_archived_raw_cleanup_candidates(
        &self,
        namespace_id: i64,
        older_than: DateTime<Utc>,
    ) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM memories
            WHERE namespace_id = ?
            AND is_active = 0
            AND is_archived = 1
            AND (
                labels LIKE '%raw-activity%'
                OR json_extract(COALESCE(metadata, '{}'), '$.raw_activity') = 1
            )
            AND json_extract(COALESCE(metadata, '{}'), '$.distillation.summary_memory_id') IS NOT NULL
            AND created_at <= ?
            "#,
        )
        .bind(namespace_id)
        .bind(older_than)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(count)
    }

    /// Delete a batch of memories by id.
    pub async fn delete_batch(&self, ids: &[i64]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }

        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM memories WHERE id IN ({placeholders})");
        let mut query = sqlx::query(&sql);
        for id in ids {
            query = query.bind(*id);
        }

        let result = query.execute(&self.pool).await.map_err(db_error)?;
        Ok(result.rows_affected())
    }

    /// Delete memories matching a content pattern (for cleaning noise)
    pub async fn delete_by_content_pattern(&self, namespace_id: i64, pattern: &str) -> Result<u64> {
        let result = sqlx::query("DELETE FROM memories WHERE namespace_id = ? AND content LIKE ?")
            .bind(namespace_id)
            .bind(pattern)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(result.rows_affected())
    }

    /// Count memories matching filters (for stats with time ranges)
    pub async fn count_filtered(
        &self,
        namespace_id: i64,
        category: Option<&str>,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
        include_raw: bool,
    ) -> Result<i64> {
        let mut conditions = vec!["namespace_id = ?1".to_string(), "is_active = 1".to_string()];
        let mut param_idx = 2u32;

        if category.is_some() {
            conditions.push(format!("category = ?{}", param_idx));
            param_idx += 1;
        }
        if since.is_some() {
            conditions.push(format!("created_at >= ?{}", param_idx));
            param_idx += 1;
        }
        if until.is_some() {
            conditions.push(format!("created_at <= ?{}", param_idx));
        }
        if !include_raw {
            conditions.push(RAW_ACTIVITY_FILTER_SQL.to_string());
        }

        let sql = format!(
            "SELECT COUNT(*) FROM memories WHERE {}",
            conditions.join(" AND "),
        );

        let mut query = sqlx::query_scalar::<_, i64>(&sql).bind(namespace_id);

        if let Some(cat) = category {
            query = query.bind(cat.to_string());
        }
        if let Some(s) = since {
            query = query.bind(s);
        }
        if let Some(u) = until {
            query = query.bind(u);
        }

        let count = query.fetch_one(&self.pool).await.map_err(db_error)?;
        Ok(count)
    }

    /// Store a distilled summary and archive its source memories atomically.
    pub async fn store_distilled_summary(
        &self,
        params: StoreMemoryParams<'_>,
        source_ids: &[i64],
    ) -> Result<Memory> {
        let labels_json = serde_json::to_string(params.labels)?;
        let metadata_json = serde_json::to_string(params.metadata)?;
        let embedding_json = params.embedding.map(serde_json::to_string).transpose()?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;

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
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;

        let summary_id = if result.last_insert_rowid() == 0 {
            let row: Option<MemoryRow> = sqlx::query_as(
                "SELECT * FROM memories WHERE namespace_id = ? AND LOWER(TRIM(content)) = LOWER(TRIM(?)) ORDER BY created_at DESC LIMIT 1",
            )
            .bind(params.namespace_id)
            .bind(params.content)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
            row.map(|memory| memory.id).ok_or_else(|| {
                nexus_core::NexusError::Storage(
                    "Duplicate distilled summary merged but matching row not found".to_string(),
                )
            })?
        } else {
            result.last_insert_rowid()
        };

        if !source_ids.is_empty() {
            for source_id in source_ids {
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO memory_evidence (
                        derived_memory_id,
                        source_memory_id,
                        evidence_role,
                        created_at
                    ) VALUES (?, ?, 'source', datetime('now'))
                    "#,
                )
                .bind(summary_id)
                .bind(*source_id)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
            }

            let placeholders = source_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                r#"
                UPDATE memories
                SET
                    is_active = 0,
                    is_archived = 1,
                    updated_at = ?,
                    metadata = json_set(
                        COALESCE(metadata, '{{}}'),
                        '$.distillation.status', 'archived',
                        '$.distillation.summary_memory_id', ?,
                        '$.distillation.archived_at', ?
                    )
                WHERE id IN ({})
                "#,
                placeholders
            );
            let archived_at = Utc::now().to_rfc3339();
            let mut query = sqlx::query(&sql)
                .bind(Utc::now())
                .bind(summary_id)
                .bind(&archived_at);
            for source_id in source_ids {
                query = query.bind(*source_id);
            }
            query.execute(&mut *tx).await.map_err(db_error)?;
        }

        tx.commit().await.map_err(db_error)?;
        self.get_by_id(summary_id).await?.ok_or_else(|| {
            nexus_core::NexusError::Storage(format!(
                "Failed to retrieve distilled summary with id {}",
                summary_id
            ))
        })
    }

    fn row_to_memory(&self, row: MemoryRow) -> Result<Memory> {
        let labels: Vec<String> = serde_json::from_str(&row.labels).map_err(|e| {
            nexus_core::NexusError::Storage(format!(
                "corrupted labels JSON for memory {}: {e}",
                row.id
            ))
        })?;
        let metadata: serde_json::Value =
            serde_json::from_str(&row.metadata).map_err(|e| {
                nexus_core::NexusError::Storage(format!(
                    "corrupted metadata JSON for memory {}: {e}",
                    row.id
                ))
            })?;
        let embedding: Option<Vec<f32>> = row
            .content_embedding
            .map(|e| {
                serde_json::from_str(&e).map_err(|err| {
                    nexus_core::NexusError::Storage(format!(
                        "corrupted embedding JSON for memory {}: {err}",
                        row.id
                    ))
                })
            })
            .transpose()?;

        Ok(Memory {
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
        })
    }

    // ---- Observability queries ----

    /// List memory jobs with optional filters.
    pub async fn list_jobs(
        &self,
        namespace_id: i64,
        job_type: Option<&str>,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MemoryJobRow>> {
        let mut where_clauses = vec!["namespace_id = ?".to_string()];
        if job_type.is_some() {
            where_clauses.push("job_type = ?".to_string());
        }
        if status.is_some() {
            where_clauses.push("status = ?".to_string());
        }
        let where_sql = where_clauses.join(" AND ");

        let sql = format!(
            "SELECT * FROM memory_jobs WHERE {} ORDER BY created_at DESC LIMIT ? OFFSET ?",
            where_sql
        );

        let mut query = sqlx::query_as::<_, MemoryJobRow>(&sql).bind(namespace_id);
        if let Some(jt) = job_type {
            query = query.bind(jt);
        }
        if let Some(st) = status {
            query = query.bind(st);
        }
        query = query.bind(limit).bind(offset);

        let rows = query.fetch_all(&self.pool).await.map_err(db_error)?;
        Ok(rows)
    }

    /// Count memory jobs with optional filters.
    pub async fn count_jobs(
        &self,
        namespace_id: i64,
        job_type: Option<&str>,
        status: Option<&str>,
    ) -> Result<i64> {
        let mut where_clauses = vec!["namespace_id = ?".to_string()];
        if job_type.is_some() {
            where_clauses.push("job_type = ?".to_string());
        }
        if status.is_some() {
            where_clauses.push("status = ?".to_string());
        }
        let where_sql = where_clauses.join(" AND ");

        let sql = format!("SELECT COUNT(*) FROM memory_jobs WHERE {}", where_sql);

        let mut query = sqlx::query_scalar::<_, i64>(&sql).bind(namespace_id);
        if let Some(jt) = job_type {
            query = query.bind(jt);
        }
        if let Some(st) = status {
            query = query.bind(st);
        }

        let count = query.fetch_one(&self.pool).await.map_err(db_error)?;
        Ok(count)
    }

    /// Count memory jobs grouped by status for a namespace.
    pub async fn count_jobs_by_status(
        &self,
        namespace_id: i64,
        job_type: Option<&str>,
    ) -> Result<Vec<(String, i64)>> {
        let mut where_clauses = vec!["namespace_id = ?".to_string()];
        if job_type.is_some() {
            where_clauses.push("job_type = ?".to_string());
        }
        let where_sql = where_clauses.join(" AND ");

        let sql = format!(
            "SELECT status, COUNT(*) as cnt FROM memory_jobs WHERE {} GROUP BY status",
            where_sql
        );

        let mut query = sqlx::query_as::<_, (String, i64)>(&sql).bind(namespace_id);
        if let Some(jt) = job_type {
            query = query.bind(jt);
        }

        let rows = query.fetch_all(&self.pool).await.map_err(db_error)?;
        Ok(rows)
    }

    /// List session digests with optional session_key filter.
    pub async fn list_digests(
        &self,
        namespace_id: i64,
        session_key: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SessionDigestRow>> {
        let mut query = if let Some(sk) = session_key {
            sqlx::query_as::<_, SessionDigestRow>(
                "SELECT * FROM session_digests WHERE namespace_id = ? AND session_key = ? ORDER BY created_at DESC LIMIT ? OFFSET ?"
            )
            .bind(namespace_id)
            .bind(sk)
        } else {
            sqlx::query_as::<_, SessionDigestRow>(
                "SELECT * FROM session_digests WHERE namespace_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?"
            )
            .bind(namespace_id)
        };

        query = query.bind(limit).bind(offset);
        let rows = query.fetch_all(&self.pool).await.map_err(db_error)?;
        Ok(rows)
    }

    /// Count session digests for a namespace, optionally filtered by session_key.
    pub async fn count_digests(&self, namespace_id: i64, session_key: Option<&str>) -> Result<i64> {
        let query = if let Some(sk) = session_key {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM session_digests WHERE namespace_id = ? AND session_key = ?",
            )
            .bind(namespace_id)
            .bind(sk)
        } else {
            sqlx::query_scalar("SELECT COUNT(*) FROM session_digests WHERE namespace_id = ?")
                .bind(namespace_id)
        };

        let count: i64 = query.fetch_one(&self.pool).await.map_err(db_error)?;
        Ok(count)
    }

    /// Count evidence edges for a namespace.
    pub async fn count_evidence(&self, namespace_id: i64) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_evidence WHERE derived_memory_id IN (SELECT id FROM memories WHERE namespace_id = ?)"
        )
        .bind(namespace_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(count)
    }

    /// Record a system metric sample.
    pub async fn record_metric(
        &self,
        metric_name: &str,
        metric_value: f64,
        labels: &serde_json::Value,
    ) -> Result<i64> {
        let labels_json = serde_json::to_string(labels)?;
        let id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO system_metrics (metric_name, metric_value, labels, recorded_at)
            VALUES (?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(metric_name)
        .bind(metric_value)
        .bind(labels_json)
        .bind(Utc::now())
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(id)
    }

    /// Persist multiple metric samples in a single transaction.
    pub async fn record_metrics_batch(&self, samples: &[MetricSample]) -> Result<()> {
        if samples.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(db_error)?;
        for sample in samples {
            let labels_json = serde_json::to_string(&sample.labels)?;
            sqlx::query(
                r#"
                INSERT INTO system_metrics (metric_name, metric_value, labels, recorded_at)
                VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(&sample.metric_name)
            .bind(sample.metric_value)
            .bind(labels_json)
            .bind(Utc::now())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    /// Fetch the newest metric samples for a namespace and optional prefix.
    pub async fn latest_metrics_for_namespace(
        &self,
        namespace_id: i64,
        metric_prefix: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SystemMetricRow>> {
        let limit = limit.max(1);
        let rows = if let Some(prefix) = metric_prefix {
            sqlx::query_as::<_, SystemMetricRow>(
                r#"
                SELECT *
                FROM system_metrics
                WHERE json_extract(COALESCE(labels, '{}'), '$.namespace_id') = ?
                  AND metric_name LIKE ?
                ORDER BY recorded_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(namespace_id)
            .bind(format!("{prefix}%"))
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?
        } else {
            sqlx::query_as::<_, SystemMetricRow>(
                r#"
                SELECT *
                FROM system_metrics
                WHERE json_extract(COALESCE(labels, '{}'), '$.namespace_id') = ?
                ORDER BY recorded_at DESC, id DESC
                LIMIT ?
                "#,
            )
            .bind(namespace_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?
        };
        Ok(rows)
    }

    /// Count active memories for a namespace at one cognitive level.
    pub async fn count_by_cognitive_level(
        &self,
        namespace_id: i64,
        level: CognitiveLevel,
    ) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM memories
            WHERE namespace_id = ?
              AND is_active = 1
              AND json_extract(COALESCE(metadata, '{}'), '$.cognitive.level') = ?
            "#,
        )
        .bind(namespace_id)
        .bind(level.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(count)
    }
}

async fn insert_memory_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    params: &StoreMemoryParams<'_>,
) -> Result<i64> {
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
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;

    let inserted_id = result.last_insert_rowid();
    if inserted_id != 0 {
        return Ok(inserted_id);
    }

    let row: Option<MemoryRow> = sqlx::query_as(
        "SELECT * FROM memories WHERE namespace_id = ? AND LOWER(TRIM(content)) = LOWER(TRIM(?)) AND is_active = 1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(params.namespace_id)
    .bind(params.content)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?;

    row.map(|memory| memory.id).ok_or_else(|| {
        nexus_core::NexusError::Storage(
            "Duplicate merged by trigger but matching row not found".to_string(),
        )
    })
}

async fn insert_evidence_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    derived_memory_id: i64,
    source_memory_id: i64,
    evidence_role: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO memory_evidence (derived_memory_id, source_memory_id, evidence_role, created_at)
        VALUES (?, ?, ?, datetime('now'))
        "#,
    )
    .bind(derived_memory_id)
    .bind(source_memory_id)
    .bind(evidence_role)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;

    Ok(())
}

fn new_claim_token(lease_owner: &str) -> String {
    let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    format!("{lease_owner}-{nanos}-{}", std::process::id())
}

fn merge_labels(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut merged = existing.to_vec();
    for label in incoming {
        if !merged
            .iter()
            .any(|current| current.eq_ignore_ascii_case(label))
        {
            merged.push(label.clone());
        }
    }
    merged
}

fn merge_duplicate_metadata(
    existing: &serde_json::Value,
    incoming: &serde_json::Value,
) -> serde_json::Value {
    let mut merged = existing.clone();

    if let Some(session_key) = incoming
        .pointer("/cognitive/session_key")
        .and_then(serde_json::Value::as_str)
    {
        let mut session_keys = existing
            .pointer("/cognitive/session_keys")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if let Some(existing_key) = existing
            .pointer("/cognitive/session_key")
            .and_then(serde_json::Value::as_str)
        {
            push_unique_json_string(&mut session_keys, existing_key);
        }
        push_unique_json_string(&mut session_keys, session_key);
        ensure_object_path(&mut merged, "cognitive").insert(
            "session_key".to_string(),
            serde_json::Value::String(session_key.to_string()),
        );
        ensure_object_path(&mut merged, "cognitive").insert(
            "session_keys".to_string(),
            serde_json::Value::Array(session_keys),
        );
    }

    if let Some(derived_session_key) = incoming
        .pointer("/source/derived_session_key")
        .and_then(serde_json::Value::as_str)
    {
        let mut derived_keys = existing
            .pointer("/source/derived_session_keys")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if let Some(existing_key) = existing
            .pointer("/source/derived_session_key")
            .and_then(serde_json::Value::as_str)
        {
            push_unique_json_string(&mut derived_keys, existing_key);
        }
        push_unique_json_string(&mut derived_keys, derived_session_key);
        ensure_object_path(&mut merged, "source").insert(
            "derived_session_key".to_string(),
            serde_json::Value::String(derived_session_key.to_string()),
        );
        ensure_object_path(&mut merged, "source").insert(
            "derived_session_keys".to_string(),
            serde_json::Value::Array(derived_keys),
        );
    }

    merged
}

fn push_unique_json_string(values: &mut Vec<serde_json::Value>, candidate: &str) {
    if values
        .iter()
        .filter_map(serde_json::Value::as_str)
        .any(|current| current.eq_ignore_ascii_case(candidate))
    {
        return;
    }
    values.push(serde_json::Value::String(candidate.to_string()));
}

fn ensure_object_path<'a>(
    root: &'a mut serde_json::Value,
    key: &str,
) -> &'a mut serde_json::Map<String, serde_json::Value> {
    if !root.is_object() {
        *root = serde_json::Value::Object(serde_json::Map::new());
    }

    let object = root.as_object_mut().expect("root object ensured");
    let entry = object
        .entry(key.to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = serde_json::Value::Object(serde_json::Map::new());
    }

    entry.as_object_mut().expect("child object ensured")
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

    /// Check if a file has been successfully processed
    pub async fn is_processed(&self, namespace_id: i64, path: &str) -> Result<bool> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM processed_files WHERE namespace_id = ? AND path = ? AND status = 'completed'")
                .bind(namespace_id)
                .bind(path)
                .fetch_optional(self.pool)
                .await
                .map_err(db_error)?;

        Ok(row.is_some())
    }

    /// Get all completed file paths for a namespace (for batch dedup checks)
    pub async fn get_completed_paths(
        &self,
        namespace_id: i64,
    ) -> Result<std::collections::HashSet<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT path FROM processed_files WHERE namespace_id = ? AND status = 'completed'",
        )
        .bind(namespace_id)
        .fetch_all(self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows.into_iter().map(|r| r.0).collect())
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
    match MemoryCategory::parse(s) {
        Some(cat) => cat,
        None => {
            warn!(category = s, "Unknown memory category in database row; defaulting to General");
            MemoryCategory::General
        }
    }
}

fn parse_memory_lane_type(s: &str) -> Option<MemoryLaneType> {
    MemoryLaneType::parse(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::MemoryLanePriorityType;
    use sqlx::sqlite::SqlitePoolOptions;

    fn cognitive_metadata(
        level: CognitiveLevel,
        perspective: &PerspectiveKey,
        times_reinforced: i64,
        times_contradicted: i64,
    ) -> serde_json::Value {
        serde_json::json!({
            "cognitive": {
                "level": level.as_str(),
                "observer": perspective.observer,
                "subject": perspective.subject,
                "session_key": perspective.session_key,
                "source_memory_ids": [],
                "confidence": 0.9,
                "times_reinforced": times_reinforced,
                "times_contradicted": times_contradicted,
                "generated_by": "test",
            }
        })
    }

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

    #[test]
    fn test_parse_category_all_variants() {
        assert!(matches!(parse_category("general"), Category::General));
        assert!(matches!(parse_category("session"), Category::Session));
        assert!(matches!(parse_category("context"), Category::Context));
        assert!(matches!(
            parse_category("specifications"),
            Category::Specifications
        ));
        assert!(matches!(parse_category("facts"), Category::Facts));
        assert!(matches!(
            parse_category("preferences"),
            Category::Preferences
        ));
        // Unknown falls back to General
        assert!(matches!(parse_category("bogus"), Category::General));
        assert!(matches!(parse_category(""), Category::General));
    }

    #[test]
    fn test_store_memory_params_fields() {
        // Verify StoreMemoryParams can be constructed with all fields
        let params = StoreMemoryParams {
            namespace_id: 1,
            content: "test content",
            category: &Category::General,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::Value::Null,
            embedding: None,
            embedding_model: None,
        };
        assert_eq!(params.namespace_id, 1);
        assert_eq!(params.content, "test content");
        assert!(params.labels.is_empty());
    }

    #[test]
    fn test_merge_duplicate_metadata_preserves_multiple_session_keys() {
        let existing = serde_json::json!({
            "cognitive": {
                "session_key": "session-a"
            },
            "source": {
                "derived_session_key": "session-a"
            }
        });
        let incoming = serde_json::json!({
            "cognitive": {
                "session_key": "session-b"
            },
            "source": {
                "derived_session_key": "session-b"
            }
        });

        let merged = merge_duplicate_metadata(&existing, &incoming);
        assert_eq!(merged["cognitive"]["session_key"], "session-b");
        assert_eq!(
            merged["cognitive"]["session_keys"],
            serde_json::json!(["session-a", "session-b"])
        );
        assert_eq!(
            merged["source"]["derived_session_keys"],
            serde_json::json!(["session-a", "session-b"])
        );
    }

    // ---- Phase 2: Job, Digest, Evidence integration tests ----

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::migrations::run_migrations(&pool).await.unwrap();
        pool
    }

    async fn create_namespace(pool: &SqlitePool, name: &str) -> i64 {
        let ns = NamespaceRepository::new(pool.clone());
        ns.get_or_create(name, "test").await.unwrap();
        ns.get_by_name(name).await.unwrap().unwrap().id
    }

    // ---- Regression tests for audit findings ----

    /// Audit finding #1: `get_by_content` must match `namespace_id + content`,
    /// not "latest active row". This prevents the duplicate-insert fallback
    /// in `store` from returning the wrong memory.
    #[tokio::test]
    async fn test_get_by_content_matches_actual_content() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        // Store two different memories in the same namespace.
        let mem_a = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "first memory content",
                category: &Category::General,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::Value::Null,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let mem_b = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "second memory content",
                category: &Category::General,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::Value::Null,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        assert_ne!(mem_a.id, mem_b.id);

        // get_by_content must return the memory with matching content,
        // NOT the newest one.
        let found_a = repo.get_by_content(ns_id, "first memory content").await.unwrap();
        assert_eq!(found_a.id, mem_a.id);
        assert_eq!(found_a.content, "first memory content");

        let found_b = repo.get_by_content(ns_id, "second memory content").await.unwrap();
        assert_eq!(found_b.id, mem_b.id);
        assert_eq!(found_b.content, "second memory content");

        // Non-existent content must error.
        let result = repo.get_by_content(ns_id, "nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_enqueue_and_claim_jobs() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        // Enqueue two jobs with different priorities.
        let id1 = repo
            .enqueue_job(EnqueueJobParams {
                namespace_id: ns_id,
                job_type: "derive_memory",
                priority: 100,
                perspective: None,
                payload: &serde_json::json!({"memory_id": 1}),
            })
            .await
            .unwrap();

        let id2 = repo
            .enqueue_job(EnqueueJobParams {
                namespace_id: ns_id,
                job_type: "derive_memory",
                priority: 50,
                perspective: None,
                payload: &serde_json::json!({"memory_id": 2}),
            })
            .await
            .unwrap();

        assert!(id1 > 0);
        assert!(id2 > 0);
        assert_ne!(id1, id2);

        // Claim one job — higher priority first.
        let claimed = repo
            .claim_jobs(ns_id, "derive_memory", "worker-1", 120, 1)
            .await
            .unwrap();

        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].row.id, id1); // Higher priority claimed first
        assert_eq!(claimed[0].row.status, "running");
        assert_eq!(claimed[0].payload["memory_id"], 1);

        // Second claim should get the lower-priority job.
        let claimed2 = repo
            .claim_jobs(ns_id, "derive_memory", "worker-2", 120, 1)
            .await
            .unwrap();

        assert_eq!(claimed2.len(), 1);
        assert_eq!(claimed2[0].row.id, id2);
    }

    #[tokio::test]
    async fn test_complete_and_fail_job() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        let _id = repo
            .enqueue_job(EnqueueJobParams {
                namespace_id: ns_id,
                job_type: "digest_session",
                priority: 100,
                perspective: None,
                payload: &serde_json::json!({"session": "s1"}),
            })
            .await
            .unwrap();

        // Claim then complete.
        let claimed = repo
            .claim_jobs(ns_id, "digest_session", "w", 60, 10)
            .await
            .unwrap();
        assert_eq!(claimed.len(), 1);

        repo.complete_job(&claimed[0]).await.unwrap();

        // Verify completed status by attempting to claim — should be empty.
        let claimed_again = repo
            .claim_jobs(ns_id, "digest_session", "w", 60, 10)
            .await
            .unwrap();
        assert!(claimed_again.is_empty());
    }

    #[tokio::test]
    async fn test_fail_job_requeues_before_limit() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        let _id = repo
            .enqueue_job(EnqueueJobParams {
                namespace_id: ns_id,
                job_type: "derive_memory",
                priority: 100,
                perspective: None,
                payload: &serde_json::json!({"test": true}),
            })
            .await
            .unwrap();

        // Claim, fail, re-claim (should work).
        let claimed = repo
            .claim_jobs(ns_id, "derive_memory", "w", 60, 10)
            .await
            .unwrap();
        repo.fail_job(&claimed[0], "transient error").await.unwrap();

        let reclaimed = repo
            .claim_jobs(ns_id, "derive_memory", "w", 60, 10)
            .await
            .unwrap();
        assert_eq!(reclaimed.len(), 1);
        assert_eq!(reclaimed[0].row.attempts, 2);
    }

    #[tokio::test]
    async fn test_complete_job_requires_matching_claim_token() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "derive_memory",
            priority: 100,
            perspective: None,
            payload: &serde_json::json!({"memory_id": 7}),
        })
        .await
        .unwrap();

        let claimed = repo
            .claim_jobs(ns_id, "derive_memory", "worker-1", 60, 1)
            .await
            .unwrap();
        let mut forged = claimed[0].clone();
        forged.row.claim_token = Some("forged-token".to_string());

        let error = repo.complete_job(&forged).await.unwrap_err();
        assert!(error.to_string().contains("lost lease ownership"));
    }

    #[tokio::test]
    async fn test_store_digest_and_latest_digest() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        // Store a memory that will serve as the digest content.
        let digest_memory = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "session summary short",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({"cognitive": {"level": "summary_short"}}),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let digest_id = repo
            .store_digest(StoreDigestParams {
                namespace_id: ns_id,
                session_key: "session-abc",
                digest_kind: "short",
                memory_id: digest_memory.id,
                start_memory_id: Some(1),
                end_memory_id: Some(100),
                token_count: 42,
            })
            .await
            .unwrap();

        assert!(digest_id > 0);

        // Retrieve latest digest.
        let result = repo
            .latest_digest_for_session(ns_id, "session-abc", "short")
            .await
            .unwrap();

        assert!(result.is_some());
        assert_eq!(result.as_ref().unwrap().id, digest_memory.id);

        let replacement_memory = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "session summary short updated",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({"cognitive": {"level": "summary_short"}}),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let replacement_digest_id = repo
            .store_digest(StoreDigestParams {
                namespace_id: ns_id,
                session_key: "session-abc",
                digest_kind: "short",
                memory_id: replacement_memory.id,
                start_memory_id: Some(1),
                end_memory_id: Some(100),
                token_count: 64,
            })
            .await
            .unwrap();

        assert_eq!(replacement_digest_id, digest_id);

        let updated = repo
            .latest_digest_for_session(ns_id, "session-abc", "short")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.id, replacement_memory.id);

        let latest_for_namespace = repo
            .latest_digest_for_namespace(ns_id, "short")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest_for_namespace.id, replacement_memory.id);
    }

    #[tokio::test]
    async fn test_session_digest_rollover_reports_new_signal_since_last_digest() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        let source = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "Implemented bounded digest rollover policy.",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({
                    "cognitive": {
                        "level": "explicit",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "session-rollover"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let first = repo
            .session_digest_rollover(ns_id, "session-rollover")
            .await
            .unwrap();
        assert_eq!(first.last_digest_end_memory_id, None);
        assert_eq!(first.new_memory_count, 1);
        assert!(first.estimated_new_tokens > 0);

        let digest_memory = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "Short digest",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({
                    "cognitive": {
                        "level": "summary_short",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "session-rollover"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        repo.store_digest(StoreDigestParams {
            namespace_id: ns_id,
            session_key: "session-rollover",
            digest_kind: "short",
            memory_id: digest_memory.id,
            start_memory_id: Some(source.id),
            end_memory_id: Some(source.id),
            token_count: 16,
        })
        .await
        .unwrap();

        let covered = repo
            .session_digest_rollover(ns_id, "session-rollover")
            .await
            .unwrap();
        assert_eq!(covered.last_digest_end_memory_id, Some(source.id));
        assert_eq!(covered.new_memory_count, 0);
        assert_eq!(covered.estimated_new_tokens, 0);

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "Added one more explicit memory after the digest coverage window.",
            category: &Category::Session,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "observer": "claude-code",
                    "subject": "claude-code",
                    "session_key": "session-rollover"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let second = repo
            .session_digest_rollover(ns_id, "session-rollover")
            .await
            .unwrap();
        assert_eq!(second.last_digest_end_memory_id, Some(source.id));
        assert_eq!(second.new_memory_count, 1);
        assert!(second.estimated_new_tokens > 0);
    }

    #[tokio::test]
    async fn test_store_with_lineage() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        // Store two source memories.
        let source1 = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "raw observation one",
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::Value::Null,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let source2 = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "raw observation two",
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::Value::Null,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        // Store derived with lineage.
        let derived = repo
            .store_with_lineage(StoreMemoryWithLineageParams {
                store: StoreMemoryParams {
                    namespace_id: ns_id,
                    content: "derived insight",
                    category: &Category::Facts,
                    memory_lane_type: None,
                    labels: &[],
                    metadata: &serde_json::json!({"cognitive": {"level": "derived"}}),
                    embedding: None,
                    embedding_model: None,
                },
                source_memory_ids: &[source1.id, source2.id],
                evidence_role: "derived_from",
            })
            .await
            .unwrap();

        assert_eq!(derived.content, "derived insight");

        // Load lineage for the derived memory.
        let lineage = repo.load_lineage(derived.id).await.unwrap();
        assert_eq!(lineage.len(), 2);
        assert!(lineage.iter().any(|e| e.source_memory_id == source1.id));
        assert!(lineage.iter().any(|e| e.source_memory_id == source2.id));
    }

    #[tokio::test]
    async fn test_cognitive_queries_by_level_and_perspective() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective =
            PerspectiveKey::new("claude-code", "claude-code", Some("session-1".into()));

        let _raw = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "raw note",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Raw, &perspective, 0, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let explicit = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "explicit note",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective, 1, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let derived = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "reinforced insight",
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Derived, &perspective, 7, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let contradiction = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "contradiction note",
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Contradiction, &perspective, 1, 5),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let explicit_rows = repo
            .get_by_cognitive_level(ns_id, CognitiveLevel::Explicit, 10)
            .await
            .unwrap();
        assert_eq!(explicit_rows.len(), 1);
        assert_eq!(explicit_rows[0].id, explicit.id);

        let recent = repo
            .get_recent_by_perspective(ns_id, &perspective, 10)
            .await
            .unwrap();
        assert_eq!(recent.len(), 4);

        let reinforced = repo
            .get_most_reinforced_by_perspective(ns_id, &perspective, 10)
            .await
            .unwrap();
        assert_eq!(reinforced[0].id, derived.id);
        assert!(reinforced
            .iter()
            .all(|memory| memory.id != contradiction.id));

        let contradictions = repo
            .get_contradictions_by_perspective(ns_id, &perspective, 10)
            .await
            .unwrap();
        assert_eq!(contradictions.len(), 1);
        assert_eq!(contradictions[0].id, contradiction.id);
    }

    #[tokio::test]
    async fn test_store_distilled_summary_archives_sources_and_records_lineage() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        let source1 = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "raw event 1",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &["raw-activity".to_string()],
                metadata: &serde_json::json!({"raw_activity": true}),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let source2 = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "raw event 2",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &["raw-activity".to_string()],
                metadata: &serde_json::json!({"raw_activity": true}),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let summary = repo
            .store_distilled_summary(
                StoreMemoryParams {
                    namespace_id: ns_id,
                    content: "distilled summary",
                    category: &Category::Session,
                    memory_lane_type: None,
                    labels: &["activity-summary".to_string()],
                    metadata: &serde_json::json!({"pipeline": "distill-v1"}),
                    embedding: None,
                    embedding_model: None,
                },
                &[source1.id, source2.id],
            )
            .await
            .unwrap();

        let source1_after = repo.get_by_id(source1.id).await.unwrap().unwrap();
        let source2_after = repo.get_by_id(source2.id).await.unwrap().unwrap();
        assert!(!source1_after.is_active);
        assert!(source1_after.is_archived);
        assert!(!source2_after.is_active);
        assert!(source2_after.is_archived);

        let lineage = repo.load_lineage(summary.id).await.unwrap();
        assert_eq!(lineage.len(), 2);
        assert!(lineage.iter().all(|entry| entry.evidence_role == "source"));
    }

    #[tokio::test]
    async fn test_load_lineage_empty() {
        let pool = setup_test_db().await;
        let _ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        let lineage = repo.load_lineage(9999).await.unwrap();
        assert!(lineage.is_empty());
    }

    // ---- Raw-noise exclusion tests ----

    #[tokio::test]
    async fn test_recent_perspective_excludes_raw_noise() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s1".into()));

        // Store a clean memory.
        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "clean observation",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective, 0, 0),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        // Store a raw-activity noise memory (both label and metadata markers).
        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "raw noise payload",
            category: &Category::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": true,
                "cognitive": {
                    "level": "raw",
                    "observer": perspective.observer,
                    "subject": perspective.subject,
                    "session_key": perspective.session_key,
                    "source_memory_ids": [],
                    "confidence": 0.5,
                    "times_reinforced": 0,
                    "times_contradicted": 0,
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        // Default query should exclude the noise.
        let recent = repo
            .get_recent_by_perspective(ns_id, &perspective, 10)
            .await
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "clean observation");

        // With include_raw, both should appear.
        let recent_all = repo
            .get_recent_by_perspective_opts(ns_id, &perspective, 10, true)
            .await
            .unwrap();
        assert_eq!(recent_all.len(), 2);
    }

    #[tokio::test]
    async fn test_semantic_candidates_respect_perspective_and_raw_noise_filtering() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s1".into()));

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "clean semantic observation",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective, 0, 0),
            embedding: Some(&[0.1_f32; 384]),
            embedding_model: Some("mock"),
        })
        .await
        .unwrap();

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "raw semantic noise",
            category: &Category::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": true,
                "cognitive": {
                    "level": "raw",
                    "observer": "claude-code",
                    "subject": "claude-code",
                    "session_key": "s1",
                    "generated_by": "test"
                }
            }),
            embedding: Some(&[0.2_f32; 384]),
            embedding_model: Some("mock"),
        })
        .await
        .unwrap();

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "other perspective semantic",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "observer": "codex",
                    "subject": "codex",
                    "session_key": "s1",
                    "generated_by": "test"
                }
            }),
            embedding: Some(&[0.3_f32; 384]),
            embedding_model: Some("mock"),
        })
        .await
        .unwrap();

        let candidates = repo
            .get_semantic_candidates(SemanticCandidateParams {
                namespace_id: ns_id,
                perspective: Some(&perspective),
                limit: 10,
                include_raw: false,
            })
            .await
            .unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].content, "clean semantic observation");
    }

    #[tokio::test]
    async fn test_semantic_candidates_match_session_keys_array() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s-array".into()));

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "session array semantic observation",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "observer": "claude-code",
                    "subject": "claude-code",
                    "session_keys": ["s-array", "s-other"],
                    "generated_by": "test"
                }
            }),
            embedding: Some(&[0.4_f32; 384]),
            embedding_model: Some("mock"),
        })
        .await
        .unwrap();

        let candidates = repo
            .get_semantic_candidates(SemanticCandidateParams {
                namespace_id: ns_id,
                perspective: Some(&perspective),
                limit: 10,
                include_raw: false,
            })
            .await
            .unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].content, "session array semantic observation");
    }

    #[tokio::test]
    async fn test_reinforced_perspective_excludes_raw_noise() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s1".into()));

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "reinforced insight",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &cognitive_metadata(CognitiveLevel::Derived, &perspective, 5, 0),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "raw noise",
            category: &Category::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": true,
                "cognitive": {
                    "level": "raw",
                    "observer": perspective.observer,
                    "subject": perspective.subject,
                    "session_key": perspective.session_key,
                    "source_memory_ids": [],
                    "confidence": 0.5,
                    "times_reinforced": 0,
                    "times_contradicted": 0,
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let reinforced = repo
            .get_most_reinforced_by_perspective(ns_id, &perspective, 10)
            .await
            .unwrap();
        assert_eq!(reinforced.len(), 1);
        assert_eq!(reinforced[0].content, "reinforced insight");
    }

    #[tokio::test]
    async fn test_contradictions_perspective_excludes_raw_noise() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s1".into()));

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "a real contradiction",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &cognitive_metadata(CognitiveLevel::Contradiction, &perspective, 0, 3),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "raw noise",
            category: &Category::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": true,
                "cognitive": {
                    "level": "raw",
                    "observer": perspective.observer,
                    "subject": perspective.subject,
                    "session_key": perspective.session_key,
                    "source_memory_ids": [],
                    "confidence": 0.5,
                    "times_reinforced": 0,
                    "times_contradicted": 0,
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let contradictions = repo
            .get_contradictions_by_perspective(ns_id, &perspective, 10)
            .await
            .unwrap();
        assert_eq!(contradictions.len(), 1);
        assert_eq!(contradictions[0].content, "a real contradiction");
    }

    // ---- search_working_set tests ----

    #[tokio::test]
    async fn test_search_working_set_basic() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s1".into()));

        // Store memories across different cognitive levels.
        let _raw = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "raw note",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Raw, &perspective, 0, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let explicit = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "explicit fact",
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective, 3, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let derived = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "derived insight",
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Derived, &perspective, 8, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let contradiction = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "contradiction",
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Contradiction, &perspective, 0, 2),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let result = repo
            .search_working_set(WorkingSetParams {
                namespace_id: ns_id,
                perspective: Some(&perspective),
                max_items: 20,
                include_raw: false,
            })
            .await
            .unwrap();

        // Should contain all non-noise memories. Order: reinforced first, then recent,
        // then contradictions.
        assert!(result.len() >= 3);
        let ids: Vec<i64> = result.iter().map(|m| m.id).collect();
        assert!(ids.contains(&explicit.id));
        assert!(ids.contains(&derived.id));
        assert!(ids.contains(&contradiction.id));
    }

    #[tokio::test]
    async fn test_search_working_set_dedupes() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s1".into()));

        // A memory that is both reinforced and recent should appear only once.
        let shared = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "shared memory",
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective, 10, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let result = repo
            .search_working_set(WorkingSetParams {
                namespace_id: ns_id,
                perspective: Some(&perspective),
                max_items: 20,
                include_raw: false,
            })
            .await
            .unwrap();

        let count = result.iter().filter(|m| m.id == shared.id).count();
        assert_eq!(count, 1, "shared memory should appear exactly once");
    }

    #[tokio::test]
    async fn test_search_working_set_respects_max_items() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s1".into()));

        for i in 0..10 {
            let content = format!("memory {}", i);
            repo.store(StoreMemoryParams {
                namespace_id: ns_id,
                content: &content,
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective, i as i64, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }

        let result = repo
            .search_working_set(WorkingSetParams {
                namespace_id: ns_id,
                perspective: Some(&perspective),
                max_items: 3,
                include_raw: false,
            })
            .await
            .unwrap();

        assert_eq!(result.len(), 3);
    }

    #[tokio::test]
    async fn test_search_working_set_excludes_raw_noise() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);
        let perspective = PerspectiveKey::new("claude-code", "claude-code", Some("s1".into()));

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "real observation",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective, 1, 0),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "raw noise",
            category: &Category::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({"raw_activity": true, "cognitive": {"level": "raw"}}),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let result = repo
            .search_working_set(WorkingSetParams {
                namespace_id: ns_id,
                perspective: Some(&perspective),
                max_items: 20,
                include_raw: false,
            })
            .await
            .unwrap();

        assert!(result.iter().all(|m| m.content != "raw noise"));
        assert!(result.iter().any(|m| m.content == "real observation"));
    }

    #[tokio::test]
    async fn test_search_working_set_without_perspective() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "namespace memory one",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({"cognitive": {"level": "explicit"}}),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "namespace memory two",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({"cognitive": {"level": "explicit"}}),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let result = repo
            .search_working_set(WorkingSetParams {
                namespace_id: ns_id,
                perspective: None,
                max_items: 20,
                include_raw: false,
            })
            .await
            .unwrap();

        assert!(result.len() >= 2);
    }

    #[tokio::test]
    async fn test_list_by_session_key_matches_session_keys_array() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "shared explicit memory",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "session_key": "session-b",
                    "session_keys": ["session-a", "session-b"]
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let session_a = repo
            .list_by_session_key(ns_id, "session-a", 10, false)
            .await
            .unwrap();
        let session_b = repo
            .list_by_session_key(ns_id, "session-b", 10, false)
            .await
            .unwrap();

        assert_eq!(session_a.len(), 1);
        assert_eq!(session_b.len(), 1);
    }

    #[tokio::test]
    async fn test_count_evidence_returns_zero_for_empty_namespace() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        let count = repo.count_evidence(ns_id).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_count_evidence_counts_lineage_edges() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        let source = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "source memory",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({}),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let _derived = repo
            .store_with_lineage(StoreMemoryWithLineageParams {
                store: StoreMemoryParams {
                    namespace_id: ns_id,
                    content: "derived with evidence",
                    category: &Category::Facts,
                    memory_lane_type: None,
                    labels: &[],
                    metadata: &serde_json::json!({"cognitive": {"level": "derived"}}),
                    embedding: None,
                    embedding_model: None,
                },
                source_memory_ids: &[source.id],
                evidence_role: "source",
            })
            .await
            .unwrap();

        let count = repo.count_evidence(ns_id).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_count_evidence_does_not_count_other_namespace() {
        let pool = setup_test_db().await;
        let ns_a = create_namespace(&pool, "agent-a").await;
        let ns_b = create_namespace(&pool, "agent-b").await;
        let repo = MemoryRepository::new(pool);

        let source = repo
            .store(StoreMemoryParams {
                namespace_id: ns_a,
                content: "source in ns-a",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({}),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let _derived = repo
            .store_with_lineage(StoreMemoryWithLineageParams {
                store: StoreMemoryParams {
                    namespace_id: ns_a,
                    content: "derived in ns-a",
                    category: &Category::Facts,
                    memory_lane_type: None,
                    labels: &[],
                    metadata: &serde_json::json!({}),
                    embedding: None,
                    embedding_model: None,
                },
                source_memory_ids: &[source.id],
                evidence_role: "source",
            })
            .await
            .unwrap();

        assert_eq!(repo.count_evidence(ns_a).await.unwrap(), 1);
        assert_eq!(repo.count_evidence(ns_b).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_count_by_cognitive_level_returns_matching_total() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "level-counts").await;
        let repo = MemoryRepository::new(pool);

        for (content, level) in [
            ("raw event", CognitiveLevel::Raw),
            ("derived insight", CognitiveLevel::Derived),
            ("derived insight 2", CognitiveLevel::Derived),
            ("contradiction note", CognitiveLevel::Contradiction),
        ] {
            repo.store(StoreMemoryParams {
                namespace_id: ns_id,
                content,
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({
                    "cognitive": {
                        "level": level.as_str(),
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "generated_by": "test"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }

        assert_eq!(
            repo.count_by_cognitive_level(ns_id, CognitiveLevel::Derived)
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            repo.count_by_cognitive_level(ns_id, CognitiveLevel::Contradiction)
                .await
                .unwrap(),
            1
        );
    }

    /// Regression test: `get_by_cognitive_level_with_perspective` must apply
    /// perspective filtering in SQL BEFORE the LIMIT, so callers receive up to
    /// `limit` matching results instead of silently getting fewer.
    #[tokio::test]
    async fn test_get_by_cognitive_level_with_perspective_filters_before_limit() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "perspective-limit").await;
        let repo = MemoryRepository::new(pool);

        let perspective_a = PerspectiveKey::new("alice", "project-x", None);
        let perspective_b = PerspectiveKey::new("bob", "project-y", None);

        // Insert 5 memories for alice + project-x at Explicit level.
        for i in 0..5 {
            repo.store(StoreMemoryParams {
                namespace_id: ns_id,
                content: &format!("alice memory {}", i),
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective_a, 0, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }

        // Insert 5 memories for bob + project-y at Explicit level.
        for i in 0..5 {
            repo.store(StoreMemoryParams {
                namespace_id: ns_id,
                content: &format!("bob memory {}", i),
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective_b, 0, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }

        // Request 3 results for alice's perspective; should get exactly 3, not fewer.
        let alice_results = repo
            .get_by_cognitive_level_with_perspective(ns_id, CognitiveLevel::Explicit, &perspective_a, 3)
            .await
            .unwrap();
        assert_eq!(alice_results.len(), 3);
        assert!(alice_results.iter().all(|m| {
            let meta = &m.metadata;
            let obs = meta.get("cognitive").and_then(|c| c.get("observer")).and_then(|v| v.as_str());
            let sub = meta.get("cognitive").and_then(|c| c.get("subject")).and_then(|v| v.as_str());
            obs == Some("alice") && sub == Some("project-x")
        }));

        // Request 10 results for alice; there are only 5, so capped at 5.
        let alice_many = repo
            .get_by_cognitive_level_with_perspective(ns_id, CognitiveLevel::Explicit, &perspective_a, 10)
            .await
            .unwrap();
        assert_eq!(alice_many.len(), 5);

        // Bob gets a separate set.
        let bob_results = repo
            .get_by_cognitive_level_with_perspective(ns_id, CognitiveLevel::Explicit, &perspective_b, 3)
            .await
            .unwrap();
        assert_eq!(bob_results.len(), 3);
        assert!(bob_results.iter().all(|m| {
            let meta = &m.metadata;
            let obs = meta.get("cognitive").and_then(|c| c.get("observer")).and_then(|v| v.as_str());
            let sub = meta.get("cognitive").and_then(|c| c.get("subject")).and_then(|v| v.as_str());
            obs == Some("bob") && sub == Some("project-y")
        }));
    }

    /// Verifies that the scalar session_key field is respected in the SQL filter.
    #[tokio::test]
    async fn test_get_by_cognitive_level_with_perspective_respects_session_key() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "session-key-scalar").await;
        let repo = MemoryRepository::new(pool);

        let perspective_s1 =
            PerspectiveKey::new("alice", "project-x", Some("session-1".to_string()));
        let perspective_s2 =
            PerspectiveKey::new("alice", "project-x", Some("session-2".to_string()));

        for i in 0..3 {
            repo.store(StoreMemoryParams {
                namespace_id: ns_id,
                content: &format!("s1 memory {}", i),
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Derived, &perspective_s1, 0, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }
        for i in 0..3 {
            repo.store(StoreMemoryParams {
                namespace_id: ns_id,
                content: &format!("s2 memory {}", i),
                category: &Category::Facts,
                memory_lane_type: None,
                labels: &[],
                metadata: &cognitive_metadata(CognitiveLevel::Derived, &perspective_s2, 0, 0),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
        }

        let s1_results = repo
            .get_by_cognitive_level_with_perspective(ns_id, CognitiveLevel::Derived, &perspective_s1, 10)
            .await
            .unwrap();
        assert_eq!(s1_results.len(), 3);
        assert!(s1_results.iter().all(|m| m.content.starts_with("s1")));

        let s2_results = repo
            .get_by_cognitive_level_with_perspective(ns_id, CognitiveLevel::Derived, &perspective_s2, 10)
            .await
            .unwrap();
        assert_eq!(s2_results.len(), 3);
        assert!(s2_results.iter().all(|m| m.content.starts_with("s2")));
    }

    /// Verifies that memories matching only the session_keys array are included.
    #[tokio::test]
    async fn test_get_by_cognitive_level_with_perspective_matches_session_keys_array() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "session-keys-array").await;
        let repo = MemoryRepository::new(pool);

        let perspective =
            PerspectiveKey::new("alice", "project-x", Some("session-a".to_string()));

        // Memory with session_key set to the same key as the perspective.
        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "scalar match",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &cognitive_metadata(CognitiveLevel::Explicit, &perspective, 0, 0),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        // Memory with session_keys array containing the key (but different scalar session_key).
        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "array match",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "observer": "alice",
                    "subject": "project-x",
                    "session_key": "session-other",
                    "session_keys": ["session-a", "session-b"],
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        // Memory that does not match at all.
        repo.store(StoreMemoryParams {
            namespace_id: ns_id,
            content: "no match",
            category: &Category::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "cognitive": {
                    "level": "explicit",
                    "observer": "alice",
                    "subject": "project-x",
                    "session_key": "session-other",
                    "session_keys": ["session-z"],
                    "generated_by": "test"
                }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let results = repo
            .get_by_cognitive_level_with_perspective(ns_id, CognitiveLevel::Explicit, &perspective, 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        let contents: Vec<_> = results.iter().map(|m| m.content.as_str()).collect();
        assert!(contents.contains(&"scalar match"));
        assert!(contents.contains(&"array match"));
    }

    #[tokio::test]
    async fn test_record_metric_and_latest_metrics_for_namespace() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "metric-ns").await;
        let other_ns = create_namespace(&pool, "metric-other").await;
        let repo = MemoryRepository::new(pool);

        repo.record_metric(
            "cognition.query.total_ms",
            12.5,
            &serde_json::json!({"namespace_id": ns_id, "stage": "total", "unit": "ms"}),
        )
        .await
        .unwrap();
        repo.record_metric(
            "cognition.query.total_ms",
            18.0,
            &serde_json::json!({"namespace_id": other_ns, "stage": "total", "unit": "ms"}),
        )
        .await
        .unwrap();
        repo.record_metric(
            "cognition.representation.total_ms",
            4.0,
            &serde_json::json!({"namespace_id": ns_id, "stage": "total", "unit": "ms"}),
        )
        .await
        .unwrap();

        let metrics = repo
            .latest_metrics_for_namespace(ns_id, Some("cognition."), 10)
            .await
            .unwrap();

        assert_eq!(metrics.len(), 2);
        assert!(metrics
            .iter()
            .all(|metric| metric.labels.contains(&ns_id.to_string())));
        assert!(metrics
            .iter()
            .any(|metric| metric.metric_name == "cognition.query.total_ms"));
        assert!(metrics
            .iter()
            .any(|metric| metric.metric_name == "cognition.representation.total_ms"));
        assert!(metrics
            .iter()
            .all(|metric| { metric.metric_name.starts_with("cognition.") }));
    }

    #[tokio::test]
    async fn test_record_metrics_batch_persists_all_samples() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "metric-batch").await;
        let repo = MemoryRepository::new(pool);

        repo.record_metrics_batch(&[
            MetricSample {
                metric_name: "cognition.query.total_ms".to_string(),
                metric_value: 9.5,
                labels: serde_json::json!({"namespace_id": ns_id, "stage": "total", "unit": "ms"}),
            },
            MetricSample {
                metric_name: "cognition.query.answer.total_tokens".to_string(),
                metric_value: 128.0,
                labels: serde_json::json!({"namespace_id": ns_id, "stage": "answer", "unit": "tokens"}),
            },
        ])
        .await
        .unwrap();

        let metrics = repo
            .latest_metrics_for_namespace(ns_id, Some("cognition."), 10)
            .await
            .unwrap();

        assert_eq!(metrics.len(), 2);
    }

    // ---- Observability query tests ----

    #[tokio::test]
    async fn test_list_jobs_returns_enqueued_jobs() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "obs-jobs").await;
        let repo = MemoryRepository::new(pool);

        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "derive",
            priority: 10,
            perspective: None,
            payload: &serde_json::json!({"a": 1}),
        })
        .await
        .unwrap();

        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "digest",
            priority: 5,
            perspective: None,
            payload: &serde_json::json!({"b": 2}),
        })
        .await
        .unwrap();

        // List all jobs.
        let all = repo.list_jobs(ns_id, None, None, 50, 0).await.unwrap();
        assert_eq!(all.len(), 2);

        // Filter by job_type.
        let derive_only = repo
            .list_jobs(ns_id, Some("derive"), None, 50, 0)
            .await
            .unwrap();
        assert_eq!(derive_only.len(), 1);
        assert_eq!(derive_only[0].job_type, "derive");

        // Filter by status (both pending).
        let pending = repo
            .list_jobs(ns_id, None, Some("pending"), 50, 0)
            .await
            .unwrap();
        assert_eq!(pending.len(), 2);

        // Combined filter.
        let digest_pending = repo
            .list_jobs(ns_id, Some("digest"), Some("pending"), 50, 0)
            .await
            .unwrap();
        assert_eq!(digest_pending.len(), 1);
    }

    #[tokio::test]
    async fn test_list_jobs_respects_limit_offset() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "obs-limit").await;
        let repo = MemoryRepository::new(pool);

        for i in 0..5 {
            repo.enqueue_job(EnqueueJobParams {
                namespace_id: ns_id,
                job_type: "derive",
                priority: i,
                perspective: None,
                payload: &serde_json::json!({"i": i}),
            })
            .await
            .unwrap();
        }

        let page1 = repo.list_jobs(ns_id, None, None, 2, 0).await.unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = repo.list_jobs(ns_id, None, None, 2, 2).await.unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = repo.list_jobs(ns_id, None, None, 2, 4).await.unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[tokio::test]
    async fn test_count_jobs_by_status() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "obs-count").await;
        let repo = MemoryRepository::new(pool);

        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "derive",
            priority: 10,
            perspective: None,
            payload: &serde_json::json!({}),
        })
        .await
        .unwrap();

        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "derive",
            priority: 5,
            perspective: None,
            payload: &serde_json::json!({}),
        })
        .await
        .unwrap();

        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "digest",
            priority: 10,
            perspective: None,
            payload: &serde_json::json!({}),
        })
        .await
        .unwrap();

        // All jobs.
        let all_counts = repo.count_jobs_by_status(ns_id, None).await.unwrap();
        let total: i64 = all_counts.iter().map(|(_, c)| c).sum();
        assert_eq!(total, 3);

        // Filtered by job_type.
        let derive_counts = repo
            .count_jobs_by_status(ns_id, Some("derive"))
            .await
            .unwrap();
        let derive_total: i64 = derive_counts.iter().map(|(_, c)| c).sum();
        assert_eq!(derive_total, 2);
    }

    #[tokio::test]
    async fn test_count_jobs_respects_filters() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "obs-job-total").await;
        let repo = MemoryRepository::new(pool);

        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "derive",
            priority: 10,
            perspective: None,
            payload: &serde_json::json!({"index": 1}),
        })
        .await
        .unwrap();
        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "derive",
            priority: 5,
            perspective: None,
            payload: &serde_json::json!({"index": 2}),
        })
        .await
        .unwrap();
        repo.enqueue_job(EnqueueJobParams {
            namespace_id: ns_id,
            job_type: "digest",
            priority: 1,
            perspective: None,
            payload: &serde_json::json!({"index": 3}),
        })
        .await
        .unwrap();

        assert_eq!(repo.count_jobs(ns_id, None, None).await.unwrap(), 3);
        assert_eq!(
            repo.count_jobs(ns_id, Some("derive"), None).await.unwrap(),
            2
        );
        assert_eq!(
            repo.count_jobs(ns_id, Some("derive"), Some("pending"))
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            repo.count_jobs(ns_id, Some("reflect"), Some("pending"))
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn test_list_digests_and_count() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "obs-digests").await;
        let repo = MemoryRepository::new(pool);

        // Store a memory to use as digest content.
        let mem = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "digest content",
                category: &Category::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({"cognitive": {"level": "summary_short"}}),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        repo.store_digest(StoreDigestParams {
            namespace_id: ns_id,
            session_key: "session-1",
            digest_kind: "short",
            memory_id: mem.id,
            start_memory_id: Some(1),
            end_memory_id: Some(10),
            token_count: 50,
        })
        .await
        .unwrap();

        repo.store_digest(StoreDigestParams {
            namespace_id: ns_id,
            session_key: "session-2",
            digest_kind: "long",
            memory_id: mem.id,
            start_memory_id: Some(11),
            end_memory_id: Some(20),
            token_count: 100,
        })
        .await
        .unwrap();

        // List all digests.
        let all = repo.list_digests(ns_id, None, 50, 0).await.unwrap();
        assert_eq!(all.len(), 2);

        let total = repo.count_digests(ns_id, None).await.unwrap();
        assert_eq!(total, 2);

        // Filter by session_key.
        let sess1 = repo
            .list_digests(ns_id, Some("session-1"), 50, 0)
            .await
            .unwrap();
        assert_eq!(sess1.len(), 1);
        assert_eq!(sess1[0].session_key, "session-1");

        let sess1_count = repo.count_digests(ns_id, Some("session-1")).await.unwrap();
        assert_eq!(sess1_count, 1);

        // Filter by non-existent session_key.
        let none = repo
            .list_digests(ns_id, Some("session-none"), 50, 0)
            .await
            .unwrap();
        assert!(none.is_empty());

        let none_count = repo
            .count_digests(ns_id, Some("session-none"))
            .await
            .unwrap();
        assert_eq!(none_count, 0);
    }

    // ---- Phase 2 Task 2: Explicit JSON decode error tests ----

    /// Malformed labels JSON must produce an explicit Storage error,
    /// not silently default to an empty Vec.
    #[tokio::test]
    async fn test_row_to_memory_rejects_malformed_labels() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        // Insert a memory with valid data first.
        let memory = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "corruption test labels",
                category: &Category::General,
                memory_lane_type: None,
                labels: &["valid-label".to_string()],
                metadata: &serde_json::Value::Null,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        // Corrupt the labels in-place to invalid JSON.
        sqlx::query("UPDATE memories SET labels = 'NOT VALID JSON{{{' WHERE id = ?")
            .bind(memory.id)
            .execute(repo.pool())
            .await
            .unwrap();

        let err = repo.get_by_id(memory.id).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("corrupted labels JSON"),
            "expected labels corruption error, got: {msg}"
        );
        assert!(msg.contains(&memory.id.to_string()));
    }

    /// Malformed metadata JSON must produce an explicit Storage error,
    /// not silently default to Value::Null.
    #[tokio::test]
    async fn test_row_to_memory_rejects_malformed_metadata() {
        let pool = setup_test_db().await;
        let repo = MemoryRepository::new(pool);

        // Construct a MemoryRow with corrupted metadata directly,
        // bypassing SQLite expression-index validation on UPDATE.
        let row = MemoryRow {
            id: 999,
            namespace_id: 1,
            content: "test".to_string(),
            category: "general".to_string(),
            memory_lane_type: None,
            labels: "[]".to_string(),
            metadata: "[truncated".to_string(), // invalid JSON
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
        };

        let err = repo.row_to_memory(row).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("corrupted metadata JSON"),
            "expected metadata corruption error, got: {msg}"
        );
    }

    /// Malformed embedding JSON must produce an explicit Storage error,
    /// not silently default to None.
    #[tokio::test]
    async fn test_row_to_memory_rejects_malformed_embedding() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        let memory = repo
            .store(StoreMemoryParams {
                namespace_id: ns_id,
                content: "corruption test embedding",
                category: &Category::General,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::Value::Null,
                embedding: Some(&[0.1, 0.2, 0.3]),
                embedding_model: Some("test-model"),
            })
            .await
            .unwrap();

        sqlx::query("UPDATE memories SET content_embedding = 'not-an-array' WHERE id = ?")
            .bind(memory.id)
            .execute(repo.pool())
            .await
            .unwrap();

        let err = repo.get_by_id(memory.id).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("corrupted embedding JSON"),
            "expected embedding corruption error, got: {msg}"
        );
    }

    /// Malformed job payload JSON must produce an explicit Storage error
    /// from claim_jobs, not silently return Value::Null.
    #[tokio::test]
    async fn test_claim_jobs_rejects_malformed_payload() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        // Insert a job directly with corrupted payload JSON.
        sqlx::query(
            r#"
            INSERT INTO memory_jobs (namespace_id, job_type, status, priority, payload_json, created_at, updated_at)
            VALUES (?, 'derive_memory', 'pending', 100, '{INVALID_JSON}', datetime('now'), datetime('now'))
            "#,
        )
        .bind(ns_id)
        .execute(repo.pool())
        .await
        .unwrap();

        let err = repo
            .claim_jobs(ns_id, "derive_memory", "worker-1", 60, 1)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("corrupted payload JSON"),
            "expected payload corruption error, got: {msg}"
        );
    }

    /// Malformed perspective JSON in claim_jobs must produce an explicit Storage error.
    #[tokio::test]
    async fn test_claim_jobs_rejects_malformed_perspective() {
        let pool = setup_test_db().await;
        let ns_id = create_namespace(&pool, "test-agent").await;
        let repo = MemoryRepository::new(pool);

        // Insert a job with valid payload but corrupted perspective JSON.
        sqlx::query(
            r#"
            INSERT INTO memory_jobs (namespace_id, job_type, status, priority, perspective_json, payload_json, created_at, updated_at)
            VALUES (?, 'derive_memory', 'pending', 100, '{BOGUS}', '{"ok": true}', datetime('now'), datetime('now'))
            "#,
        )
        .bind(ns_id)
        .execute(repo.pool())
        .await
        .unwrap();

        let err = repo
            .claim_jobs(ns_id, "derive_memory", "worker-1", 60, 1)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("corrupted perspective JSON"),
            "expected perspective corruption error, got: {msg}"
        );
    }
}
