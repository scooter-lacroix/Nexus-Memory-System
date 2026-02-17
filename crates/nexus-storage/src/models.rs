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
