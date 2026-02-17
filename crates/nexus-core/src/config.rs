//! Configuration types for Nexus Memory System

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main configuration for Nexus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Database configuration
    pub database: DatabaseConfig,

    /// Server configuration
    pub server: ServerConfig,

    /// Embedding configuration
    pub embedding: EmbeddingConfig,

    /// Sync configuration
    pub sync: SyncConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: DatabaseConfig::default(),
            server: ServerConfig::default(),
            embedding: EmbeddingConfig::default(),
            sync: SyncConfig::default(),
        }
    }
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
