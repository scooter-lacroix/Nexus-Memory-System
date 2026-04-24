//! Droid (Factory CLI) hook implementation.
//!
//! Installs native lifecycle hooks in `~/.factory/settings.json`.

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

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
    readonly: bool,
    settings_path_override: Option<PathBuf>,
}

impl DroidHook {
    pub const CONFIG_DIR: &'static str = ".factory";

    pub fn new() -> Self {
        Self::new_with_mode(false)
    }

    pub fn new_readonly() -> Self {
        Self::new_with_mode(true)
    }

    fn new_with_mode(readonly: bool) -> Self {
        let settings_hook_installed = Self::default_settings_path()
            .ok()
            .and_then(|path| Self::has_settings_hooks_at_path(&path).ok())
            .unwrap_or(false);
        Self {
            base: BaseHook::new("droid"),
            settings_hook_installed,
            process_monitor: ProcessMonitor::new(),
            readonly,
            settings_path_override: None,
        }
    }

    #[cfg(test)]
    fn with_settings_path(settings_path: PathBuf, readonly: bool) -> Self {
        Self {
            base: BaseHook::new("droid"),
            settings_hook_installed: false,
            process_monitor: ProcessMonitor::new(),
            readonly,
            settings_path_override: Some(settings_path),
        }
    }

    fn default_settings_path() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| {
            HookError::InstallationFailed(format!(
                "Home directory unavailable; cannot resolve {}/settings.json",
                Self::CONFIG_DIR
            ))
        })?;
        Ok(home.join(Self::CONFIG_DIR).join("settings.json"))
    }

    fn settings_path(&self) -> Result<PathBuf> {
        if let Some(path) = &self.settings_path_override {
            return Ok(path.clone());
        }
        Self::default_settings_path()
    }

    fn find_nexus_binary() -> String {
        if let Ok(bin) = std::env::var("NEXUS_HOOK_BINARY") {
            if !bin.trim().is_empty() {
                return bin;
            }
        }

        if let Ok(current_exe) = std::env::current_exe() {
            let name = current_exe.file_name().and_then(|n| n.to_str());
            #[cfg(not(windows))]
            let valid = name.is_some_and(|n| matches!(n, "nexus" | "nexus-bin"));
            #[cfg(windows)]
            let valid = name.is_some_and(|n| {
                matches!(n, "nexus" | "nexus-bin" | "nexus.exe" | "nexus-bin.exe")
            });
            if valid {
                return current_exe.to_string_lossy().to_string();
            }
        }

        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut candidates: Vec<PathBuf> = [
            dirs::home_dir().map(|h| h.join(".cargo").join("bin").join("nexus")),
            dirs::home_dir().map(|h| h.join(".cargo").join("bin").join("nexus-bin")),
            dirs::home_dir().map(|h| h.join(".local").join("bin").join("nexus")),
            dirs::home_dir().map(|h| h.join(".local").join("bin").join("nexus-bin")),
            Some(PathBuf::from("/usr/local/bin/nexus")),
            Some(PathBuf::from("/usr/local/bin/nexus-bin")),
        ]
        .into_iter()
        .flatten()
        .collect();

        #[cfg(windows)]
        {
            if let Some(home) = dirs::home_dir() {
                candidates.extend([
                    home.join(".cargo").join("bin").join("nexus.exe"),
                    home.join(".cargo").join("bin").join("nexus-bin.exe"),
                    home.join(".local").join("bin").join("nexus.exe"),
                    home.join(".local").join("bin").join("nexus-bin.exe"),
                ]);
            }
        }

        for candidate in candidates {
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }

        "nexus".to_string()
    }

    fn desired_commands() -> [(String, String); 5] {
        let nexus_bin = Self::find_nexus_binary()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");

        let scoped = |base: &str| -> String {
            #[cfg(not(windows))]
            {
                format!(
                    "bash -c \"{base} --session-key \\\"${{FACTORY_SESSION_ID:-${{SESSION_ID:-}}}}\\\" --cwd \\\"${{FACTORY_CWD:-${{PWD:-}}}}\\\"\""
                )
            }
            #[cfg(windows)]
            {
                // On Windows, invoke the binary directly without bash.
                // Shell variable expansion is not available; nexus gracefully
                // handles missing session-key/cwd at runtime via derived keys.
                format!("{base}")
            }
        };

        [
            (
                SESSION_START_EVENT.to_string(),
                scoped(&format!(
                    "\"{nexus_bin}\" session start --agent droid --mode session"
                )),
            ),
            (
                SESSION_END_EVENT.to_string(),
                scoped(&format!(
                    "\"{nexus_bin}\" session end --agent droid --reason session-end"
                )),
            ),
            (
                CHECKPOINT_EVENT.to_string(),
                scoped(&format!(
                    "\"{nexus_bin}\" session event --agent droid --kind checkpoint"
                )),
            ),
            (
                COMPACT_EVENT.to_string(),
                scoped(&format!(
                    "\"{nexus_bin}\" session event --agent droid --kind compact"
                )),
            ),
            (
                ERROR_EVENT.to_string(),
                scoped(&format!(
                    "\"{nexus_bin}\" session event --agent droid --kind error"
                )),
            ),
        ]
    }

    fn has_settings_hooks_at_path(settings_path: &Path) -> Result<bool> {
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

    fn has_settings_hooks(&self) -> Result<bool> {
        let settings_path = self.settings_path()?;
        Self::has_settings_hooks_at_path(&settings_path)
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

    async fn install_settings_hooks(&mut self) -> Result<()> {
        self.ensure_mutable()?;

        if self.settings_hook_installed && self.has_settings_hooks().unwrap_or(false) {
            return Ok(());
        }

        let settings_path = self.settings_path()?;
        let mut settings = if fs::try_exists(&settings_path).await.map_err(|e| {
            HookError::InstallationFailed(format!("Failed to check settings.json: {}", e))
        })? {
            let content = fs::read_to_string(&settings_path).await.map_err(|e| {
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
            fs::create_dir_all(parent).await.map_err(|e| {
                HookError::InstallationFailed(format!("Failed to create settings dir: {}", e))
            })?;
        }

        let serialized = serde_json::to_string_pretty(&settings).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to serialize settings: {}", e))
        })?;
        let tmp_path = settings_path.with_extension("json.tmp");
        fs::write(&tmp_path, serialized).await.map_err(|e| {
            HookError::InstallationFailed(format!("Failed to write temporary settings: {}", e))
        })?;
        #[cfg(windows)]
        if fs::try_exists(&settings_path).await.map_err(|e| {
            HookError::InstallationFailed(format!("Failed to check settings.json: {}", e))
        })? {
            fs::remove_file(&settings_path).await.map_err(|e| {
                HookError::InstallationFailed(format!(
                    "Failed to remove existing settings.json before replace: {}",
                    e
                ))
            })?;
        }
        fs::rename(&tmp_path, &settings_path).await.map_err(|e| {
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
        entries: &mut Vec<serde_json::Value>,
        desired_command: &str,
    ) -> bool {
        let mut replaced = false;
        let mut canonical_seen = false;
        let mut rewritten = Vec::with_capacity(entries.len());

        for mut entry in entries.drain(..) {
            let mut managed_in_entry = false;

            if let Some(command) = entry.get("command").and_then(|value| value.as_str()) {
                if Self::is_nexus_managed_command(command) {
                    managed_in_entry = true;
                    replaced = true;
                    if canonical_seen {
                        continue;
                    }
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert(
                            "command".to_string(),
                            serde_json::Value::String(desired_command.to_string()),
                        );
                        obj.insert(
                            "type".to_string(),
                            serde_json::Value::String("command".into()),
                        );
                    }
                    canonical_seen = true;
                }
            }

            if let Some(hooks) = entry
                .get_mut("hooks")
                .and_then(|value| value.as_array_mut())
            {
                let mut filtered_hooks = Vec::with_capacity(hooks.len());
                for mut hook in hooks.drain(..) {
                    let managed = hook
                        .get("command")
                        .and_then(|value| value.as_str())
                        .is_some_and(Self::is_nexus_managed_command);
                    if managed {
                        managed_in_entry = true;
                        replaced = true;
                        if canonical_seen {
                            continue;
                        }
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
                        canonical_seen = true;
                    }
                    filtered_hooks.push(hook);
                }
                *hooks = filtered_hooks;
            }

            if !managed_in_entry || canonical_seen {
                rewritten.push(entry);
            }
        }

        *entries = rewritten;
        replaced
    }

    fn is_nexus_managed_command(command: &str) -> bool {
        let command = command.to_ascii_lowercase();
        command.contains("nexus")
            && command.contains("--agent droid")
            && (command.contains(" session ") || command.contains("ingest-hook-event"))
    }

    fn ensure_mutable(&self) -> Result<()> {
        if self.readonly {
            return Err(HookError::NotSupported(
                "DroidHook readonly mode does not allow hook installation".to_string(),
            ));
        }
        Ok(())
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
        self.ensure_mutable()?;
        self.base.add_session_start_callback(callback);
        self.install_settings_hooks().await
    }

    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.ensure_mutable()?;
        self.base.add_callback(callback);
        self.base.installed = true;
        self.install_settings_hooks().await
    }

    async fn install_checkpoint_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.ensure_mutable()?;
        self.base.add_checkpoint_callback(callback);
        self.install_settings_hooks().await
    }

    async fn install_compact_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.ensure_mutable()?;
        self.base.add_callback(callback);
        self.install_settings_hooks().await
    }

    async fn install_error_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.ensure_mutable()?;
        self.base.add_error_callback(callback);
        self.install_settings_hooks().await
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
        let home = tempfile::tempdir().unwrap();
        let settings_path = home.path().join(".factory").join("settings.json");
        let mut hook = DroidHook::with_settings_path(settings_path, false);
        let callback = std::sync::Arc::new(|_ctx| {});
        let result = hook.install_session_end_hook(callback).await;
        assert!(
            result.is_ok(),
            "install_session_end_hook should succeed in a temp dir: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_readonly_droid_hook_rejects_install() {
        let home = tempfile::tempdir().unwrap();
        let settings_path = home.path().join(".factory").join("settings.json");
        let mut hook = DroidHook::with_settings_path(settings_path, true);
        let callback = std::sync::Arc::new(|_ctx| {});
        let result = hook.install_session_start_hook(callback).await;
        assert!(matches!(result, Err(HookError::NotSupported(_))));
    }
}
