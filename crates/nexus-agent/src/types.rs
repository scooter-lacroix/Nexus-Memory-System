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
    #[serde(default = "default_strength")]
    pub strength: f32,
}

fn default_strength() -> f32 {
    0.5
}

/// Which representation bucket a memory was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryBucket {
    Digests,
    Recent,
    Semantic,
    Derived,
    Contradictions,
}

impl std::fmt::Display for MemoryBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryBucket::Digests => write!(f, "digests"),
            MemoryBucket::Recent => write!(f, "recent"),
            MemoryBucket::Semantic => write!(f, "semantic"),
            MemoryBucket::Derived => write!(f, "derived"),
            MemoryBucket::Contradictions => write!(f, "contradictions"),
        }
    }
}

/// Explains why a memory appeared in query context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLineage {
    pub memory_id: i64,
    pub bucket: MemoryBucket,
    pub phase: String,
    pub relevance_score: Option<f32>,
}

/// Answer to a user query with citations and lineage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAnswer {
    pub answer: String,
    pub citations: Vec<MemoryCitation>,
    pub confidence: f32,
    #[serde(default)]
    pub lineages: Vec<MemoryLineage>,
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
