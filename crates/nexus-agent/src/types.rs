//! Types for agent operations

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Result of extracting information from ingested content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestExtraction {
    pub summary: String,
    pub entities: Vec<String>,
    pub topics: Vec<String>,
    pub importance_score: f32,
}

/// Result of consolidation analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    pub summary: String,
    pub insight: String,
    pub connections: Vec<MemoryConnection>,
}

/// Connection between memories discovered during consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConnection {
    pub from_id: i64,
    pub to_id: i64,
    pub relationship: String,
    pub strength: f32,
}

/// Answer to a user query with citations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAnswer {
    pub answer: String,
    pub citations: Vec<MemoryCitation>,
    pub confidence: f32,
}

/// Citation to a memory in a query answer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCitation {
    pub memory_id: i64,
    pub title: String,
    pub excerpt: String,
}

/// Agent status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub enabled: bool,
    pub namespace: String,
    pub inbox_dir: String,
    pub last_scan: Option<DateTime<Utc>>,
    pub last_consolidation: Option<DateTime<Utc>>,
    pub files_processed: u64,
    pub memories_consolidated: u64,
    pub queries_answered: u64,
    pub errors: Vec<String>,
}

/// Request to ingest content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestRequest {
    pub content: String,
    pub source: String,
    pub namespace_id: i64,
}

/// Request to query memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub question: String,
    pub namespace_id: i64,
    pub context_limit: Option<usize>,
}

/// Request to trigger consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidateRequest {
    pub namespace_id: i64,
    pub batch_size: Option<usize>,
}
