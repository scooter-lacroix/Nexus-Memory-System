//! Claude Code hook implementation
//!
//! Uses Skills-based lifecycle hooks for native integration.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, SessionEndCallback};
use crate::error::{HookError, Result};
use crate::monitor::ProcessMonitor;
use crate::session::SessionContext;
use crate::types::{AgentType, SessionActivity};

/// Claude Code hook using Skills lifecycle
///
/// Installation:
/// 1. Creates Claude Code Skill at ~/.claude/skills/nexus-memory/SKILL.md
/// 2. Skill auto-triggers on session_end, checkpoint, completion
/// 3. Skill calls MCP tool to store memory
pub struct ClaudeCodeHook {
    /// Base hook functionality
    base: BaseHook,

    /// Skill path
    skill_path: PathBuf,

    /// Whether skill is installed
    skill_installed: bool,

    /// Process monitor for fallback detection
    process_monitor: ProcessMonitor,
}

impl ClaudeCodeHook {
    /// Skill name
    pub const SKILL_NAME: &'static str = "nexus-memory-extraction";

    /// Config directory
    pub const CONFIG_DIR: &'static str = ".claude";

    /// Skills subdirectory
    pub const SKILLS_DIR: &'static str = "skills";

    /// Create a new Claude Code hook
    pub fn new() -> Self {
        let skill_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(Self::CONFIG_DIR)
            .join(Self::SKILLS_DIR)
            .join(Self::SKILL_NAME);

        let mut hook = Self {
            base: BaseHook::new("claude-code"),
            skill_path,
            skill_installed: false,
            process_monitor: ProcessMonitor::new(),
        };

        // Try to install skill
        if let Err(e) = hook.install_skill() {
            tracing::warn!("Failed to install Claude Code skill: {}", e);
        }

        hook
    }

    /// Install the SKILL.md file
    fn install_skill(&mut self) -> Result<()> {
        // Create skill directory
        std::fs::create_dir_all(&self.skill_path)
            .map_err(|e| HookError::InstallationFailed(format!("Failed to create skill dir: {}", e)))?;

        let skill_md = self.skill_path.join("SKILL.md");

        let skill_content = r#"---
name: nexus-memory-extraction
description: Automatically extract session context to Nexus Memory System
version: 1.0.0
author: Nexus Memory System
trigger:
  - on_session_end
  - on_checkpoint
  - on_completion
  - on_error
priority: high
---

# Nexus Memory Extraction Skill

## Overview

This skill automatically triggers when your Claude Code session ends, ensuring no context is lost.

## What It Does

1. **Captures Context**: Extracts current conversation, decisions, and context
2. **Summarizes**: Creates structured summary of key points
3. **Stores**: Automatically stores to Nexus Memory System
4. **Confirms**: Shows what was stored

## Triggers

- **on_session_end**: When you close Claude Code
- **on_checkpoint**: At periodic checkpoints during long sessions
- **on_completion**: When a task is completed
- **on_error**: If an error occurs (stores context for debugging)

## No Manual Action Required

This skill runs automatically. You don't need to remember to trigger it.

## Configuration

The skill reads from:
- `NEXUS_AUTO_INGEST=true` environment variable
- `NEXUS_SERVER_URL` for connection

## Output

After storing, you'll see:
```
[Nexus] Stored 3 memories from Claude Code session:
  - 2 decisions
  - 1 context item
  - Memory IDs: nexus_123, nexus_124, nexus_125
```
"#;

        std::fs::write(&skill_md, skill_content)
            .map_err(|e| HookError::InstallationFailed(format!("Failed to write skill file: {}", e)))?;

        self.skill_installed = true;
        tracing::info!("Claude Code Skill installed at: {:?}", self.skill_path);

        Ok(())
    }

    /// Read session file
    fn read_session_file(&self) -> Option<serde_json::Value> {
        let session_file = dirs::home_dir()?
            .join(Self::CONFIG_DIR)
            .join("session.json");

        if session_file.exists() {
            let content = std::fs::read_to_string(&session_file).ok()?;
            serde_json::from_str(&content).ok()
        } else {
            None
        }
    }

    /// Read checkpoint data
    fn read_checkpoint_data(&self) -> Option<Vec<serde_json::Value>> {
        let checkpoint_dir = dirs::home_dir()?
            .join(Self::CONFIG_DIR)
            .join("checkpoints");

        if !checkpoint_dir.exists() {
            return None;
        }

        let mut checkpoints = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&checkpoint_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if let Ok(data) = serde_json::from_str(&content) {
                            checkpoints.push(data);
                        }
                    }
                }
            }
        }

        Some(checkpoints)
    }
}

impl Default for ClaudeCodeHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHook for ClaudeCodeHook {
    fn agent_type(&self) -> &str {
        &self.base.agent_type
    }

    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.base.installed = true;

        if !self.skill_installed {
            tracing::warn!("Claude Code Skill not installed, using fallback detection");
        }

        Ok(())
    }

    async fn detect_session_activity(&self) -> Result<SessionActivity> {
        // Refresh process monitor
        let mut monitor = self.process_monitor.clone();
        let processes = monitor.find_agent_processes(AgentType::ClaudeCode);

        let mut activity = SessionActivity::new(AgentType::ClaudeCode);

        if !processes.is_empty() {
            activity.is_active = true;
            activity.processes = processes;
        }

        // Also check for session file
        if let Some(session) = self.read_session_file() {
            if let Some(id) = session.get("session_id").and_then(|s| s.as_str()) {
                activity.session_id = Some(id.to_string());
            }
        }

        Ok(activity)
    }

    async fn extract_session_context(&self) -> Result<SessionContext> {
        let mut context = SessionContext::new("claude-code")
            .with_source("native")
            .with_reliability(1.0);

        // Read session file
        if let Some(session) = self.read_session_file() {
            if let Some(messages) = session.get("messages").and_then(|m| m.as_array()) {
                for msg in messages {
                    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("unknown");
                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    context.add_message(role, content);
                }
            }

            if let Some(project_ctx) = session.get("project_context") {
                context.add_custom("project_context", project_ctx.clone());
            }
        }

        // Read checkpoint data
        if let Some(checkpoints) = self.read_checkpoint_data() {
            for checkpoint in checkpoints {
                if let Some(decisions) = checkpoint.get("decisions").and_then(|d| d.as_array()) {
                    for decision in decisions {
                        if let Some(summary) = decision.get("summary").and_then(|s| s.as_str()) {
                            let mut dec = crate::session::Decision::new(summary);
                            if let Some(rationale) = decision.get("rationale").and_then(|r| r.as_str()) {
                                dec.rationale = Some(rationale.to_string());
                            }
                            context.add_decision(dec);
                        }
                    }
                }

                if let Some(files) = checkpoint.get("files").and_then(|f| f.as_array()) {
                    for file in files {
                        if let Some(path) = file.get("path").and_then(|p| p.as_str()) {
                            let action = file
                                .get("action")
                                .and_then(|a| a.as_str())
                                .unwrap_or("modified");
                            let file_action = match action {
                                "created" => crate::session::FileAction::Created,
                                "deleted" => crate::session::FileAction::Deleted,
                                "read" => crate::session::FileAction::Read,
                                _ => crate::session::FileAction::Modified,
                            };
                            context.add_file(crate::session::FileInfo::new(path, file_action));
                        }
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
            0.95 // Fallback to process monitoring
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_hook_new() {
        let hook = ClaudeCodeHook::new();
        assert_eq!(hook.agent_type(), "claude-code");
    }

    #[tokio::test]
    async fn test_claude_hook_detect_activity() {
        let hook = ClaudeCodeHook::new();
        let activity = hook.detect_session_activity().await.unwrap();

        assert_eq!(activity.agent_type, AgentType::ClaudeCode);
    }
}
