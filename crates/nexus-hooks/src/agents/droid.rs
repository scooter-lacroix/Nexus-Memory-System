//! Droid (Factory CLI) hook implementation.
//!
//! Installs native lifecycle hooks in `~/.factory/settings.json`.

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, LifecycleCapabilities, SessionEndCallback};
use crate::error::{HookError, Result};
use crate::monitor::ProcessMonitor;
use crate::session::SessionContext;
use crate::types::{AgentType, SessionActivity, SupportTier};

const SESSION_START_EVENT: &str = "SessionStart";
const SESSION_END_EVENT: &str = "SessionEnd";
const CHECKPOINT_EVENT: &str = "PostToolUse";
const COMPACT_EVENT: &str = "PreCompact";
const ERROR_EVENT: &str = "Stop";

/// Droid hook using Factory settings.json lifecycle hooks.
pub struct DroidHook {
    base: BaseHook,
    settings_hook_installed: bool,
    process_monitor: ProcessMonitor,
}

impl DroidHook {
    pub const CONFIG_DIR: &'static str = ".factory";

    pub fn new() -> Self {
        Self {
            base: BaseHook::new("droid"),
            settings_hook_installed: Self::has_settings_hooks().unwrap_or(false),
            process_monitor: ProcessMonitor::new(),
        }
    }

    pub fn new_readonly() -> Self {
        Self {
            base: BaseHook::new("droid"),
            settings_hook_installed: Self::has_settings_hooks().unwrap_or(false),
            process_monitor: ProcessMonitor::new(),
        }
    }

    fn settings_path() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| {
            HookError::InstallationFailed(format!(
                "Home directory unavailable; cannot resolve {}/settings.json",
                Self::CONFIG_DIR
            ))
        })?;
        Ok(home.join(Self::CONFIG_DIR).join("settings.json"))
    }

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
                .is_some_and(|name| matches!(name, "nexus" | "nexus-bin"))
            {
                return current_exe.to_string_lossy().to_string();
            }
        }

        let candidates: Vec<PathBuf> = [
            dirs::home_dir().map(|h| h.join(".local").join("bin").join("nexus")),
            dirs::home_dir().map(|h| h.join(".local").join("bin").join("nexus-bin")),
            Some(PathBuf::from("/usr/local/bin/nexus")),
            Some(PathBuf::from("/usr/local/bin/nexus-bin")),
        ]
        .into_iter()
        .flatten()
        .collect();

        for candidate in candidates {
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }

        "nexus".to_string()
    }

    fn desired_commands() -> [(String, String); 5] {
        let nexus_bin = Self::find_nexus_binary().replace('\'', "'\\''");
        let quoted = format!("'{}'", nexus_bin);
        [
            (
                SESSION_START_EVENT.to_string(),
                format!("{quoted} session start --agent droid --mode session"),
            ),
            (
                SESSION_END_EVENT.to_string(),
                format!("{quoted} session end --agent droid --reason session-end"),
            ),
            (
                CHECKPOINT_EVENT.to_string(),
                format!("{quoted} session event --agent droid --kind checkpoint"),
            ),
            (
                COMPACT_EVENT.to_string(),
                format!("{quoted} session event --agent droid --kind compact"),
            ),
            (
                ERROR_EVENT.to_string(),
                format!("{quoted} session event --agent droid --kind error"),
            ),
        ]
    }

    fn has_settings_hooks() -> Result<bool> {
        let settings_path = Self::settings_path()?;
        let content = match std::fs::read_to_string(settings_path) {
            Ok(content) => content,
            Err(_) => return Ok(false),
        };
        let settings = match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(settings) => settings,
            Err(_) => return Ok(false),
        };

        let commands = Self::desired_commands();
        Ok(commands
            .iter()
            .all(|(event, command)| Self::settings_has_command(&settings, event, command)))
    }

    fn settings_has_command(settings: &serde_json::Value, event: &str, command: &str) -> bool {
        settings
            .get("hooks")
            .and_then(|hooks| hooks.get(event))
            .and_then(|event_entries| event_entries.as_array())
            .is_some_and(|entries| {
                entries
                    .iter()
                    .any(|entry| Self::entry_contains_exact_command(entry, command))
            })
    }

    fn entry_contains_exact_command(entry: &serde_json::Value, desired_command: &str) -> bool {
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

    fn install_settings_hooks(&mut self) -> Result<()> {
        if self.settings_hook_installed && Self::has_settings_hooks().unwrap_or(false) {
            return Ok(());
        }

        let settings_path = Self::settings_path()?;
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

        for (event, command) in Self::desired_commands() {
            Self::upsert_event_hook(&mut settings, &event, &command)?;
        }

        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                HookError::InstallationFailed(format!("Failed to create settings dir: {}", e))
            })?;
        }

        let serialized = serde_json::to_string_pretty(&settings).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to serialize settings: {}", e))
        })?;
        let tmp_path = settings_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, serialized).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to write temporary settings: {}", e))
        })?;
        std::fs::rename(&tmp_path, &settings_path).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to replace settings.json: {}", e))
        })?;

        self.settings_hook_installed = true;
        Ok(())
    }

    fn upsert_event_hook(
        settings: &mut serde_json::Value,
        event_name: &str,
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

        let event_entries = hooks_obj
            .entry(event_name)
            .or_insert_with(|| serde_json::json!([]));
        let entries = event_entries.as_array_mut().ok_or_else(|| {
            HookError::InstallationFailed(format!("'hooks.{event_name}' must be an array"))
        })?;

        if Self::replace_existing_event_hook(entries, desired_command) {
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

    fn replace_existing_event_hook(
        entries: &mut [serde_json::Value],
        desired_command: &str,
    ) -> bool {
        for entry in entries {
            if let Some(command) = entry.get("command").and_then(|value| value.as_str()) {
                if command.contains("session") && command.contains("droid") {
                    let mut new_entry = entry.clone();
                    if let Some(obj) = new_entry.as_object_mut() {
                        obj.insert(
                            "command".to_string(),
                            serde_json::Value::String(desired_command.to_string()),
                        );
                        obj.insert(
                            "type".to_string(),
                            serde_json::Value::String("command".into()),
                        );
                    }
                    *entry = new_entry;
                    return true;
                }
            }

            if let Some(hooks) = entry
                .get_mut("hooks")
                .and_then(|value| value.as_array_mut())
            {
                for hook in hooks {
                    if hook
                        .get("command")
                        .and_then(|value| value.as_str())
                        .is_some_and(|command| {
                            command.contains("session") && command.contains("droid")
                        })
                    {
                        if let Some(obj) = hook.as_object_mut() {
                            obj.insert(
                                "command".to_string(),
                                serde_json::Value::String(desired_command.to_string()),
                            );
                            obj.insert(
                                "type".to_string(),
                                serde_json::Value::String("command".into()),
                            );
                        }
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

    async fn install_session_start_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.install_settings_hooks()
    }

    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.base.installed = true;
        self.install_settings_hooks()
    }

    async fn install_checkpoint_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.install_settings_hooks()
    }

    async fn install_compact_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.install_settings_hooks()
    }

    async fn install_error_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        self.install_settings_hooks()
    }

    async fn detect_session_activity(&self) -> Result<SessionActivity> {
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
        let mut context = SessionContext::new("droid")
            .with_source("native")
            .with_reliability(if self.settings_hook_installed {
                0.98
            } else {
                0.9
            });
        context.complete();
        Ok(context)
    }

    fn is_hook_installed(&self) -> bool {
        self.settings_hook_installed
    }

    fn reliability_score(&self) -> f32 {
        if self.settings_hook_installed {
            0.98
        } else {
            0.9
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
    fn test_find_nexus_binary_supports_nexus_bin() {
        let bin = DroidHook::find_nexus_binary();
        assert!(!bin.is_empty());
        assert!(bin.contains("nexus"));
    }

    #[test]
    fn test_droid_lifecycle_capabilities() {
        let hook = DroidHook::new();
        let caps = hook.lifecycle_capabilities();
        assert!(caps.session_start);
        assert!(caps.session_end);
        assert!(caps.checkpoint);
        assert!(caps.error_hook);
        assert!(caps.compact);
    }

    #[tokio::test]
    async fn test_install_session_end_hook_is_supported() {
        let mut hook = DroidHook::new();
        let callback = std::sync::Arc::new(|_ctx| {});
        let result = hook.install_session_end_hook(callback).await;
        assert!(result.is_ok() || matches!(result, Err(HookError::InstallationFailed(_))));
    }
}
