//! Generic CLI hook implementation for agents without native hooks
//!
//! Uses atexit/signals for detection.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, SessionEndCallback};
use crate::error::Result;
use crate::monitor::ProcessMonitor;
use crate::session::SessionContext;
use crate::types::{AgentType, SessionActivity};

/// Generic CLI hook using atexit and signal handling
///
/// Used for agents that don't have native hook support:
/// - OpenCode
/// - Codex
/// - Amp
/// - Droid
/// - Other generic agents
pub struct CLIHook {
    /// Base hook functionality
    base: BaseHook,

    /// Agent type name
    agent_type_name: String,

    /// Agent type enum
    agent_type: AgentType,

    /// Process monitor
    process_monitor: ProcessMonitor,
}

impl CLIHook {
    /// Create a new CLI hook for the given agent type
    pub fn new(agent_type: impl Into<String>) -> Self {
        let agent_type_name = agent_type.into();
        let agent_type = AgentType::from_str(&agent_type_name).unwrap_or(AgentType::Generic);

        Self {
            base: BaseHook::new(&agent_type_name),
            agent_type_name,
            agent_type,
            process_monitor: ProcessMonitor::new(),
        }
    }

    /// Get config path for this agent
    fn config_path(&self) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(self.agent_type.config_dir())
    }

    /// Read session data if available
    fn read_session_data(&self) -> Option<serde_json::Value> {
        let session_file = self.config_path().join("session.json");

        if session_file.exists() {
            let content = std::fs::read_to_string(&session_file).ok()?;
            serde_json::from_str(&content).ok()
        } else {
            None
        }
    }
}

#[async_trait]
impl AgentHook for CLIHook {
    fn agent_type(&self) -> &str {
        &self.base.agent_type
    }

    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.base.installed = true;

        // CLI hooks rely on atexit and signal handlers
        // These are set up by the signal module when the extractor starts

        Ok(())
    }

    async fn detect_session_activity(&self) -> Result<SessionActivity> {
        let mut monitor = self.process_monitor.clone();
        let processes = monitor.find_agent_processes(self.agent_type);

        let mut activity = SessionActivity::new(self.agent_type);

        if !processes.is_empty() {
            activity.is_active = true;
            activity.processes = processes;
        }

        // Check session directory for recent activity
        let session_dir = self.config_path().join("sessions");
        if session_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&session_dir) {
                let most_recent = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map(|ext| ext == "json").unwrap_or(false))
                    .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));

                if let Some(entry) = most_recent {
                    if let Ok(metadata) = entry.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            let age = std::time::SystemTime::now()
                                .duration_since(modified)
                                .unwrap_or(std::time::Duration::MAX);

                            // Consider active if modified in last 5 minutes
                            if age.as_secs() < 300 {
                                activity.is_active = true;
                                activity.session_id =
                                    Some(entry.path().file_stem().unwrap().to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }

        Ok(activity)
    }

    async fn extract_session_context(&self) -> Result<SessionContext> {
        let mut context = SessionContext::new(&self.agent_type_name)
            .with_source("cli")
            .with_reliability(0.95);

        // Read session data if available
        if let Some(session) = self.read_session_data() {
            if let Some(messages) = session.get("messages").and_then(|m| m.as_array()) {
                for msg in messages {
                    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("unknown");
                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    context.add_message(role, content);
                }
            }

            if let Some(commands) = session.get("commands").and_then(|c| c.as_array()) {
                for cmd in commands {
                    if let Some(cmd_str) = cmd.as_str() {
                        context.add_command(cmd_str);
                    }
                }
            }
        }

        // Try to get git status for modified files
        let git_status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .ok();

        if let Some(output) = git_status {
            if output.status.success() {
                let status = String::from_utf8_lossy(&output.stdout);
                for line in status.lines() {
                    if line.len() > 3 {
                        let status_char = line.chars().next().unwrap_or(' ');
                        let file_path = &line[3..];
                        let action = match status_char {
                            '?' => crate::session::FileAction::Created,
                            'D' => crate::session::FileAction::Deleted,
                            _ => crate::session::FileAction::Modified,
                        };
                        context.add_file(crate::session::FileInfo::new(file_path, action));
                    }
                }
            }
        }

        context.complete();
        Ok(context)
    }

    fn is_hook_installed(&self) -> bool {
        self.base.installed
    }

    fn reliability_score(&self) -> f32 {
        0.95 // CLI hooks have 95% reliability
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_hook_new() {
        let hook = CLIHook::new("opencode");
        assert_eq!(hook.agent_type(), "opencode");
    }

    #[tokio::test]
    async fn test_cli_hook_detect_activity() {
        let hook = CLIHook::new("codex");
        let activity = hook.detect_session_activity().await.unwrap();

        assert_eq!(activity.agent_type, AgentType::Codex);
    }
}
