//! Session context for extracted data

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Extracted session context from an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    /// Agent type that created this context
    pub agent_type: String,

    /// When extraction started
    pub extraction_started: DateTime<Utc>,

    /// When extraction completed
    pub extraction_completed: Option<DateTime<Utc>>,

    /// Session ID (if available)
    pub session_id: Option<String>,

    /// Conversation messages
    pub conversation: Vec<ConversationMessage>,

    /// Decisions made during session
    pub decisions: Vec<Decision>,

    /// Files worked on
    pub files: Vec<FileInfo>,

    /// Tasks completed
    pub tasks: Vec<TaskInfo>,

    /// Key insights/learnings
    pub insights: Vec<String>,

    /// Errors encountered
    pub errors: Vec<ErrorInfo>,

    /// Subagent executions (for pi-mono, oh-my-pi)
    pub subagent_executions: Vec<SubagentExecution>,

    /// Commands run
    pub commands_run: Vec<String>,

    /// Custom context data
    pub custom: HashMap<String, serde_json::Value>,

    /// Source of extraction
    pub extraction_source: String,

    /// Reliability score (0.0-1.0)
    pub reliability_score: f32,

    /// Optional re-scorer for active sessions
    #[serde(skip)]
    pub rescorer: Option<std::sync::Arc<crate::rescorer::SessionRescorer>>,
}

impl Default for SessionContext {
    fn default() -> Self {
        Self {
            agent_type: String::new(),
            extraction_started: Utc::now(),
            extraction_completed: None,
            session_id: None,
            conversation: Vec::new(),
            decisions: Vec::new(),
            files: Vec::new(),
            tasks: Vec::new(),
            insights: Vec::new(),
            errors: Vec::new(),
            subagent_executions: Vec::new(),
            commands_run: Vec::new(),
            custom: HashMap::new(),
            extraction_source: "unknown".to_string(),
            reliability_score: 1.0,
            rescorer: None,
        }
    }
}

impl SessionContext {
    /// Create a new session context for an agent
    pub fn new(agent_type: impl Into<String>) -> Self {
        Self {
            agent_type: agent_type.into(),
            ..Default::default()
        }
    }

    /// Create with extraction source
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.extraction_source = source.into();
        self
    }

    /// Create with reliability score
    pub fn with_reliability(mut self, score: f32) -> Self {
        self.reliability_score = score.clamp(0.0, 1.0);
        self
    }

    /// Mark extraction as complete
    pub fn complete(&mut self) {
        self.extraction_completed = Some(Utc::now());
    }

    /// Add a conversation message
    pub fn add_message(&mut self, role: impl Into<String>, content: impl Into<String>) {
        let content_str = content.into();
        self.conversation.push(ConversationMessage {
            role: role.into(),
            content: content_str.clone(),
            timestamp: Utc::now(),
        });

        // Trigger re-score if rescorer is present
        if let Some(rescorer) = self.rescorer.as_ref() {
            let rescorer = rescorer.clone();
            let agent_type = self.agent_type.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let config = nexus_core::Config::from_env().unwrap_or_default();
                    let embeddings = if config.embedding.enabled {
                        nexus_memory_agent::runtime::create_embedding_service(&config).await
                    } else {
                        None
                    };
                    if rescorer
                        .on_turn(&content_str, embeddings.as_deref())
                        .await
                        .is_some()
                    {
                        let _ = rescorer.rescore(embeddings.as_deref(), &agent_type).await;
                    }
                });
            }
        }
    }

    /// Add a decision
    pub fn add_decision(&mut self, decision: Decision) {
        self.decisions.push(decision);
    }

    /// Add a file
    pub fn add_file(&mut self, file: FileInfo) {
        self.files.push(file);
    }

    /// Add a task
    pub fn add_task(&mut self, task: TaskInfo) {
        self.tasks.push(task);
    }

    /// Add an insight
    pub fn add_insight(&mut self, insight: impl Into<String>) {
        self.insights.push(insight.into());
    }

    /// Add an error
    pub fn add_error(&mut self, error: ErrorInfo) {
        self.errors.push(error);
    }

    /// Add a command
    pub fn add_command(&mut self, command: impl Into<String>) {
        self.commands_run.push(command.into());
    }

    /// Add custom data
    pub fn add_custom(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.custom.insert(key.into(), value);
    }

    /// Check if context is empty
    pub fn is_empty(&self) -> bool {
        self.conversation.is_empty()
            && self.decisions.is_empty()
            && self.files.is_empty()
            && self.tasks.is_empty()
            && self.insights.is_empty()
            && self.errors.is_empty()
            && self.commands_run.is_empty()
    }

    /// Get summary statistics
    pub fn stats(&self) -> SessionStats {
        SessionStats {
            message_count: self.conversation.len(),
            decision_count: self.decisions.len(),
            file_count: self.files.len(),
            task_count: self.tasks.len(),
            insight_count: self.insights.len(),
            error_count: self.errors.len(),
            command_count: self.commands_run.len(),
        }
    }

    /// Convert to memory content string
    pub fn to_memory_content(&self) -> String {
        let mut parts = Vec::new();

        parts.push(format!("Agent: {}", self.agent_type));
        parts.push(format!("Source: {}", self.extraction_source));
        parts.push(format!(
            "Reliability: {:.0}%",
            self.reliability_score * 100.0
        ));

        if !self.conversation.is_empty() {
            parts.push(format!(
                "\nConversation: {} messages",
                self.conversation.len()
            ));
        }

        if !self.decisions.is_empty() {
            parts.push(format!("\nDecisions: {}", self.decisions.len()));
            for decision in &self.decisions {
                parts.push(format!("  - {}", decision.summary));
            }
        }

        if !self.files.is_empty() {
            parts.push(format!("\nFiles: {}", self.files.len()));
            for file in &self.files {
                parts.push(format!("  - {} ({})", file.path, file.action));
            }
        }

        if !self.insights.is_empty() {
            parts.push(format!("\nInsights: {}", self.insights.len()));
            for insight in &self.insights {
                parts.push(format!("  - {}", insight));
            }
        }

        if !self.errors.is_empty() {
            parts.push(format!("\nErrors: {}", self.errors.len()));
            for error in &self.errors {
                parts.push(format!("  - {}: {}", error.error_type, error.message));
            }
        }

        parts.join("\n")
    }
}

/// Conversation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// Decision made during session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub summary: String,
    pub rationale: Option<String>,
    pub alternatives: Vec<String>,
    pub impact: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl Decision {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            rationale: None,
            alternatives: Vec::new(),
            impact: None,
            timestamp: Utc::now(),
        }
    }
}

/// File operation info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub action: FileAction,
    pub lines_added: Option<usize>,
    pub lines_removed: Option<usize>,
    pub timestamp: DateTime<Utc>,
}

impl FileInfo {
    pub fn new(path: impl Into<String>, action: FileAction) -> Self {
        Self {
            path: path.into(),
            action,
            lines_added: None,
            lines_removed: None,
            timestamp: Utc::now(),
        }
    }
}

/// File action type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    Created,
    Modified,
    Deleted,
    Read,
}

impl std::fmt::Display for FileAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileAction::Created => write!(f, "created"),
            FileAction::Modified => write!(f, "modified"),
            FileAction::Deleted => write!(f, "deleted"),
            FileAction::Read => write!(f, "read"),
        }
    }
}

/// Task info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub description: String,
    pub status: TaskStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub subagent: Option<String>,
}

impl TaskInfo {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            status: TaskStatus::Pending,
            started_at: None,
            completed_at: None,
            subagent: None,
        }
    }
}

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// Error info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub error_type: String,
    pub message: String,
    pub stack_trace: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl ErrorInfo {
    pub fn new(error_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error_type: error_type.into(),
            message: message.into(),
            stack_trace: None,
            timestamp: Utc::now(),
        }
    }
}

/// Subagent execution (for pi-mono, oh-my-pi)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentExecution {
    pub subagent_type: String,
    pub task: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result_summary: Option<String>,
}

/// Session statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub message_count: usize,
    pub decision_count: usize,
    pub file_count: usize,
    pub task_count: usize,
    pub insight_count: usize,
    pub error_count: usize,
    pub command_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_context_new() {
        let ctx = SessionContext::new("claude-code");
        assert_eq!(ctx.agent_type, "claude-code");
        assert!(ctx.is_empty());
    }

    #[test]
    fn test_session_context_add_items() {
        let mut ctx = SessionContext::new("test");

        ctx.add_message("user", "Hello");
        ctx.add_message("assistant", "Hi there!");
        ctx.add_decision(Decision::new("Use Rust"));
        ctx.add_file(FileInfo::new("/src/main.rs", FileAction::Created));
        ctx.add_insight("Rust is fast");
        ctx.add_command("cargo build");

        assert!(!ctx.is_empty());
        assert_eq!(ctx.conversation.len(), 2);
        assert_eq!(ctx.decisions.len(), 1);
        assert_eq!(ctx.files.len(), 1);
        assert_eq!(ctx.insights.len(), 1);
        assert_eq!(ctx.commands_run.len(), 1);
    }

    #[test]
    fn test_session_context_to_memory_content() {
        let mut ctx = SessionContext::new("claude-code")
            .with_source("native")
            .with_reliability(0.95);

        ctx.add_insight("Test insight");

        let content = ctx.to_memory_content();
        assert!(content.contains("claude-code"));
        assert!(content.contains("native"));
        assert!(content.contains("95%"));
        assert!(content.contains("Test insight"));
    }

    #[test]
    fn test_session_stats() {
        let mut ctx = SessionContext::new("test");
        ctx.add_message("user", "test");
        ctx.add_decision(Decision::new("decide"));
        ctx.add_error(ErrorInfo::new("test", "error"));

        let stats = ctx.stats();
        assert_eq!(stats.message_count, 1);
        assert_eq!(stats.decision_count, 1);
        assert_eq!(stats.error_count, 1);
    }
}
