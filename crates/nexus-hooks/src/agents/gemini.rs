//! Gemini hook implementation
//!
//! Uses Function Calling for native integration.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, LifecycleCapabilities, SessionEndCallback};
use crate::error::Result;
use crate::monitor::ProcessMonitor;
use crate::session::SessionContext;
use crate::types::{AgentType, SessionActivity};

/// Gemini hook using Function Calling
pub struct GeminiHook {
    /// Base hook functionality
    base: BaseHook,

    /// Config path
    config_path: PathBuf,

    /// Process monitor for fallback detection
    process_monitor: ProcessMonitor,
}

impl GeminiHook {
    /// Config directory
    pub const CONFIG_DIR: &'static str = ".gemini";

    /// Create a new Gemini hook
    pub fn new() -> Self {
        let config_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(Self::CONFIG_DIR);

        Self {
            base: BaseHook::new("gemini"),
            config_path,
            process_monitor: ProcessMonitor::new(),
        }
    }

    /// Read session data
    fn read_session_data(&self) -> Option<serde_json::Value> {
        let session_file = self.config_path.join("session.json");

        if session_file.exists() {
            let content = std::fs::read_to_string(&session_file).ok()?;
            serde_json::from_str(&content).ok()
        } else {
            None
        }
    }
}

impl Default for GeminiHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHook for GeminiHook {
    fn agent_type(&self) -> &str {
        &self.base.agent_type
    }

    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.base.installed = true;

        // Gemini uses function calling which requires setup in the agent config
        // For now, we rely on process monitoring

        Ok(())
    }

    async fn detect_session_activity(&self) -> Result<SessionActivity> {
        let mut monitor = self.process_monitor.clone();
        let processes = monitor.find_agent_processes(AgentType::Gemini);

        let mut activity = SessionActivity::new(AgentType::Gemini);

        if !processes.is_empty() {
            activity.is_active = true;
            activity.processes = processes;
        }

        Ok(activity)
    }

    async fn extract_session_context(&self) -> Result<SessionContext> {
        let mut context = SessionContext::new("gemini")
            .with_source("native")
            .with_reliability(0.95);

        if let Some(session) = self.read_session_data() {
            if let Some(messages) = session.get("messages").and_then(|m| m.as_array()) {
                for msg in messages {
                    let role = msg
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or("unknown");
                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    context.add_message(role, content);
                }
            }
        }

        context.complete();
        Ok(context)
    }

    fn reliability_score(&self) -> f32 {
        0.95
    }

    fn lifecycle_capabilities(&self) -> LifecycleCapabilities {
        LifecycleCapabilities::monitor_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_hook_new() {
        let hook = GeminiHook::new();
        assert_eq!(hook.agent_type(), "gemini");
    }

    #[tokio::test]
    async fn test_gemini_hook_detect_activity() {
        let hook = GeminiHook::new();
        let activity = hook.detect_session_activity().await.unwrap();

        assert_eq!(activity.agent_type, AgentType::Gemini);
    }

    #[test]
    fn test_gemini_hook_lifecycle_capabilities() {
        let hook = GeminiHook::new();
        let caps = hook.lifecycle_capabilities();

        assert!(!caps.session_start, "Gemini does not support session_start");
        assert!(!caps.session_end, "Gemini is monitor-only");
        assert!(!caps.checkpoint, "Gemini does not support checkpoint");
    }
}
