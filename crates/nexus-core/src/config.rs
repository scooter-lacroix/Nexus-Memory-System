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

/// Cognition and runtime orchestration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitionConfig {
    /// Enable session-scoped runtime orchestration for hook-driven agent sessions
    pub auto_runtime_enabled: bool,
    /// Enable derivation of explicit observations from raw memories
    pub derive_enabled: bool,
    /// Enable session digest generation
    pub digest_enabled: bool,
    /// Enable reflective/dream processing
    pub reflect_enabled: bool,
    /// Enable low-signal activity distillation into higher-level summaries
    pub activity_distill_enabled: bool,
    /// Whether to trigger a bounded dream pass when a session ends
    pub dream_on_session_end: bool,
    /// Whether compact/checkpoint events should trigger a bounded flush
    pub checkpoint_flush_enabled: bool,
    /// Idle timeout, in seconds, for session-scoped runtime state
    pub runtime_idle_timeout_secs: u64,
    /// Maximum number of cognition jobs to claim in one worker batch
    pub max_job_batch: usize,
    /// Lease TTL, in seconds, for claimed cognition jobs
    pub lease_ttl_secs: u64,
    /// Maximum memories to include when building a working representation
    pub representation_max_items: usize,
    /// Target token budget for short digests
    pub digest_short_target_tokens: usize,
    /// Target token budget for long digests
    pub digest_long_target_tokens: usize,
    /// Timeout, in seconds, for direct hook-enrichment attempts
    pub direct_enrichment_timeout_secs: u64,
    /// Minimum number of events required before activity distillation runs
    pub activity_distill_min_events: usize,
    /// Maximum number of events to include in one activity distillation batch
    pub activity_distill_max_events: usize,
    /// Whether raw memories should be included by default in retrieval surfaces
    pub include_raw_by_default: bool,
    /// Timeout, in seconds, for best-effort dream work during session shutdown
    pub session_end_dream_timeout_secs: u64,
    /// Maximum retry-buffer artifacts to process during a bounded flush
    pub retry_buffer_drain_limit: usize,
    /// Enable belief revision when contradictions are detected (reduce confidence on sources)
    pub contradiction_belief_revision_enabled: bool,
    /// How much to reduce confidence on each contradicted memory (0.0–1.0)
    pub contradiction_confidence_penalty: f32,
    /// Enable memory aging decay in ranking blend
    pub memory_decay_enabled: bool,
    /// Days before a memory starts decaying in ranking score
    pub memory_decay_age_days: u64,
    /// Days that a recent access resets the decay clock
    pub memory_decay_access_boost_days: u64,
    /// Enable adaptive dream interval in the periodic supervisor loop
    pub adaptive_dream_enabled: bool,
    /// Floor (minimum) for adaptive dream interval in seconds
    pub adaptive_dream_min_interval_secs: u64,
    /// Ceiling (maximum) for adaptive dream interval in seconds
    pub adaptive_dream_max_interval_secs: u64,
}

impl Default for CognitionConfig {
    fn default() -> Self {
        Self {
            auto_runtime_enabled: true,
            derive_enabled: true,
            digest_enabled: true,
            reflect_enabled: true,
            activity_distill_enabled: true,
            dream_on_session_end: true,
            checkpoint_flush_enabled: true,
            runtime_idle_timeout_secs: 900,
            max_job_batch: 8,
            lease_ttl_secs: 120,
            representation_max_items: 24,
            digest_short_target_tokens: 600,
            digest_long_target_tokens: 1800,
            direct_enrichment_timeout_secs: 8,
            activity_distill_min_events: 8,
            activity_distill_max_events: 60,
            include_raw_by_default: false,
            session_end_dream_timeout_secs: 8,
            retry_buffer_drain_limit: 8,
            contradiction_belief_revision_enabled: true,
            contradiction_confidence_penalty: 0.15,
            memory_decay_enabled: true,
            memory_decay_age_days: 90,
            memory_decay_access_boost_days: 30,
            adaptive_dream_enabled: true,
            adaptive_dream_min_interval_secs: 60,
            adaptive_dream_max_interval_secs: 1800,
        }
    }
}

/// Dream trigger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamTriggerConfig {
    /// Whether to trigger a nap on session end
    pub nap_on_session_end: bool,
    /// Idle timeout in seconds before a nap is triggered
    pub nap_idle_timeout_secs: u64,
    /// Number of new memories that trigger a dream cycle
    pub dream_memory_threshold: usize,
    /// Minimum hours between deep dream cycles
    pub deep_dream_cooldown_hours: u64,
    /// Minimum inactivity minutes before deep dream
    pub deep_dream_inactivity_mins: u64,
}

impl Default for DreamTriggerConfig {
    fn default() -> Self {
        Self {
            nap_on_session_end: true,
            nap_idle_timeout_secs: 600,
            dream_memory_threshold: 20,
            deep_dream_cooldown_hours: 24,
            deep_dream_inactivity_mins: 30,
        }
    }
}

/// Autonomous cognitive system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveSystemConfig {
    /// Whether the cognitive system is enabled
    pub enabled: bool,
    /// System bootstrap mode (silent, chatty)
    pub bootstrap_mode: String,
    /// Dream triggers
    pub dream_triggers: DreamTriggerConfig,
    /// Maximum entries in the hot cognitive cache
    pub hot_cache_max_entries: usize,
    /// Percentage of context window allocated to Nexus context
    pub context_allocation_pct: f32,
    /// Whether mid-session rescoring is enabled
    pub mid_session_rescore_enabled: bool,
    /// Turn interval for rescoring
    pub rescore_turn_interval: u32,
    /// Topic drift threshold for rescoring
    pub rescore_drift_threshold: f32,
}

impl Default for CognitiveSystemConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bootstrap_mode: "silent".to_string(),
            dream_triggers: DreamTriggerConfig::default(),
            hot_cache_max_entries: 20,
            context_allocation_pct: 0.10,
            mid_session_rescore_enabled: true,
            rescore_turn_interval: 5,
            rescore_drift_threshold: 0.70,
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

    /// Cognition/runtime configuration
    pub cognition: CognitionConfig,

    /// Autonomous cognitive system configuration
    pub cognitive_system: CognitiveSystemConfig,
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
            config.embedding.enabled = enabled.parse().unwrap_or(false);
        }

        if let Ok(backend) = std::env::var("NEXUS_EMBEDDING_BACKEND") {
            config.embedding.backend = backend;
        }

        if let Ok(provider) = std::env::var("NEXUS_EMBEDDING_PROVIDER") {
            config.embedding.provider = provider;
        }

        if let Ok(model) = std::env::var("NEXUS_EMBEDDING_MODEL") {
            config.embedding.model = model;
        }

        if let Ok(key_env) = std::env::var("NEXUS_EMBEDDING_API_KEY_ENV") {
            config.embedding.api_key_env = key_env;
        }

        if let Ok(base_url) = std::env::var("NEXUS_EMBEDDING_BASE_URL") {
            config.embedding.base_url = Some(base_url);
        }

        if let Ok(dimension) = std::env::var("NEXUS_EMBEDDING_DIMENSION") {
            config.embedding.dimension = dimension
                .parse()
                .unwrap_or(EmbeddingConfig::default().dimension);
        }

        if let Ok(timeout) = std::env::var("NEXUS_EMBEDDING_TIMEOUT_SECS") {
            config.embedding.timeout_secs = timeout
                .parse()
                .unwrap_or(EmbeddingConfig::default().timeout_secs);
        }

        if let Ok(model_path) = std::env::var("NEXUS_EMBEDDING_MODEL_PATH") {
            config.embedding.local_model_path = Some(model_path);
        }

        if let Ok(tokenizer_path) = std::env::var("NEXUS_TOKENIZER_PATH") {
            config.embedding.local_tokenizer_path = Some(tokenizer_path);
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
            config.agent.consolidation_interval_mins = interval
                .parse()
                .unwrap_or(AgentConfig::default().consolidation_interval_mins);
        } else if let Ok(interval) = std::env::var("NEXUS_AGENT_CONSOLIDATION_INTERVAL") {
            // Backward compat: old name without unit suffix
            config.agent.consolidation_interval_mins = interval
                .parse()
                .unwrap_or(AgentConfig::default().consolidation_interval_mins);
        }
        if let Ok(interval) = std::env::var("NEXUS_AGENT_SCAN_INTERVAL_SECS") {
            config.agent.scan_interval_secs = interval
                .parse()
                .unwrap_or(AgentConfig::default().scan_interval_secs);
        } else if let Ok(interval) = std::env::var("NEXUS_AGENT_SCAN_INTERVAL") {
            // Backward compat: old name without unit suffix
            config.agent.scan_interval_secs = interval
                .parse()
                .unwrap_or(AgentConfig::default().scan_interval_secs);
        }

        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_AUTO_RUNTIME_ENABLED") {
            config.cognition.auto_runtime_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_DERIVE_ENABLED") {
            config.cognition.derive_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_DIGEST_ENABLED") {
            config.cognition.digest_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_REFLECT_ENABLED") {
            config.cognition.reflect_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_ACTIVITY_DISTILL_ENABLED") {
            config.cognition.activity_distill_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_DREAM_ON_SESSION_END") {
            config.cognition.dream_on_session_end = enabled.parse().unwrap_or(true);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_CHECKPOINT_FLUSH_ENABLED") {
            config.cognition.checkpoint_flush_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(timeout) = std::env::var("NEXUS_COGNITION_RUNTIME_IDLE_TIMEOUT_SECS") {
            config.cognition.runtime_idle_timeout_secs = timeout
                .parse()
                .unwrap_or(CognitionConfig::default().runtime_idle_timeout_secs);
        }
        if let Ok(batch) = std::env::var("NEXUS_COGNITION_MAX_JOB_BATCH") {
            config.cognition.max_job_batch = batch
                .parse()
                .unwrap_or(CognitionConfig::default().max_job_batch);
        }
        if let Ok(ttl) = std::env::var("NEXUS_COGNITION_LEASE_TTL_SECS") {
            config.cognition.lease_ttl_secs = ttl
                .parse()
                .unwrap_or(CognitionConfig::default().lease_ttl_secs);
        }
        if let Ok(items) = std::env::var("NEXUS_COGNITION_REPRESENTATION_MAX_ITEMS") {
            config.cognition.representation_max_items = items
                .parse()
                .unwrap_or(CognitionConfig::default().representation_max_items);
        }
        if let Ok(tokens) = std::env::var("NEXUS_COGNITION_DIGEST_SHORT_TARGET_TOKENS") {
            config.cognition.digest_short_target_tokens = tokens
                .parse()
                .unwrap_or(CognitionConfig::default().digest_short_target_tokens);
        }
        if let Ok(tokens) = std::env::var("NEXUS_COGNITION_DIGEST_LONG_TARGET_TOKENS") {
            config.cognition.digest_long_target_tokens = tokens
                .parse()
                .unwrap_or(CognitionConfig::default().digest_long_target_tokens);
        }
        if let Ok(timeout) = std::env::var("NEXUS_COGNITION_DIRECT_ENRICHMENT_TIMEOUT_SECS") {
            config.cognition.direct_enrichment_timeout_secs = timeout
                .parse()
                .unwrap_or(CognitionConfig::default().direct_enrichment_timeout_secs);
        }
        if let Ok(events) = std::env::var("NEXUS_COGNITION_ACTIVITY_DISTILL_MIN_EVENTS") {
            config.cognition.activity_distill_min_events = events
                .parse()
                .unwrap_or(CognitionConfig::default().activity_distill_min_events);
        }
        if let Ok(events) = std::env::var("NEXUS_COGNITION_ACTIVITY_DISTILL_MAX_EVENTS") {
            config.cognition.activity_distill_max_events = events
                .parse()
                .unwrap_or(CognitionConfig::default().activity_distill_max_events);
        }
        if let Ok(include_raw) = std::env::var("NEXUS_COGNITION_INCLUDE_RAW_BY_DEFAULT") {
            config.cognition.include_raw_by_default = include_raw.parse().unwrap_or(false);
        }
        if let Ok(timeout) = std::env::var("NEXUS_COGNITION_SESSION_END_DREAM_TIMEOUT_SECS") {
            config.cognition.session_end_dream_timeout_secs = timeout
                .parse()
                .unwrap_or(CognitionConfig::default().session_end_dream_timeout_secs);
        }
        if let Ok(limit) = std::env::var("NEXUS_COGNITION_RETRY_BUFFER_DRAIN_LIMIT") {
            config.cognition.retry_buffer_drain_limit = limit
                .parse()
                .unwrap_or(CognitionConfig::default().retry_buffer_drain_limit);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_CONTRADICTION_BELIEF_REVISION_ENABLED")
        {
            config.cognition.contradiction_belief_revision_enabled =
                enabled.parse().unwrap_or(true);
        }
        if let Ok(penalty) = std::env::var("NEXUS_COGNITION_CONTRADICTION_CONFIDENCE_PENALTY") {
            config.cognition.contradiction_confidence_penalty = penalty
                .parse()
                .unwrap_or(CognitionConfig::default().contradiction_confidence_penalty);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_MEMORY_DECAY_ENABLED") {
            config.cognition.memory_decay_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(days) = std::env::var("NEXUS_COGNITION_MEMORY_DECAY_AGE_DAYS") {
            config.cognition.memory_decay_age_days = days
                .parse()
                .unwrap_or(CognitionConfig::default().memory_decay_age_days);
        }
        if let Ok(days) = std::env::var("NEXUS_COGNITION_MEMORY_DECAY_ACCESS_BOOST_DAYS") {
            config.cognition.memory_decay_access_boost_days = days
                .parse()
                .unwrap_or(CognitionConfig::default().memory_decay_access_boost_days);
        }
        if let Ok(enabled) = std::env::var("NEXUS_COGNITION_ADAPTIVE_DREAM_ENABLED") {
            config.cognition.adaptive_dream_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(secs) = std::env::var("NEXUS_COGNITION_ADAPTIVE_DREAM_MIN_INTERVAL_SECS") {
            config.cognition.adaptive_dream_min_interval_secs = secs
                .parse()
                .unwrap_or(CognitionConfig::default().adaptive_dream_min_interval_secs);
        }
        if let Ok(secs) = std::env::var("NEXUS_COGNITION_ADAPTIVE_DREAM_MAX_INTERVAL_SECS") {
            config.cognition.adaptive_dream_max_interval_secs = secs
                .parse()
                .unwrap_or(CognitionConfig::default().adaptive_dream_max_interval_secs);
        }

        // Cognitive System configuration
        if let Ok(enabled) = std::env::var("NEXUS_COGNITIVE_ENABLED") {
            config.cognitive_system.enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(mode) = std::env::var("NEXUS_BOOTSTRAP_MODE") {
            config.cognitive_system.bootstrap_mode = mode;
        }
        if let Ok(max) = std::env::var("NEXUS_HOT_CACHE_MAX") {
            config.cognitive_system.hot_cache_max_entries = max
                .parse()
                .unwrap_or(CognitiveSystemConfig::default().hot_cache_max_entries);
        }
        if let Ok(pct) = std::env::var("NEXUS_CONTEXT_ALLOCATION_PCT") {
            config.cognitive_system.context_allocation_pct = pct
                .parse()
                .unwrap_or(CognitiveSystemConfig::default().context_allocation_pct);
        }
        if let Ok(enabled) = std::env::var("NEXUS_RESCORE_ENABLED") {
            config.cognitive_system.mid_session_rescore_enabled = enabled.parse().unwrap_or(true);
        }
        if let Ok(interval) = std::env::var("NEXUS_RESCORE_TURN_INTERVAL") {
            config.cognitive_system.rescore_turn_interval = interval
                .parse()
                .unwrap_or(CognitiveSystemConfig::default().rescore_turn_interval);
        }
        if let Ok(threshold) = std::env::var("NEXUS_DREAM_THRESHOLD") {
            config
                .cognitive_system
                .dream_triggers
                .dream_memory_threshold = threshold
                .parse()
                .unwrap_or(DreamTriggerConfig::default().dream_memory_threshold);
        }
        if let Ok(hours) = std::env::var("NEXUS_DEEP_DREAM_COOLDOWN_HOURS") {
            config
                .cognitive_system
                .dream_triggers
                .deep_dream_cooldown_hours = hours
                .parse()
                .unwrap_or(DreamTriggerConfig::default().deep_dream_cooldown_hours);
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

    /// Transport type (stdio, web)
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

    /// Embedding backend (`local` or `openai-compatible`)
    pub backend: String,

    /// Embedding provider/profile (`inherit`, `openai`, `gemini`, `openrouter`,
    /// `vllm`, `lmstudio`, `llamacpp`, `custom`, etc.)
    pub provider: String,

    /// Embedding model name
    pub model: String,

    /// API key environment variable name for remote embedding providers
    pub api_key_env: String,

    /// Base URL override for remote or local OpenAI-compatible runtimes
    pub base_url: Option<String>,

    /// Embedding dimension
    pub dimension: usize,

    /// Request timeout for remote embedding providers
    pub timeout_secs: u64,

    /// Path to a local ONNX embedding model
    pub local_model_path: Option<String>,

    /// Path to the tokenizer directory for local ONNX embeddings
    pub local_tokenizer_path: Option<String>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: "local".to_string(),
            provider: "local".to_string(),
            model: "all-MiniLM-L6-v2".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            base_url: None,
            dimension: 384,
            timeout_secs: 60,
            local_model_path: Some("models/all-MiniLM-L6-v2.onnx".to_string()),
            local_tokenizer_path: Some("models/all-MiniLM-L6-v2-tokenizer".to_string()),
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
    use serial_test::serial;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.embedding.enabled);
        assert_eq!(config.embedding.backend, "local");
        assert_eq!(config.embedding.provider, "local");
        assert_eq!(config.embedding.dimension, 384);
        assert_eq!(config.server.port, 8768);
    }

    #[test]
    fn test_database_url() {
        let config = Config::default();
        let url = config.database_url();
        assert!(url.starts_with("sqlite:"));
    }

    #[test]
    fn test_cognition_config_defaults() {
        let config = Config::default();
        assert!(config.cognition.derive_enabled);
        assert!(config.cognition.digest_enabled);
        assert!(config.cognition.reflect_enabled);
        assert!(config.cognition.activity_distill_enabled);
        assert_eq!(config.cognition.representation_max_items, 24);
        assert!(!config.cognition.include_raw_by_default);
        assert!(config.cognition.contradiction_belief_revision_enabled);
        assert!((config.cognition.contradiction_confidence_penalty - 0.15).abs() < f32::EPSILON);
        assert!(config.cognition.memory_decay_enabled);
        assert_eq!(config.cognition.memory_decay_age_days, 90);
        assert!(config.cognition.adaptive_dream_enabled);
        assert_eq!(config.cognition.adaptive_dream_min_interval_secs, 60);
        assert_eq!(config.cognition.adaptive_dream_max_interval_secs, 1800);
    }

    #[test]
    #[serial]
    fn test_cognition_config_from_env() {
        std::env::set_var("NEXUS_COGNITION_DERIVE_ENABLED", "false");
        std::env::set_var("NEXUS_COGNITION_MAX_JOB_BATCH", "16");
        std::env::set_var("NEXUS_COGNITION_REPRESENTATION_MAX_ITEMS", "42");
        std::env::set_var("NEXUS_COGNITION_INCLUDE_RAW_BY_DEFAULT", "true");

        let config = Config::from_env().expect("config from env");
        assert!(!config.cognition.derive_enabled);
        assert_eq!(config.cognition.max_job_batch, 16);
        assert_eq!(config.cognition.representation_max_items, 42);
        assert!(config.cognition.include_raw_by_default);

        std::env::remove_var("NEXUS_COGNITION_DERIVE_ENABLED");
        std::env::remove_var("NEXUS_COGNITION_MAX_JOB_BATCH");
        std::env::remove_var("NEXUS_COGNITION_REPRESENTATION_MAX_ITEMS");
        std::env::remove_var("NEXUS_COGNITION_INCLUDE_RAW_BY_DEFAULT");
    }

    #[test]
    fn test_cognitive_system_config_defaults() {
        let config = Config::default();
        assert!(config.cognitive_system.enabled);
        assert_eq!(config.cognitive_system.bootstrap_mode, "silent");
        assert_eq!(config.cognitive_system.hot_cache_max_entries, 20);
        assert!((config.cognitive_system.context_allocation_pct - 0.10).abs() < f32::EPSILON);
        assert!(config.cognitive_system.dream_triggers.nap_on_session_end);
        assert_eq!(
            config
                .cognitive_system
                .dream_triggers
                .dream_memory_threshold,
            20
        );
    }

    #[test]
    #[serial]
    fn test_cognitive_system_config_from_env() {
        std::env::set_var("NEXUS_COGNITIVE_ENABLED", "false");
        std::env::set_var("NEXUS_BOOTSTRAP_MODE", "chatty");
        std::env::set_var("NEXUS_HOT_CACHE_MAX", "50");
        std::env::set_var("NEXUS_DREAM_THRESHOLD", "10");

        let config = Config::from_env().expect("config from env");
        assert!(!config.cognitive_system.enabled);
        assert_eq!(config.cognitive_system.bootstrap_mode, "chatty");
        assert_eq!(config.cognitive_system.hot_cache_max_entries, 50);
        assert_eq!(
            config
                .cognitive_system
                .dream_triggers
                .dream_memory_threshold,
            10
        );

        std::env::remove_var("NEXUS_COGNITIVE_ENABLED");
        std::env::remove_var("NEXUS_BOOTSTRAP_MODE");
        std::env::remove_var("NEXUS_HOT_CACHE_MAX");
        std::env::remove_var("NEXUS_DREAM_THRESHOLD");
    }

    #[test]
    #[serial]
    fn test_embedding_config_from_env() {
        std::env::set_var("NEXUS_EMBEDDINGS_ENABLED", "true");
        std::env::set_var("NEXUS_EMBEDDING_BACKEND", "openai-compatible");
        std::env::set_var("NEXUS_EMBEDDING_PROVIDER", "inherit");
        std::env::set_var("NEXUS_EMBEDDING_MODEL", "text-embedding-004");
        std::env::set_var("NEXUS_EMBEDDING_API_KEY_ENV", "GEMINI_API_KEY");
        std::env::set_var(
            "NEXUS_EMBEDDING_BASE_URL",
            "https://generativelanguage.googleapis.com/v1beta/openai",
        );
        std::env::set_var("NEXUS_EMBEDDING_TIMEOUT_SECS", "45");

        let config = Config::from_env().expect("config from env");
        assert!(config.embedding.enabled);
        assert_eq!(config.embedding.backend, "openai-compatible");
        assert_eq!(config.embedding.provider, "inherit");
        assert_eq!(config.embedding.model, "text-embedding-004");
        assert_eq!(config.embedding.api_key_env, "GEMINI_API_KEY");
        assert_eq!(
            config.embedding.base_url.as_deref(),
            Some("https://generativelanguage.googleapis.com/v1beta/openai")
        );
        assert_eq!(config.embedding.timeout_secs, 45);

        std::env::remove_var("NEXUS_EMBEDDINGS_ENABLED");
        std::env::remove_var("NEXUS_EMBEDDING_BACKEND");
        std::env::remove_var("NEXUS_EMBEDDING_PROVIDER");
        std::env::remove_var("NEXUS_EMBEDDING_MODEL");
        std::env::remove_var("NEXUS_EMBEDDING_API_KEY_ENV");
        std::env::remove_var("NEXUS_EMBEDDING_BASE_URL");
        std::env::remove_var("NEXUS_EMBEDDING_TIMEOUT_SECS");
    }
}
