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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryAnswer {
    pub answer: String,
    pub citations: Vec<MemoryCitation>,
    pub confidence: f32,
    #[serde(default)]
    pub lineages: Vec<MemoryLineage>,
    /// Optional introspection into ranking decisions. Populated only when
    /// explicitly requested via `query_with_introspection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introspection: Option<QueryIntrospection>,
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

// ---------------------------------------------------------------------------
// Query introspection types (Phase 16)
// ---------------------------------------------------------------------------

/// Why a memory was excluded from the final working set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// Cut by `max_items` budget truncation — scored below the inclusion threshold.
    BudgetTruncation,
    /// Cognitive confidence fell below the bucket's inclusion threshold.
    ConfidenceBelowThreshold,
    /// Memory appeared in multiple buckets; the lower-scoring copy was dropped.
    Deduplicated,
}

impl std::fmt::Display for ExclusionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExclusionReason::BudgetTruncation => write!(f, "budget_truncation"),
            ExclusionReason::ConfidenceBelowThreshold => write!(f, "confidence_below_threshold"),
            ExclusionReason::Deduplicated => write!(f, "deduplicated"),
        }
    }
}

/// A memory that was considered but excluded from the final context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedCandidate {
    pub memory_id: i64,
    pub bucket: MemoryBucket,
    pub blended_score: f32,
    pub reason: ExclusionReason,
    pub content_preview: String,
}

/// A structured signal that contributed to a memory's inclusion score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InclusionSignal {
    /// Signal category: "recency", "cognitive_level", "semantic_similarity",
    /// "perspective_match", "bucket_boost", or "raw_activity_penalty".
    pub signal_type: String,
    /// Human-readable description of the signal's effect.
    pub description: String,
    /// Approximate contribution to the blended score.
    pub weight_contribution: f32,
}

/// Human-readable explanation for why a memory was included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InclusionReason {
    pub memory_id: i64,
    pub bucket: MemoryBucket,
    pub phase: String,
    pub relevance_score: Option<f32>,
    pub blended_score: f32,
    pub reason: String,
    /// Structured signals that contributed to the inclusion decision.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<InclusionSignal>,
}

/// A recent dream/reflective inference relevant to the query context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevantReflection {
    pub memory_id: i64,
    pub reflection_type: String,
    pub content_preview: String,
    pub confidence: Option<f64>,
    pub created_at: String,
}

/// Per-bucket statistics for the ranking pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketIntrospectionStats {
    pub bucket: MemoryBucket,
    pub fetched: usize,
    pub included: usize,
    pub excluded: usize,
}

/// Snapshot of the representation configuration used for an introspection query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepresentationConfigSnapshot {
    pub max_items: usize,
    pub include_raw: bool,
    pub include_digests: bool,
    pub include_semantic: bool,
    pub include_derived: bool,
    pub include_contradictions: bool,
}

/// Full introspection of a query's ranking decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryIntrospection {
    pub included: Vec<InclusionReason>,
    pub excluded_candidates: Vec<ExcludedCandidate>,
    pub relevant_reflections: Vec<RelevantReflection>,
    pub bucket_stats: Vec<BucketIntrospectionStats>,
    /// Wall-clock latency of the introspection pipeline in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_latency_ms: Option<u64>,
    /// Snapshot of the representation configuration that shaped this result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representation_config: Option<RepresentationConfigSnapshot>,
}
