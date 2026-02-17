//! Pi-Skills cross-compatible hook implementation (MANDATORY)
//!
//! Cross-compatible skills repository supporting multiple agent platforms.
//!
//! Repository: https://github.com/badlogic/pi-skills
//! Compatible with: pi-mono, oh-my-pi, Claude Code, Codex CLI, Amp, Droid
//!
//! Available Skills:
//! - brave-search
//! - browser-tools
//! - gccli, gdcli, gmcli
//! - transcribe
//! - vscode
//! - youtube-transcript

use async_trait::async_trait;
use std::path::PathBuf;

use crate::base::{AgentHook, BaseHook, SessionEndCallback};
use crate::error::{HookError, Result};
use crate::monitor::ProcessMonitor;
use crate::session::{FileInfo, FileAction, SessionContext};
use crate::types::{AgentType, SessionActivity, SkillMetadata};

/// Pi-Skills cross-compatible hook
///
/// Supports skills from the badlogic/pi-skills repository.
/// Compatible with multiple agent platforms including pi-mono, oh-my-pi,
/// Claude Code, Codex CLI, Amp, and Droid.
///
/// # Skills Format
///
/// Uses SKILL.md format with `{baseDir}` placeholder:
/// ```markdown
/// ---
/// name: skill-name
/// description: Short description
/// ---
///
/// # Instructions
/// Helper files at: {baseDir}/
/// ```
///
/// # Available Skills
///
/// - brave-search: Web search via Brave API
/// - browser-tools: Browser automation tools
/// - gccli: Google Cloud CLI integration
/// - gdcli: Google Drive CLI integration
/// - gmcli: Gmail CLI integration
/// - transcribe: Audio transcription
/// - vscode: VS Code integration
/// - youtube-transcript: YouTube video transcripts
pub struct PiSkillsHook {
    /// Base hook functionality
    base: BaseHook,

    /// Skills directory (may be None if not found)
    skills_dir: Option<PathBuf>,

    /// Process monitor
    process_monitor: ProcessMonitor,

    /// Whether skill is installed
    skill_installed: bool,

    /// Detected skills
    detected_skills: Vec<SkillMetadata>,
}

impl PiSkillsHook {
    /// Agent type string
    pub const AGENT_TYPE: &'static str = "pi-skills";

    /// Skills directory names to check
    pub const SKILL_DIRS: &'static [&'static str] = &[".pi-skills", ".pi/skills", ".omp/skills"];

    /// Known skills from pi-skills repository
    pub const KNOWN_SKILLS: &'static [&'static str] = &[
        "brave-search",
        "browser-tools",
        "gccli",
        "gdcli",
        "gmcli",
        "transcribe",
        "vscode",
        "youtube-transcript",
    ];

    /// Create a new Pi-Skills hook
    pub fn new() -> Self {
        let skills_dir = Self::find_skills_dir();

        let mut hook = Self {
            base: BaseHook::new(Self::AGENT_TYPE),
            skills_dir: skills_dir.clone(),
            process_monitor: ProcessMonitor::new(),
            skill_installed: false,
            detected_skills: Vec::new(),
        };

        // Discover available skills
        if let Some(ref dir) = skills_dir {
            hook.discover_skills(dir);
        }

        // Try to install nexus skill
        if let Some(ref dir) = skills_dir {
            if let Err(e) = hook.install_skill(dir) {
                tracing::warn!("Failed to install pi-skills skill: {}", e);
            }
        }

        hook
    }

    /// Find skills directory
    fn find_skills_dir() -> Option<PathBuf> {
        let home = dirs::home_dir()?;

        for dir_name in Self::SKILL_DIRS {
            let dir = home.join(dir_name);
            if dir.exists() {
                return Some(dir);
            }
        }

        None
    }

    /// Discover available skills
    fn discover_skills(&mut self, skills_dir: &PathBuf) {
        if !skills_dir.exists() {
            return;
        }

        if let Ok(entries) = std::fs::read_dir(skills_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let skill_md = entry.path().join("SKILL.md");
                if skill_md.exists() {
                    if let Ok(content) = std::fs::read_to_string(&skill_md) {
                        if let Some(metadata) = self.parse_skill_metadata(&content) {
                            self.detected_skills.push(metadata);
                        }
                    }
                }
            }
        }
    }

    /// Parse SKILL.md frontmatter
    fn parse_skill_metadata(&self, content: &str) -> Option<SkillMetadata> {
        let content = content.trim();

        if !content.starts_with("---") {
            return None;
        }

        let end = content[3..].find("---")?;
        let frontmatter = &content[3..end + 3];

        // Parse YAML frontmatter (simplified)
        let mut metadata = SkillMetadata::default();

        for line in frontmatter.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');

                match key {
                    "name" => metadata.name = value.to_string(),
                    "description" => metadata.description = Some(value.to_string()),
                    "version" => metadata.version = Some(value.to_string()),
                    "author" => metadata.author = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        if !metadata.name.is_empty() {
            Some(metadata)
        } else {
            None
        }
    }

    /// Install the nexus-memory-extraction skill
    fn install_skill(&mut self, skills_dir: &PathBuf) -> Result<()> {
        std::fs::create_dir_all(skills_dir)
            .map_err(|e| HookError::InstallationFailed(format!("Failed to create skills dir: {}", e)))?;

        let skill_dir = skills_dir.join("nexus-memory-extraction");
        std::fs::create_dir_all(&skill_dir)
            .map_err(|e| HookError::InstallationFailed(format!("Failed to create skill dir: {}", e)))?;

        let skill_md = skill_dir.join("SKILL.md");

        // Cross-compatible skill format
        let skill_content = r#"---
name: nexus-memory-extraction
description: Automatically extract session context to Nexus Memory System
version: 1.0.0
author: Nexus Memory System
triggers:
  - on_session_end
  - on_checkpoint
---

# Nexus Memory Extraction Skill

Cross-compatible skill for extracting session context.

## Compatible Platforms

- pi-mono
- oh-my-pi
- Claude Code
- Codex CLI
- Amp
- Droid

## Usage

This skill runs automatically when sessions end.

## Configuration

Helper files available at: {baseDir}/

Set environment variables:
- `NEXUS_AUTO_INGEST=true`
- `NEXUS_SERVER_URL=http://localhost:8768`
"#;

        std::fs::write(&skill_md, skill_content)
            .map_err(|e| HookError::InstallationFailed(format!("Failed to write skill: {}", e)))?;

        self.skill_installed = true;
        tracing::info!("Pi-skills skill installed at: {:?}", skill_dir);

        Ok(())
    }

    /// Get list of available skills
    pub fn available_skills(&self) -> &[SkillMetadata] {
        &self.detected_skills
    }

    /// Check if a specific skill is available
    pub fn has_skill(&self, name: &str) -> bool {
        self.detected_skills.iter().any(|s| s.name == name)
    }

    /// Get skill by name
    pub fn get_skill(&self, name: &str) -> Option<&SkillMetadata> {
        self.detected_skills.iter().find(|s| s.name == name)
    }
}

impl Default for PiSkillsHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentHook for PiSkillsHook {
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
        let processes = monitor.find_agent_processes(AgentType::PiSkills);

        let mut activity = SessionActivity::new(AgentType::PiSkills);

        if !processes.is_empty() {
            activity.is_active = true;
            activity.processes = processes;
        }

        // Check for skills directory activity
        if let Some(ref dir) = self.skills_dir {
            if dir.exists() {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let skill_md = entry.path().join("SKILL.md");
                        if skill_md.exists() {
                            if let Ok(metadata) = std::fs::metadata(&skill_md) {
                                if let Ok(modified) = metadata.modified() {
                                    let age = std::time::SystemTime::now()
                                        .duration_since(modified)
                                        .unwrap_or(std::time::Duration::MAX);

                                    if age.as_secs() < 300 {
                                        activity.is_active = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(activity)
    }

    async fn extract_session_context(&self) -> Result<SessionContext> {
        let mut context = SessionContext::new("pi-skills")
            .with_source("native")
            .with_reliability(1.0);

        // Add detected skills info
        let skill_names: Vec<String> = self.detected_skills.iter()
            .map(|s| s.name.clone())
            .collect();

        context.add_custom(
            "available_skills",
            serde_json::to_value(&skill_names).unwrap_or(serde_json::Value::Null),
        );

        // Add skill details
        for skill in &self.detected_skills {
            if let Some(ref desc) = skill.description {
                context.add_insight(format!("Skill '{}': {}", skill.name, desc));
            }
        }

        // Check for known skills availability
        for known_skill in Self::KNOWN_SKILLS {
            let is_available = self.has_skill(known_skill);
            context.add_custom(
                &format!("skill_{}_available", known_skill.replace('-', "_")),
                serde_json::Value::Bool(is_available),
            );
        }

        // Get git status for skills repo if it's a git repo
        if let Some(ref dir) = self.skills_dir {
            let git_status = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(dir)
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
    fn test_pi_skills_hook_new() {
        let hook = PiSkillsHook::new();
        assert_eq!(hook.agent_type(), "pi-skills");
    }

    #[tokio::test]
    async fn test_pi_skills_hook_detect_activity() {
        let hook = PiSkillsHook::new();
        let activity = hook.detect_session_activity().await.unwrap();

        assert_eq!(activity.agent_type, AgentType::PiSkills);
    }

    #[test]
    fn test_pi_skills_hook_constants() {
        assert_eq!(PiSkillsHook::AGENT_TYPE, "pi-skills");

        let known_skills = PiSkillsHook::KNOWN_SKILLS;
        assert!(known_skills.contains(&"brave-search"));
        assert!(known_skills.contains(&"transcribe"));
        assert!(known_skills.contains(&"youtube-transcript"));
    }

    #[test]
    fn test_pi_skills_hook_has_skill() {
        let hook = PiSkillsHook::new();

        // Should not have unknown skill
        assert!(!hook.has_skill("nonexistent-skill"));
    }
}
