//! Configuration types for Nexus Memory System

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// LLM provider configuration for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// LLM provider name (openai, anthropic, gemini, openrouter, groq, zai, minimax, mistral)
    pub provider: String,
    /// Model name (e.g., "gpt-4o-mini", "claude-sonnet-4-20250514")
    pub model: String,
    /// API key environment variable name (e.g., "OPENAI_API_KEY")
    pub api_key_env: String,
    /// Base URL override (optional)
    pub base_url: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Temperature for generation (0.0 to 1.0)
    pub temperature: f32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            base_url: None,
            timeout_secs: 60,
            max_tokens: 4096,
            temperature: 0.3,
        }
    }
}

/// Always-on agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Whether the always-on agent is enabled
    pub enabled: bool,
    /// Namespace name for agent-generated memories
    pub namespace: String,
    /// Directory to watch for new files
    pub inbox_dir: String,
    /// File scan interval in seconds
    pub scan_interval_secs: u64,
    /// Consolidation interval in minutes
    pub consolidation_interval_mins: u64,
    /// Maximum memories to consolidate per run
    pub consolidation_batch_size: usize,
    /// Maximum memories to include in query context
    pub query_context_limit: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            namespace: "nexus-agent".to_string(),
            inbox_dir: "./inbox".to_string(),
            scan_interval_secs: 5,
            consolidation_interval_mins: 30,
            consolidation_batch_size: 10,
            query_context_limit: 50,
        }
    }
}

/// Main configuration for Nexus
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Database configuration
    pub database: DatabaseConfig,

    /// Server configuration
    pub server: ServerConfig,

    /// Embedding configuration
    pub embedding: EmbeddingConfig,

    /// Sync configuration
    pub sync: SyncConfig,

    /// LLM configuration
    pub llm: LlmConfig,

    /// Agent configuration
    pub agent: AgentConfig,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> crate::Result<Self> {
        let mut config = Self::default();

        if let Ok(path) = std::env::var("NEXUS_DATABASE_PATH") {
            config.database.path = PathBuf::from(path);
        }

        if let Ok(host) = std::env::var("NEXUS_HOST") {
            config.server.host = host;
        }

        if let Ok(port) = std::env::var("NEXUS_PORT") {
            config.server.port = port.parse().unwrap_or(8768);
        }

        if let Ok(enabled) = std::env::var("NEXUS_EMBEDDINGS_ENABLED") {
            config.embedding.enabled = enabled.parse().unwrap_or(true);
        }

        if let Ok(model) = std::env::var("NEXUS_EMBEDDING_MODEL") {
            config.embedding.model = model;
        }

        if let Ok(policy) = std::env::var("NEXUS_SYNC_POLICY") {
            config.sync.policy = policy;
        }

        // LLM configuration
        if let Ok(provider) = std::env::var("NEXUS_LLM_PROVIDER") {
            config.llm.provider = provider;
        }
        if let Ok(model) = std::env::var("NEXUS_LLM_MODEL") {
            config.llm.model = model;
        }
        if let Ok(key_env) = std::env::var("NEXUS_LLM_API_KEY_ENV") {
            config.llm.api_key_env = key_env;
        }
        if let Ok(base_url) = std::env::var("NEXUS_LLM_BASE_URL") {
            config.llm.base_url = Some(base_url);
        }

        // Agent configuration
        if let Ok(enabled) = std::env::var("NEXUS_AGENT_ENABLED") {
            config.agent.enabled = enabled.parse().unwrap_or(false);
        }
        if let Ok(namespace) = std::env::var("NEXUS_AGENT_NAMESPACE") {
            config.agent.namespace = namespace;
        }
        if let Ok(inbox) = std::env::var("NEXUS_AGENT_INBOX_DIR") {
            config.agent.inbox_dir = inbox;
        }
        if let Ok(interval) = std::env::var("NEXUS_AGENT_CONSOLIDATION_INTERVAL_MINS") {
            config.agent.consolidation_interval_mins = interval.parse().unwrap_or(30);
        } else if let Ok(interval) = std::env::var("NEXUS_AGENT_CONSOLIDATION_INTERVAL") {
            // Backward compat: old name without unit suffix
            config.agent.consolidation_interval_mins = interval.parse().unwrap_or(30);
        }
        if let Ok(interval) = std::env::var("NEXUS_AGENT_SCAN_INTERVAL_SECS") {
            config.agent.scan_interval_secs = interval.parse().unwrap_or(5);
        } else if let Ok(interval) = std::env::var("NEXUS_AGENT_SCAN_INTERVAL") {
            // Backward compat: old name without unit suffix
            config.agent.scan_interval_secs = interval.parse().unwrap_or(5);
        }

        Ok(config)
    }

    /// Get the database URL
    pub fn database_url(&self) -> String {
        format!("sqlite:{}", self.database.path.display())
    }
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Path to SQLite database file
    pub path: PathBuf,

    /// Enable foreign key constraints
    pub foreign_keys: bool,

    /// Connection pool size
    pub pool_size: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let base_path = PathBuf::from(home).join(".nexus");

        Self {
            path: base_path.join("nexus.db"),
            foreign_keys: true,
            pool_size: 5,
        }
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server host
    pub host: String,

    /// Server port
    pub port: u16,

    /// Web dashboard port
    pub web_port: u16,

    /// Transport type (stdio, http, web)
    pub transport: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8768,
            web_port: 8768,
            transport: "stdio".to_string(),
        }
    }
}

/// Embedding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Enable embeddings
    pub enabled: bool,

    /// Embedding model name
    pub model: String,

    /// Embedding dimension
    pub dimension: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: "all-MiniLM-L6-v2".to_string(),
            dimension: 384,
        }
    }
}

/// Sync configuration for cross-agent synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Sync policy (manual, auto, aggressive)
    pub policy: String,

    /// Sync interval in seconds (for auto policy)
    pub interval_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            policy: "manual".to_string(),
            interval_secs: 300,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.embedding.enabled);
        assert_eq!(config.embedding.dimension, 384);
        assert_eq!(config.server.port, 8768);
    }

    #[test]
    fn test_database_url() {
        let config = Config::default();
        let url = config.database_url();
        assert!(url.starts_with("sqlite:"));
    }
}
