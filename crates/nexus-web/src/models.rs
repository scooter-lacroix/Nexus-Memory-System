//! Request and response models for the web dashboard API

use chrono::{DateTime, Utc};
use nexus_core::{Memory, MemoryCategory, MemoryLaneType, AgentNamespace};
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
}
