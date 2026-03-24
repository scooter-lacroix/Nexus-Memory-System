Always-On Memory Agent Integration Plan for Nexus Memory System

Maestro Track: nexus-always-on-agent

Version: 1.0
Status: Ready for maestro:implement
Estimated Scope: L (1–2 days across 6 sequential tracks)
Source Reference: GoogleCloudPlatform/generative-ai/gemini/agents/always-on-memory-agent

---

## MCP Usage Directives

### Required MCP Servers

| Server | Purpose | Required Tools |
|--------|---------|----------------|
| **LeIndex** | Code exploration, search, and editing | `leindex_read_file`, `leindex_edit_apply`, `leindex_project_map`, `leindex_text_search`, `leindex_grep_symbols` |
| **Sequential Thinking** | Task planning and complex problem decomposition | `sequentialthinking` |

### LeIndex Tool Usage Rules

**MANDATORY**: Use LeIndex tools instead of standard file operations:

| Instead Of | Use LeIndex |
|------------|-------------|
| `read_file` | `leindex_read_file` |
| `search_file_content` / `grep` | `leindex_text_search` |
| `glob` / `list_directory` | `leindex_project_map` |
| `replace` / `write_file` | `leindex_edit_apply` |
| Finding symbols | `leindex_grep_symbols` |

**Workflow**:
1. **Explore**: Use `leindex_project_map` to understand file structure
2. **Read**: Use `leindex_read_file` to examine existing code
3. **Search**: Use `leindex_text_search` or `leindex_grep_symbols` to find specific code
4. **Edit**: Use `leindex_edit_apply` for all file modifications
5. **Verify**: Use `leindex_read_file` to confirm changes

### Sequential Thinking Usage

**MANDATORY**: Use `sequentialthinking` for:
- Pre-implementation planning before each track
- Complex refactoring decisions
- Debugging compilation errors
- Integration planning between components

**Pattern**:
```
1. Call sequentialthinking to plan approach
2. Execute planned edits using LeIndex tools
3. Verify results
4. Update task list status
```

---

Table of Contents

Executive Summary
Architecture Overview
Track 1: Core Config & Contracts
Track 2: nexus-llm Crate
Track 3: Storage Extensions
Track 4: nexus-agent Crate
Track 5: Serve/Web Integration
Track 6: Tests & Verification
Additional: Multi-Provider API Key Compatibility
Additional: Targeted Documentation Updates

---

1. Executive Summary

What We're Building

The Google always-on-memory-agent is a Python/ADK system with three LLM-driven sub-agents:

Agent
Function
IngestAgent
Accepts raw text/media → LLM extracts summary, entities, topics, importance → stores structured memory
ConsolidateAgent
Runs on timer → reads unconsolidated memories → LLM finds connections/patterns → stores insights + relations
QueryAgent
Accepts questions → reads all memories + consolidation history → LLM synthesizes answer with citations

Plus: file watcher (inbox folder), HTTP API, Streamlit dashboard, SQLite persistence.

How We're Integrating It

We are NOT porting the Python code 1:1. We are building the equivalent functionality as native Rust services within the Nexus workspace, reusing existing infrastructure:

Existing memories table → stores both raw and insight memories (no separate consolidations table needed)
Existing memory_relations table → stores connections discovered during consolidation
Existing Memory.metadata → stores LLM extraction results (summary, entities, topics, importance)
Existing Memory.labels → stores topics/entities as searchable labels
Existing event bus → publishes MemoryStored, MemoryUpdated events
Existing web routes → augmented with 4 new agent-specific endpoints
Existing nexus serve → spawns the agent supervisor as an optional in-process component

Two new crates are created:
nexus-llm: Multi-provider LLM abstraction (OpenAI, Anthropic, Gemini, OpenRouter, Groq, Z.ai, Minimax, Mistral)
nexus-agent: Always-on memory agent services (ingest, consolidate, query, inbox scanner, supervisor)

Why This Design

Decision
Rationale
New nexus-agent crate, not in orchestrator
Orchestrator is generic runtime plumbing (sessions, events, sync). Agent is domain logic + LLM orchestration. Separation prevents bloat.
New nexus-llm crate, not in core
Provider HTTP clients and fast-changing API details don't belong in foundational types. Isolation enables independent iteration.
No consolidations table
A consolidation output IS a memory with memory_lane_type = Insight + metadata. The existing memory_relations table handles connections. Adding a table duplicates what already exists.
No agent_memory_config table
Config belongs in Config struct first. DB-backed runtime config is premature until UI-driven editing is needed.
Store enrichment in metadata JSON
The Memory struct already has a metadata: serde_json::Value field. Storing {summary, entities, topics, importance_score, source, generated_by} there avoids schema changes.

---

2. Architecture Overview

┌─────────────────────────────────────────────────────────────────────┐
│                         External Surfaces                           │
├─────────────────────────────────────────────────────────────────────┤
│  nexus-cli  │  nexus-hooks  │  nexus-mcp  │  nexus-web            │
│             │               │             │  + /api/agent/*        │
└─────────────────────────────────────────────────────────────────────┘
                 \        |        |        /
                  \       |        |       /
                   └──────┴────────┴──────┘
                              |
              ┌───────────────┼───────────────┐
              v               v               v
     ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
     │ nexus-agent  │ │ nexus-core   │ │  nexus-llm   │
     │ IngestSvc    │ │ types+config │ │ LlmClient    │
     │ ConsolidSvc  │ │ +AgentConfig │ │ OpenAI compat│
     │ QuerySvc     │ │ +LlmConfig   │ │ Anthropic    │
     │ InboxScanner │ └──────┬───────┘ │ Gemini       │
     │ Supervisor   │        |         └──────────────┘
     └──────┬───────┘        |
            |    ┌───────────┼───────────────────┐
            v    v           v                   v
   ┌────────────────┐ ┌────────────────┐ ┌────────────────────┐
   │ nexus-storage  │ │ nexus-vectors  │ │ nexus-embeddings   │
   │ +ProcessedFile │ │ vector search  │ │ embedding pipeline │
   │  Repository    │ │                │ │                    │
   └────────────────┘ └────────────────┘ └────────────────────┘
            \                |                     /
             └───────────────┴────────────────────┘
                              |
                     ┌────────────────────┐
                     │ nexus-orchestrator │
                     │ context + sync     │
                     │ + event publishing │
                     └────────────────────┘

Data Flow: Ingest

User/File → IngestService
  1. Receive raw text + source metadata
  2. Call LlmClient::generate_json() with ingest prompt
  3. LLM returns: {summary, entities, topics, importance}
  4. Build Memory {
       content: raw_text,
       category: General (or LLM-suggested),
       labels: entities + topics,
       metadata: {
         "agent": {
           "summary": "...",
           "entities": ["..."],
           "topics": ["..."],
           "importance_score": 0.8,
           "source": "inbox/notes.txt",
           "generated_by": "ingest_agent"
         }
       }
     }
  5. Store via MemoryRepository
  6. Publish EventType::MemoryStored

Data Flow: Consolidate

Timer tick → ConsolidateService
  1. Query unconsolidated memories (metadata.agent.consolidated != true, limit 10)
  2. If < 2, skip
  3. Call LlmClient::generate_json() with consolidation prompt + memory summaries
  4. LLM returns: {summary, insight, connections: [{from_id, to_id, relationship}]}
  5. Store insight as new Memory {
       category: Context,
       memory_lane_type: Some(Cognitive(Semantic)),
       labels: ["consolidation", "insight"],
       metadata: {
         "agent": {
           "summary": insight_summary,
           "generated_by": "consolidate_agent",
           "source_memory_ids": [12, 44, 90],
           "run_at": "2026-03-24T..."
         }
       }
     }
  6. For each connection: INSERT INTO memory_relations
  7. Mark source memories as consolidated: UPDATE metadata SET agent.consolidated = true
  8. Publish EventType::MemoryStored for insight

Data Flow: Query

User question → QueryService
  1. Search memories (text search via LIKE, or semantic search if embeddings enabled)
  2. Load 1-hop relations for top results
  3. Load recent consolidation insights (WHERE metadata LIKE '%consolidate_agent%')
  4. Build context window from candidate memories
  5. Call LlmClient::generate_text() with query prompt + context
  6. Return answer with citations [Memory #id]

---

3. Track 1: Core Config & Contracts

Size: S (<1h)
Crate: nexus-core
Files modified: crates/nexus-core/src/config.rs, crates/nexus-core/src/error.rs
Depends on: Nothing

3.1 Why

The always-on agent needs configuration for: which LLM provider to use, API keys, inbox directory, scan intervals, consolidation intervals, and whether the agent is enabled. These must live in nexus-core::config so all crates can read them.

3.2 What to Add to config.rs

Add these two new config structs and wire them into the main Config:

// ─── Add to crates/nexus-core/src/config.rs ───

/// LLM provider configuration for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// LLM provider name
    /// Valid values: "openai", "anthropic", "gemini", "openrouter", "groq", "zai", "minimax", "mistral"
    pub provider: String,

    /// Model name (e.g., "gpt-4o-mini", "claude-sonnet-4-20250514", "gemini-3-flash-preview")
    pub model: String,

    /// API key (read from env var specified here, e.g., "OPENAI_API_KEY")
    /// The value is the ENV VAR NAME, not the key itself.
    pub api_key_env: String,

    /// Base URL override (optional; if empty, uses provider default)
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

Then modify the main Config struct — add two new fields:

// ─── Modify existing Config struct in config.rs ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub embedding: EmbeddingConfig,
    pub sync: SyncConfig,
    // NEW:
    pub llm: LlmConfig,
    pub agent: AgentConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database: DatabaseConfig::default(),
            server: ServerConfig::default(),
            embedding: EmbeddingConfig::default(),
            sync: SyncConfig::default(),
            // NEW:
            llm: LlmConfig::default(),
            agent: AgentConfig::default(),
        }
    }
}

Add env var loading in Config::from_env():

// ─── Add to Config::from_env() in config.rs ───

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
if let Ok(interval) = std::env::var("NEXUS_AGENT_CONSOLIDATION_INTERVAL") {
    config.agent.consolidation_interval_mins = interval.parse().unwrap_or(30);
}

3.3 What to Add to error.rs

Add one new error variant for LLM operations:

// ─── Add variant to NexusError enum in error.rs ───

#[error("LLM error: {0}")]
Llm(String),

#[error("Agent error: {0}")]
Agent(String),

3.4 Verification

cd crates/nexus-core && cargo check
cargo test -p nexus-memory-core

---

4. Track 2: nexus-llm Crate

Size: M (1–3h)
New crate: crates/nexus-llm
Depends on: Track 1

4.1 Why

The always-on agent needs to call LLMs to: (1) extract structured info from raw text during ingestion, (2) find patterns during consolidation, (3) synthesize answers during query. We need a provider-agnostic abstraction that works with 8+ providers.

4.2 Provider Analysis

After thorough research, the 8 required providers break down into THREE protocol families:

Protocol
Providers
Base URL
Auth Header
Endpoint
OpenAI-compatible
OpenAI, OpenRouter, Groq, Mistral, Minimax (v1 compat)
varies
Authorization: Bearer <key>
POST /v1/chat/completions
Anthropic-compatible
Anthropic, Z.ai
varies
x-api-key: <key> + anthropic-version: 2023-06-01
POST /v1/messages
Gemini OpenAI-compat
Google Gemini
https://generativelanguage.googleapis.com/v1beta/openai/
Authorization: Bearer <key>
POST /chat/completions (path already in base_url)

Verified provider configurations:

┌────────────┬──────────────────────────────────────────────────────────┬──────────────────────────┬──────────────────┐
│ Provider   │ Base URL                                                 │ Auth Header              │ Protocol         │
├────────────┼──────────────────────────────────────────────────────────┼──────────────────────────┼──────────────────┤
│ OpenAI     │ https://api.openai.com/v1                                │ Authorization: Bearer    │ OpenAI           │
│ OpenRouter │ https://openrouter.ai/api/v1                             │ Authorization: Bearer    │ OpenAI           │
│ Groq       │ https://api.groq.com/openai/v1                           │ Authorization: Bearer    │ OpenAI           │
│ Mistral    │ https://api.mistral.ai/v1                                │ Authorization: Bearer    │ OpenAI           │
│ Minimax    │ https://api.minimax.io/v1                                │ Authorization: Bearer    │ OpenAI (v1 chat) │
│ Gemini     │ https://generativelanguage.googleapis.com/v1beta/openai/ │ Authorization: Bearer    │ OpenAI           │
│ Anthropic  │ https://api.anthropic.com                                │ x-api-key: <key>         │ Anthropic        │
│ Z.ai       │ https://api.z.ai/api/anthropic                           │ x-api-key: <key>         │ Anthropic        │
└────────────┴──────────────────────────────────────────────────────────┴──────────────────────────┴──────────────────┘

Key insight: Z.ai exposes an Anthropic-compatible proxy at https://api.z.ai/api/anthropic, so it uses the exact same protocol as Anthropic (same headers, same request/response schema). Gemini exposes an OpenAI-compatible endpoint. Minimax has an OpenAI-compatible /v1/chat/completions endpoint. This means we only need TWO client implementations:

OpenAiCompatibleClient — handles OpenAI, OpenRouter, Groq, Mistral, Minimax, Gemini
AnthropicCompatibleClient — handles Anthropic, Z.ai

4.3 Crate Structure

crates/nexus-llm/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── client.rs          # LlmClient trait
    ├── factory.rs         # create_client() from config
    ├── openai.rs          # OpenAI-compatible client
    ├── anthropic.rs       # Anthropic-compatible client
    ├── error.rs           # LLM-specific errors
    ├── provider.rs        # Provider enum + default configs
    └── types.rs           # Request/response types

4.4 Cargo.toml — Copy Verbatim

[package]
name = "nexus-memory-llm"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
description = "LLM provider abstraction for Nexus Memory System"

[dependencies]
nexus-core = { workspace = true }

# HTTP client
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }

# Async
tokio = { workspace = true }
async-trait = { workspace = true }

# Serialization
serde = { workspace = true }
serde_json = { workspace = true }

# Error handling
thiserror = { workspace = true }
anyhow = { workspace = true }

# Logging
tracing = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util"] }

4.5 src/types.rs — Copy Verbatim

//! Shared types for LLM requests and responses

use serde::{Deserialize, Serialize};

/// A chat message for LLM interaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".to_string(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".to_string(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".to_string(), content: content.into() }
    }
}

/// Parameters for LLM generation
#[derive(Debug, Clone)]
pub struct GenerateParams {
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// If true, instruct the LLM to return valid JSON
    pub json_mode: bool,
}

impl Default for GenerateParams {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            max_tokens: 4096,
            temperature: 0.3,
            json_mode: false,
        }
    }
}

/// Response from LLM generation
#[derive(Debug, Clone)]
pub struct GenerateResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<TokenUsage>,
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

4.6 src/error.rs — Copy Verbatim

//! LLM-specific error types

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Missing API key: env var '{0}' not set")]
    MissingApiKey(String),

    #[error("Unsupported provider: {0}")]
    UnsupportedProvider(String),

    #[error("Response did not contain valid content")]
    EmptyResponse,

    #[error("Invalid JSON response from LLM: {0}")]
    InvalidJsonResponse(String),

    #[error("Timeout after {0}s")]
    Timeout(u64),
}

pub type Result<T> = std::result::Result<T, LlmError>;

4.7 src/provider.rs — Copy Verbatim

//! Provider definitions and default configurations

use serde::{Deserialize, Serialize};

/// Supported LLM providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenAi,
    Anthropic,
    Gemini,
    OpenRouter,
    Groq,
    Zai,
    Minimax,
    Mistral,
}

impl Provider {
    /// Parse provider from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" | "open_ai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            "gemini" | "google" => Some(Self::Gemini),
            "openrouter" | "open_router" => Some(Self::OpenRouter),
            "groq" => Some(Self::Groq),
            "zai" | "z.ai" | "zhipu" | "bigmodel" => Some(Self::Zai),
            "minimax" => Some(Self::Minimax),
            "mistral" => Some(Self::Mistral),
            _ => None,
        }
    }

    /// Get the default base URL for this provider
    pub fn default_base_url(&self) -> &'static str {
        match self {
            Provider::OpenAi => "https://api.openai.com/v1",
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
            Provider::Groq => "https://api.groq.com/openai/v1",
            Provider::Zai => "https://api.z.ai/api/anthropic",
            Provider::Minimax => "https://api.minimax.io/v1",
            Provider::Mistral => "https://api.mistral.ai/v1",
        }
    }

    /// Get the default API key environment variable name for this provider
    pub fn default_api_key_env(&self) -> &'static str {
        match self {
            Provider::OpenAi => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::Gemini => "GEMINI_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
            Provider::Groq => "GROQ_API_KEY",
            Provider::Zai => "ZAI_API_KEY",
            Provider::Minimax => "MINIMAX_API_KEY",
            Provider::Mistral => "MISTRAL_API_KEY",
        }
    }

    /// Get the default model for this provider
    pub fn default_model(&self) -> &'static str {
        match self {
            Provider::OpenAi => "gpt-4o-mini",
            Provider::Anthropic => "claude-sonnet-4-20250514",
            Provider::Gemini => "gemini-3-flash-preview",
            Provider::OpenRouter => "openai/gpt-4o-mini",
            Provider::Groq => "llama-3.3-70b-versatile",
            Provider::Zai => "glm-4.7",
            Provider::Minimax => "MiniMax-M1-80k",
            Provider::Mistral => "mistral-small-latest",
        }
    }

    /// Whether this provider uses the Anthropic protocol
    pub fn is_anthropic_protocol(&self) -> bool {
        matches!(self, Provider::Anthropic | Provider::Zai)
    }

    /// Whether this provider uses the OpenAI-compatible protocol
    pub fn is_openai_protocol(&self) -> bool {
        !self.is_anthropic_protocol()
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::OpenAi => write!(f, "openai"),
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::Gemini => write!(f, "gemini"),
            Provider::OpenRouter => write!(f, "openrouter"),
            Provider::Groq => write!(f, "groq"),
            Provider::Zai => write!(f, "zai"),
            Provider::Minimax => write!(f, "minimax"),
            Provider::Mistral => write!(f, "mistral"),
        }
    }
}

4.8 src/client.rs — Copy Verbatim

//! Core LLM client trait

use async_trait::async_trait;
use crate::error::Result;
use crate::types::{GenerateParams, GenerateResponse};

/// Trait for LLM client implementations
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Generate text from a prompt
    async fn generate(&self, params: GenerateParams) -> Result<GenerateResponse>;

    /// Generate and parse JSON response
    /// Sends the request with json_mode=true and attempts to parse the response
    async fn generate_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        params: GenerateParams,
    ) -> Result<T> {
        let mut params = params;
        params.json_mode = true;
        let response = self.generate(params).await?;

        // Try to extract JSON from the response (handle markdown code blocks)
        let content = response.content.trim();
        let json_str = if content.starts_with("```") {
            // Strip markdown code block
            let start = content.find('\n').unwrap_or(3) + 1;
            let end = content.rfind("```").unwrap_or(content.len());
            &content[start..end]
        } else {
            content
        };

        serde_json::from_str(json_str.trim())
            .map_err(|e| crate::error::LlmError::InvalidJsonResponse(
                format!("Failed to parse: {}. Raw: {}", e, &json_str[..json_str.len().min(200)])
            ))
    }

    /// Get the provider name
    fn provider_name(&self) -> &str;

    /// Get the model name
    fn model_name(&self) -> &str;
}

4.9 src/openai.rs — Copy Verbatim

This handles: OpenAI, OpenRouter, Groq, Mistral, Minimax, Gemini.

//! OpenAI-compatible LLM client
//!
//! Works with: OpenAI, OpenRouter, Groq, Mistral, Minimax, Gemini (OpenAI compat endpoint)

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

use crate::client::LlmClient;
use crate::error::{LlmError, Result};
use crate::provider::Provider;
use crate::types::{ChatMessage, GenerateParams, GenerateResponse, TokenUsage};

/// OpenAI-compatible chat completion request
#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

/// OpenAI-compatible chat completion response
#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    model: Option<String>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

/// OpenAI API error response
#[derive(Debug, Deserialize)]
struct OpenAiErrorResponse {
    error: Option<OpenAiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct OpenAiErrorDetail {
    message: Option<String>,
}

pub struct OpenAiCompatibleClient {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    provider: Provider,
    max_tokens: u32,
    temperature: f32,
}

impl OpenAiCompatibleClient {
    pub fn new(
        provider: Provider,
        base_url: String,
        api_key: String,
        model: String,
        timeout_secs: u64,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(LlmError::Http)?;

        Ok(Self {
            client,
            base_url,
            api_key,
            model,
            provider,
            max_tokens,
            temperature,
        })
    }

    fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/chat/completions", base)
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn generate(&self, params: GenerateParams) -> Result<GenerateResponse> {
        let messages: Vec<OpenAiMessage> = params.messages.iter().map(|m| OpenAiMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        }).collect();

        let response_format = if params.json_mode {
            Some(ResponseFormat { format_type: "json_object".to_string() })
        } else {
            None
        };

        let request = OpenAiRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(if params.max_tokens > 0 { params.max_tokens } else { self.max_tokens }),
            temperature: Some(params.temperature.max(0.01)),  // Some providers reject 0.0
            response_format,
        };

        let url = self.chat_completions_url();
        debug!(provider = %self.provider, url = %url, model = %self.model, "Sending OpenAI-compatible request");

        let resp = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(LlmError::Http)?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().await.unwrap_or_default();
            let message = serde_json::from_str::<OpenAiErrorResponse>(&body)
                .ok()
                .and_then(|e| e.error)
                .and_then(|e| e.message)
                .unwrap_or(body);
            return Err(LlmError::Api { status, message });
        }

        let body: OpenAiResponse = resp.json().await.map_err(LlmError::Http)?;
        let content = body.choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or(LlmError::EmptyResponse)?;

        let usage = body.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens.unwrap_or(0),
            completion_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        });

        Ok(GenerateResponse {
            content,
            model: body.model.unwrap_or_else(|| self.model.clone()),
            usage,
        })
    }

    fn provider_name(&self) -> &str {
        match self.provider {
            Provider::OpenAi => "openai",
            Provider::OpenRouter => "openrouter",
            Provider::Groq => "groq",
            Provider::Mistral => "mistral",
            Provider::Minimax => "minimax",
            Provider::Gemini => "gemini",
            _ => "openai-compatible",
        }
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

4.10 src/anthropic.rs — Copy Verbatim

This handles: Anthropic, Z.ai.

//! Anthropic-compatible LLM client
//!
//! Works with: Anthropic (api.anthropic.com), Z.ai (api.z.ai/api/anthropic)

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use crate::client::LlmClient;
use crate::error::{LlmError, Result};
use crate::provider::Provider;
use crate::types::{GenerateParams, GenerateResponse, TokenUsage};

/// Anthropic Messages API request
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

/// Anthropic Messages API response
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    model: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

/// Anthropic API error response
#[derive(Debug, Deserialize)]
struct AnthropicErrorResponse {
    error: Option<AnthropicErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    message: Option<String>,
}

pub struct AnthropicCompatibleClient {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    provider: Provider,
    max_tokens: u32,
    temperature: f32,
}

impl AnthropicCompatibleClient {
    pub fn new(
        provider: Provider,
        base_url: String,
        api_key: String,
        model: String,
        timeout_secs: u64,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(LlmError::Http)?;

        Ok(Self {
            client,
            base_url,
            api_key,
            model,
            provider,
            max_tokens,
            temperature,
        })
    }

    fn messages_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/v1/messages", base)
    }
}

#[async_trait]
impl LlmClient for AnthropicCompatibleClient {
    async fn generate(&self, params: GenerateParams) -> Result<GenerateResponse> {
        // Separate system message from user/assistant messages
        let mut system_msg: Option<String> = None;
        let mut messages: Vec<AnthropicMessage> = Vec::new();

        for msg in &params.messages {
            if msg.role == "system" {
                // Anthropic uses a top-level "system" field, not a system message in the array
                system_msg = Some(msg.content.clone());
            } else {
                messages.push(AnthropicMessage {
                    role: msg.role.clone(),
                    content: msg.content.clone(),
                });
            }
        }

        // If json_mode, append instruction to the system prompt
        let system_msg = if params.json_mode {
            let base = system_msg.unwrap_or_default();
            Some(format!(
                "{}\n\nIMPORTANT: You MUST respond with valid JSON only. No markdown, no explanation, just the JSON object.",
                base
            ))
        } else {
            system_msg
        };

        let request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: if params.max_tokens > 0 { params.max_tokens } else { self.max_tokens },
            messages,
            system: system_msg,
            temperature: Some(params.temperature.max(0.01)),
        };

        let url = self.messages_url();
        debug!(provider = %self.provider, url = %url, model = %self.model, "Sending Anthropic-compatible request");

        let resp = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request)
            .send()
            .await
            .map_err(LlmError::Http)?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().await.unwrap_or_default();
            let message = serde_json::from_str::<AnthropicErrorResponse>(&body)
                .ok()
                .and_then(|e| e.error)
                .and_then(|e| e.message)
                .unwrap_or(body);
            return Err(LlmError::Api { status, message });
        }

        let body: AnthropicResponse = resp.json().await.map_err(LlmError::Http)?;
        let content = body.content
            .iter()
            .filter(|c| c.content_type == "text")
            .filter_map(|c| c.text.as_ref())
            .collect::<Vec<_>>()
            .join("");

        if content.is_empty() {
            return Err(LlmError::EmptyResponse);
        }

        let usage = body.usage.map(|u| TokenUsage {
            prompt_tokens: u.input_tokens.unwrap_or(0),
            completion_tokens: u.output_tokens.unwrap_or(0),
            total_tokens: u.input_tokens.unwrap_or(0) + u.output_tokens.unwrap_or(0),
        });

        Ok(GenerateResponse {
            content,
            model: body.model.unwrap_or_else(|| self.model.clone()),
            usage,
        })
    }

    fn provider_name(&self) -> &str {
        match self.provider {
            Provider::Anthropic => "anthropic",
            Provider::Zai => "zai",
            _ => "anthropic-compatible",
        }
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

4.11 src/factory.rs — Copy Verbatim

//! Factory for creating LLM clients from configuration

use crate::anthropic::AnthropicCompatibleClient;
use crate::client::LlmClient;
use crate::error::{LlmError, Result};
use crate::openai::OpenAiCompatibleClient;
use crate::provider::Provider;
use nexus_core::config::LlmConfig;
use tracing::info;

/// Create an LLM client from configuration
pub fn create_client(config: &LlmConfig) -> Result<Box<dyn LlmClient>> {
    let provider = Provider::from_str(&config.provider)
        .ok_or_else(|| LlmError::UnsupportedProvider(config.provider.clone()))?;

    // Resolve API key from environment
    let api_key = std::env::var(&config.api_key_env)
        .map_err(|_| LlmError::MissingApiKey(config.api_key_env.clone()))?;

    // Resolve base URL (user override or provider default)
    let base_url = config.base_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| provider.default_base_url())
        .to_string();

    // Resolve model (use config model, fall back to provider default)
    let model = if config.model.is_empty() {
        provider.default_model().to_string()
    } else {
        config.model.clone()
    };

    info!(
        provider = %provider,
        model = %model,
        base_url = %base_url,
        "Creating LLM client"
    );

    if provider.is_anthropic_protocol() {
        let client = AnthropicCompatibleClient::new(
            provider,
            base_url,
            api_key,
            model,
            config.timeout_secs,
            config.max_tokens,
            config.temperature,
        )?;
        Ok(Box::new(client))
    } else {
        let client = OpenAiCompatibleClient::new(
            provider,
            base_url,
            api_key,
            model,
            config.timeout_secs,
            config.max_tokens,
            config.temperature,
        )?;
        Ok(Box::new(client))
    }
}

4.12 src/lib.rs — Copy Verbatim

//! Nexus LLM - Multi-provider LLM abstraction for Nexus Memory System
//!
//! Supports: OpenAI, Anthropic, Gemini, OpenRouter, Groq, Z.ai, Minimax, Mistral
//!
//! # Example
//!
//! ```rust,ignore
//! use nexus_llm::{create_client, LlmClient};
//! use nexus_core::config::LlmConfig;
//!
//! let config = LlmConfig::default();
//! let client = create_client(&config)?;
//! let response = client.generate(params).await?;
//! ```

pub mod anthropic;
pub mod client;
pub mod error;
pub mod factory;
pub mod openai;
pub mod provider;
pub mod types;

pub use client::LlmClient;
pub use error::{LlmError, Result};
pub use factory::create_client;
pub use provider::Provider;
pub use types::*;

4.13 Workspace Registration

Add to root Cargo.toml:

# In [workspace] members list, add:
"crates/nexus-llm",

# In [workspace.dependencies], add:
nexus-llm = { package = "nexus-memory-llm", version = "1.1.2", path = "crates/nexus-llm" }

# Also add reqwest:
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }

4.14 Verification

cargo check -p nexus-memory-llm

---

5. Track 3: Storage Extensions

Size: M (1–3h)
Crate: nexus-storage
Files modified: migrations.rs, repository.rs, models.rs, lib.rs
Depends on: Track 1

5.1 Why

We need: (1) a processed_files table for inbox file deduplication, (2) a ProcessedFileRepository for CRUD, (3) a MemoryRelationRepository for consolidation connections, (4) helper queries for unconsolidated memories and related memories.

5.2 Migration: processed_files table

Add to migrations.rs:

// ─── Add new function at end of migrations.rs ───

async fn create_processed_files_table(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS processed_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            content_hash TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            memory_id INTEGER,
            last_error TEXT,
            processed_at DATETIME,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME,
            FOREIGN KEY (namespace_id) REFERENCES agent_namespaces(id),
            FOREIGN KEY (memory_id) REFERENCES memories(id),
            UNIQUE(namespace_id, path)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_processed_files_status ON processed_files(status)")
        .execute(pool)
        .await
        .map_err(db_error)?;

    Ok(())
}

Add call in run_migrations():

// ─── Add at end of run_migrations() ───
create_processed_files_table(pool).await?;

5.3 Model: ProcessedFileRow

Add to models.rs:

// ─── Add to models.rs ───

/// Database row for processed_files table
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProcessedFileRow {
    pub id: i64,
    pub namespace_id: i64,
    pub path: String,
    pub content_hash: Option<String>,
    pub status: String,
    pub memory_id: Option<i64>,
    pub last_error: Option<String>,
    pub processed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

5.4 Repository: ProcessedFileRepository

Add new repository to repository.rs:

// ─── Add to repository.rs ───

use crate::models::ProcessedFileRow;

/// Repository for processed file tracking (inbox deduplication)
pub struct ProcessedFileRepository {
    pool: SqlitePool,
}

impl ProcessedFileRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Check if a file path has already been processed
    pub async fn is_processed(&self, namespace_id: i64, path: &str) -> Result<bool> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM processed_files WHERE namespace_id = ? AND path = ? AND status = 'processed'"
        )
        .bind(namespace_id)
        .bind(path)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(row.is_some())
    }

    /// Mark a file as processed
    pub async fn mark_processed(
        &self,
        namespace_id: i64,
        path: &str,
        content_hash: Option<&str>,
        memory_id: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO processed_files (namespace_id, path, content_hash, status, memory_id, processed_at, updated_at)
            VALUES (?, ?, ?, 'processed', ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT(namespace_id, path) DO UPDATE SET
                content_hash = excluded.content_hash,
                status = 'processed',
                memory_id = excluded.memory_id,
                processed_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP,
                last_error = NULL
            "#,
        )
        .bind(namespace_id)
        .bind(path)
        .bind(content_hash)
        .bind(memory_id)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    /// Mark a file as failed
    pub async fn mark_failed(
        &self,
        namespace_id: i64,
        path: &str,
        error: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO processed_files (namespace_id, path, status, last_error, updated_at)
            VALUES (?, ?, 'failed', ?, CURRENT_TIMESTAMP)
            ON CONFLICT(namespace_id, path) DO UPDATE SET
                status = 'failed',
                last_error = excluded.last_error,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(namespace_id)
        .bind(path)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(())
    }

    /// Clear all processed file records for a namespace
    pub async fn clear_namespace(&self, namespace_id: i64) -> Result<u64> {
        let result = sqlx::query("DELETE FROM processed_files WHERE namespace_id = ?")
            .bind(namespace_id)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;

        Ok(result.rows_affected())
    }
}

5.5 Repository: MemoryRelationRepository

Add to repository.rs:

// ─── Add to repository.rs ───

use crate::models::MemoryRelationRow;

/// Repository for memory relation operations
pub struct MemoryRelationRepository {
    pool: SqlitePool,
}

impl MemoryRelationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store a new relation between memories
    pub async fn store(
        &self,
        source_memory_id: i64,
        target_memory_id: i64,
        relation_type: &str,
        strength: f32,
        metadata: Option<&serde_json::Value>,
    ) -> Result<i64> {
        let metadata_json = metadata.map(|m| serde_json::to_string(m)).transpose()?;

        let result = sqlx::query(
            r#"
            INSERT INTO memory_relations (source_memory_id, target_memory_id, relation_type, strength, metadata, created_at)
            VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
            "#,
        )
        .bind(source_memory_id)
        .bind(target_memory_id)
        .bind(relation_type)
        .bind(strength)
        .bind(&metadata_json)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(result.last_insert_rowid())
    }

    /// Get all relations for a memory (both directions)
    pub async fn get_related(&self, memory_id: i64) -> Result<Vec<MemoryRelationRow>> {
        let rows: Vec<MemoryRelationRow> = sqlx::query_as(
            "SELECT * FROM memory_relations WHERE source_memory_id = ? OR target_memory_id = ? ORDER BY strength DESC"
        )
        .bind(memory_id)
        .bind(memory_id)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(rows)
    }
}

5.6 Helper queries on MemoryRepository

Add new methods to the existing MemoryRepository:

// ─── Add methods to MemoryRepository impl block in repository.rs ───

/// Get unconsolidated memories (those without agent.consolidated in metadata)
/// Used by the consolidation service
pub async fn get_unconsolidated(
    &self,
    namespace_id: i64,
    limit: usize,
) -> Result<Vec<Memory>> {
    let rows: Vec<MemoryRow> = sqlx::query_as(
        r#"
        SELECT * FROM memories
        WHERE namespace_id = ? AND is_active = 1
          AND (metadata NOT LIKE '%"consolidated":true%' OR metadata IS NULL)
          AND (metadata NOT LIKE '%"generated_by"%' OR metadata IS NULL)
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(namespace_id)
    .bind(limit as i64)
    .fetch_all(&self.pool)
    .await
    .map_err(db_error)?;

    Ok(rows.into_iter().map(|r| self.row_to_memory(r)).collect())
}

/// Mark a memory as consolidated by updating its metadata
pub async fn mark_consolidated(&self, id: i64) -> Result<()> {
    // Read current metadata, merge consolidated flag
    let row: Option<MemoryRow> = sqlx::query_as("SELECT * FROM memories WHERE id = ?")
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

    if let Some(row) = row {
        let mut metadata: serde_json::Value =
            serde_json::from_str(&row.metadata).unwrap_or(serde_json::json!({}));

        if let Some(obj) = metadata.as_object_mut() {
            let agent = obj.entry("agent").or_insert(serde_json::json!({}));
            if let Some(agent_obj) = agent.as_object_mut() {
                agent_obj.insert("consolidated".to_string(), serde_json::json!(true));
            }
        }

        let metadata_str = serde_json::to_string(&metadata)?;
        sqlx::query("UPDATE memories SET metadata = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(&metadata_str)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
    }

    Ok(())
}

/// Search memories by content text (LIKE search)
pub async fn search_by_text(
    &self,
    namespace_id: i64,
    query: &str,
    limit: usize,
) -> Result<Vec<Memory>> {
    let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
    let rows: Vec<MemoryRow> = sqlx::query_as(
        r#"
        SELECT * FROM memories
        WHERE namespace_id = ? AND is_active = 1
          AND (content LIKE ? OR metadata LIKE ?)
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(namespace_id)
    .bind(&pattern)
    .bind(&pattern)
    .bind(limit as i64)
    .fetch_all(&self.pool)
    .await
    .map_err(db_error)?;

    Ok(rows.into_iter().map(|r| self.row_to_memory(r)).collect())
}

5.7 Export new repositories

Update lib.rs:

// ─── Modify pub use in nexus-storage/src/lib.rs ───
pub use repository::{MemoryRepository, MemoryRelationRepository, NamespaceRepository, ProcessedFileRepository};

5.8 Verification

cargo check -p nexus-memory-storage
cargo test -p nexus-memory-storage

---

6. Track 4: nexus-agent Crate

Size: L (1–2 days)
New crate: crates/nexus-agent
Depends on: Tracks 1, 2, 3

6.1 Crate Structure

crates/nexus-agent/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── supervisor.rs      # AgentSupervisor — spawns/manages loops
    ├── ingest.rs          # IngestService — LLM-driven memory enrichment
    ├── consolidate.rs     # ConsolidateService — periodic pattern finding
    ├── query.rs           # QueryService — LLM-driven query answering
    ├── inbox.rs           # InboxScanner — file watcher polling loop
    ├── prompts.rs         # LLM prompt templates
    └── types.rs           # Agent-specific types (IngestResult, etc.)

6.2 Cargo.toml — Copy Verbatim

[package]
name = "nexus-memory-agent"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
homepage.workspace = true
description = "Always-on memory agent for Nexus Memory System"

[dependencies]
nexus-core = { workspace = true }
nexus-storage = { workspace = true }
nexus-llm = { workspace = true }

# Async
tokio = { workspace = true }
async-trait = { workspace = true }

# Serialization
serde = { workspace = true }
serde_json = { workspace = true }

# Error handling
thiserror = { workspace = true }
anyhow = { workspace = true }

# Logging
tracing = { workspace = true }

# Utilities
chrono = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util"] }
tempfile = { workspace = true }

6.3 src/types.rs — Copy Verbatim

//! Agent-specific types

use serde::{Deserialize, Serialize};

/// Result of LLM-driven ingestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestExtraction {
    pub summary: String,
    pub entities: Vec<String>,
    pub topics: Vec<String>,
    pub importance: f32,
    #[serde(default)]
    pub suggested_category: Option<String>,
}

/// Result of LLM-driven consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    pub summary: String,
    pub insight: String,
    pub connections: Vec<ConsolidationConnection>,
}

/// A connection between two memories discovered during consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationConnection {
    pub from_id: i64,
    pub to_id: i64,
    pub relationship: String,
}

/// Result of LLM-driven query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAnswer {
    pub answer: String,
    pub cited_memory_ids: Vec<i64>,
    pub confidence: f32,
}

/// Status of the agent supervisor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub enabled: bool,
    pub inbox_dir: String,
    pub total_ingested: u64,
    pub total_consolidated: u64,
    pub total_queries: u64,
    pub last_scan_at: Option<String>,
    pub last_consolidation_at: Option<String>,
    pub uptime_secs: u64,
}

6.4 src/prompts.rs — Copy Verbatim

These are the LLM prompt templates, adapted from the original Python agent's instructions.

//! LLM prompt templates for agent operations

/// System prompt for the ingest agent
pub const INGEST_SYSTEM_PROMPT: &str = r#"You are a Memory Ingest Agent. When given raw text, you extract structured information for storage.

For any input you receive:
1. Create a concise 1-2 sentence summary
2. Extract key entities (people, companies, products, concepts)
3. Assign 2-4 topic tags
4. Rate importance from 0.0 to 1.0

You MUST respond with valid JSON in exactly this format:
{
  "summary": "A concise 1-2 sentence summary",
  "entities": ["entity1", "entity2"],
  "topics": ["topic1", "topic2"],
  "importance": 0.7,
  "suggested_category": "general"
}

The suggested_category MUST be one of: general, facts, preferences, context, specifications, session.

Be concise and accurate. Extract the most important information."#;

/// Build user prompt for ingestion
pub fn ingest_user_prompt(text: &str, source: &str) -> String {
    let source_info = if source.is_empty() {
        String::new()
    } else {
        format!(" (source: {})", source)
    };
    format!(
        "Extract structured information from this text{}:\n\n{}",
        source_info,
        &text[..text.len().min(10000)]
    )
}

/// System prompt for the consolidation agent
pub const CONSOLIDATE_SYSTEM_PROMPT: &str = r#"You are a Memory Consolidation Agent. You find patterns and connections across memories.

Given a set of memories, you:
1. Find connections and patterns across them
2. Create a synthesized summary
3. Generate one key insight
4. Identify specific connections between memory pairs

You MUST respond with valid JSON in exactly this format:
{
  "summary": "A synthesized summary across all source memories",
  "insight": "One key pattern or insight discovered",
  "connections": [
    {"from_id": 1, "to_id": 2, "relationship": "description of relationship"}
  ]
}

Think deeply about cross-cutting patterns. Be concise but insightful."#;

/// Build user prompt for consolidation
pub fn consolidate_user_prompt(memories: &[(i64, String)]) -> String {
    let mut prompt = String::from("Find connections and patterns across these memories:\n\n");
    for (id, summary) in memories {
        prompt.push_str(&format!("Memory #{}: {}\n", id, summary));
    }
    prompt
}

/// System prompt for the query agent
pub const QUERY_SYSTEM_PROMPT: &str = r#"You are a Memory Query Agent. You answer questions based on stored memories.

When answering:
1. Synthesize an answer based ONLY on the provided memories
2. Reference memory IDs like [Memory #1], [Memory #2]
3. If no relevant memories exist, say so honestly
4. Be thorough but concise

Always cite your sources using memory IDs."#;

/// Build user prompt for queries
pub fn query_user_prompt(question: &str, memories: &[(i64, String)], insights: &[(i64, String)]) -> String {
    let mut prompt = String::from("Based on the following stored memories, answer this question:\n\n");
    prompt.push_str(&format!("QUESTION: {}\n\n", question));

    if !memories.is_empty() {
        prompt.push_str("MEMORIES:\n");
        for (id, content) in memories {
            prompt.push_str(&format!("Memory #{}: {}\n", id, content));
        }
        prompt.push('\n');
    }

    if !insights.is_empty() {
        prompt.push_str("CONSOLIDATION INSIGHTS:\n");
        for (id, insight) in insights {
            prompt.push_str(&format!("Insight #{}: {}\n", id, insight));
        }
        prompt.push('\n');
    }

    if memories.is_empty() && insights.is_empty() {
        prompt.push_str("No memories are currently stored.\n");
    }

    prompt
}

6.5 src/ingest.rs — Copy Verbatim

//! IngestService — LLM-driven memory enrichment

use nexus_core::MemoryCategory;
use nexus_llm::{ChatMessage, GenerateParams, LlmClient};
use nexus_storage::MemoryRepository;
use serde_json::json;
use tracing::{info, warn};

use crate::prompts;
use crate::types::IngestExtraction;

pub struct IngestService {
    llm: Box<dyn LlmClient>,
    memory_repo: MemoryRepository,
}

impl IngestService {
    pub fn new(llm: Box<dyn LlmClient>, memory_repo: MemoryRepository) -> Self {
        Self { llm, memory_repo }
    }

    /// Ingest raw text: run LLM extraction, then store enriched memory
    pub async fn ingest(
        &self,
        namespace_id: i64,
        text: &str,
        source: &str,
    ) -> anyhow::Result<nexus_core::Memory> {
        // 1. Call LLM to extract structured info
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(prompts::INGEST_SYSTEM_PROMPT),
                ChatMessage::user(prompts::ingest_user_prompt(text, source)),
            ],
            max_tokens: 1024,
            temperature: 0.3,
            json_mode: true,
        };

        let extraction: IngestExtraction = match self.llm.generate_json(params).await {
            Ok(e) => e,
            Err(e) => {
                warn!("LLM extraction failed, storing raw: {}", e);
                // Fallback: store raw without enrichment
                IngestExtraction {
                    summary: text[..text.len().min(200)].to_string(),
                    entities: Vec::new(),
                    topics: Vec::new(),
                    importance: 0.5,
                    suggested_category: None,
                }
            }
        };

        // 2. Determine category
        let category = extraction
            .suggested_category
            .as_deref()
            .and_then(MemoryCategory::from_str)
            .unwrap_or(MemoryCategory::General);

        // 3. Build labels from entities + topics
        let mut labels: Vec<String> = extraction.topics.clone();
        labels.extend(extraction.entities.iter().take(10).cloned());
        labels.dedup();

        // 4. Build metadata with extraction results
        let metadata = json!({
            "agent": {
                "summary": extraction.summary,
                "entities": extraction.entities,
                "topics": extraction.topics,
                "importance_score": extraction.importance,
                "source": source,
                "generated_by": "ingest_agent",
                "consolidated": false
            }
        });

        // 5. Store enriched memory
        let memory = self.memory_repo.store(
            namespace_id,
            text,
            &category,
            None, // memory_lane_type
            &labels,
            &metadata,
            None, // embedding
            None, // embedding_model
        ).await?;

        info!(
            memory_id = memory.id,
            summary = %extraction.summary[..extraction.summary.len().min(60)],
            importance = extraction.importance,
            "Ingested memory"
        );

        Ok(memory)
    }
}

6.6 src/consolidate.rs — Copy Verbatim

//! ConsolidateService — periodic pattern finding across memories

use nexus_core::MemoryCategory;
use nexus_llm::{ChatMessage, GenerateParams, LlmClient};
use nexus_storage::{MemoryRelationRepository, MemoryRepository};
use serde_json::json;
use tracing::{info, warn};

use crate::prompts;
use crate::types::ConsolidationResult;

pub struct ConsolidateService {
    llm: Box<dyn LlmClient>,
    memory_repo: MemoryRepository,
    relation_repo: MemoryRelationRepository,
}

impl ConsolidateService {
    pub fn new(
        llm: Box<dyn LlmClient>,
        memory_repo: MemoryRepository,
        relation_repo: MemoryRelationRepository,
    ) -> Self {
        Self { llm, memory_repo, relation_repo }
    }

    /// Run one consolidation pass
    /// Returns the number of memories consolidated, or 0 if skipped
    pub async fn run_once(&self, namespace_id: i64, batch_size: usize) -> anyhow::Result<usize> {
        // 1. Fetch unconsolidated memories
        let memories = self.memory_repo.get_unconsolidated(namespace_id, batch_size).await?;

        if memories.len() < 2 {
            info!(count = memories.len(), "Skipping consolidation — fewer than 2 unconsolidated memories");
            return Ok(0);
        }

        info!(count = memories.len(), "Running consolidation");

        // 2. Build summaries for LLM context
        let summaries: Vec<(i64, String)> = memories.iter().map(|m| {
            let summary = m.metadata
                .get("agent")
                .and_then(|a| a.get("summary"))
                .and_then(|s| s.as_str())
                .unwrap_or(&m.content[..m.content.len().min(200)]);
            (m.id, summary.to_string())
        }).collect();

        // 3. Call LLM to find patterns
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(prompts::CONSOLIDATE_SYSTEM_PROMPT),
                ChatMessage::user(prompts::consolidate_user_prompt(&summaries)),
            ],
            max_tokens: 2048,
            temperature: 0.5,
            json_mode: true,
        };

        let result: ConsolidationResult = match self.llm.generate_json(params).await {
            Ok(r) => r,
            Err(e) => {
                warn!("Consolidation LLM call failed: {}", e);
                return Ok(0);
            }
        };

        // 4. Store insight as a new memory
        let source_ids: Vec<i64> = memories.iter().map(|m| m.id).collect();
        let insight_metadata = json!({
            "agent": {
                "summary": result.summary,
                "generated_by": "consolidate_agent",
                "source_memory_ids": source_ids,
                "insight": result.insight,
                "run_at": chrono::Utc::now().to_rfc3339()
            }
        });

        let insight_content = format!(
            "Consolidation Insight: {}\n\nSummary: {}",
            result.insight, result.summary
        );

        let insight_memory = self.memory_repo.store(
            namespace_id,
            &insight_content,
            &MemoryCategory::Context,
            None, // Could use MemoryLaneType::Cognitive(Semantic) here
            &["consolidation".to_string(), "insight".to_string()],
            &insight_metadata,
            None,
            None,
        ).await?;

        // 5. Store relations
        for conn in &result.connections {
            // Validate that from_id and to_id exist in our source memories
            if source_ids.contains(&conn.from_id) && source_ids.contains(&conn.to_id) {
                let _ = self.relation_repo.store(
                    conn.from_id,
                    conn.to_id,
                    "related",
                    1.0,
                    Some(&json!({"relationship": conn.relationship})),
                ).await;
            }

            // Also link source memories to the insight
            let _ = self.relation_repo.store(
                insight_memory.id,
                conn.from_id,
                "references",
                0.8,
                None,
            ).await;
        }

        // 6. Mark source memories as consolidated
        for m in &memories {
            let _ = self.memory_repo.mark_consolidated(m.id).await;
        }

        info!(
            insight_id = insight_memory.id,
            source_count = memories.len(),
            connections = result.connections.len(),
            "Consolidation complete"
        );

        Ok(memories.len())
    }
}

6.7 src/query.rs — Copy Verbatim

//! QueryService — LLM-driven query answering with memory citations

use nexus_llm::{ChatMessage, GenerateParams, LlmClient};
use nexus_storage::MemoryRepository;
use tracing::info;

use crate::prompts;
use crate::types::QueryAnswer;

pub struct QueryService {
    llm: Box<dyn LlmClient>,
    memory_repo: MemoryRepository,
}

impl QueryService {
    pub fn new(llm: Box<dyn LlmClient>, memory_repo: MemoryRepository) -> Self {
        Self { llm, memory_repo }
    }

    /// Answer a question using stored memories
    pub async fn query(
        &self,
        namespace_id: i64,
        question: &str,
        context_limit: usize,
    ) -> anyhow::Result<String> {
        // 1. Search for relevant memories (text search)
        let memories = self.memory_repo.search_by_text(
            namespace_id,
            question,
            context_limit,
        ).await?;

        // 2. Also get recent memories if text search returns few results
        let memories = if memories.len() < 5 {
            let mut all = memories;
            let recent = self.memory_repo.search_by_namespace(
                namespace_id,
                context_limit,
                0,
            ).await?;
            for m in recent {
                if !all.iter().any(|existing| existing.id == m.id) {
                    all.push(m);
                }
                if all.len() >= context_limit {
                    break;
                }
            }
            all
        } else {
            memories
        };

        // 3. Separate regular memories from consolidation insights
        let mut regular: Vec<(i64, String)> = Vec::new();
        let mut insights: Vec<(i64, String)> = Vec::new();

        for m in &memories {
            let is_insight = m.metadata
                .get("agent")
                .and_then(|a| a.get("generated_by"))
                .and_then(|g| g.as_str())
                == Some("consolidate_agent");

            let display = m.metadata
                .get("agent")
                .and_then(|a| a.get("summary"))
                .and_then(|s| s.as_str())
                .unwrap_or(&m.content);

            if is_insight {
                insights.push((m.id, display.to_string()));
            } else {
                regular.push((m.id, display.to_string()));
            }
        }

        // 4. Call LLM to synthesize answer
        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(prompts::QUERY_SYSTEM_PROMPT),
                ChatMessage::user(prompts::query_user_prompt(question, &regular, &insights)),
            ],
            max_tokens: 2048,
            temperature: 0.3,
            json_mode: false,
        };

        let response = self.llm.generate(params).await
            .map_err(|e| anyhow::anyhow!("LLM query failed: {}", e))?;

        info!(
            question = %question[..question.len().min(60)],
            memories_used = regular.len(),
            insights_used = insights.len(),
            "Query answered"
        );

        Ok(response.content)
    }
}

6.8 src/inbox.rs — Copy Verbatim

//! InboxScanner — file watcher using polling

use std::path::{Path, PathBuf};
use tokio::time::{self, Duration};
use tracing::{info, warn, error};

use crate::ingest::IngestService;
use nexus_storage::ProcessedFileRepository;

/// Supported file extensions for text ingestion
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "json", "csv", "log", "xml", "yaml", "yml",
];

pub struct InboxScanner {
    inbox_dir: PathBuf,
    scan_interval_secs: u64,
}

impl InboxScanner {
    pub fn new(inbox_dir: impl Into<PathBuf>, scan_interval_secs: u64) -> Self {
        Self {
            inbox_dir: inbox_dir.into(),
            scan_interval_secs,
        }
    }

    /// Run the inbox scanning loop (call this as a spawned task)
    pub async fn run(
        &self,
        namespace_id: i64,
        ingest_service: &IngestService,
        processed_file_repo: &ProcessedFileRepository,
    ) {
        // Create inbox directory if it doesn't exist
        if let Err(e) = tokio::fs::create_dir_all(&self.inbox_dir).await {
            error!(dir = %self.inbox_dir.display(), error = %e, "Failed to create inbox directory");
            return;
        }

        info!(dir = %self.inbox_dir.display(), interval = self.scan_interval_secs, "Inbox scanner started");

        let mut interval = time::interval(Duration::from_secs(self.scan_interval_secs));

        loop {
            interval.tick().await;

            if let Err(e) = self.scan_once(namespace_id, ingest_service, processed_file_repo).await {
                error!(error = %e, "Inbox scan error");
            }
        }
    }

    async fn scan_once(
        &self,
        namespace_id: i64,
        ingest_service: &IngestService,
        processed_file_repo: &ProcessedFileRepository,
    ) -> anyhow::Result<()> {
        let mut dir = tokio::fs::read_dir(&self.inbox_dir).await?;

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();

            // Skip directories and hidden files
            if path.is_dir() || path.file_name().map_or(true, |n| n.to_string_lossy().starts_with('.')) {
                continue;
            }

            // Check file extension
            let ext = path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if !TEXT_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let path_str = path.to_string_lossy().to_string();

            // Check if already processed
            if processed_file_repo.is_processed(namespace_id, &path_str).await? {
                continue;
            }

            info!(file = %path.display(), "Processing inbox file");

            // Read file content
            match tokio::fs::read_to_string(&path).await {
                Ok(content) => {
                    let content = content.chars().take(10000).collect::<String>();
                    if content.trim().is_empty() {
                        continue;
                    }

                    let filename = path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    match ingest_service.ingest(namespace_id, &content, &filename).await {
                        Ok(memory) => {
                            processed_file_repo.mark_processed(
                                namespace_id,
                                &path_str,
                                None,
                                Some(memory.id),
                            ).await?;
                            info!(file = %filename, memory_id = memory.id, "File ingested");
                        }
                        Err(e) => {
                            processed_file_repo.mark_failed(
                                namespace_id,
                                &path_str,
                                &e.to_string(),
                            ).await?;
                            warn!(file = %filename, error = %e, "File ingestion failed");
                        }
                    }
                }
                Err(e) => {
                    warn!(file = %path.display(), error = %e, "Failed to read file");
                }
            }
        }

        Ok(())
    }
}

6.9 src/supervisor.rs — Key Architectural Piece

//! AgentSupervisor — spawns and manages the always-on background loops

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{self, Duration};
use tracing::{info, error};

use nexus_core::config::{AgentConfig, LlmConfig};
use nexus_llm::create_client;
use nexus_storage::{MemoryRelationRepository, MemoryRepository, NamespaceRepository, ProcessedFileRepository};
use sqlx::SqlitePool;

use crate::consolidate::ConsolidateService;
use crate::inbox::InboxScanner;
use crate::ingest::IngestService;
use crate::query::QueryService;
use crate::types::AgentStatus;

pub struct AgentSupervisor {
    config: AgentConfig,
    llm_config: LlmConfig,
    pool: SqlitePool,
    start_time: Instant,
    ingest_count: Arc<AtomicU64>,
    consolidation_count: Arc<AtomicU64>,
    query_count: Arc<AtomicU64>,
}

impl AgentSupervisor {
    pub fn new(config: AgentConfig, llm_config: LlmConfig, pool: SqlitePool) -> Self {
        Self {
            config,
            llm_config,
            pool,
            start_time: Instant::now(),
            ingest_count: Arc::new(AtomicU64::new(0)),
            consolidation_count: Arc::new(AtomicU64::new(0)),
            query_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Start all background loops. Call this from `nexus serve`.
    pub async fn start(&self) -> anyhow::Result<()> {
        if !self.config.enabled {
            info!("Agent is disabled, skipping startup");
            return Ok(());
        }

        // Ensure namespace exists
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let namespace = ns_repo.get_or_create(&self.config.namespace, "nexus-agent").await?;
        let namespace_id = namespace.id;

        info!(
            namespace = %self.config.namespace,
            namespace_id = namespace_id,
            inbox = %self.config.inbox_dir,
            consolidation_interval = self.config.consolidation_interval_mins,
            "Starting always-on memory agent"
        );

        // Spawn inbox scanner
        {
            let pool = self.pool.clone();
            let llm_config = self.llm_config.clone();
            let inbox_dir = self.config.inbox_dir.clone();
            let scan_interval = self.config.scan_interval_secs;

            tokio::spawn(async move {
                let llm = match create_client(&llm_config) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to create LLM client for inbox scanner: {}", e);
                        return;
                    }
                };

                let memory_repo = MemoryRepository::new(pool.clone());
                let processed_repo = ProcessedFileRepository::new(pool.clone());
                let ingest_svc = IngestService::new(llm, memory_repo);
                let scanner = InboxScanner::new(inbox_dir, scan_interval);

                scanner.run(namespace_id, &ingest_svc, &processed_repo).await;
            });
        }

        // Spawn consolidation loop
        {
            let pool = self.pool.clone();
            let llm_config = self.llm_config.clone();
            let interval_mins = self.config.consolidation_interval_mins;
            let batch_size = self.config.consolidation_batch_size;

            tokio::spawn(async move {
                let llm = match create_client(&llm_config) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to create LLM client for consolidation: {}", e);
                        return;
                    }
                };

                let memory_repo = MemoryRepository::new(pool.clone());
                let relation_repo = MemoryRelationRepository::new(pool.clone());
                let consolidate_svc = ConsolidateService::new(llm, memory_repo, relation_repo);

                let mut interval = time::interval(Duration::from_secs(interval_mins * 60));
                // Skip the first immediate tick
                interval.tick().await;

                loop {
                    interval.tick().await;
                    match consolidate_svc.run_once(namespace_id, batch_size).await {
                        Ok(count) => {
                            if count > 0 {
                                info!(count, "Consolidation pass completed");
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Consolidation error");
                        }
                    }
                }
            });
        }

        info!("Always-on memory agent started");
        Ok(())
    }

    /// Create an IngestService for on-demand ingestion (via API)
    pub fn create_ingest_service(&self) -> anyhow::Result<IngestService> {
        let llm = create_client(&self.llm_config)
            .map_err(|e| anyhow::anyhow!("LLM client error: {}", e))?;
        let memory_repo = MemoryRepository::new(self.pool.clone());
        Ok(IngestService::new(llm, memory_repo))
    }

    /// Create a QueryService for on-demand queries (via API)
    pub fn create_query_service(&self) -> anyhow::Result<QueryService> {
        let llm = create_client(&self.llm_config)
            .map_err(|e| anyhow::anyhow!("LLM client error: {}", e))?;
        let memory_repo = MemoryRepository::new(self.pool.clone());
        Ok(QueryService::new(llm, memory_repo))
    }

    /// Create a ConsolidateService for on-demand consolidation (via API)
    pub fn create_consolidate_service(&self) -> anyhow::Result<ConsolidateService> {
        let llm = create_client(&self.llm_config)
            .map_err(|e| anyhow::anyhow!("LLM client error: {}", e))?;
        let memory_repo = MemoryRepository::new(self.pool.clone());
        let relation_repo = MemoryRelationRepository::new(self.pool.clone());
        Ok(ConsolidateService::new(llm, memory_repo, relation_repo))
    }

    /// Get the agent namespace ID
    pub async fn namespace_id(&self) -> anyhow::Result<i64> {
        let ns_repo = NamespaceRepository::new(self.pool.clone());
        let ns = ns_repo.get_or_create(&self.config.namespace, "nexus-agent").await?;
        Ok(ns.id)
    }

    /// Get current agent status
    pub fn status(&self) -> AgentStatus {
        AgentStatus {
            enabled: self.config.enabled,
            inbox_dir: self.config.inbox_dir.clone(),
            total_ingested: self.ingest_count.load(Ordering::Relaxed),
            total_consolidated: self.consolidation_count.load(Ordering::Relaxed),
            total_queries: self.query_count.load(Ordering::Relaxed),
            last_scan_at: None,
            last_consolidation_at: None,
            uptime_secs: self.start_time.elapsed().as_secs(),
        }
    }
}

6.10 src/lib.rs — Copy Verbatim

//! Nexus Agent - Always-on memory agent for Nexus Memory System
//!
//! Provides LLM-driven memory ingestion, consolidation, and query services.
//!
//! # Features
//!
//! - **IngestService**: Raw text → LLM extraction → enriched memory storage
//! - **ConsolidateService**: Periodic pattern finding across unconsolidated memories
//! - **QueryService**: LLM-synthesized answers with memory citations
//! - **InboxScanner**: File watcher for automatic ingestion
//! - **AgentSupervisor**: Manages background loops
//!
//! # Example
//!
//! ```rust,ignore
//! use nexus_agent::AgentSupervisor;
//!
//! let supervisor = AgentSupervisor::new(agent_config, llm_config, pool);
//! supervisor.start().await?;
//! ```

pub mod consolidate;
pub mod inbox;
pub mod ingest;
pub mod prompts;
pub mod query;
pub mod supervisor;
pub mod types;

pub use consolidate::ConsolidateService;
pub use inbox::InboxScanner;
pub use ingest::IngestService;
pub use query::QueryService;
pub use supervisor::AgentSupervisor;
pub use types::*;

6.11 Workspace Registration

Add to root Cargo.toml:

# In [workspace] members list, add:
"crates/nexus-agent",

# In [workspace.dependencies], add:
nexus-agent = { package = "nexus-memory-agent", version = "1.1.2", path = "crates/nexus-agent" }

6.12 Verification

cargo check -p nexus-memory-agent

---

7. Track 5: Serve/Web Integration

Size: M (1–3h)
Crates modified: nexus-web, nexus-cli
Depends on: Tracks 1–4

7.1 Why

The agent needs to be accessible via: (1) nexus serve --agent to start the background loops, (2) four new web API endpoints for on-demand ingest/query/consolidate/status.

7.2 New Web API Endpoints

Add 4 agent-specific routes alongside the existing API:

Method
Path
Handler
Description
POST
/api/agent/ingest
agent_ingest
Ingest raw text with LLM enrichment
POST
/api/agent/query
agent_query
Query memory with LLM synthesis
POST
/api/agent/consolidate
agent_consolidate
Trigger manual consolidation
GET
/api/agent/status
agent_status
Get agent health/stats

7.3 Request/Response Models

Add to nexus-web/src/models.rs:

// ─── Add to models.rs ───

// =============================================================================
// Agent Request/Response Models
// =============================================================================

/// Request to ingest via the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIngestRequest {
    pub text: String,
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "api".to_string()
}

/// Response from agent ingest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIngestResponse {
    pub success: bool,
    pub memory_id: Option<i64>,
    pub summary: Option<String>,
    pub error: Option<String>,
}

/// Request to query the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQueryRequest {
    pub question: String,
}

/// Response from agent query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentQueryResponse {
    pub success: bool,
    pub question: String,
    pub answer: Option<String>,
    pub error: Option<String>,
}

/// Response from agent consolidate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConsolidateResponse {
    pub success: bool,
    pub memories_processed: usize,
    pub error: Option<String>,
}

/// Response from agent status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusResponse {
    pub enabled: bool,
    pub inbox_dir: String,
    pub total_ingested: u64,
    pub total_consolidated: u64,
    pub total_queries: u64,
    pub uptime_secs: u64,
}

7.4 Agent API Handlers

Create new file nexus-web/src/api/agent.rs:

//! Agent API endpoints

use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error};

use crate::{
    error::{Result, WebError},
    models::{
        AgentIngestRequest, AgentIngestResponse,
        AgentQueryRequest, AgentQueryResponse,
        AgentConsolidateResponse, AgentStatusResponse,
    },
    state::AppState,
};

/// POST /api/agent/ingest — Ingest text with LLM enrichment
pub async fn agent_ingest(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(request): Json<AgentIngestRequest>,
) -> Result<Json<AgentIngestResponse>> {
    let state = state.read().await;

    let supervisor = state.agent_supervisor.as_ref()
        .ok_or_else(|| WebError::InvalidRequest("Agent is not enabled".to_string()))?;

    if request.text.trim().is_empty() {
        return Err(WebError::InvalidRequest("Text cannot be empty".to_string()));
    }

    let ingest_svc = supervisor.create_ingest_service()
        .map_err(|e| WebError::Internal(e.to_string()))?;

    let namespace_id = supervisor.namespace_id().await
        .map_err(|e| WebError::Internal(e.to_string()))?;

    match ingest_svc.ingest(namespace_id, &request.text, &request.source).await {
        Ok(memory) => {
            let summary = memory.metadata
                .get("agent")
                .and_then(|a| a.get("summary"))
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());

            Ok(Json(AgentIngestResponse {
                success: true,
                memory_id: Some(memory.id),
                summary,
                error: None,
            }))
        }
        Err(e) => {
            error!(error = %e, "Agent ingest failed");
            Ok(Json(AgentIngestResponse {
                success: false,
                memory_id: None,
                summary: None,
                error: Some(e.to_string()),
            }))
        }
    }
}

/// POST /api/agent/query — Query memory with LLM synthesis
pub async fn agent_query(
    State(state): State<Arc<RwLock<AppState>>>,
    Json(request): Json<AgentQueryRequest>,
) -> Result<Json<AgentQueryResponse>> {
    let state = state.read().await;

    let supervisor = state.agent_supervisor.as_ref()
        .ok_or_else(|| WebError::InvalidRequest("Agent is not enabled".to_string()))?;

    if request.question.trim().is_empty() {
        return Err(WebError::InvalidRequest("Question cannot be empty".to_string()));
    }

    let query_svc = supervisor.create_query_service()
        .map_err(|e| WebError::Internal(e.to_string()))?;

    let namespace_id = supervisor.namespace_id().await
        .map_err(|e| WebError::Internal(e.to_string()))?;

    match query_svc.query(namespace_id, &request.question, 50).await {
        Ok(answer) => Ok(Json(AgentQueryResponse {
            success: true,
            question: request.question,
            answer: Some(answer),
            error: None,
        })),
        Err(e) => Ok(Json(AgentQueryResponse {
            success: false,
            question: request.question,
            answer: None,
            error: Some(e.to_string()),
        })),
    }
}

/// POST /api/agent/consolidate — Trigger manual consolidation
pub async fn agent_consolidate(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Result<Json<AgentConsolidateResponse>> {
    let state = state.read().await;

    let supervisor = state.agent_supervisor.as_ref()
        .ok_or_else(|| WebError::InvalidRequest("Agent is not enabled".to_string()))?;

    let consolidate_svc = supervisor.create_consolidate_service()
        .map_err(|e| WebError::Internal(e.to_string()))?;

    let namespace_id = supervisor.namespace_id().await
        .map_err(|e| WebError::Internal(e.to_string()))?;

    match consolidate_svc.run_once(namespace_id, 10).await {
        Ok(count) => Ok(Json(AgentConsolidateResponse {
            success: true,
            memories_processed: count,
            error: None,
        })),
        Err(e) => Ok(Json(AgentConsolidateResponse {
            success: false,
            memories_processed: 0,
            error: Some(e.to_string()),
        })),
    }
}

/// GET /api/agent/status — Get agent status
pub async fn agent_status(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Result<Json<AgentStatusResponse>> {
    let state = state.read().await;

    match &state.agent_supervisor {
        Some(supervisor) => {
            let status = supervisor.status();
            Ok(Json(AgentStatusResponse {
                enabled: status.enabled,
                inbox_dir: status.inbox_dir,
                total_ingested: status.total_ingested,
                total_consolidated: status.total_consolidated,
                total_queries: status.total_queries,
                uptime_secs: status.uptime_secs,
            }))
        }
        None => Ok(Json(AgentStatusResponse {
            enabled: false,
            inbox_dir: String::new(),
            total_ingested: 0,
            total_consolidated: 0,
            total_queries: 0,
            uptime_secs: 0,
        })),
    }
}

7.5 Wire Routes into Router

In nexus-web/src/lib.rs, add agent routes:

// ─── Add to api module imports ───
use api::{agent_ingest, agent_query, agent_consolidate, agent_status};

// ─── Add agent routes inside build_router(), alongside existing api_routes ───
let agent_routes = Router::new()
    .route("/agent/ingest", post(agent_ingest))
    .route("/agent/query", post(agent_query))
    .route("/agent/consolidate", post(agent_consolidate))
    .route("/agent/status", get(agent_status));

// Merge into the api_routes nest:
// Change: .nest("/api", api_routes)
// To:     .nest("/api", api_routes.merge(agent_routes))

7.6 Add agent_supervisor to AppState

In nexus-web/src/state.rs:

// ─── Add field to AppState struct ───
pub agent_supervisor: Option<nexus_agent::AgentSupervisor>,

// ─── Initialize as None in AppState::new() ───
// agent_supervisor: None,

The supervisor is set from outside after construction, or passed into a new constructor variant.

7.7 Add --agent flag to CLI serve command

In nexus-cli/src/main.rs, modify the Serve variant:

Serve {
    #[arg(short = 't', long, default_value = "stdio")]
    transport: String,
    #[arg(short, long, default_value = "8768")]
    port: u16,
    /// Enable the always-on memory agent
    #[arg(long)]
    agent: bool,
},

7.8 Add nexus-agent and nexus-llm to web and CLI Cargo.toml dependencies

# In crates/nexus-web/Cargo.toml [dependencies]:
nexus-agent = { workspace = true }
nexus-llm = { workspace = true }

# In crates/nexus-cli/Cargo.toml [dependencies]:
nexus-agent = { workspace = true }

7.9 Verification

cargo check -p nexus-memory-web
cargo check -p nexus-memory
cargo check --workspace

---

8. Track 6: Tests & Verification

Size: M (1–3h)
Depends on: All previous tracks

8.1 Unit Tests for nexus-llm

Test Provider::from_str() for all 8 providers
Test Provider::default_base_url() returns correct URLs
Test Provider::is_anthropic_protocol() returns true only for Anthropic and Z.ai
Test factory rejects unknown providers
Test factory rejects missing API key env vars

8.2 Unit Tests for nexus-agent

Test prompt builders produce non-empty strings
Test IngestExtraction deserializes from expected JSON
Test ConsolidationResult deserializes from expected JSON

8.3 Integration Tests

Test ProcessedFileRepository idempotency (mark processed twice → no error)
Test MemoryRelationRepository stores and retrieves relations
Test MemoryRepository::get_unconsolidated() excludes consolidated memories
Test MemoryRepository::mark_consolidated() updates metadata correctly

8.4 Build Verification

cargo build --workspace
cargo test --workspace
cargo clippy --workspace

---

9. Additional: Multi-Provider API Key Compatibility

This section is IN ADDITION to the rest of the plan.

All 8 providers must be verified as correct configurations. Here is the definitive reference table:

9.1 Complete Provider Configuration Matrix

┌─────────────┬──────────────────────────────────────────────────────────────┬──────────────────────┬─────────────────────────────────────┬────────────────────────────────────┐
│ Provider    │ Base URL                                                     │ API Key Env Var      │ Auth Headers                        │ Chat Endpoint                      │
├─────────────┼──────────────────────────────────────────────────────────────┼──────────────────────┼─────────────────────────────────────┼────────────────────────────────────┤
│ OpenAI      │ https://api.openai.com/v1                                    │ OPENAI_API_KEY       │ Authorization: Bearer <key>         │ POST /chat/completions             │
│             │                                                              │                      │ Content-Type: application/json      │                                    │
├─────────────┼──────────────────────────────────────────────────────────────┼──────────────────────┼─────────────────────────────────────┼────────────────────────────────────┤
│ Anthropic   │ https://api.anthropic.com                                    │ ANTHROPIC_API_KEY     │ x-api-key: <key>                    │ POST /v1/messages                  │
│             │                                                              │                      │ anthropic-version: 2023-06-01       │                                    │
│             │                                                              │                      │ Content-Type: application/json      │                                    │
├─────────────┼──────────────────────────────────────────────────────────────┼──────────────────────┼─────────────────────────────────────┼────────────────────────────────────┤
│ Gemini      │ https://generativelanguage.googleapis.com/v1beta/openai      │ GEMINI_API_KEY       │ Authorization: Bearer <key>         │ POST /chat/completions             │
│             │ (Google's OpenAI-compat endpoint — /chat/completions is      │                      │ Content-Type: application/json      │ (appended to base_url)             │
│             │  appended to base; no /v1 prefix needed)                     │                      │                                     │                                    │
├─────────────┼──────────────────────────────────────────────────────────────┼──────────────────────┼─────────────────────────────────────┼────────────────────────────────────┤
│ OpenRouter  │ https://openrouter.ai/api/v1                                 │ OPENROUTER_API_KEY   │ Authorization: Bearer <key>         │ POST /chat/completions             │
│             │                                                              │                      │ Content-Type: application/json      │                                    │
├─────────────┼──────────────────────────────────────────────────────────────┼──────────────────────┼─────────────────────────────────────┼────────────────────────────────────┤
│ Groq        │ https://api.groq.com/openai/v1                               │ GROQ_API_KEY         │ Authorization: Bearer <key>         │ POST /chat/completions             │
│             │                                                              │                      │ Content-Type: application/json      │                                    │
├─────────────┼──────────────────────────────────────────────────────────────┼──────────────────────┼─────────────────────────────────────┼────────────────────────────────────┤
│ Z.ai        │ https://api.z.ai/api/anthropic                               │ ZAI_API_KEY          │ x-api-key: <key>                    │ POST /v1/messages                  │
│             │ (Anthropic-compatible proxy; same request/response format)    │                      │ anthropic-version: 2023-06-01       │                                    │
│             │                                                              │                      │ Content-Type: application/json      │                                    │
├─────────────┼──────────────────────────────────────────────────────────────┼──────────────────────┼─────────────────────────────────────┼────────────────────────────────────┤
│ Minimax     │ https://api.minimax.io/v1                                    │ MINIMAX_API_KEY      │ Authorization: Bearer <key>         │ POST /chat/completions             │
│             │ (OpenAI-compatible /v1/chat/completions endpoint)             │                      │ Content-Type: application/json      │                                    │
├─────────────┼──────────────────────────────────────────────────────────────┼──────────────────────┼─────────────────────────────────────┼────────────────────────────────────┤
│ Mistral     │ https://api.mistral.ai/v1                                    │ MISTRAL_API_KEY      │ Authorization: Bearer <key>         │ POST /chat/completions             │
│             │ (La Plateforme — standard OpenAI-compatible format)           │                      │ Content-Type: application/json      │                                    │
└─────────────┴──────────────────────────────────────────────────────────────┴──────────────────────┴─────────────────────────────────────┴────────────────────────────────────┘

9.2 Verification Evidence

OpenAI: Standard /v1/chat/completions with Authorization: Bearer. Industry standard.
Anthropic: Uses /v1/messages with x-api-key header + anthropic-version: 2023-06-01. Verified via official API docs.
Gemini: Google provides an official OpenAI-compatible endpoint at https://generativelanguage.googleapis.com/v1beta/openai/. Uses Authorization: Bearer GEMINI_API_KEY. Verified via https://ai.google.dev/gemini-api/docs/openai.
OpenRouter: Standard OpenAI-compatible at https://openrouter.ai/api/v1. Verified widely.
Groq: Standard OpenAI-compatible at https://api.groq.com/openai/v1. Verified via benchmarks and official SDK.
Z.ai: Exposes an Anthropic-compatible proxy at https://api.z.ai/api/anthropic. Verified via https://docs.z.ai/devpack/tool/claude — uses ANTHROPIC_AUTH_TOKEN / x-api-key header, same protocol as Anthropic.
Minimax: Has an OpenAI-compatible /v1/chat/completions endpoint at https://api.minimax.io/v1. Verified via https://platform.minimax.io/docs/api-reference/text-chat — uses Bearer auth.
Mistral: Standard OpenAI-compatible at https://api.mistral.ai/v1. Uses Authorization: Bearer. La Plateforme API.

9.3 How Users Configure

Add to .env.example:

# ─── Always-On Agent Configuration ───

# LLM Provider (openai, anthropic, gemini, openrouter, groq, zai, minimax, mistral)
NEXUS_LLM_PROVIDER=openai
NEXUS_LLM_MODEL=gpt-4o-mini

# API Key env var name (the agent reads the key from THIS env var)
NEXUS_LLM_API_KEY_ENV=OPENAI_API_KEY

# Override base URL (optional — uses provider default if empty)
# NEXUS_LLM_BASE_URL=

# Agent toggle
NEXUS_AGENT_ENABLED=false
NEXUS_AGENT_NAMESPACE=nexus-agent
NEXUS_AGENT_INBOX_DIR=./inbox
NEXUS_AGENT_CONSOLIDATION_INTERVAL=30

# ─── Provider API Keys (set the one matching your provider) ───
# OPENAI_API_KEY=sk-...
# ANTHROPIC_API_KEY=sk-ant-...
# GEMINI_API_KEY=AI...
# OPENROUTER_API_KEY=sk-or-...
# GROQ_API_KEY=gsk_...
# ZAI_API_KEY=...
# MINIMAX_API_KEY=...
# MISTRAL_API_KEY=...

---

10. Additional: Targeted Documentation Updates

This section is IN ADDITION to the rest of the plan. All changes are ADDITIVE — no existing content is removed.

10.1 ARCHITECTURE.md — Add Section

Append after "## Design Notes":

## Always-On Memory Agent

Nexus includes an optional always-on memory agent that provides LLM-driven memory processing:

### Agent Crates

- **`nexus-llm`**: Multi-provider LLM abstraction supporting OpenAI, Anthropic, Gemini, OpenRouter, Groq, Z.ai, Minimax, and Mistral.
- **`nexus-agent`**: Always-on memory agent with three core services:
  - **IngestService**: Accepts raw text → LLM extracts summary, entities, topics, importance → stores enriched memory
  - **ConsolidateService**: Runs on timer → finds connections between memories → stores insights as new memories with relations
  - **QueryService**: Accepts questions → reads memories + insights → LLM synthesizes answer with citations

### Agent Data Flow

The agent stores all data using existing tables:
- Enriched memories are stored in `memories` with extraction results in the `metadata` JSON field
- Consolidation insights are stored as `memories` with `generated_by: "consolidate_agent"` in metadata
- Connections are stored in `memory_relations`
- Processed inbox files are tracked in `processed_files`

### Agent Endpoints

When the agent is enabled (`NEXUS_AGENT_ENABLED=true` or `nexus serve --agent`):
- `POST /api/agent/ingest` — Ingest text with LLM enrichment
- `POST /api/agent/query` — Query memory with LLM synthesis
- `POST /api/agent/consolidate` — Trigger manual consolidation
- `GET /api/agent/status` — Agent health and statistics

10.2 README.md — Add Section

Append after "### Start the HTTP server" section:

### Always-On Memory Agent

Nexus includes an optional LLM-powered agent that continuously processes, consolidates, and serves memory.

#### Quick start

```bash
# Set your LLM provider and API key
export NEXUS_LLM_PROVIDER=openai
export OPENAI_API_KEY=sk-your-key-here
export NEXUS_AGENT_ENABLED=true

# Start with agent enabled
nexus serve --agent --port 8768

Ingest information

curl -X POST http://localhost:8768/api/agent/ingest \
  -H "Content-Type: application/json" \
  -d '{"text": "AI agents are the fastest growing category", "source": "article"}'

Query your memory

curl -X POST http://localhost:8768/api/agent/query \
  -H "Content-Type: application/json" \
  -d '{"question": "What do you know about AI agents?"}'

Supported LLM providers

Provider
Env Var
Default Model
OpenAI
OPENAI_API_KEY
gpt-4o-mini
Anthropic
ANTHROPIC_API_KEY
claude-sonnet-4-20250514
Gemini
GEMINI_API_KEY
gemini-3-flash-preview
OpenRouter
OPENROUTER_API_KEY
openai/gpt-4o-mini
Groq
GROQ_API_KEY
llama-3.3-70b-versatile
Z.ai
ZAI_API_KEY
glm-4.7
Minimax
MINIMAX_API_KEY
MiniMax-M1-80k
Mistral
MISTRAL_API_KEY
mistral-small-latest

### 10.3 `docs/api/rest-api.md` — Add Section

Append agent endpoints documentation.

### 10.4 `docs/guide/` — Add New File `always-on-agent.md`

Create `docs/guide/always-on-agent.md` with:
- Full configuration reference
- Provider setup guides for each of the 8 providers
- Inbox file watcher usage
- Consolidation behavior explanation
- Query syntax and citation format
- Architecture diagram

### 10.5 `.env.example` — Add Agent Variables

As specified in section 9.3 above.

### 10.6 `CHANGELOG.md` — Add Entry

```markdown
## [Unreleased]

### Added
- Always-on memory agent (`nexus-agent` crate) with LLM-driven ingest, consolidate, and query services
- Multi-provider LLM abstraction (`nexus-llm` crate) supporting OpenAI, Anthropic, Gemini, OpenRouter, Groq, Z.ai, Minimax, and Mistral
- Agent API endpoints: `/api/agent/ingest`, `/api/agent/query`, `/api/agent/consolidate`, `/api/agent/status`
- Inbox file watcher for automatic ingestion
- Periodic memory consolidation with pattern discovery
- `processed_files` table for inbox deduplication
- `--agent` flag for `nexus serve` command
- `AgentConfig` and `LlmConfig` in core configuration

---

Maestro Track Summary

For direct conversion to maestro:implement:

Track
Name
Size
Dependencies
Key Deliverables
1
Core Config & Contracts
S
—
LlmConfig, AgentConfig in nexus-core, new error variants
2
nexus-llm Crate
M
T1
Full crate: LlmClient trait, OpenAiCompatibleClient, AnthropicCompatibleClient, factory, 8 providers
3
Storage Extensions
M
T1
processed_files table, ProcessedFileRepository, MemoryRelationRepository, unconsolidated query helpers
4
nexus-agent Crate
L
T1,T2,T3
Full crate: IngestService, ConsolidateService, QueryService, InboxScanner, AgentSupervisor, prompts
5
Serve/Web Integration
M
T1–T4
4 agent API endpoints, --agent CLI flag, AppState agent field, route wiring
6
Tests & Verification
M
T1–T5
Unit tests, integration tests, cargo build/test/clippy --workspace
—
Docs (additive)
S
T1–T5
ARCHITECTURE.md, README.md, .env.example, CHANGELOG.md, docs/guide/always-on-agent.md

Total: ~1–2 days for a single implementer, parallelizable across T2/T3.
