//! Oh-My-Pi (OMP) hook implementation (MANDATORY)
//!
//! Oh-my-pi is a fork of pi-mono with additional features including
//! Rust N-API native addon support.
//!
//! Repository: https://github.com/can1357/oh-my-pi
//! Stack: TypeScript, Bun runtime, Rust N-API
//! Config: ~/.omp/agent/skills/, .omp/skills/
//! Detection: `omp` or `oh-my-pi` process
//!
//! Features:
//! - Native Rust engine: grep, shell, text, keys, highlight, glob, task, ps, prof, clipboard
//! - LSP integration with format-on-write
//! - Browser automation (Puppeteer with stealth)
//! - Task tool (subagent system)
//! - TTSR (Time Traveling Streamed Rules)

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, SessionEndCallback};
use crate::error::{HookError, Result};
use crate::monitor::ProcessMonitor;
use crate::session::{
    FileAction, FileInfo, SessionContext, SubagentExecution, TaskInfo, TaskStatus,
};
use crate::types::{AgentType, SessionActivity};

/// Oh-My-Pi hook for extracting memory from OMP session execution.
///
/// Oh-My-Pi is a fork of pi-mono with additional features and modifications.
/// It maintains similar session management but has:
/// - Different CLI name (omp instead of pi)
/// - Different session storage location
/// - Rust N-API native addon support
///
/// # Detection Paths
///
/// - ~/.local/bin/omp
/// - /usr/local/bin/omp
/// - $PATH
///
/// # Session Files
///
/// - ~/.omp/sessions/ - Session history
/// - ~/.omp/logs/ - Centralized logs
///
/// # Native Features (Rust N-API)
///
/// - grep, shell, text, keys, highlight, glob, task, ps, prof, clipboard
/// - LSP integration
/// - Browser automation
pub struct OhMyPiHook {
    /// Base hook functionality
    base: BaseHook,

    /// Config directory
    config_dir: PathBuf,

    /// Session directory
    session_dir: PathBuf,

    /// Skills directory
    skills_dir: PathBuf,

    /// Process monitor
    process_monitor: ProcessMonitor,

    /// Whether skill is installed
    skill_installed: bool,

    /// Has native engine (Rust N-API)
    has_native_engine: bool,
}

impl OhMyPiHook {
    /// Agent type string
    pub const AGENT_TYPE: &'static str = "oh-my-pi";

    /// Config directory name
    pub const CONFIG_DIR_NAME: &'static str = ".omp";

    /// Skills subdirectory
    pub const SKILLS_SUBDIR: &'static str = "agent/skills";

    /// Sessions subdirectory
    pub const SESSIONS_SUBDIR: &'static str = "sessions";

    /// Logs subdirectory
    pub const LOGS_SUBDIR: &'static str = "logs";

    /// Create a new Oh-My-Pi hook
    pub fn new() -> Self {
        let config_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(Self::CONFIG_DIR_NAME);

        let session_dir = config_dir.join(Self::SESSIONS_SUBDIR);
        let skills_dir = config_dir.join(Self::SKILLS_SUBDIR);

        let mut hook = Self {
            base: BaseHook::new(Self::AGENT_TYPE),
            config_dir,
            session_dir,
            skills_dir,
            process_monitor: ProcessMonitor::new(),
            skill_installed: false,
            has_native_engine: Self::detect_native_engine(),
        };

        // Try to install skill
        if let Err(e) = hook.install_skill() {
            tracing::warn!("Failed to install oh-my-pi skill: {}", e);
        }

        hook
    }

    /// Detect if native Rust engine is available
    fn detect_native_engine() -> bool {
        // Check for native addon presence
        if let Some(home) = dirs::home_dir() {
            let native_addon = home
                .join(Self::CONFIG_DIR_NAME)
                .join("native")
                .join("libnexus_native.so");

            if native_addon.exists() {
                return true;
            }

            // Also check for .node addon
            let node_addon = home
                .join(Self::CONFIG_DIR_NAME)
                .join("native")
                .join("nexus_native.node");

            if node_addon.exists() {
                return true;
            }
        }

        false
    }

    /// Install the nexus-memory-extraction skill with TTSR support
    fn install_skill(&mut self) -> Result<()> {
        std::fs::create_dir_all(&self.skills_dir).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to create skills dir: {}", e))
        })?;

        let skill_dir = self.skills_dir.join("nexus-memory-extraction");
        std::fs::create_dir_all(&skill_dir).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to create skill dir: {}", e))
        })?;

        let skill_md = skill_dir.join("SKILL.md");

        // Oh-my-pi supports TTSR (Time Traveling Streamed Rules)
        let skill_content = r#"---
name: nexus-memory-extraction
description: Automatically extract session context to Nexus Memory System
version: 1.0.0
author: Nexus Memory System
triggers:
  - on_session_end
  - on_checkpoint
  - on_completion
  - on_error
priority: high
---

# Nexus Memory Extraction Skill (Oh-My-Pi)

This skill automatically extracts session context when oh-my-pi sessions end.

## Features

- **Native Rust Integration**: Works with OMP's native engine
- **TTSR Support**: Time Traveling Streamed Rules for complex workflows
- **Full Context Capture**: Conversations, decisions, files, commands

## Native Engine Features

The skill leverages OMP's native Rust engine for:
- `grep`: Fast searching
- `shell`: Command execution
- `glob`: File pattern matching
- `task`: Subagent management

## Configuration

Set environment variables:
- `NEXUS_AUTO_INGEST=true`
- `NEXUS_SERVER_URL=http://localhost:8768`
"#;

        std::fs::write(&skill_md, skill_content)
            .map_err(|e| HookError::InstallationFailed(format!("Failed to write skill: {}", e)))?;

        self.skill_installed = true;
        tracing::info!("Oh-my-pi skill installed at: {:?}", skill_dir);

        Ok(())
    }

    /// Read session files
    fn read_session_files(&self) -> Vec<serde_json::Value> {
        let mut sessions = Vec::new();

        if !self.session_dir.exists() {
            return sessions;
        }

        if let Ok(entries) = std::fs::read_dir(&self.session_dir) {
            let mut session_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "json")
                        .unwrap_or(false)
                })
                .collect();

            session_files.sort_by(|a, b| {
                let time_a = a.metadata().ok().and_then(|m| m.modified().ok());
                let time_b = b.metadata().ok().and_then(|m| m.modified().ok());
                time_b.cmp(&time_a)
            });

            for entry in session_files.into_iter().take(10) {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(data) = serde_json::from_str(&content) {
                        sessions.push(data);
                    }
                }
            }
        }

        sessions
    }

    /// Read log files from centralized log directory
    fn read_log_files(&self) -> Vec<String> {
        let mut commands = Vec::new();
        let logs_dir = self.config_dir.join(Self::LOGS_SUBDIR);

        if !logs_dir.exists() {
            return commands;
        }

        if let Ok(entries) = std::fs::read_dir(&logs_dir) {
            let mut log_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "log" || ext == "txt")
                        .unwrap_or(false)
                })
                .collect();

            log_files.sort_by(|a, b| {
                let time_a = a.metadata().ok().and_then(|m| m.modified().ok());
                let time_b = b.metadata().ok().and_then(|m| m.modified().ok());
                time_b.cmp(&time_a)
            });

            for entry in log_files.into_iter().take(5) {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    for line in content.lines() {
                        if line.contains("Executing:")
                            || line.contains("Command:")
                            || line.contains("OMP:")
                        {
                            commands.push(line.to_string());
                        }
                    }
                }
            }
        }

        commands
    }

    /// Read OMP configuration
    fn read_config(&self) -> Option<serde_json::Value> {
        let config_file = self.config_dir.join("config.json");

        if config_file.exists() {
            let content = std::fs::read_to_string(&config_file).ok()?;
            serde_json::from_str(&content).ok()
        } else {
            None
        }
    }

    /// Check if native feature is available
    pub fn has_native_feature(&self, feature: &str) -> bool {
        if !self.has_native_engine {
            return false;
        }

        matches!(
            feature,
            "grep"
                | "shell"
                | "text"
                | "keys"
                | "highlight"
                | "glob"
                | "task"
                | "ps"
                | "prof"
                | "clipboard"
        )
    }

    /// Get list of available native features
    pub fn native_features(&self) -> &'static [&'static str] {
        &[
            "grep",
            "shell",
            "text",
            "keys",
            "highlight",
            "glob",
            "task",
            "ps",
            "prof",
            "clipboard",
        ]
    }
}

impl Default for OhMyPiHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHook for OhMyPiHook {
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
        let processes = monitor.find_agent_processes(AgentType::OhMyPi);

        let mut activity = SessionActivity::new(AgentType::OhMyPi);

        if !processes.is_empty() {
            activity.is_active = true;
            activity.processes = processes;
        }

        // Check for recent session file activity
        if self.session_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&self.session_dir) {
                if let Some(most_recent) = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|ext| ext == "json")
                            .unwrap_or(false)
                    })
                    .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
                {
                    if let Ok(metadata) = most_recent.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            let age = std::time::SystemTime::now()
                                .duration_since(modified)
                                .unwrap_or(std::time::Duration::MAX);

                            if age.as_secs() < 300 {
                                activity.is_active = true;
                                activity.session_id = Some(
                                    most_recent
                                        .path()
                                        .file_stem()
                                        .unwrap()
                                        .to_string_lossy()
                                        .to_string(),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Check for OMP-specific extensions
        let ext_check = std::process::Command::new("pgrep")
            .arg("-f")
            .arg("omp-agent|oh-my-skill")
            .output()
            .ok();

        if let Some(output) = ext_check {
            if output.status.success() && !output.stdout.is_empty() {
                activity.is_active = true;
            }
        }

        Ok(activity)
    }

    async fn extract_session_context(&self) -> Result<SessionContext> {
        let mut context = SessionContext::new("oh-my-pi")
            .with_source("native")
            .with_reliability(1.0);

        // Track fork-specific features
        let mut fork_features: std::collections::HashMap<String, i32> =
            std::collections::HashMap::new();

        // Add native engine info
        context.add_custom(
            "has_native_engine",
            serde_json::Value::Bool(self.has_native_engine),
        );

        if self.has_native_engine {
            context.add_custom(
                "native_features",
                serde_json::to_value(self.native_features()).unwrap_or(serde_json::Value::Null),
            );
        }

        // Extract from session files
        for session_data in self.read_session_files() {
            // Extract session info
            if let Some(timestamp) = session_data.get("timestamp").and_then(|t| t.as_str()) {
                context.add_custom(
                    "session_timestamp",
                    serde_json::Value::String(timestamp.to_string()),
                );
            }

            // Extract tasks with OMP-specific features
            if let Some(tasks) = session_data.get("tasks").and_then(|t| t.as_array()) {
                for task in tasks {
                    let description = task
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    let feature = task
                        .get("feature")
                        .or_else(|| task.get("role"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("unknown");

                    let mut task_info = TaskInfo::new(description);
                    task_info.subagent = Some(feature.to_string());

                    if let Some(status) = task.get("status").and_then(|s| s.as_str()) {
                        task_info.status = match status {
                            "completed" => TaskStatus::Completed,
                            "failed" => TaskStatus::Failed,
                            "in_progress" => TaskStatus::InProgress,
                            _ => TaskStatus::Pending,
                        };
                    }

                    context.tasks.push(task_info);

                    // Track fork-specific features
                    *fork_features.entry(feature.to_string()).or_insert(0) += 1;

                    // Add as subagent execution
                    context.subagent_executions.push(SubagentExecution {
                        subagent_type: feature.to_string(),
                        task: description.to_string(),
                        status: "completed".to_string(),
                        started_at: chrono::Utc::now(),
                        completed_at: Some(chrono::Utc::now()),
                        result_summary: None,
                    });
                }
            }

            // Extract files modified
            if let Some(files) = session_data
                .get("files_modified")
                .and_then(|f| f.as_array())
            {
                for file in files {
                    if let Some(path) = file.as_str() {
                        context.add_file(FileInfo::new(path, FileAction::Modified));
                    }
                }
            }

            // Extract extensions used (OMP-specific)
            if let Some(extensions) = session_data
                .get("extensions_used")
                .and_then(|e| e.as_array())
            {
                for ext in extensions {
                    if let Some(ext_str) = ext.as_str() {
                        context.add_custom(
                            &format!("extension_{}", ext_str),
                            serde_json::Value::Bool(true),
                        );
                    }
                }
            }
        }

        // Extract commands from logs
        for cmd in self.read_log_files() {
            context.add_command(cmd);
        }

        // Store fork feature stats
        context.add_custom(
            "fork_features",
            serde_json::to_value(&fork_features).unwrap_or(serde_json::Value::Null),
        );

        // Read OMP config
        if let Some(config) = self.read_config() {
            context.add_custom("config", config);
        }

        // Get git status
        let git_status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .ok();

        if let Some(output) = git_status {
            if output.status.success() {
                let status = String::from_utf8_lossy(&output.stdout);
                for line in status.lines() {
                    if line.len() > 3 {
                        let file_path = &line[3..];
                        context.add_file(FileInfo::new(file_path, FileAction::Modified));
                    }
                }
            }
        }

        context.complete();
        Ok(context)
    }

    fn is_hook_installed(&self) -> bool {
        self.skill_installed
    }

    fn reliability_score(&self) -> f32 {
        if self.skill_installed {
            1.0
        } else {
            0.95
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oh_my_pi_hook_new() {
        let hook = OhMyPiHook::new();
        assert_eq!(hook.agent_type(), "oh-my-pi");
    }

    #[tokio::test]
    async fn test_oh_my_pi_hook_detect_activity() {
        let hook = OhMyPiHook::new();
        let activity = hook.detect_session_activity().await.unwrap();

        assert_eq!(activity.agent_type, AgentType::OhMyPi);
    }

    #[test]
    fn test_oh_my_pi_hook_constants() {
        assert_eq!(OhMyPiHook::AGENT_TYPE, "oh-my-pi");
        assert_eq!(OhMyPiHook::CONFIG_DIR_NAME, ".omp");
        assert_eq!(OhMyPiHook::SKILLS_SUBDIR, "agent/skills");
    }

    #[test]
    fn test_oh_my_pi_hook_native_features() {
        let hook = OhMyPiHook::new();
        let features = hook.native_features();

        assert!(features.contains(&"grep"));
        assert!(features.contains(&"shell"));
        assert!(features.contains(&"task"));
    }

    #[test]
    fn test_oh_my_pi_hook_has_native_feature() {
        let hook = OhMyPiHook::new();

        // May or may not have native engine, but should handle the check
        if hook.has_native_engine {
            assert!(hook.has_native_feature("grep"));
            assert!(hook.has_native_feature("shell"));
        }

        assert!(!hook.has_native_feature("unknown"));
    }
}
