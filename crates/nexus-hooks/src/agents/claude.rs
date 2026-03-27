//! Claude Code hook implementation
//!
//! Uses Skills-based lifecycle hooks for native integration.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, LifecycleCapabilities, SessionEndCallback};
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
///
/// Lifecycle support:
/// - **session_start**: Via settings.json `SessionStart` hook entry
/// - **session_end**: Via skill (on_session_end trigger)
/// - **checkpoint**: Via skill (on_checkpoint trigger)
/// - **error**: Via skill (on_error trigger)
/// - **compact**: Via skill (on_completion trigger)
pub struct ClaudeCodeHook {
    /// Base hook functionality
    base: BaseHook,

    /// Skill path
    skill_path: PathBuf,

    /// Whether skill is installed
    skill_installed: bool,

    /// Whether a SessionStart hook was written to settings.json
    settings_hook_installed: bool,

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
            settings_hook_installed: Self::has_settings_hook(),
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
        std::fs::create_dir_all(&self.skill_path).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to create skill dir: {}", e))
        })?;

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
You do not need to start a Nexus server manually for normal CLI memory capture.

## Configuration

The skill reads from:
- `NEXUS_AUTO_INGEST=true` environment variable
- the local Nexus CLI runtime for default operation

Optional:
- an external Nexus endpoint only when explicitly configured for advanced remote workflows

## Output

After storing, you'll see:
```
[Nexus] Stored 3 memories from Claude Code session:
  - 2 decisions
  - 1 context item
  - Memory IDs: nexus_123, nexus_124, nexus_125
```
"#;

        std::fs::write(&skill_md, skill_content).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to write skill file: {}", e))
        })?;

        self.skill_installed = true;
        tracing::info!("Claude Code Skill installed at: {:?}", self.skill_path);

        Ok(())
    }

    /// Settings file path for Claude Code hooks configuration.
    fn settings_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(Self::CONFIG_DIR)
            .join("settings.json")
    }

    /// Install a SessionStart hook entry into Claude Code's settings.json.
    ///
    /// Claude Code natively supports `SessionStart` as a hook event type.
    /// This writes a hook entry that invokes `nexus session start` when a
    /// new Claude Code session begins.
    fn install_settings_hook(&mut self) -> Result<()> {
        let settings_path = Self::settings_path();
        let command = Self::desired_session_start_command();

        let mut settings = if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path).map_err(|e| {
                HookError::InstallationFailed(format!("Failed to read settings.json: {}", e))
            })?;
            serde_json::from_str::<serde_json::Value>(&content).map_err(|e| {
                HookError::InstallationFailed(format!("Failed to parse settings.json: {}", e))
            })?
        } else {
            serde_json::json!({})
        };

        Self::upsert_session_start_hook(&mut settings, &command)?;

        // Write back
        let serialized = serde_json::to_string_pretty(&settings).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to serialize settings: {}", e))
        })?;

        // Create parent dir if needed
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                HookError::InstallationFailed(format!("Failed to create settings dir: {}", e))
            })?;
        }

        std::fs::write(&settings_path, serialized).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to write settings.json: {}", e))
        })?;

        self.settings_hook_installed = true;
        tracing::info!(
            "Claude Code SessionStart hook written to: {:?}",
            settings_path
        );

        Ok(())
    }

    /// Find the nexus binary path for use in hook commands.
    fn find_nexus_binary() -> String {
        if let Ok(bin) = std::env::var("NEXUS_HOOK_BINARY") {
            if !bin.trim().is_empty() {
                return bin;
            }
        }

        if let Ok(current_exe) = std::env::current_exe() {
            if current_exe
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "nexus")
            {
                return current_exe.to_string_lossy().to_string();
            }
        }

        // Check common installation paths
        let candidates: Vec<PathBuf> = [
            dirs::home_dir().map(|h| h.join(".local").join("bin").join("nexus")),
            Some(PathBuf::from("/usr/local/bin/nexus")),
        ]
        .into_iter()
        .flatten()
        .collect();

        for candidate in candidates {
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }

        // Fallback: assume it's in PATH
        "nexus".to_string()
    }

    fn desired_session_start_command() -> String {
        let nexus_bin = Self::find_nexus_binary();
        format!(
            "'{}' session start --agent claude-code --mode session",
            nexus_bin.replace('\'', "'\\''")
        )
    }

    fn has_settings_hook() -> bool {
        let settings_path = Self::settings_path();
        let Ok(content) = std::fs::read_to_string(settings_path) else {
            return false;
        };
        let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) else {
            return false;
        };
        let desired_command = Self::desired_session_start_command();
        settings
            .get("hooks")
            .and_then(|hooks| hooks.get("SessionStart"))
            .and_then(|value| value.as_array())
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    Self::entry_contains_exact_session_start_hook(entry, &desired_command)
                })
            })
    }

    #[cfg(test)]
    fn entry_has_session_start_hook(entry: &serde_json::Value) -> bool {
        entry
            .get("command")
            .and_then(|command| command.as_str())
            .map(Self::command_is_session_start_hook)
            .unwrap_or(false)
            || entry
                .get("hooks")
                .and_then(|hooks| hooks.as_array())
                .is_some_and(|hooks| {
                    hooks.iter().any(|hook| {
                        hook.get("command")
                            .and_then(|command| command.as_str())
                            .map(Self::command_is_session_start_hook)
                            .unwrap_or(false)
                    })
                })
    }

    fn command_is_session_start_hook(command: &str) -> bool {
        command.contains("nexus")
            && command.contains("session start")
            && command.contains("claude-code")
    }

    fn entry_contains_exact_session_start_hook(
        entry: &serde_json::Value,
        desired_command: &str,
    ) -> bool {
        entry
            .get("command")
            .and_then(|command| command.as_str())
            .map(|command| command == desired_command)
            .unwrap_or(false)
            || entry
                .get("hooks")
                .and_then(|hooks| hooks.as_array())
                .is_some_and(|hooks| {
                    hooks.iter().any(|hook| {
                        hook.get("command")
                            .and_then(|command| command.as_str())
                            .map(|command| command == desired_command)
                            .unwrap_or(false)
                    })
                })
    }

    fn upsert_session_start_hook(
        settings: &mut serde_json::Value,
        desired_command: &str,
    ) -> Result<()> {
        let settings_obj = settings.as_object_mut().ok_or_else(|| {
            HookError::InstallationFailed(
                "settings.json must contain a top-level JSON object".to_string(),
            )
        })?;

        let hooks = settings_obj
            .entry("hooks")
            .or_insert_with(|| serde_json::json!({}));
        let hooks_obj = hooks.as_object_mut().ok_or_else(|| {
            HookError::InstallationFailed("'hooks' must be a JSON object".to_string())
        })?;

        let session_start = hooks_obj
            .entry("SessionStart")
            .or_insert_with(|| serde_json::json!([]));
        let entries = session_start.as_array_mut().ok_or_else(|| {
            HookError::InstallationFailed("'hooks.SessionStart' must be an array".to_string())
        })?;

        if Self::replace_existing_session_start_hook(entries, desired_command) {
            return Ok(());
        }

        entries.push(serde_json::json!({
            "matcher": "",
            "hooks": [{
                "type": "command",
                "command": desired_command,
            }]
        }));

        Ok(())
    }

    fn replace_existing_session_start_hook(
        entries: &mut [serde_json::Value],
        desired_command: &str,
    ) -> bool {
        for entry in entries {
            if entry
                .get("command")
                .and_then(|value| value.as_str())
                .is_some_and(Self::command_is_session_start_hook)
            {
                *entry = serde_json::json!({
                    "type": "command",
                    "command": desired_command,
                });
                return true;
            }

            if let Some(hooks) = entry
                .get_mut("hooks")
                .and_then(|value| value.as_array_mut())
            {
                for hook in hooks {
                    if hook
                        .get("command")
                        .and_then(|value| value.as_str())
                        .is_some_and(Self::command_is_session_start_hook)
                    {
                        *hook = serde_json::json!({
                            "type": "command",
                            "command": desired_command,
                        });
                        return true;
                    }
                }
            }
        }

        false
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
        let checkpoint_dir = dirs::home_dir()?.join(Self::CONFIG_DIR).join("checkpoints");

        if !checkpoint_dir.exists() {
            return None;
        }

        let mut checkpoints = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&checkpoint_dir) {
            for entry in entries.flatten() {
                if entry
                    .path()
                    .extension()
                    .map(|e| e == "json")
                    .unwrap_or(false)
                {
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

    /// Install a SessionStart hook via Claude Code's settings.json.
    ///
    /// Claude Code natively supports the `SessionStart` hook event type,
    /// which fires when a new Claude Code session begins. This writes a
    /// hook entry that invokes `nexus session start --agent claude-code`.
    async fn install_session_start_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);

        self.install_settings_hook()?;

        Ok(())
    }

    /// Checkpoint hooks are supported via the installed skill's on_checkpoint trigger.
    async fn install_checkpoint_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        Ok(())
    }

    /// Compact hooks are supported via the installed skill's on_completion trigger.
    async fn install_compact_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        Ok(())
    }

    /// Error hooks are supported via the installed skill's on_error trigger.
    async fn install_error_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
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
                    let role = msg
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or("unknown");
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
                            if let Some(rationale) =
                                decision.get("rationale").and_then(|r| r.as_str())
                            {
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
        self.skill_installed || self.settings_hook_installed
    }

    fn reliability_score(&self) -> f32 {
        if self.skill_installed && self.settings_hook_installed {
            1.0
        } else if self.skill_installed || self.settings_hook_installed {
            0.98
        } else {
            0.95 // Fallback to process monitoring
        }
    }

    fn lifecycle_capabilities(&self) -> LifecycleCapabilities {
        LifecycleCapabilities {
            session_start: true,
            session_end: true,
            checkpoint: true,
            error_hook: true,
            compact: true,
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

    #[test]
    fn test_claude_hook_lifecycle_capabilities() {
        let hook = ClaudeCodeHook::new();
        let caps = hook.lifecycle_capabilities();

        assert!(
            caps.session_start,
            "Claude Code should support session_start"
        );
        assert!(caps.session_end, "Claude Code should support session_end");
        assert!(caps.checkpoint, "Claude Code should support checkpoint");
        assert!(caps.error_hook, "Claude Code should support error_hook");
        assert!(caps.compact, "Claude Code should support compact");
    }

    #[tokio::test]
    async fn test_claude_hook_install_session_start() {
        let mut hook = ClaudeCodeHook::new();
        let callback = std::sync::Arc::new(|_ctx| {});

        // Should succeed (may write to settings.json)
        let result = hook.install_session_start_hook(callback).await;
        // Result depends on whether settings.json is writable, but should not be NotSupported
        match result {
            Ok(()) => {
                assert!(hook.settings_hook_installed);
            }
            Err(HookError::InstallationFailed(_)) => {
                // Acceptable if file system is not writable in test env
            }
            Err(HookError::NotSupported(msg)) => {
                panic!(
                    "Session start should be supported for Claude Code, got: {}",
                    msg
                );
            }
            Err(e) => {
                panic!("Unexpected error: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_claude_hook_install_checkpoint_supported() {
        let mut hook = ClaudeCodeHook::new();
        let callback = std::sync::Arc::new(|_ctx| {});

        let result = hook.install_checkpoint_hook(callback).await;
        assert!(
            result.is_ok(),
            "Checkpoint should be supported for Claude Code"
        );
    }

    #[tokio::test]
    async fn test_claude_hook_install_error_supported() {
        let mut hook = ClaudeCodeHook::new();
        let callback = std::sync::Arc::new(|_ctx| {});

        let result = hook.install_error_hook(callback).await;
        assert!(
            result.is_ok(),
            "Error hook should be supported for Claude Code"
        );
    }

    #[test]
    fn test_find_nexus_binary() {
        let bin = ClaudeCodeHook::find_nexus_binary();
        assert!(!bin.is_empty());
        // Should either be a full path or "nexus" fallback
        assert!(bin.contains("nexus"));
    }

    #[test]
    fn test_entry_has_session_start_hook_detects_nested_command() {
        let entry = serde_json::json!({
            "matcher": "",
            "hooks": [
                {
                    "type": "command",
                    "command": "/tmp/nexus session start --agent claude-code --mode session"
                }
            ]
        });

        assert!(ClaudeCodeHook::entry_has_session_start_hook(&entry));
    }

    #[test]
    fn test_upsert_session_start_hook_repairs_stale_command() {
        let desired = "'/new/nexus' session start --agent claude-code --mode session";
        let mut settings = serde_json::json!({
            "hooks": {
                "SessionStart": [{
                    "matcher": "",
                    "hooks": [{
                        "type": "command",
                        "command": "'/old/nexus' session start --agent claude-code --mode session"
                    }]
                }]
            }
        });

        ClaudeCodeHook::upsert_session_start_hook(&mut settings, desired).unwrap();

        let hooks = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["hooks"][0]["command"], desired);
    }

    #[test]
    fn test_upsert_session_start_hook_rejects_invalid_shapes() {
        let mut settings = serde_json::json!({
            "hooks": {
                "SessionStart": {}
            }
        });

        let error = ClaudeCodeHook::upsert_session_start_hook(
            &mut settings,
            "'/nexus' session start --agent claude-code --mode session",
        )
        .unwrap_err();

        assert!(error.to_string().contains("SessionStart"));
    }
}
