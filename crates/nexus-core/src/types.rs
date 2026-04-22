//! Core types for Nexus Memory System

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Memory category types (Nexus categories)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MemoryCategory {
    #[default]
    General,
    Facts,
    Preferences,
    Context,
    Specifications,
    Session,
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryCategory::General => write!(f, "general"),
            MemoryCategory::Facts => write!(f, "facts"),
            MemoryCategory::Preferences => write!(f, "preferences"),
            MemoryCategory::Context => write!(f, "context"),
            MemoryCategory::Specifications => write!(f, "specifications"),
            MemoryCategory::Session => write!(f, "session"),
        }
    }
}

impl MemoryCategory {
    /// Get description for this category
    pub fn description(&self) -> &'static str {
        match self {
            MemoryCategory::General => "General purpose memories",
            MemoryCategory::Facts => "Factual information",
            MemoryCategory::Preferences => "User preferences and settings",
            MemoryCategory::Context => "Situational context",
            MemoryCategory::Specifications => "Task specifications",
            MemoryCategory::Session => "Session-based memories and context",
        }
    }

    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "general" => Some(Self::General),
            "facts" => Some(Self::Facts),
            "preferences" => Some(Self::Preferences),
            "context" => Some(Self::Context),
            "specifications" => Some(Self::Specifications),
            "session" => Some(Self::Session),
            _ => None,
        }
    }
}

/// Memory Lane cognitive science-based types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLaneCognitiveType {
    /// General knowledge and facts
    Semantic,
    /// Event-based experiences
    Episodic,
    /// How-to knowledge and processes
    Procedural,
    /// Temporary active processing
    Working,
    /// Conscious declarative facts
    Explicit,
    /// Unconscious patterns
    Implicit,
    /// High-importance events
    Flashbulb,
    /// Knowledge about memory
    Metamemory,
    /// Cross-agent shared knowledge
    Collective,
}

impl std::fmt::Display for MemoryLaneCognitiveType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryLaneCognitiveType::Semantic => write!(f, "semantic"),
            MemoryLaneCognitiveType::Episodic => write!(f, "episodic"),
            MemoryLaneCognitiveType::Procedural => write!(f, "procedural"),
            MemoryLaneCognitiveType::Working => write!(f, "working"),
            MemoryLaneCognitiveType::Explicit => write!(f, "explicit"),
            MemoryLaneCognitiveType::Implicit => write!(f, "implicit"),
            MemoryLaneCognitiveType::Flashbulb => write!(f, "flashbulb"),
            MemoryLaneCognitiveType::Metamemory => write!(f, "metamemory"),
            MemoryLaneCognitiveType::Collective => write!(f, "collective"),
        }
    }
}

impl MemoryLaneCognitiveType {
    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "semantic" => Some(Self::Semantic),
            "episodic" => Some(Self::Episodic),
            "procedural" => Some(Self::Procedural),
            "working" => Some(Self::Working),
            "explicit" => Some(Self::Explicit),
            "implicit" => Some(Self::Implicit),
            "flashbulb" => Some(Self::Flashbulb),
            "metamemory" => Some(Self::Metamemory),
            "collective" => Some(Self::Collective),
            _ => None,
        }
    }
}

/// Memory Lane priority types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLanePriorityType {
    // High priority
    Correction,
    Decision,
    Commitment,
    // Medium priority
    Insight,
    Learning,
    Confidence,
    // Low priority
    PatternSeed,
    CrossAgent,
    WorkflowNote,
    Gap,
}

impl std::fmt::Display for MemoryLanePriorityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryLanePriorityType::Correction => write!(f, "correction"),
            MemoryLanePriorityType::Decision => write!(f, "decision"),
            MemoryLanePriorityType::Commitment => write!(f, "commitment"),
            MemoryLanePriorityType::Insight => write!(f, "insight"),
            MemoryLanePriorityType::Learning => write!(f, "learning"),
            MemoryLanePriorityType::Confidence => write!(f, "confidence"),
            MemoryLanePriorityType::PatternSeed => write!(f, "pattern_seed"),
            MemoryLanePriorityType::CrossAgent => write!(f, "cross_agent"),
            MemoryLanePriorityType::WorkflowNote => write!(f, "workflow_note"),
            MemoryLanePriorityType::Gap => write!(f, "gap"),
        }
    }
}

impl MemoryLanePriorityType {
    /// Get priority level (1=high, 2=medium, 3=low)
    pub fn priority_level(&self) -> u8 {
        match self {
            MemoryLanePriorityType::Correction
            | MemoryLanePriorityType::Decision
            | MemoryLanePriorityType::Commitment => 1,
            MemoryLanePriorityType::Insight
            | MemoryLanePriorityType::Learning
            | MemoryLanePriorityType::Confidence => 2,
            MemoryLanePriorityType::PatternSeed
            | MemoryLanePriorityType::CrossAgent
            | MemoryLanePriorityType::WorkflowNote
            | MemoryLanePriorityType::Gap => 3,
        }
    }

    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "correction" => Some(Self::Correction),
            "decision" => Some(Self::Decision),
            "commitment" => Some(Self::Commitment),
            "insight" => Some(Self::Insight),
            "learning" => Some(Self::Learning),
            "confidence" => Some(Self::Confidence),
            "pattern_seed" => Some(Self::PatternSeed),
            "cross_agent" => Some(Self::CrossAgent),
            "workflow_note" => Some(Self::WorkflowNote),
            "gap" => Some(Self::Gap),
            _ => None,
        }
    }
}

/// Combined Memory Lane type (either cognitive or priority)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MemoryLaneType {
    Cognitive(MemoryLaneCognitiveType),
    Priority(MemoryLanePriorityType),
}

impl std::fmt::Display for MemoryLaneType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryLaneType::Cognitive(t) => write!(f, "{}", t),
            MemoryLaneType::Priority(t) => write!(f, "{}", t),
        }
    }
}

impl MemoryLaneType {
    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        MemoryLaneCognitiveType::parse(s)
            .map(Self::Cognitive)
            .or_else(|| MemoryLanePriorityType::parse(s).map(Self::Priority))
    }
}

// Type alias for backward compatibility
pub type Category = MemoryCategory;

/// Cognitive layer for a memory inside the Nexus cognition system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CognitiveLevel {
    Raw,
    Explicit,
    Derived,
    SummaryShort,
    SummaryLong,
    Contradiction,
}

impl CognitiveLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Explicit => "explicit",
            Self::Derived => "derived",
            Self::SummaryShort => "summary_short",
            Self::SummaryLong => "summary_long",
            Self::Contradiction => "contradiction",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "raw" => Some(Self::Raw),
            "explicit" => Some(Self::Explicit),
            "derived" => Some(Self::Derived),
            "summary_short" => Some(Self::SummaryShort),
            "summary_long" => Some(Self::SummaryLong),
            "contradiction" => Some(Self::Contradiction),
            _ => None,
        }
    }
}

impl std::fmt::Display for CognitiveLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Perspective information for observer-scoped memory retrieval and formation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PerspectiveKey {
    pub observer: String,
    pub subject: String,
    pub session_key: Option<String>,
}

impl PerspectiveKey {
    pub fn new(
        observer: impl Into<String>,
        subject: impl Into<String>,
        session_key: Option<String>,
    ) -> Self {
        Self {
            observer: observer.into(),
            subject: subject.into(),
            session_key,
        }
    }
}

/// Source stage requesting a perspective default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerspectiveSource {
    HookIngest,
    SessionLifecycle,
    Digest,
    Reflection,
    Query,
}

pub fn infer_perspective(
    source: PerspectiveSource,
    observer: impl Into<String>,
    subject_hint: Option<String>,
    session_key: Option<String>,
) -> PerspectiveKey {
    let observer = observer.into();
    let default_subject = match source {
        PerspectiveSource::HookIngest => "agent_activity",
        PerspectiveSource::SessionLifecycle => "user_session",
        PerspectiveSource::Digest => "session_consolidation",
        PerspectiveSource::Reflection => "knowledge_distillation",
        PerspectiveSource::Query => "knowledge_retrieval",
    };
    PerspectiveKey {
        observer,
        subject: subject_hint
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| default_subject.to_string()),
        session_key: session_key.filter(|s| !s.trim().is_empty()),
    }
}

/// Cognitive metadata for a memory, providing provenance and confidence signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveMetadata {
    pub level: CognitiveLevel,
    pub observer: String,
    pub subject: String,
    pub session_key: Option<String>,
    #[serde(default)]
    pub session_keys: Vec<String>,
    #[serde(default)]
    pub source_stage: String,
    #[serde(default)]
    pub source_memory_ids: Vec<i64>,
    #[serde(default)]
    pub times_reinforced: i64,
    #[serde(default)]
    pub times_contradicted: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_belief_revision: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_status: Option<String>,
}

impl CognitiveMetadata {
    pub fn new(
        level: CognitiveLevel,
        observer: impl Into<String>,
        subject: impl Into<String>,
        session_key: Option<String>,
        source_stage: impl Into<String>,
    ) -> Self {
        Self {
            level,
            observer: observer.into(),
            subject: subject.into(),
            session_key,
            session_keys: Vec::new(),
            source_stage: source_stage.into(),
            source_memory_ids: Vec::new(),
            times_reinforced: 0,
            times_contradicted: 0,
            confidence: None,
            generated_by: None,
            derived_at: None,
            last_belief_revision: None,
            resolution_status: None,
        }
    }

    /// Extract cognitive metadata from a memory's generic metadata JSON.
    pub fn from_metadata(metadata: &serde_json::Value) -> Option<Self> {
        metadata
            .get("cognitive")
            .and_then(|value| match serde_json::from_value(value.clone()) {
                Ok(meta) => Some(meta),
                Err(e) => {
                    tracing::debug!("Failed to deserialize cognitive metadata: {e}");
                    None
                }
            })
    }

    /// Merge this cognitive metadata into a generic metadata JSON.
    pub fn merge_into(&self, metadata: &serde_json::Value) -> serde_json::Value {
        let mut map = match metadata {
            serde_json::Value::Object(m) => m.clone(),
            _ => serde_json::Map::new(),
        };
        map.insert(
            "cognitive".to_string(),
            serde_json::to_value(self).unwrap_or(serde_json::Value::Null),
        );
        serde_json::Value::Object(map)
    }
}

pub fn cognitive_level_from_metadata(metadata: &serde_json::Value) -> CognitiveLevel {
    CognitiveMetadata::from_metadata(metadata)
        .map(|m| m.level)
        .unwrap_or(CognitiveLevel::Raw)
}

pub fn perspective_from_metadata(metadata: &serde_json::Value) -> Option<PerspectiveKey> {
    CognitiveMetadata::from_metadata(metadata).map(|m| PerspectiveKey {
        observer: m.observer,
        subject: m.subject,
        session_key: m.session_key,
    })
}

/// Core Memory model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub namespace_id: i64,
    pub content: String,
    pub category: Category,
    pub memory_lane_type: Option<MemoryLaneType>,
    pub labels: Vec<String>,
    pub metadata: serde_json::Value,
    pub similarity_score: Option<f32>,
    pub relevance_score: Option<f32>,
    pub content_embedding: Option<Vec<f32>>,
    pub embedding_model: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub last_accessed: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub is_archived: bool,
    pub access_count: i64,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            id: 0,
            namespace_id: 0,
            content: String::new(),
            category: Category::default(),
            memory_lane_type: None,
            labels: Vec::new(),
            metadata: serde_json::Value::Null,
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
        }
    }
}

impl Memory {
    pub fn level(&self) -> CognitiveLevel {
        cognitive_level_from_metadata(&self.metadata)
    }

    pub fn cognitive_metadata(&self) -> Option<CognitiveMetadata> {
        CognitiveMetadata::from_metadata(&self.metadata)
    }
}

/// Agent namespace for per-agent isolation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNamespace {
    pub id: i64,
    pub name: String,
    pub agent_type: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Request to store a new memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMemoryRequest {
    pub namespace_name: String,
    pub content: String,
    pub category: Category,
    pub memory_lane_type: Option<MemoryLaneType>,
    pub labels: Vec<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Search query for memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub namespace_name: String,
    pub query: String,
    pub category: Option<Category>,
    pub memory_lane_type: Option<MemoryLaneType>,
    pub labels: Vec<String>,
    pub limit: usize,
    pub offset: usize,
    pub use_semantic_search: bool,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            namespace_name: String::new(),
            query: String::new(),
            category: None,
            memory_lane_type: None,
            labels: Vec::new(),
            limit: 10,
            offset: 0,
            use_semantic_search: true,
        }
    }
}

/// Search result with memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub memories: Vec<Memory>,
    pub total_count: i64,
    pub query_time_ms: u64,
}

/// Statistics for a namespace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceStats {
    pub namespace_name: String,
    pub total_memories: i64,
    pub active_memories: i64,
    pub archived_memories: i64,
    pub categories: std::collections::HashMap<String, i64>,
    pub oldest_memory: Option<DateTime<Utc>>,
    pub newest_memory: Option<DateTime<Utc>>,
}

/// A ranked collection of memories ready for LLM context injection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkingRepresentation {
    pub digests: Vec<Memory>,
    pub recent: Vec<Memory>,
    pub semantic: Vec<Memory>,
    pub derived: Vec<Memory>,
    pub contradictions: Vec<Memory>,
}

/// Parameters for building a working representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingRepresentationRequest {
    pub namespace_id: i64,
    pub perspective: Option<PerspectiveKey>,
    pub query: Option<String>,
    pub max_items: usize,
    pub include_raw: bool,
    pub include_digests: bool,
    pub include_recent: bool,
    pub include_semantic: bool,
    pub include_derived: bool,
    pub include_contradictions: bool,
    #[serde(default)]
    pub cross_namespace_ids: Vec<i64>,
}

impl Default for WorkingRepresentationRequest {
    fn default() -> Self {
        Self {
            namespace_id: 0,
            perspective: None,
            query: None,
            max_items: 24,
            include_raw: false,
            include_digests: true,
            include_recent: true,
            include_semantic: true,
            include_derived: true,
            include_contradictions: true,
            cross_namespace_ids: Vec::new(),
        }
    }
}

pub fn canonicalize_agent_type(agent_type: &str) -> String {
    match agent_type.to_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" => "claude-code".to_string(),
        "pi" | "pi-mono" | "pimono" => "pi-mono".to_string(),
        "omp" | "oh-my-pi" | "ohmypi" => "oh-my-pi".to_string(),
        "amp" => "amp".to_string(),
        "codex" => "codex".to_string(),
        "droid" | "factory" => "droid".to_string(),
        _ => agent_type.to_lowercase(),
    }
}

pub fn normalize_project_path(path: &str) -> String {
    // Collapse consecutive slashes, then strip trailing slash (keep root "/").
    let collapsed: String = path.chars().fold(String::new(), |mut acc, c| {
        if c == '/' && acc.ends_with('/') {
            acc
        } else {
            acc.push(c);
            acc
        }
    });
    if collapsed.len() > 1 && collapsed.ends_with('/') {
        collapsed[..collapsed.len() - 1].to_string()
    } else {
        collapsed
    }
}
