//! Hook factory for creating agent-specific hooks

use std::collections::HashMap;

use crate::agents::{
    CLIHook, ClaudeCodeHook, DroidHook, GeminiHook, OhMyPiHook, PiMonoHook, PiSkillsHook, QwenHook,
};
use crate::base::AgentHook;
use crate::error::{HookError, Result};
use crate::types::{AgentType, SupportTier};

/// Factory for creating agent-specific hooks
///
/// The factory maintains a registry of supported agent types and
/// creates the appropriate hook implementation for each.
///
/// # Example
///
/// ```rust
/// use nexus_memory_hooks::HookFactory;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let factory = HookFactory::new();
///
///     // Create hook for specific agent
///     let hook = factory.create_hook("claude-code")?;
///     let hook = factory.create_hook("pi-mono")?;
///     let hook = factory.create_hook("oh-my-pi")?;
///
///     // List supported agents
///     for agent in factory.supported_agents() {
///         println!("Supported: {}", agent);
///     }
///     Ok(())
/// }
/// ```
pub struct HookFactory {
    /// Supported agent types
    supported: HashMap<String, AgentType>,

    /// Aliases for agent types
    aliases: HashMap<String, String>,
}

impl HookFactory {
    /// Create a new hook factory
    pub fn new() -> Self {
        let mut supported = HashMap::new();
        let mut aliases = HashMap::new();

        // Register all supported agent types
        for agent_type in &[
            AgentType::ClaudeCode,
            AgentType::Gemini,
            AgentType::Qwen,
            AgentType::PiMono,
            AgentType::OhMyPi,
            AgentType::PiSkills,
            AgentType::OpenCode,
            AgentType::Codex,
            AgentType::Amp,
            AgentType::Droid,
            AgentType::Hermes,
            AgentType::Generic,
        ] {
            supported.insert(agent_type.to_string(), *agent_type);
        }

        // Register aliases
        aliases.insert("claude".to_string(), "claude-code".to_string());
        aliases.insert("pimono".to_string(), "pi-mono".to_string());
        aliases.insert("pi".to_string(), "pi-mono".to_string());
        aliases.insert("omp".to_string(), "oh-my-pi".to_string());
        aliases.insert("ohmypi".to_string(), "oh-my-pi".to_string());
        aliases.insert("factory".to_string(), "droid".to_string());
        aliases.insert("factory-cli".to_string(), "droid".to_string());

        Self { supported, aliases }
    }

    /// Create a hook for the specified agent type
    pub fn create_hook(&self, agent_type: &str) -> Result<Box<dyn AgentHook>> {
        self.create_hook_internal(agent_type, false)
    }

    /// Create a hook for inspection/status reporting without mutating user state.
    pub fn create_hook_readonly(&self, agent_type: &str) -> Result<Box<dyn AgentHook>> {
        self.create_hook_internal(agent_type, true)
    }

    fn create_hook_internal(&self, agent_type: &str, readonly: bool) -> Result<Box<dyn AgentHook>> {
        // Normalize agent type
        let normalized = self.normalize_agent_type(agent_type);

        // Check if supported
        let agent_type_enum = self.supported.get(&normalized).copied();

        match agent_type_enum {
            Some(AgentType::ClaudeCode) => Ok(Box::new(ClaudeCodeHook::new())),
            Some(AgentType::Gemini) => Ok(Box::new(GeminiHook::new())),
            Some(AgentType::Qwen) => Ok(Box::new(QwenHook::new())),
            Some(AgentType::PiMono) => Ok(Box::new(if readonly {
                PiMonoHook::new_readonly()
            } else {
                PiMonoHook::new()
            })),
            Some(AgentType::OhMyPi) => Ok(Box::new(if readonly {
                OhMyPiHook::new_readonly()
            } else {
                OhMyPiHook::new()
            })),
            Some(AgentType::PiSkills) => Ok(Box::new(if readonly {
                PiSkillsHook::new_readonly()
            } else {
                PiSkillsHook::new()
            })),
            Some(AgentType::OpenCode)
            | Some(AgentType::Codex)
            | Some(AgentType::Amp)
            | Some(AgentType::Hermes)
            | Some(AgentType::Generic) => Ok(Box::new(CLIHook::new(normalized.clone()))),
            Some(AgentType::Droid) => Ok(Box::new(DroidHook::new())),
            None => Err(HookError::AgentNotFound(format!(
                "Unknown agent type: {}",
                agent_type
            ))),
        }
    }

    /// Check if an agent type is supported
    pub fn is_supported(&self, agent_type: &str) -> bool {
        let normalized = self.normalize_agent_type(agent_type);
        self.supported.contains_key(&normalized)
    }

    /// Get list of supported agent types
    pub fn supported_agents(&self) -> Vec<String> {
        self.supported.keys().cloned().collect()
    }

    /// Get agent type info
    pub fn get_agent_info(&self, agent_type: &str) -> Option<AgentInfo> {
        let normalized = self.normalize_agent_type(agent_type);
        self.supported.get(&normalized).map(|&t| AgentInfo {
            agent_type: t.to_string(),
            detection_layer: t.detection_layer(),
            support_tier: t.support_tier(),
            process_names: t.process_names().iter().map(|s| s.to_string()).collect(),
            config_dir: t.config_dir().to_string(),
        })
    }

    /// Normalize agent type string
    fn normalize_agent_type(&self, agent_type: &str) -> String {
        let lower = agent_type.to_lowercase();

        // Check aliases first
        if let Some(alias) = self.aliases.get(&lower) {
            alias.clone()
        } else {
            lower
        }
    }

    /// Register a custom alias
    pub fn register_alias(&mut self, alias: &str, target: &str) {
        self.aliases
            .insert(alias.to_lowercase(), target.to_lowercase());
    }
}

impl Default for HookFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about an agent type
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub agent_type: String,
    pub detection_layer: crate::types::DetectionLayer,
    pub support_tier: SupportTier,
    pub process_names: Vec<String>,
    pub config_dir: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_new() {
        let factory = HookFactory::new();
        assert!(factory.is_supported("claude-code"));
        assert!(factory.is_supported("pi-mono"));
        assert!(factory.is_supported("oh-my-pi"));
    }

    #[test]
    fn test_factory_aliases() {
        let factory = HookFactory::new();

        assert!(factory.is_supported("claude"));
        assert!(factory.is_supported("pi"));
        assert!(factory.is_supported("omp"));
        assert!(factory.is_supported("ohmypi"));
        assert!(factory.is_supported("factory"));
        assert!(factory.is_supported("factory-cli"));
    }

    #[test]
    fn test_factory_create_hook() {
        let factory = HookFactory::new();

        let hook = factory.create_hook("claude-code").unwrap();
        assert_eq!(hook.agent_type(), "claude-code");

        let hook = factory.create_hook("pi-mono").unwrap();
        assert_eq!(hook.agent_type(), "pi-mono");

        let hook = factory.create_hook("oh-my-pi").unwrap();
        assert_eq!(hook.agent_type(), "oh-my-pi");
    }

    #[test]
    fn test_factory_create_hook_alias() {
        let factory = HookFactory::new();

        let hook = factory.create_hook("claude").unwrap();
        assert_eq!(hook.agent_type(), "claude-code");

        let hook = factory.create_hook("omp").unwrap();
        assert_eq!(hook.agent_type(), "oh-my-pi");
    }

    #[test]
    fn test_factory_unsupported() {
        let factory = HookFactory::new();

        let result = factory.create_hook("unknown-agent");
        assert!(result.is_err());
    }

    #[test]
    fn test_factory_supported_agents() {
        let factory = HookFactory::new();
        let agents = factory.supported_agents();

        assert!(agents.contains(&"claude-code".to_string()));
        assert!(agents.contains(&"pi-mono".to_string()));
        assert!(agents.contains(&"oh-my-pi".to_string()));
        assert!(agents.contains(&"hermes".to_string()));
    }

    #[test]
    fn test_factory_get_agent_info() {
        let factory = HookFactory::new();

        let info = factory.get_agent_info("pi-mono").unwrap();
        assert_eq!(info.agent_type, "pi-mono");
        assert_eq!(info.config_dir, ".pi");
    }

    #[test]
    fn test_factory_register_alias() {
        let mut factory = HookFactory::new();
        factory.register_alias("my-agent", "claude-code");

        assert!(factory.is_supported("my-agent"));
        let hook = factory.create_hook("my-agent").unwrap();
        assert_eq!(hook.agent_type(), "claude-code");
    }
}
