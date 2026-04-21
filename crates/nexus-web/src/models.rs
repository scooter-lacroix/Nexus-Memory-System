//! Request and response models for the web dashboard API

use chrono::Utc;
use nexus_core::{AgentNamespace, Memory, MemoryCategory, MemoryLaneType};
use serde::{Deserialize, Serialize};

// =============================================================================
// Memory Request/Response Models
// =============================================================================

/// Request to create a new memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMemoryRequest {
    pub content: String,
    pub agent_type: String,
    #[serde(default)]
    pub category: MemoryCategory,
    pub memory_lane_type: Option<MemoryLaneType>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// Request to update an existing memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMemoryRequest {
    pub content: Option<String>,
    pub category: Option<MemoryCategory>,
    pub memory_lane_type: Option<MemoryLaneType>,
    pub labels: Option<Vec<String>>,
    pub metadata: Option<serde_json::Value>,
    pub is_active: Option<bool>,
    pub is_archived: Option<bool>,
}

/// Memory response model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResponse {
    pub id: i64,
    pub content: String,
    pub category: String,
    pub category_description: Option<String>,
    pub memory_lane_type: Option<String>,
    pub labels: Vec<String>,
    pub metadata: serde_json::Value,
    pub similarity_score: Option<f32>,
    pub relevance_score: Option<f32>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub last_accessed: Option<String>,
    pub is_active: bool,
    pub is_archived: bool,
    pub access_count: i64,
}

impl From<Memory> for MemoryResponse {
    fn from(memory: Memory) -> Self {
        Self {
            id: memory.id,
            content: memory.content,
            category: memory.category.to_string(),
            category_description: Some(memory.category.description().to_string()),
            memory_lane_type: memory.memory_lane_type.map(|t| t.to_string()),
            labels: memory.labels,
            metadata: memory.metadata,
            similarity_score: memory.similarity_score,
            relevance_score: memory.relevance_score,
            created_at: memory.created_at.to_rfc3339(),
            updated_at: memory.updated_at.map(|d| d.to_rfc3339()),
            last_accessed: memory.last_accessed.map(|d| d.to_rfc3339()),
            is_active: memory.is_active,
            is_archived: memory.is_archived,
            access_count: memory.access_count,
        }
    }
}

/// Response for listing memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryListResponse {
    pub success: bool,
    pub total: i64,
    pub results: Vec<MemoryResponse>,
    pub query: Option<String>,
    pub agent_type: String,
    pub filters: serde_json::Value,
}

/// Response for creating a memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCreateResponse {
    pub success: bool,
    pub memory_id: Option<i64>,
    pub agent_type: String,
    pub category: String,
    pub error: Option<String>,
}

// =============================================================================
// Search Request/Response Models
// =============================================================================

/// Request for semantic search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_agent_type")]
    pub agent_type: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub category: Option<MemoryCategory>,
    pub memory_lane_type: Option<MemoryLaneType>,
    pub threshold: Option<f32>,
}

fn default_agent_type() -> String {
    "general".to_string()
}

fn default_limit() -> usize {
    20
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            query: String::new(),
            agent_type: default_agent_type(),
            limit: default_limit(),
            offset: 0,
            category: None,
            memory_lane_type: None,
            threshold: None,
        }
    }
}

/// Response for semantic search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub success: bool,
    pub results: Vec<MemoryResponse>,
    pub total: i64,
    pub query: String,
    pub agent_type: String,
    pub filters: serde_json::Value,
    pub error: Option<String>,
}

// =============================================================================
// Namespace Request/Response Models
// =============================================================================

/// Request to create a namespace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNamespaceRequest {
    pub name: String,
    pub agent_type: String,
    pub description: Option<String>,
}

/// Namespace response model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceResponse {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub agent_type: String,
    pub created_at: String,
    pub updated_at: Option<String>,
}

impl From<AgentNamespace> for NamespaceResponse {
    fn from(ns: AgentNamespace) -> Self {
        Self {
            id: ns.id,
            name: ns.name,
            description: ns.description,
            agent_type: ns.agent_type,
            created_at: ns.created_at.to_rfc3339(),
            updated_at: ns.updated_at.map(|d| d.to_rfc3339()),
        }
    }
}

/// Response for listing namespaces
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceListResponse {
    pub success: bool,
    pub namespaces: Vec<NamespaceResponse>,
    pub total: usize,
}

// =============================================================================
// Stats Request/Response Models
// =============================================================================

/// Response for statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub success: bool,
    pub total_memories: i64,
    pub active_memories: i64,
    pub archived_memories: i64,
    pub categories: serde_json::Value,
    pub agents: Vec<AgentStats>,
    pub system_info: Option<SystemInfo>,
}

/// Statistics for a single agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStats {
    pub agent_type: String,
    pub namespace_name: String,
    pub total_memories: i64,
    pub active_memories: i64,
    pub archived_memories: i64,
    pub categories: serde_json::Value,
    pub oldest_memory: Option<String>,
    pub newest_memory: Option<String>,
}

/// System information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub version: String,
    pub uptime_seconds: u64,
    pub active_sessions: usize,
}

// =============================================================================
// Health and Error Response Models
// =============================================================================

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: String,
    pub version: String,
}

/// Error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
    pub detail: Option<String>,
}

// =============================================================================
// WebSocket Message Models
// =============================================================================

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSocketMessageType {
    MemoryStored,
    MemoryUpdated,
    MemoryDeleted,
    SessionStarted,
    SessionEnded,
    StatsUpdated,
    Ping,
    Pong,
    CognitiveDrift,
    DreamCompleted,
    MorningRecall,
}

/// WebSocket message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketMessage {
    #[serde(rename = "type")]
    pub message_type: WebSocketMessageType,
    pub data: serde_json::Value,
    pub timestamp: String,
}

impl WebSocketMessage {
    pub fn new(message_type: WebSocketMessageType, data: serde_json::Value) -> Self {
        Self {
            message_type,
            data,
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    pub fn memory_stored(memory: &MemoryResponse, agent_type: &str) -> Self {
        let data = serde_json::json!({
            "memory": memory,
            "agent_type": agent_type,
        });
        Self::new(WebSocketMessageType::MemoryStored, data)
    }

    pub fn memory_updated(memory_id: i64) -> Self {
        let data = serde_json::json!({
            "memory_id": memory_id,
        });
        Self::new(WebSocketMessageType::MemoryUpdated, data)
    }

    pub fn memory_deleted(memory_id: i64) -> Self {
        let data = serde_json::json!({
            "memory_id": memory_id,
        });
        Self::new(WebSocketMessageType::MemoryDeleted, data)
    }

    pub fn ping() -> Self {
        Self::new(WebSocketMessageType::Ping, serde_json::Value::Null)
    }

    pub fn pong() -> Self {
        Self::new(WebSocketMessageType::Pong, serde_json::Value::Null)
    }

    pub fn cognitive_drift(similarity: f32, agent_type: &str) -> Self {
        let data = serde_json::json!({
            "similarity": similarity,
            "agent_type": agent_type,
        });
        Self::new(WebSocketMessageType::CognitiveDrift, data)
    }

    pub fn dream_completed(agent_type: &str, processed: usize) -> Self {
        let data = serde_json::json!({
            "agent_type": agent_type,
            "processed": processed,
        });
        Self::new(WebSocketMessageType::DreamCompleted, data)
    }

    pub fn morning_recall(namespace: &str, count: usize) -> Self {
        let data = serde_json::json!({
            "namespace": namespace,
            "count": count,
        });
        Self::new(WebSocketMessageType::MorningRecall, data)
    }
}

// =============================================================================
// Agent Request/Response Models
// =============================================================================

/// Request to ingest via the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIngestRequest {
    pub text: String,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "api".to_string()
}

/// Response from agent ingest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIngestResponse {
    pub success: bool,
    pub memory_id: Option<i64>,
    pub summary: Option<String>,
    pub error: Option<String>,
}

/// Request to query the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQueryRequest {
    pub question: String,
}

/// Response from agent query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQueryResponse {
    pub success: bool,
    pub question: String,
    pub answer: Option<String>,
    pub error: Option<String>,
}

/// Response from agent consolidate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConsolidateResponse {
    pub success: bool,
    pub memories_processed: usize,
    pub error: Option<String>,
}

/// Response from agent status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusResponse {
    pub enabled: bool,
    pub namespace: String,
    pub inbox_dir: String,
    pub files_processed: u64,
    pub memories_consolidated: u64,
    pub queries_answered: u64,
    pub last_scan: Option<String>,
    pub last_consolidation: Option<String>,
    pub errors: Vec<String>,
    pub uptime_secs: u64,
}

/// Request to pin or boost a memory in cognitive cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBoostRequest {
    pub memory_id: i64,
    #[serde(default)]
    pub pin: bool,
    pub boost_score: Option<f32>,
    /// Project root path. Required for the web API — the handler does not fall back to cwd.
    #[serde(default)]
    pub root_dir: Option<String>,
}

/// Response from agent boost
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBoostResponse {
    pub success: bool,
    pub error: Option<String>,
}

// =============================================================================
// Observability Request/Response Models
// =============================================================================

/// A single job entry returned by the observability endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobEntry {
    pub id: i64,
    pub job_type: String,
    pub status: String,
    pub priority: i64,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<nexus_storage::MemoryJobRow> for JobEntry {
    fn from(row: nexus_storage::MemoryJobRow) -> Self {
        Self {
            id: row.id,
            job_type: row.job_type,
            status: row.status,
            priority: row.priority,
            attempts: row.attempts,
            last_error: row.last_error,
            lease_owner: row.lease_owner,
            lease_expires_at: row.lease_expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Response for listing jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobListResponse {
    pub success: bool,
    pub namespace: String,
    pub jobs: Vec<JobEntry>,
    pub total: i64,
}

/// Response for job status summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSummaryResponse {
    pub success: bool,
    pub namespace: String,
    pub counts: std::collections::HashMap<String, i64>,
}

/// A single session digest entry returned by the observability endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestEntry {
    pub id: i64,
    pub session_key: String,
    pub digest_kind: String,
    pub memory_id: i64,
    pub start_memory_id: Option<i64>,
    pub end_memory_id: Option<i64>,
    pub token_count: i64,
    pub created_at: String,
}

impl From<nexus_storage::SessionDigestRow> for DigestEntry {
    fn from(row: nexus_storage::SessionDigestRow) -> Self {
        Self {
            id: row.id,
            session_key: row.session_key,
            digest_kind: row.digest_kind,
            memory_id: row.memory_id,
            start_memory_id: row.start_memory_id,
            end_memory_id: row.end_memory_id,
            token_count: row.token_count,
            created_at: row.created_at,
        }
    }
}

/// Response for listing digests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestListResponse {
    pub success: bool,
    pub namespace: String,
    pub digests: Vec<DigestEntry>,
    pub total: i64,
}

/// Response for runtime health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeResponse {
    pub success: bool,
    pub version: String,
    pub uptime_seconds: u64,
    pub db_connected: bool,
    pub agent_enabled: bool,
    pub active_sessions: usize,
}

/// A compact reflection sample shown in the observability endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionSampleEntry {
    pub id: i64,
    pub content: String,
    pub created_at: String,
    pub confidence: Option<f64>,
}

impl From<Memory> for ReflectionSampleEntry {
    fn from(memory: Memory) -> Self {
        let confidence = memory
            .metadata
            .get("cognitive")
            .and_then(|cognitive| cognitive.get("confidence"))
            .and_then(|value| value.as_f64());

        Self {
            id: memory.id,
            content: memory.content,
            created_at: memory.created_at.to_rfc3339(),
            confidence,
        }
    }
}

/// Response for reflection/dream observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionStateResponse {
    pub success: bool,
    pub namespace: String,
    pub contradiction_count: i64,
    pub derived_count: i64,
    pub recent_contradictions: Vec<ReflectionSampleEntry>,
    pub recent_derived: Vec<ReflectionSampleEntry>,
}

/// Response for cognition overview (aggregated job/digest/evidence counts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitionOverviewResponse {
    pub success: bool,
    pub namespace: String,
    pub jobs_by_status: std::collections::HashMap<String, i64>,
    pub digest_count: i64,
    pub evidence_count: i64,
    pub stage_metrics: std::collections::HashMap<String, f64>,
}

/// Response for query introspection (ranking decisions without LLM calls).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryIntrospectionResponse {
    pub success: bool,
    pub namespace: String,
    pub question: String,
    pub introspection: nexus_agent::QueryIntrospection,
}

// =============================================================================
// Operator Dashboard Response Models
// =============================================================================

/// Dream throughput and job lifecycle counters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamState {
    pub completed_reflections: i64,
    pub completed_digests: i64,
    pub failed_jobs: i64,
    pub pending_jobs: i64,
    pub last_dream_at: Option<String>,
}

/// Digest freshness and session coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestFreshnessState {
    pub total_digests: i64,
    pub sessions_with_cognition: i64,
    pub latest_digest_age_seconds: Option<i64>,
    pub latest_digest_at: Option<String>,
}

/// Breakdown of memories by cognitive level for recall composition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallComposition {
    pub raw: i64,
    pub explicit: i64,
    pub derived: i64,
    pub summary_short: i64,
    pub summary_long: i64,
    pub contradiction: i64,
    pub total: i64,
}

/// Adaptive dream scheduling state and configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveDreamState {
    pub enabled: bool,
    pub current_interval_secs: u64,
    pub min_interval_secs: u64,
    pub max_interval_secs: u64,
    pub contradiction_count: i64,
    pub contradiction_density: f64,
}

/// Operator dashboard response — at-a-glance cognition health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardResponse {
    pub success: bool,
    pub namespace: String,
    pub dream: DreamState,
    pub digest: DigestFreshnessState,
    pub recall: RecallComposition,
    pub adaptive: AdaptiveDreamState,
}
