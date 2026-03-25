//! Phase types and definitions

use serde::{Deserialize, Serialize};

/// Phase types for memory organization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PhaseType {
    /// Planning and analysis phase
    Planning,

    /// Execution and implementation phase
    Execution,

    /// Testing and verification phase
    Verification,

    /// Debugging and troubleshooting phase
    Debugging,

    /// Refinement and optimization phase
    Refinement,

    /// General/unclassified phase
    #[default]
    General,
}

impl std::fmt::Display for PhaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhaseType::Planning => write!(f, "planning"),
            PhaseType::Execution => write!(f, "execution"),
            PhaseType::Verification => write!(f, "verification"),
            PhaseType::Debugging => write!(f, "debugging"),
            PhaseType::Refinement => write!(f, "refinement"),
            PhaseType::General => write!(f, "general"),
        }
    }
}

impl PhaseType {
    /// Get description for this phase type
    pub fn description(&self) -> &'static str {
        match self {
            PhaseType::Planning => "Planning and analysis activities",
            PhaseType::Execution => "Implementation and coding activities",
            PhaseType::Verification => "Testing and quality assurance",
            PhaseType::Debugging => "Problem solving and bug fixing",
            PhaseType::Refinement => "Optimization and improvement",
            PhaseType::General => "Unclassified activities",
        }
    }

    /// Get priority level (1=highest)
    pub fn priority(&self) -> u8 {
        match self {
            PhaseType::Debugging => 1,
            PhaseType::Execution => 2,
            PhaseType::Planning => 2,
            PhaseType::Verification => 3,
            PhaseType::Refinement => 4,
            PhaseType::General => 5,
        }
    }

    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "planning" => Some(Self::Planning),
            "execution" => Some(Self::Execution),
            "verification" => Some(Self::Verification),
            "debugging" => Some(Self::Debugging),
            "refinement" => Some(Self::Refinement),
            "general" => Some(Self::General),
            _ => None,
        }
    }
}

/// Phase information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    /// Phase type
    pub phase_type: PhaseType,

    /// Optional sub-phase
    pub sub_phase: Option<String>,

    /// Phase start time
    pub started_at: chrono::DateTime<chrono::Utc>,

    /// Related memory count
    pub memory_count: usize,
}

impl Default for Phase {
    fn default() -> Self {
        Self {
            phase_type: PhaseType::default(),
            sub_phase: None,
            started_at: chrono::Utc::now(),
            memory_count: 0,
        }
    }
}

impl Phase {
    /// Create a new phase
    pub fn new(phase_type: PhaseType) -> Self {
        Self {
            phase_type,
            sub_phase: None,
            started_at: chrono::Utc::now(),
            memory_count: 0,
        }
    }

    /// Create with sub-phase
    pub fn with_sub_phase(mut self, sub_phase: impl Into<String>) -> Self {
        self.sub_phase = Some(sub_phase.into());
        self
    }

    /// Check if this phase is active (high priority)
    pub fn is_active(&self) -> bool {
        matches!(
            self.phase_type,
            PhaseType::Debugging | PhaseType::Execution | PhaseType::Planning
        )
    }

    /// Get the full phase name (including sub-phase)
    pub fn full_name(&self) -> String {
        match &self.sub_phase {
            Some(sub) => format!("{}:{}", self.phase_type, sub),
            None => self.phase_type.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_type_display() {
        assert_eq!(PhaseType::Planning.to_string(), "planning");
        assert_eq!(PhaseType::Execution.to_string(), "execution");
    }

    #[test]
    fn test_phase_type_parse() {
        assert_eq!(PhaseType::parse("planning"), Some(PhaseType::Planning));
        assert_eq!(PhaseType::parse("DEBUGGING"), Some(PhaseType::Debugging));
        assert_eq!(PhaseType::parse("unknown"), None);
    }

    #[test]
    fn test_phase_type_priority() {
        assert_eq!(PhaseType::Debugging.priority(), 1);
        assert_eq!(PhaseType::General.priority(), 5);
    }

    #[test]
    fn test_phase_new() {
        let phase = Phase::new(PhaseType::Planning);

        assert_eq!(phase.phase_type, PhaseType::Planning);
        assert!(phase.sub_phase.is_none());
        assert_eq!(phase.memory_count, 0);
    }

    #[test]
    fn test_phase_with_sub_phase() {
        let phase = Phase::new(PhaseType::Execution).with_sub_phase("feature-x");

        assert_eq!(phase.sub_phase, Some("feature-x".to_string()));
    }

    #[test]
    fn test_phase_is_active() {
        let active = Phase::new(PhaseType::Debugging);
        assert!(active.is_active());

        let inactive = Phase::new(PhaseType::General);
        assert!(!inactive.is_active());
    }

    #[test]
    fn test_phase_full_name() {
        let simple = Phase::new(PhaseType::Planning);
        assert_eq!(simple.full_name(), "planning");

        let with_sub = Phase::new(PhaseType::Execution).with_sub_phase("auth");
        assert_eq!(with_sub.full_name(), "execution:auth");
    }
}
