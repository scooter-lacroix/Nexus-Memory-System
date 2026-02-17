//! Qwen hook implementation
//!
//! Uses Hooks SubAgent for native integration.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, SessionEndCallback};
use crate::error::Result;
use crate::monitor::ProcessMonitor;
use crate::session::SessionContext;
use crate::types::{AgentType, SessionActivity};

/// Qwen hook using Hooks SubAgent
pub struct QwenHook {
    /// Base hook functionality
    base: BaseHook,

    /// Config path
    config_path: PathBuf,

    /// Process monitor for fallback detection
    process_monitor: ProcessMonitor,
}

impl QwenHook {
    /// Config directory
    pub const CONFIG_DIR: &'static str = ".qwen";

    /// Create a new Qwen hook
    pub fn new() -> Self {
        let config_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(Self::CONFIG_DIR);

        Self {
            base: BaseHook::new("qwen"),
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

impl Default for QwenHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHook for QwenHook {
    fn agent_type(&self) -> &str {
        &self.base.agent_type
    }

    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.base.installed = true;

        Ok(())
    }

    async fn detect_session_activity(&self) -> Result<SessionActivity> {
        let mut monitor = self.process_monitor.clone();
        let processes = monitor.find_agent_processes(AgentType::Qwen);

        let mut activity = SessionActivity::new(AgentType::Qwen);

        if !processes.is_empty() {
            activity.is_active = true;
            activity.processes = processes;
        }

        Ok(activity)
    }

    async fn extract_session_context(&self) -> Result<SessionContext> {
        let mut context = SessionContext::new("qwen")
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qwen_hook_new() {
        let hook = QwenHook::new();
        assert_eq!(hook.agent_type(), "qwen");
    }

    #[tokio::test]
    async fn test_qwen_hook_detect_activity() {
        let hook = QwenHook::new();
        let activity = hook.detect_session_activity().await.unwrap();

        assert_eq!(activity.agent_type, AgentType::Qwen);
    }
}
