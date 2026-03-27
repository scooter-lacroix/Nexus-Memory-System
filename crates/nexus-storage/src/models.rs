//! Database models for SQLx

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Database row for memories table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MemoryRow {
    pub id: i64,
    pub namespace_id: i64,
    pub content: String,
    pub category: String,
    pub memory_lane_type: Option<String>,
    pub labels: String,   // JSON array
    pub metadata: String, // JSON object
    pub similarity_score: Option<f32>,
    pub relevance_score: Option<f32>,
    pub content_embedding: Option<String>, // JSON array of f32
    pub embedding_model: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub last_accessed: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub is_archived: bool,
    pub access_count: i64,
}

/// Database row for agent_namespaces table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AgentNamespaceRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub agent_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Database row for task_specifications table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TaskSpecificationRow {
    pub id: i64,
    pub namespace_id: i64,
    pub spec_id: String,
    pub task_description: String,
    pub spec_content: String, // JSON
    pub complexity_score: f32,
    pub usage_count: i64,
    pub success_rate: f32,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Database row for memory_relations table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MemoryRelationRow {
    pub id: i64,
    pub source_memory_id: i64,
    pub target_memory_id: i64,
    pub relation_type: String,
    pub strength: f32,
    pub metadata: Option<String>, // JSON
    pub created_at: DateTime<Utc>,
}

/// Database row for system_metrics table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SystemMetricRow {
    pub id: i64,
    pub metric_name: String,
    pub metric_value: f64,
    pub labels: String, // JSON
    pub recorded_at: DateTime<Utc>,
}

/// Represents a processed file record for inbox deduplication
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProcessedFileRow {
    pub id: i64,
    pub namespace_id: i64,
    pub path: String,
    pub content_hash: Option<String>,
    pub status: String,
    pub memory_id: Option<i64>,
    pub last_error: Option<String>,
    pub processed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Status values for processed files
pub mod processed_file_status {
    pub const PENDING: &str = "pending";
    pub const PROCESSING: &str = "processing";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
}

// ---------------------------------------------------------------------------
// Cognition tables (Phase 2)
// ---------------------------------------------------------------------------

/// Database row for memory_jobs table
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemoryJobRow {
    pub id: i64,
    pub namespace_id: i64,
    pub job_type: String,
    pub status: String,
    pub priority: i64,
    pub perspective_json: Option<String>,
    pub payload_json: String,
    pub lease_owner: Option<String>,
    pub claim_token: Option<String>,
    pub lease_expires_at: Option<String>,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Database row for session_digests table
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionDigestRow {
    pub id: i64,
    pub namespace_id: i64,
    pub session_key: String,
    pub digest_kind: String,
    pub memory_id: i64,
    pub start_memory_id: Option<i64>,
    pub end_memory_id: Option<i64>,
    pub token_count: i64,
    pub created_at: String,
}

/// Database row for memory_evidence table
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemoryEvidenceRow {
    pub id: i64,
    pub derived_memory_id: i64,
    pub source_memory_id: i64,
    pub evidence_role: String,
    pub created_at: String,
}

/// Status values for memory jobs
pub mod memory_job_status {
    pub const PENDING: &str = "pending";
    pub const RUNNING: &str = "running";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
}

/// Evidence role constants
pub mod evidence_role {
    pub const SOURCE: &str = "source";
    pub const DERIVED_FROM: &str = "derived_from";
    pub const CONTRADICTS: &str = "contradicts";
    pub const SUPPORTS: &str = "supports";
}

/// A claimed job with deserialized perspective and payload.
#[derive(Debug, Clone)]
pub struct ClaimedMemoryJob {
    pub row: MemoryJobRow,
    pub perspective: Option<nexus_core::PerspectiveKey>,
    pub payload: serde_json::Value,
}

/// Parameters for enqueuing a new memory job.
pub struct EnqueueJobParams<'a> {
    pub namespace_id: i64,
    pub job_type: &'a str,
    pub priority: i64,
    pub perspective: Option<&'a serde_json::Value>,
    pub payload: &'a serde_json::Value,
}

/// Lineage info for a derived memory.
#[derive(Debug, Clone)]
pub struct MemoryLineageEntry {
    pub derived_memory_id: i64,
    pub source_memory_id: i64,
    pub evidence_role: String,
}
