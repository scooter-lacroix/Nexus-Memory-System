//! AgentHook trait definition
//!
//! This module defines the core AgentHook trait that all agent hooks must implement.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::Result;
use crate::session::SessionContext;
use crate::types::{ExtractionSource, SessionActivity, SupportTier};

/// Callback type for session end events
pub type SessionEndCallback = Arc<dyn Fn(SessionContext) + Send + Sync>;

/// Describes which lifecycle events an agent hook can handle.
///
/// Each field indicates whether the agent's native hook/config model
/// supports that particular lifecycle event. Agents should override
/// `AgentHook::lifecycle_capabilities()` to report their honest support.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleCapabilities {
    /// Native session start hook support (e.g. Claude Code's SessionStart hook)
    pub session_start: bool,
    /// Session end hook support (via skills, native hooks, or atexit)
    pub session_end: bool,
    /// Periodic checkpoint hook support
    pub checkpoint: bool,
    /// Error-triggered hook support
    pub error_hook: bool,
    /// Compact/compression hook support
    pub compact: bool,
}

impl LifecycleCapabilities {
    /// Helper to create a capabilities set with only session end support.
    pub fn end_only() -> Self {
        Self {
            session_end: true,
            ..Default::default()
        }
    }

    /// Helper to create a monitor-only capabilities set (process detection, atexit).
    pub fn monitor_only() -> Self {
        Self::default()
    }
}

/// Result of a hook operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResult {
    /// Whether the operation succeeded
    pub success: bool,

    /// Agent type
    pub agent_type: String,

    /// Source of the hook trigger
    pub source: ExtractionSource,

    /// Extracted context (if any)
    pub context: Option<SessionContext>,

    /// Error message (if failed)
    pub error: Option<String>,

    /// When this result was created
    pub timestamp: DateTime<Utc>,
}

impl HookResult {
    /// Create a successful result
    pub fn success(agent_type: impl Into<String>, source: ExtractionSource) -> Self {
        Self {
            success: true,
            agent_type: agent_type.into(),
            source,
            context: None,
            error: None,
            timestamp: Utc::now(),
        }
    }

    /// Create a successful result with context
    pub fn success_with_context(
        agent_type: impl Into<String>,
        source: ExtractionSource,
        context: SessionContext,
    ) -> Self {
        Self {
            success: true,
            agent_type: agent_type.into(),
            source,
            context: Some(context),
            error: None,
            timestamp: Utc::now(),
        }
    }

    /// Create a failed result
    pub fn failure(
        agent_type: impl Into<String>,
        source: ExtractionSource,
        error: impl Into<String>,
    ) -> Self {
        Self {
            success: false,
            agent_type: agent_type.into(),
            source,
            context: None,
            error: Some(error.into()),
            timestamp: Utc::now(),
        }
    }
}

/// AgentHook trait - all agent hooks must implement this
///
/// This trait defines the interface for agent-specific hooks that enable
/// automated memory extraction from agent sessions.
///
/// # Implementation Notes
///
/// - All methods are async for non-blocking operation
/// - Hooks should be thread-safe (Send + Sync)
/// - Installation should be idempotent
///
/// # Example
///
/// ```rust,ignore
/// use nexus_memory_hooks::{AgentHook, SessionContext};
/// use async_trait::async_trait;
///
/// struct MyAgentHook {
///     agent_type: String,
/// }
///
/// #[async_trait]
/// impl AgentHook for MyAgentHook {
///     fn agent_type(&self) -> &str {
///         &self.agent_type
///     }
///
///     async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
///         // Install hook...
///         Ok(())
///     }
///
///     async fn detect_session_activity(&self) -> Result<SessionActivity> {
///         // Detect activity...
///         Ok(SessionActivity::new(AgentType::Generic))
///     }
///
///     async fn extract_session_context(&self) -> Result<SessionContext> {
///         // Extract context...
///         Ok(SessionContext::new(self.agent_type.clone()))
///     }
/// }
/// ```
#[async_trait]
pub trait AgentHook: Send + Sync {
    /// Get the agent type this hook handles
    fn agent_type(&self) -> &str;

    /// Install the session end hook
    ///
    /// This sets up the native hook mechanism for the agent. When a session
    /// ends, the callback will be invoked with the extracted context.
    ///
    /// # Arguments
    ///
    /// * `callback` - Function to call when session ends
    ///
    /// # Returns
    ///
    /// Ok(()) if hook was installed successfully
    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()>;

    /// Optional: Install the session start hook.
    async fn install_session_start_hook(&mut self, _callback: SessionEndCallback) -> Result<()> {
        Err(crate::error::HookError::NotSupported(
            "Session start hooks not supported for this agent".to_string(),
        ))
    }

    /// Optional: Install a compact/checkpoint hook.
    async fn install_compact_hook(&mut self, _callback: SessionEndCallback) -> Result<()> {
        Err(crate::error::HookError::NotSupported(
            "Compact/checkpoint hooks not supported for this agent".to_string(),
        ))
    }

    /// Detect if the agent session is currently active
    ///
    /// This method checks for agent activity through various means:
    /// - Process detection
    /// - Session file monitoring
    /// - Agent-specific indicators
    ///
    /// # Returns
    ///
    /// SessionActivity with current state
    async fn detect_session_activity(&self) -> Result<SessionActivity>;

    /// Extract session context from the agent
    ///
    /// This method extracts all relevant context from the current or
    /// recent agent session, including:
    /// - Conversation history
    /// - Decisions made
    /// - Files modified
    /// - Commands executed
    /// - Errors encountered
    ///
    /// # Returns
    ///
    /// SessionContext with extracted data
    async fn extract_session_context(&self) -> Result<SessionContext>;

    /// Optional: Install a checkpoint hook
    ///
    /// Some agents support periodic checkpointing during long sessions.
    /// This allows for incremental context extraction.
    async fn install_checkpoint_hook(&mut self, _callback: SessionEndCallback) -> Result<()> {
        // Default: not supported
        Err(crate::error::HookError::NotSupported(
            "Checkpoint hooks not supported for this agent".to_string(),
        ))
    }

    /// Optional: Install an error hook
    ///
    /// Some agents can trigger hooks when errors occur.
    async fn install_error_hook(&mut self, _callback: SessionEndCallback) -> Result<()> {
        // Default: not supported
        Err(crate::error::HookError::NotSupported(
            "Error hooks not supported for this agent".to_string(),
        ))
    }

    /// Optional: Check if native hook is installed
    fn is_hook_installed(&self) -> bool {
        false
    }

    /// Optional: Uninstall all hooks
    async fn uninstall_hooks(&mut self) -> Result<()> {
        Ok(())
    }

    /// Optional: Get hook reliability score (0.0-1.0)
    fn reliability_score(&self) -> f32 {
        1.0
    }

    /// Report which lifecycle events this agent hook supports.
    ///
    /// Default assumes session-end only. Agents with richer native hook
    /// support (e.g. Claude Code's SessionStart hook) should override this.
    fn lifecycle_capabilities(&self) -> LifecycleCapabilities {
        LifecycleCapabilities::end_only()
    }

    /// Report the support tier for this agent hook.
    ///
    /// Default assumes monitor-only. Agents with dedicated hook files
    /// and skill installation should override this.
    fn support_tier(&self) -> SupportTier {
        SupportTier::MonitorOnly
    }
}

/// Base hook implementation with common functionality
pub struct BaseHook {
    /// Agent type name
    pub agent_type: String,

    /// Whether hook is installed
    pub installed: bool,

    /// Registered callbacks
    pub callbacks: Vec<SessionEndCallback>,
}

impl BaseHook {
    /// Create a new base hook
    pub fn new(agent_type: impl Into<String>) -> Self {
        Self {
            agent_type: agent_type.into(),
            installed: false,
            callbacks: Vec::new(),
        }
    }

    /// Add a callback
    pub fn add_callback(&mut self, callback: SessionEndCallback) {
        self.callbacks.push(callback);
    }

    /// Trigger all callbacks
    pub fn trigger_callbacks(&self, context: SessionContext) {
        for callback in &self.callbacks {
            callback(context.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_result_success() {
        let result = HookResult::success("test-agent", ExtractionSource::Manual);
        assert!(result.success);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_hook_result_failure() {
        let result = HookResult::failure(
            "test-agent",
            ExtractionSource::Manual,
            "Something went wrong",
        );
        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(result.error.unwrap(), "Something went wrong");
    }

    #[test]
    fn test_hook_result_with_context() {
        let ctx = SessionContext::new("test");
        let result = HookResult::success_with_context(
            "test-agent",
            ExtractionSource::NativeHook("skill".to_string()),
            ctx,
        );
        assert!(result.success);
        assert!(result.context.is_some());
    }

    #[test]
    fn test_base_hook() {
        let mut hook = BaseHook::new("test");
        assert_eq!(hook.agent_type, "test");
        assert!(!hook.installed);

        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();
        hook.add_callback(Arc::new(move |_ctx| {
            called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        }));

        hook.trigger_callbacks(SessionContext::new("test"));
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_lifecycle_capabilities_default() {
        let caps = LifecycleCapabilities::default();
        assert!(!caps.session_start);
        assert!(!caps.session_end);
        assert!(!caps.checkpoint);
        assert!(!caps.error_hook);
        assert!(!caps.compact);
    }

    #[test]
    fn test_lifecycle_capabilities_end_only() {
        let caps = LifecycleCapabilities::end_only();
        assert!(!caps.session_start);
        assert!(caps.session_end);
        assert!(!caps.checkpoint);
        assert!(!caps.error_hook);
        assert!(!caps.compact);
    }

    #[test]
    fn test_lifecycle_capabilities_monitor_only() {
        let caps = LifecycleCapabilities::monitor_only();
        assert!(!caps.session_end);
        assert!(!caps.session_start);
    }
}
