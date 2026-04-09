//! Nexus Hooks - Agent hooks system for automated memory extraction
//!
//! This crate provides a four-layer extraction system for capturing
//! agent session context with 95-100% reliability:
//!
//! 1. **Native Hooks** (98-100%): Claude Skills, pi-mono, oh-my-pi, pi-skills
//! 2. **Session Monitor** (95%): Process monitoring via sysinfo
//! 3. **Inactivity Detector** (90%): Configurable timeout detection
//! 4. **Persistent Buffer** (99%): Crash recovery from buffer
//!
//! # Supported Agents
//!
//! ## Native Lifecycle (dedicated hook implementation + skill installation)
//!
//! - **Claude Code**: Skills-based (SKILL.md format) — session start, end, checkpoint, error, compact
//! - **Droid (Factory CLI)**: settings-based (`~/.factory/settings.json`) — SessionStart, SessionEnd, PostToolUse, PreCompact, Stop
//! - **pi-mono**: Skills-based (TypeScript/Bun) — session end, checkpoint, compact
//! - **oh-my-pi**: Skills-based (TypeScript/Bun + Rust N-API) — session end, checkpoint, error, compact
//! - **pi-skills**: Cross-compatible skills — session end, checkpoint, compact
//!
//! ## Monitor Only (process detection, no native hooks)
//!
//! - **Gemini**: Process monitoring only (function calling not yet wired)
//! - **Qwen**: Process monitoring only (hooks subagent not yet wired)
//!
//! ## Wrapper Lifecycle (generic CLI wrapper, atexit + process detection)
//!
//! - **CLI Agents**: Amp, OpenCode, Codex, Hermes (shared CLIHook implementation)
//!
//! # Example
//!
//! ```rust,no_run
//! use nexus_memory_hooks::{HookFactory, AgentHook, MultiLayerExtractor};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create hook for specific agent
//!     let factory = HookFactory::new();
//!     let mut hook = factory.create_hook("claude-code")?;
//!
//!     // Check if session is active
//!     let activity = hook.detect_session_activity().await?;
//!     println!("Session active: {}", activity.is_active);
//!
//!     // Extract session context
//!     if activity.is_active {
//!         let context = hook.extract_session_context().await?;
//!         println!("Extracted context: {:?}", context);
//!     }
//!
//!     Ok(())
//! }
//! ```

pub mod agents;
pub mod base;
pub mod buffer;
pub mod candidate;
pub mod claude_payload;
pub mod detector;
pub mod enrichment;
pub mod error;
pub mod extractor;
pub mod factory;
pub mod monitor;
pub mod persistence;
pub mod retry_buffer;
pub mod session;
pub mod signal;
pub mod types;

// Re-export main types
pub use base::{AgentHook, HookResult, LifecycleCapabilities};
pub use buffer::PersistentBuffer;
pub use candidate::{derive_candidates, MemoryCandidate};
pub use claude_payload::{
    flatten_text_value, normalize_claude_payload, normalize_generic_payload, normalize_payload,
    NormalizedHookEvent,
};
pub use detector::InactivityDetector;
pub use enrichment::{EnrichedMemory, EnrichmentBatchResult, EnrichmentService};
pub use error::{HookError, Result};
pub use extractor::MultiLayerExtractor;
pub use factory::HookFactory;
pub use monitor::{ProcessMonitor, SessionMonitor};
pub use persistence::{persist_enriched_memories, PersistResult};
pub use retry_buffer::{RetryArtifact, RetryBuffer};
pub use session::SessionContext;
pub use types::{AgentType, DetectionLayer, ExtractionSource, SessionActivity, SupportTier};

// Re-export agent hooks
pub use agents::{
    CLIHook, ClaudeCodeHook, DroidHook, GeminiHook, OhMyPiHook, PiMonoHook, PiSkillsHook, QwenHook,
};

/// Hook version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default inactivity timeout in seconds (5 minutes)
pub const DEFAULT_INACTIVITY_TIMEOUT_SECS: u64 = 300;

/// Default buffer flush interval in seconds
pub const DEFAULT_BUFFER_FLUSH_INTERVAL_SECS: u64 = 10;

/// Default process polling interval in seconds
pub const DEFAULT_POLLING_INTERVAL_SECS: u64 = 5;
