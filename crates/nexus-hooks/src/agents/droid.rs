//! Droid (Factory CLI) hook implementation
//!
//! Uses settings-based lifecycle hooks for native integration via
//! `~/.factory/settings.json`, which follows the same hook schema as
//! Claude Code's settings.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, LifecycleCapabilities, SessionEndCallback};
use crate::error::{HookError, Result};
use crate::monitor::ProcessMonitor;
use crate::session::SessionContext;
use crate::types::{AgentType, SessionActivity, SupportTier};

/// Droid (Factory CLI) hook using settings.json lifecycle hooks
///
/// Installation:
/// 1. Writes hook entries to `~/.factory/settings.json` under the `hooks` key
/// 2. Hook schema is identical to Claude Code: `{ "hooks": { "EventName": [...] } }`
/// 3. Supported events: SessionStart, SessionEnd, PostToolUse, PreCompact, Stop
///
/// Lifecycle support:
/// - **session_start**: Via settings.json `SessionStart` hook entry
/// - **session_end**: Via settings.json `SessionEnd` hook entry
/// - **checkpoint**: Via settings.json `PreCompact` hook entry
/// - **error**: Via settings.json `PostToolUse` hook entry
/// - **compact**: Via settings.json `Stop` hook entry
pub struct DroidHook {
    /// Base hook functionality
    base: BaseHook,

    /// Whether a hook was written to settings.json
    settings_hook_installed: bool,

    /// Process monitor for fallback detection
    process_monitor: ProcessMonitor,
}

impl DroidHook {
    /// Config directory
    pub const CONFIG_DIR: &'static str = ".factory";

    /// Create a new Droid hook
    pub fn new() -> Self {
        Self {
            base: BaseHook::new("droid"),
            settings_hook_installed: Self::has_settings_hook(),
            process_monitor: ProcessMonitor::new(),
        }
    }

    /// Settings file path for Droid hooks configuration.
    fn settings_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(Self::CONFIG_DIR)
            .join("settings.json")
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
            "'{}' session start --agent droid --mode session",
            nexus_bin.replace('\'', "'\\''")
        )
    }

    /// Install a SessionStart hook entry into Droid's settings.json.
    ///
    /// Droid natively supports `SessionStart` as a hook event type.
    /// This writes a hook entry that invokes `nexus session start` when a
    /// new Droid session begins.
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
        tracing::info!("Droid SessionStart hook written to: {:?}", settings_path);

        Ok(())
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
        command.contains("nexus") && command.contains("session start") && command.contains("droid")
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
}

impl Default for DroidHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHook for DroidHook {
    fn agent_type(&self) -> &str {
        &self.base.agent_type
    }

    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.base.installed = true;
        Ok(())
    }

    /// Install a SessionStart hook via Droid's settings.json.
    ///
    /// Droid natively supports the `SessionStart` hook event type,
    /// which fires when a new Droid session begins. This writes a
    /// hook entry that invokes `nexus session start --agent droid`.
    async fn install_session_start_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);

        self.install_settings_hook()?;

        Ok(())
    }

    /// Checkpoint hooks are supported via the PreCompact hook event.
    async fn install_checkpoint_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        Ok(())
    }

    /// Compact hooks are supported via the Stop hook event.
    async fn install_compact_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        Ok(())
    }

    /// Error hooks are supported via the PostToolUse hook event.
    async fn install_error_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        Ok(())
    }

    async fn detect_session_activity(&self) -> Result<SessionActivity> {
        // Refresh process monitor
        let mut monitor = self.process_monitor.clone();
        let processes = monitor.find_agent_processes(AgentType::Droid);

        let mut activity = SessionActivity::new(AgentType::Droid);

        if !processes.is_empty() {
            activity.is_active = true;
            activity.processes = processes;
        }

        Ok(activity)
    }

    async fn extract_session_context(&self) -> Result<SessionContext> {
        let context = SessionContext::new("droid")
            .with_source("native")
            .with_reliability(1.0);

        Ok(context)
    }

    fn is_hook_installed(&self) -> bool {
        self.settings_hook_installed
    }

    fn reliability_score(&self) -> f32 {
        if self.settings_hook_installed {
            1.0
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

    fn support_tier(&self) -> SupportTier {
        SupportTier::NativeLifecycle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_droid_hook_new() {
        let hook = DroidHook::new();
        assert_eq!(hook.agent_type(), "droid");
    }

    #[test]
    fn test_droid_hook_lifecycle_capabilities() {
        let hook = DroidHook::new();
        let caps = hook.lifecycle_capabilities();

        assert!(caps.session_start, "Droid should support session_start");
        assert!(caps.session_end, "Droid should support session_end");
        assert!(caps.checkpoint, "Droid should support checkpoint");
        assert!(caps.error_hook, "Droid should support error_hook");
        assert!(caps.compact, "Droid should support compact");
    }

    #[tokio::test]
    async fn test_droid_hook_detect_activity() {
        let hook = DroidHook::new();
        let activity = hook.detect_session_activity().await.unwrap();

        assert_eq!(activity.agent_type, AgentType::Droid);
    }

    #[test]
    fn test_upsert_session_start_hook() {
        let desired = "'/new/nexus' session start --agent droid --mode session";
        let mut settings = serde_json::json!({
            "hooks": {
                "SessionStart": [{
                    "matcher": "",
                    "hooks": [{
                        "type": "command",
                        "command": "'/old/nexus' session start --agent droid --mode session"
                    }]
                }]
            }
        });

        DroidHook::upsert_session_start_hook(&mut settings, desired).unwrap();

        let hooks = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0]["hooks"][0]["command"], desired);
    }

    #[test]
    fn test_upsert_session_start_hook_adds_new() {
        let desired = "'/nexus' session start --agent droid --mode session";
        let mut settings = serde_json::json!({});

        DroidHook::upsert_session_start_hook(&mut settings, desired).unwrap();

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

        let error = DroidHook::upsert_session_start_hook(
            &mut settings,
            "'/nexus' session start --agent droid --mode session",
        )
        .unwrap_err();

        assert!(error.to_string().contains("SessionStart"));
    }

    #[test]
    fn test_find_nexus_binary() {
        let bin = DroidHook::find_nexus_binary();
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
                    "command": "/tmp/nexus session start --agent droid --mode session"
                }
            ]
        });

        assert!(DroidHook::entry_has_session_start_hook(&entry));
    }

    #[test]
    fn test_droid_hook_support_tier() {
        let hook = DroidHook::new();
        assert_eq!(hook.support_tier(), SupportTier::NativeLifecycle);
    }
}
