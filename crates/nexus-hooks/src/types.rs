//! Common types for the hooks system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Supported agent types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentType {
    // Native hooks
    ClaudeCode,
    Gemini,
    Qwen,

    // Pi-agent family (MANDATORY)
    PiMono,
    OhMyPi,
    PiSkills,

    // CLI-based agents
    OpenCode,
    Codex,
    Amp,
    Droid,

    // Generic
    Generic,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentType::ClaudeCode => write!(f, "claude-code"),
            AgentType::Gemini => write!(f, "gemini"),
            AgentType::Qwen => write!(f, "qwen"),
            AgentType::PiMono => write!(f, "pi-mono"),
            AgentType::OhMyPi => write!(f, "oh-my-pi"),
            AgentType::PiSkills => write!(f, "pi-skills"),
            AgentType::OpenCode => write!(f, "opencode"),
            AgentType::Codex => write!(f, "codex"),
            AgentType::Amp => write!(f, "amp"),
            AgentType::Droid => write!(f, "droid"),
            AgentType::Generic => write!(f, "generic"),
        }
    }
}

impl AgentType {
    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude-code" | "claude" => Some(Self::ClaudeCode),
            "gemini" => Some(Self::Gemini),
            "qwen" => Some(Self::Qwen),
            "pi-mono" | "pimono" | "pi" => Some(Self::PiMono),
            "oh-my-pi" | "ohmypi" | "omp" => Some(Self::OhMyPi),
            "pi-skills" => Some(Self::PiSkills),
            "opencode" => Some(Self::OpenCode),
            "codex" => Some(Self::Codex),
            "amp" => Some(Self::Amp),
            "droid" => Some(Self::Droid),
            _ => None,
        }
    }

    /// Get the detection layer type for this agent
    pub fn detection_layer(&self) -> DetectionLayer {
        match self {
            AgentType::ClaudeCode
            | AgentType::Gemini
            | AgentType::Qwen
            | AgentType::PiMono
            | AgentType::OhMyPi
            | AgentType::PiSkills => DetectionLayer::Native,
            AgentType::OpenCode | AgentType::Codex | AgentType::Amp | AgentType::Droid => {
                DetectionLayer::CLI
            }
            AgentType::Generic => DetectionLayer::CLI,
        }
    }

    /// Get process names to detect for this agent
    pub fn process_names(&self) -> &'static [&'static str] {
        match self {
            AgentType::ClaudeCode => &["claude", "claude-code"],
            AgentType::Gemini => &["gemini", "gemini-cli"],
            AgentType::Qwen => &["qwen", "qwen-agent"],
            AgentType::PiMono => &["pi", "pi-coding-agent", "pi-mono"],
            AgentType::OhMyPi => &["omp", "oh-my-pi", "ohmypi"],
            AgentType::PiSkills => &["pi-skills"],
            AgentType::OpenCode => &["opencode"],
            AgentType::Codex => &["codex", "codex-cli"],
            AgentType::Amp => &["amp"],
            AgentType::Droid => &["droid"],
            AgentType::Generic => &[],
        }
    }

    /// Get config directory for this agent
    pub fn config_dir(&self) -> &'static str {
        match self {
            AgentType::ClaudeCode => ".claude",
            AgentType::Gemini => ".gemini",
            AgentType::Qwen => ".qwen",
            AgentType::PiMono => ".pi",
            AgentType::OhMyPi => ".omp",
            AgentType::PiSkills => ".pi-skills",
            AgentType::OpenCode => ".opencode",
            AgentType::Codex => ".codex",
            AgentType::Amp => ".amp",
            AgentType::Droid => ".droid",
            AgentType::Generic => ".nexus",
        }
    }

    /// Get skills directory relative to config dir
    pub fn skills_dir(&self) -> &'static str {
        match self {
            AgentType::ClaudeCode => "skills",
            AgentType::PiMono => "agent/skills",
            AgentType::OhMyPi => "agent/skills",
            AgentType::PiSkills => "skills",
            _ => "skills",
        }
    }
}

/// Detection layer type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionLayer {
    /// Native hooks (Skills, Functions, etc.) - 100% reliability
    Native,
    /// CLI-based hooks (atexit, signals) - 95% reliability
    CLI,
    /// Process monitoring - 95% reliability
    Monitor,
    /// Inactivity detection - 90% reliability
    Inactivity,
    /// Buffer recovery - 99% reliability
    Buffer,
}

impl std::fmt::Display for DetectionLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DetectionLayer::Native => write!(f, "native"),
            DetectionLayer::CLI => write!(f, "cli"),
            DetectionLayer::Monitor => write!(f, "monitor"),
            DetectionLayer::Inactivity => write!(f, "inactivity"),
            DetectionLayer::Buffer => write!(f, "buffer"),
        }
    }
}

/// Source of extraction trigger
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionSource {
    /// Native hook triggered
    NativeHook(String),
    /// Process monitor detected session end
    ProcessMonitor,
    /// Inactivity timeout
    InactivityTimeout,
    /// Signal handler (SIGTERM/SIGINT)
    SignalHandler(String),
    /// Buffer recovery after crash
    BufferRecovery,
    /// Manual trigger
    Manual,
}

impl std::fmt::Display for ExtractionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractionSource::NativeHook(name) => write!(f, "native:{}", name),
            ExtractionSource::ProcessMonitor => write!(f, "process_monitor"),
            ExtractionSource::InactivityTimeout => write!(f, "inactivity_timeout"),
            ExtractionSource::SignalHandler(sig) => write!(f, "signal:{}", sig),
            ExtractionSource::BufferRecovery => write!(f, "buffer_recovery"),
            ExtractionSource::Manual => write!(f, "manual"),
        }
    }
}

/// SKILL.md front matter metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    #[serde(default)]
    pub triggers: Vec<SkillTrigger>,
    pub priority: Option<String>,
}

/// Skill trigger types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrigger {
    OnSessionEnd,
    OnCheckpoint,
    OnCompletion,
    OnError,
    Manual,
}

impl std::fmt::Display for SkillTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillTrigger::OnSessionEnd => write!(f, "on_session_end"),
            SkillTrigger::OnCheckpoint => write!(f, "on_checkpoint"),
            SkillTrigger::OnCompletion => write!(f, "on_completion"),
            SkillTrigger::OnError => write!(f, "on_error"),
            SkillTrigger::Manual => write!(f, "manual"),
        }
    }
}

/// Process information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub status: String,
    pub command: Option<String>,
    pub working_dir: Option<String>,
    pub create_time: Option<DateTime<Utc>>,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
}

/// Session activity status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionActivity {
    pub agent_type: AgentType,
    pub is_active: bool,
    pub processes: Vec<ProcessInfo>,
    pub session_id: Option<String>,
    pub last_activity: Option<DateTime<Utc>>,
    pub context: HashMap<String, serde_json::Value>,
}

impl SessionActivity {
    pub fn new(agent_type: AgentType) -> Self {
        Self {
            agent_type,
            is_active: false,
            processes: Vec::new(),
            session_id: None,
            last_activity: None,
            context: HashMap::new(),
        }
    }

    pub fn with_active(mut self, active: bool) -> Self {
        self.is_active = active;
        self
    }

    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn with_process(mut self, process: ProcessInfo) -> Self {
        self.processes.push(process);
        self.is_active = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_display() {
        assert_eq!(AgentType::ClaudeCode.to_string(), "claude-code");
        assert_eq!(AgentType::PiMono.to_string(), "pi-mono");
        assert_eq!(AgentType::OhMyPi.to_string(), "oh-my-pi");
    }

    #[test]
    fn test_agent_type_parse() {
        assert_eq!(AgentType::parse("claude-code"), Some(AgentType::ClaudeCode));
        assert_eq!(AgentType::parse("claude"), Some(AgentType::ClaudeCode));
        assert_eq!(AgentType::parse("pi"), Some(AgentType::PiMono));
        assert_eq!(AgentType::parse("omp"), Some(AgentType::OhMyPi));
        assert_eq!(AgentType::parse("unknown"), None);
    }

    #[test]
    fn test_agent_type_detection_layer() {
        assert_eq!(
            AgentType::ClaudeCode.detection_layer(),
            DetectionLayer::Native
        );
        assert_eq!(AgentType::PiMono.detection_layer(), DetectionLayer::Native);
        assert_eq!(AgentType::Amp.detection_layer(), DetectionLayer::CLI);
    }

    #[test]
    fn test_agent_type_process_names() {
        let names = AgentType::PiMono.process_names();
        assert!(names.contains(&"pi"));
        assert!(names.contains(&"pi-mono"));
    }

    #[test]
    fn test_agent_type_config_dir() {
        assert_eq!(AgentType::PiMono.config_dir(), ".pi");
        assert_eq!(AgentType::OhMyPi.config_dir(), ".omp");
        assert_eq!(AgentType::ClaudeCode.config_dir(), ".claude");
    }

    #[test]
    fn test_detection_layer_display() {
        assert_eq!(DetectionLayer::Native.to_string(), "native");
        assert_eq!(DetectionLayer::Buffer.to_string(), "buffer");
    }

    #[test]
    fn test_session_activity() {
        let activity = SessionActivity::new(AgentType::ClaudeCode)
            .with_active(true)
            .with_session_id("test-123");

        assert!(activity.is_active);
        assert_eq!(activity.session_id, Some("test-123".to_string()));
    }
}
