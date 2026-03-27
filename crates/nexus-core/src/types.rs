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
        PerspectiveSource::HookIngest
        | PerspectiveSource::SessionLifecycle
        | PerspectiveSource::Digest
        | PerspectiveSource::Reflection
        | PerspectiveSource::Query => observer.clone(),
    };

    PerspectiveKey {
        observer,
        subject: subject_hint
            .filter(|subject| !subject.trim().is_empty())
            .unwrap_or(default_subject),
        session_key: session_key.filter(|key| !key.trim().is_empty()),
    }
}

/// Canonical cognitive metadata envelope stored inside `memory.metadata.cognitive`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CognitiveMetadata {
    pub level: CognitiveLevel,
    pub observer: String,
    pub subject: String,
    pub session_key: Option<String>,
    #[serde(default)]
    pub source_memory_ids: Vec<i64>,
    pub confidence: Option<f32>,
    #[serde(default)]
    pub times_reinforced: i64,
    #[serde(default)]
    pub times_contradicted: i64,
    pub derived_at: Option<DateTime<Utc>>,
    pub generated_by: String,
}

impl CognitiveMetadata {
    pub fn new(
        level: CognitiveLevel,
        observer: impl Into<String>,
        subject: impl Into<String>,
        session_key: Option<String>,
        generated_by: impl Into<String>,
    ) -> Self {
        Self {
            level,
            observer: observer.into(),
            subject: subject.into(),
            session_key,
            source_memory_ids: Vec::new(),
            confidence: None,
            times_reinforced: 0,
            times_contradicted: 0,
            derived_at: Some(Utc::now()),
            generated_by: generated_by.into(),
        }
    }

    pub fn from_metadata(metadata: &serde_json::Value) -> Option<Self> {
        metadata
            .get("cognitive")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    pub fn merge_into(&self, metadata: &serde_json::Value) -> serde_json::Value {
        let mut merged = match metadata {
            serde_json::Value::Object(map) => serde_json::Value::Object(map.clone()),
            _ => serde_json::json!({}),
        };

        if let serde_json::Value::Object(root) = &mut merged {
            root.insert("cognitive".to_string(), self.to_value());
        }

        merged
    }

    pub fn perspective(&self) -> PerspectiveKey {
        PerspectiveKey {
            observer: self.observer.clone(),
            subject: self.subject.clone(),
            session_key: self.session_key.clone(),
        }
    }
}

pub fn cognitive_level_from_metadata(metadata: &serde_json::Value) -> CognitiveLevel {
    CognitiveMetadata::from_metadata(metadata)
        .map(|cognitive| cognitive.level)
        .unwrap_or(CognitiveLevel::Raw)
}

pub fn perspective_from_metadata(metadata: &serde_json::Value) -> Option<PerspectiveKey> {
    CognitiveMetadata::from_metadata(metadata).map(|cognitive| cognitive.perspective())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingRepresentationRequest {
    pub namespace_id: i64,
    pub perspective: Option<PerspectiveKey>,
    pub query: Option<String>,
    pub max_items: usize,
    pub include_raw: bool,
    pub include_recent: bool,
    pub include_semantic: bool,
    pub include_derived: bool,
    pub include_digests: bool,
    pub include_contradictions: bool,
}

impl Default for WorkingRepresentationRequest {
    fn default() -> Self {
        Self {
            namespace_id: 0,
            perspective: None,
            query: None,
            max_items: 24,
            include_raw: false,
            include_recent: true,
            include_semantic: true,
            include_derived: true,
            include_digests: true,
            include_contradictions: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkingRepresentation {
    pub digests: Vec<Memory>,
    pub recent: Vec<Memory>,
    pub semantic: Vec<Memory>,
    pub derived: Vec<Memory>,
    pub contradictions: Vec<Memory>,
}

/// Relation types between memories
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationType {
    Similar,
    Related,
    Parent,
    Child,
    References,
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelationType::Similar => write!(f, "similar"),
            RelationType::Related => write!(f, "related"),
            RelationType::Parent => write!(f, "parent"),
            RelationType::Child => write!(f, "child"),
            RelationType::References => write!(f, "references"),
        }
    }
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

/// Agent namespace for per-agent isolation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNamespace {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub agent_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl AgentNamespace {
    pub fn new(name: impl Into<String>, agent_type: impl Into<String>) -> Self {
        Self {
            id: 0,
            name: name.into(),
            description: None,
            agent_type: agent_type.into(),
            created_at: Utc::now(),
            updated_at: None,
        }
    }
}

/// Task specification for reusable task definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpecification {
    pub id: i64,
    pub namespace_id: i64,
    pub spec_id: String,
    pub task_description: String,
    pub spec_content: serde_json::Value,
    pub complexity_score: f32,
    pub usage_count: i64,
    pub success_rate: f32,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Memory relation for linking memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRelation {
    pub id: i64,
    pub source_memory_id: i64,
    pub target_memory_id: i64,
    pub relation_type: RelationType,
    pub strength: f32,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_display() {
        assert_eq!(MemoryCategory::Facts.to_string(), "facts");
        assert_eq!(MemoryCategory::Preferences.to_string(), "preferences");
    }

    #[test]
    fn test_category_parse() {
        assert_eq!(MemoryCategory::parse("facts"), Some(MemoryCategory::Facts));
        assert_eq!(MemoryCategory::parse("invalid"), None);
    }

    #[test]
    fn test_category_description() {
        assert_eq!(MemoryCategory::Facts.description(), "Factual information");
        assert_eq!(
            MemoryCategory::General.description(),
            "General purpose memories"
        );
    }

    #[test]
    fn test_memory_lane_cognitive_type_display() {
        assert_eq!(MemoryLaneCognitiveType::Semantic.to_string(), "semantic");
        assert_eq!(MemoryLaneCognitiveType::Episodic.to_string(), "episodic");
    }

    #[test]
    fn test_memory_lane_priority_type_display() {
        assert_eq!(MemoryLanePriorityType::Correction.to_string(), "correction");
        assert_eq!(
            MemoryLanePriorityType::PatternSeed.to_string(),
            "pattern_seed"
        );
    }

    #[test]
    fn test_memory_lane_priority_level() {
        assert_eq!(MemoryLanePriorityType::Correction.priority_level(), 1);
        assert_eq!(MemoryLanePriorityType::Insight.priority_level(), 2);
        assert_eq!(MemoryLanePriorityType::PatternSeed.priority_level(), 3);
    }

    #[test]
    fn test_memory_lane_type_parse() {
        let cognitive = MemoryLaneType::parse("semantic");
        assert!(matches!(
            cognitive,
            Some(MemoryLaneType::Cognitive(MemoryLaneCognitiveType::Semantic))
        ));

        let priority = MemoryLaneType::parse("correction");
        assert!(matches!(
            priority,
            Some(MemoryLaneType::Priority(MemoryLanePriorityType::Correction))
        ));
    }

    #[test]
    fn test_memory_default() {
        let memory = Memory::default();
        assert!(memory.is_active);
        assert!(!memory.is_archived);
        assert_eq!(memory.access_count, 0);
        assert_eq!(memory.category, MemoryCategory::General);
    }

    #[test]
    fn test_agent_namespace_new() {
        let ns = AgentNamespace::new("claude-code", "claude");
        assert_eq!(ns.name, "claude-code");
        assert_eq!(ns.agent_type, "claude");
    }

    #[test]
    fn test_search_query_default() {
        let query = SearchQuery::default();
        assert_eq!(query.limit, 10);
        assert_eq!(query.offset, 0);
        assert!(query.use_semantic_search);
    }

    #[test]
    fn test_relation_type_display() {
        assert_eq!(RelationType::Similar.to_string(), "similar");
        assert_eq!(RelationType::Parent.to_string(), "parent");
    }

    #[test]
    fn test_backward_compat_category_alias() {
        // Ensure Category alias works for backward compatibility
        let cat: Category = Category::Facts;
        assert_eq!(cat.to_string(), "facts");
    }

    #[test]
    fn test_cognitive_level_parse_and_display() {
        assert_eq!(CognitiveLevel::parse("raw"), Some(CognitiveLevel::Raw));
        assert_eq!(
            CognitiveLevel::parse("summary_long"),
            Some(CognitiveLevel::SummaryLong)
        );
        assert_eq!(CognitiveLevel::Contradiction.to_string(), "contradiction");
        assert_eq!(CognitiveLevel::parse("unknown"), None);
    }

    #[test]
    fn test_cognitive_level_defaults_to_raw_when_missing() {
        assert_eq!(
            cognitive_level_from_metadata(&serde_json::json!({"source": {"agent": "claude"}})),
            CognitiveLevel::Raw
        );
    }

    #[test]
    fn test_cognitive_metadata_merge_and_parse() {
        let cognitive = CognitiveMetadata {
            level: CognitiveLevel::Derived,
            observer: "agent-a".to_string(),
            subject: "project-x".to_string(),
            session_key: Some("sess-1".to_string()),
            source_memory_ids: vec![1, 2, 3],
            confidence: Some(0.92),
            times_reinforced: 4,
            times_contradicted: 1,
            derived_at: Some(Utc::now()),
            generated_by: "reflect_service".to_string(),
        };
        let merged = cognitive.merge_into(&serde_json::json!({
            "source": { "agent": "agent-a" }
        }));

        let parsed = CognitiveMetadata::from_metadata(&merged).expect("cognitive metadata");
        assert_eq!(parsed.level, CognitiveLevel::Derived);
        assert_eq!(parsed.observer, "agent-a");
        assert_eq!(parsed.subject, "project-x");
        assert_eq!(parsed.source_memory_ids, vec![1, 2, 3]);
        assert!(merged.get("source").is_some());
    }

    #[test]
    fn test_perspective_from_metadata() {
        let metadata = serde_json::json!({
            "cognitive": {
                "level": "explicit",
                "observer": "agent-a",
                "subject": "agent-b",
                "session_key": "sess-2",
                "source_memory_ids": [],
                "confidence": 0.7,
                "times_reinforced": 0,
                "times_contradicted": 0,
                "derived_at": null,
                "generated_by": "derive_service"
            }
        });

        let perspective = perspective_from_metadata(&metadata).expect("perspective");
        assert_eq!(perspective.observer, "agent-a");
        assert_eq!(perspective.subject, "agent-b");
        assert_eq!(perspective.session_key.as_deref(), Some("sess-2"));
    }

    #[test]
    fn test_working_representation_request_default() {
        let request = WorkingRepresentationRequest::default();
        assert_eq!(request.max_items, 24);
        assert!(!request.include_raw);
        assert!(request.include_recent);
        assert!(request.include_semantic);
        assert!(request.include_derived);
        assert!(request.include_digests);
        assert!(request.include_contradictions);
    }

    #[test]
    fn test_infer_perspective_defaults_subject_to_observer() {
        let perspective = infer_perspective(
            PerspectiveSource::HookIngest,
            "agent-a",
            None,
            Some("sess-1".to_string()),
        );
        assert_eq!(perspective.observer, "agent-a");
        assert_eq!(perspective.subject, "agent-a");
        assert_eq!(perspective.session_key.as_deref(), Some("sess-1"));
    }

    #[test]
    fn test_infer_perspective_uses_subject_hint_when_present() {
        let perspective = infer_perspective(
            PerspectiveSource::Reflection,
            "agent-a",
            Some("project-x".to_string()),
            Some("sess-2".to_string()),
        );
        assert_eq!(perspective.observer, "agent-a");
        assert_eq!(perspective.subject, "project-x");
        assert_eq!(perspective.session_key.as_deref(), Some("sess-2"));
    }
}
