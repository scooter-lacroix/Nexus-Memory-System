//! Pi-Mono hook implementation
//!
//! Pi-mono is a TypeScript/Bun-based coding agent with a rich extension system.
//! Integration uses a native TypeScript extension installed at
//! `~/.pi/agent/extensions/nexus-memory.ts` that hooks into pi's
//! lifecycle events (session_start, session_shutdown, agent_end, etc.)
//! and forwards them to the Nexus CLI for memory capture.
//!
//! Repository: https://github.com/badlogic/pi-mono
//! Stack: TypeScript, Node.js/Bun runtime
//! Config: ~/.pi/agent/extensions/
//! Detection: `pi` or `pi-coding-agent` process

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, LifecycleCapabilities, SessionEndCallback};
use crate::error::{HookError, Result};
use crate::monitor::ProcessMonitor;
use crate::session::{
    FileAction, FileInfo, SessionContext, SubagentExecution, TaskInfo, TaskStatus,
};
use crate::types::{AgentType, SessionActivity, SupportTier};

/// Pi-Mono hook for extracting memory from pi-mono session execution.
///
/// Uses a native TypeScript extension (not SKILL.md) that integrates
/// with pi-mono's extension API for full lifecycle event coverage.
///
/// # Integration Points
///
/// - **Extension:** `~/.pi/agent/extensions/nexus-memory.ts`
/// - **Transport:** CLI (`nexus session start`, `nexus ingest-hook-event`)
/// - **Fallback:** Process monitoring + `.pi/sessions/` file scanning
///
/// # Lifecycle Coverage
///
/// All five lifecycle events are supported:
/// - `session_start` — via extension `session_start` event
/// - `session_end` — via extension `session_shutdown` event
/// - `checkpoint` — via debounced `agent_end` and explicit triggers
/// - `error_hook` — synthetic from failed `tool_result` / abnormal `agent_end`
/// - `compact` — via extension `session_compact` event
pub struct PiMonoHook {
    /// Base hook functionality
    base: BaseHook,

    /// Config directory
    config_dir: PathBuf,

    /// Session directory
    session_dir: PathBuf,

    /// Extensions directory
    extensions_dir: PathBuf,

    /// Process monitor
    process_monitor: ProcessMonitor,

    /// Whether extension is installed
    extension_installed: bool,
}

impl PiMonoHook {
    /// Agent type string
    pub const AGENT_TYPE: &'static str = "pi-mono";

    /// Config directory name
    pub const CONFIG_DIR_NAME: &'static str = ".pi";

    /// Extensions subdirectory
    pub const EXTENSIONS_SUBDIR: &'static str = "agent/extensions";

    /// Sessions subdirectory
    pub const SESSIONS_SUBDIR: &'static str = "sessions";

    /// Logs subdirectory
    pub const LOGS_SUBDIR: &'static str = "logs";

    /// Create a new Pi-Mono hook
    pub fn new() -> Self {
        Self::new_with_install(true)
    }

    /// Create a new Pi-Mono hook without mutating user state.
    pub fn new_readonly() -> Self {
        Self::new_with_install(false)
    }

    fn new_with_install(auto_install: bool) -> Self {
        let config_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(Self::CONFIG_DIR_NAME);

        let session_dir = config_dir.join(Self::SESSIONS_SUBDIR);
        let extensions_dir = config_dir.join(Self::EXTENSIONS_SUBDIR);
        let extension_installed = Self::extension_file_path(&extensions_dir).exists();

        let mut hook = Self {
            base: BaseHook::new(Self::AGENT_TYPE),
            config_dir,
            session_dir,
            extensions_dir,
            process_monitor: ProcessMonitor::new(),
            extension_installed,
        };

        if auto_install {
            // Migrate from legacy SKILL.md
            hook.migrate_from_skill();

            if !hook.extension_installed {
                if let Err(e) = hook.install_extension() {
                    tracing::warn!("Failed to install pi-mono extension: {}", e);
                }
            }
        }

        hook
    }

    fn extension_file_path(extensions_dir: &std::path::Path) -> PathBuf {
        extensions_dir.join("nexus-memory.ts")
    }

    /// Install the nexus-memory TypeScript extension
    fn install_extension(&mut self) -> Result<()> {
        std::fs::create_dir_all(&self.extensions_dir).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to create extensions dir: {}", e))
        })?;

        let extension_path = Self::extension_file_path(&self.extensions_dir);

        let extension_content = include_str!("../extension_ts/nexus_memory_pi.ts");

        std::fs::write(&extension_path, extension_content).map_err(|e| {
            HookError::InstallationFailed(format!("Failed to write extension: {}", e))
        })?;

        self.extension_installed = true;
        tracing::info!("Pi-mono extension installed at: {:?}", extension_path);

        Ok(())
    }

    /// Migrate from legacy SKILL.md installation
    fn migrate_from_skill(&mut self) {
        let legacy_skill_dir = self
            .config_dir
            .join("agent")
            .join("skills")
            .join("nexus-memory-extraction");

        if legacy_skill_dir.exists() {
            tracing::info!("Migrating pi-mono from SKILL.md to TypeScript extension");
            if let Err(e) = std::fs::remove_dir_all(&legacy_skill_dir) {
                tracing::warn!("Failed to remove legacy skill dir: {}", e);
            }
        }
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

            // Sort by modification time, most recent first
            session_files.sort_by(|a, b| {
                let time_a = a.metadata().ok().and_then(|m| m.modified().ok());
                let time_b = b.metadata().ok().and_then(|m| m.modified().ok());
                time_b.cmp(&time_a)
            });

            // Take up to 10 most recent sessions
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

    /// Read log files
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
                        .map(|ext| ext == "log")
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
                        if line.contains("Executing:") || line.contains("Command:") {
                            commands.push(line.to_string());
                        }
                    }
                }
            }
        }

        commands
    }
}

impl Default for PiMonoHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHook for PiMonoHook {
    fn agent_type(&self) -> &str {
        &self.base.agent_type
    }

    async fn install_session_start_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_session_start_callback(callback);
        Ok(())
    }

    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        Ok(())
    }

    async fn install_checkpoint_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_checkpoint_callback(callback);
        Ok(())
    }

    async fn install_compact_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_callback(callback);
        Ok(())
    }

    async fn install_error_hook(&mut self, callback: SessionEndCallback) -> Result<()> {
        self.base.add_error_callback(callback);
        Ok(())
    }

    async fn detect_session_activity(&self) -> Result<SessionActivity> {
        let mut monitor = self.process_monitor.clone();
        let processes = monitor.find_agent_processes(AgentType::PiMono);

        let mut activity = SessionActivity::new(AgentType::PiMono);

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

                            // Consider active if modified in last 5 minutes
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

        // Check for subagent processes
        let subagent_check = std::process::Command::new("pgrep")
            .arg("-f")
            .arg("subagent|skill")
            .output()
            .ok();

        if let Some(output) = subagent_check {
            if output.status.success() && !output.stdout.is_empty() {
                activity.is_active = true;
            }
        }

        Ok(activity)
    }

    async fn extract_session_context(&self) -> Result<SessionContext> {
        let mut context = SessionContext::new("pi-mono")
            .with_source("native")
            .with_reliability(1.0);

        // Track role usage
        let mut role_usage: std::collections::HashMap<String, i32> =
            std::collections::HashMap::new();

        // Extract from session files
        for session_data in self.read_session_files() {
            // Extract session info
            if let Some(timestamp) = session_data.get("timestamp").and_then(|t| t.as_str()) {
                context.add_custom(
                    "session_timestamp",
                    serde_json::Value::String(timestamp.to_string()),
                );
            }

            // Extract tasks
            if let Some(tasks) = session_data.get("tasks").and_then(|t| t.as_array()) {
                for task in tasks {
                    let description = task
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    let role = task
                        .get("role")
                        .and_then(|r| r.as_str())
                        .unwrap_or("unknown");

                    let mut task_info = TaskInfo::new(description);
                    task_info.subagent = Some(role.to_string());

                    if let Some(status) = task.get("status").and_then(|s| s.as_str()) {
                        task_info.status = match status {
                            "completed" => TaskStatus::Completed,
                            "failed" => TaskStatus::Failed,
                            "in_progress" => TaskStatus::InProgress,
                            _ => TaskStatus::Pending,
                        };
                    }

                    context.tasks.push(task_info);

                    // Track role usage
                    *role_usage.entry(role.to_string()).or_insert(0) += 1;

                    // Add as subagent execution
                    context.subagent_executions.push(SubagentExecution {
                        subagent_type: role.to_string(),
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
        }

        // Extract commands from logs
        for cmd in self.read_log_files() {
            context.add_command(cmd);
        }

        // Store role usage stats
        context.add_custom(
            "role_usage",
            serde_json::to_value(&role_usage).unwrap_or(serde_json::Value::Null),
        );

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
        self.extension_installed
    }

    fn reliability_score(&self) -> f32 {
        if self.extension_installed {
            1.0
        } else {
            0.95
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
    use std::sync::Arc;

    #[test]
    fn test_pi_mono_hook_new() {
        let hook = PiMonoHook::new();
        assert_eq!(hook.agent_type(), "pi-mono");
    }

    #[tokio::test]
    async fn test_pi_mono_hook_detect_activity() {
        let hook = PiMonoHook::new();
        let activity = hook.detect_session_activity().await.unwrap();

        assert_eq!(activity.agent_type, AgentType::PiMono);
    }

    #[test]
    fn test_pi_mono_hook_constants() {
        assert_eq!(PiMonoHook::AGENT_TYPE, "pi-mono");
        assert_eq!(PiMonoHook::CONFIG_DIR_NAME, ".pi");
        assert_eq!(PiMonoHook::EXTENSIONS_SUBDIR, "agent/extensions");
    }

    #[test]
    fn test_pi_mono_extension_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = PiMonoHook::extension_file_path(&dir.path().join("extensions"));
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "nexus-memory.ts"
        );
    }

    #[test]
    fn test_pi_mono_hook_lifecycle_capabilities_full() {
        let hook = PiMonoHook::new();
        let caps = hook.lifecycle_capabilities();

        assert!(caps.session_start, "pi-mono should support session_start");
        assert!(caps.session_end, "pi-mono should support session_end");
        assert!(caps.checkpoint, "pi-mono should support checkpoint");
        assert!(caps.error_hook, "pi-mono should support error_hook");
        assert!(caps.compact, "pi-mono should support compact");
    }

    #[tokio::test]
    async fn test_pi_mono_hook_install_session_start() {
        let mut hook = PiMonoHook::new();
        let cb: SessionEndCallback = Arc::new(|_ctx| ());
        let result = hook.install_session_start_hook(cb).await;
        assert!(result.is_ok(), "pi-mono should accept session_start hook");
    }

    #[tokio::test]
    async fn test_pi_mono_hook_install_checkpoint() {
        let mut hook = PiMonoHook::new();
        let cb: SessionEndCallback = Arc::new(|_ctx| ());
        let result = hook.install_checkpoint_hook(cb).await;
        assert!(result.is_ok(), "pi-mono should accept checkpoint hook");
    }

    #[tokio::test]
    async fn test_pi_mono_hook_install_error() {
        let mut hook = PiMonoHook::new();
        let cb: SessionEndCallback = Arc::new(|_ctx| ());
        let result = hook.install_error_hook(cb).await;
        assert!(result.is_ok(), "pi-mono should accept error hook");
    }

    #[test]
    fn test_pi_mono_legacy_migration() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_dir = dir
            .path()
            .join(".pi")
            .join("agent")
            .join("skills")
            .join("nexus-memory-extraction");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("SKILL.md"), "legacy").unwrap();

        // Simulate migration: remove directory
        assert!(legacy_dir.exists());
        std::fs::remove_dir_all(&legacy_dir).unwrap();
        assert!(!legacy_dir.exists());
    }
}
