//! Nexus Hooks - Agent hooks system for automated memory extraction
//!
//! This crate provides a four-layer extraction system for capturing
//! agent session context with 95-100% reliability:
//!
//! 1. **Native Hooks** (100%): Claude Skills, Gemini Functions, Qwen Hooks, pi-mono, oh-my-pi
//! 2. **Session Monitor** (95%): Process monitoring via sysinfo
//! 3. **Inactivity Detector** (90%): Configurable timeout detection
//! 4. **Persistent Buffer** (99%): Crash recovery from buffer
//!
//! # Supported Agents
//!
//! - **Claude Code**: Skills-based (SKILL.md format)
//! - **Gemini**: Function Calling
//! - **Qwen**: Hooks SubAgent
//! - **pi-mono**: Skills-based (TypeScript/Bun)
//! - **oh-my-pi**: Skills-based (TypeScript/Bun + Rust N-API)
//! - **pi-skills**: Cross-compatible skills
//! - **CLI Agents**: Amp, Droid, OpenCode, Codex (atexit/signals)
//!
//! # Example
//!
//! ```rust,no_run
//! use nexus_hooks::{HookFactory, AgentHook, MultiLayerExtractor};
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

pub mod base;
pub mod factory;
pub mod session;
pub mod buffer;
pub mod monitor;
pub mod detector;
pub mod extractor;
pub mod agents;
pub mod types;
pub mod error;
pub mod signal;

// Re-export main types
pub use base::{AgentHook, HookResult};
pub use factory::HookFactory;
pub use session::SessionContext;
pub use buffer::PersistentBuffer;
pub use monitor::{SessionMonitor, ProcessMonitor};
pub use detector::InactivityDetector;
pub use extractor::MultiLayerExtractor;
pub use types::*;
pub use error::{HookError, Result};

// Re-export agent hooks
pub use agents::{
    ClaudeCodeHook, GeminiHook, QwenHook, CLIHook,
    PiMonoHook, OhMyPiHook, PiSkillsHook,
};

/// Hook version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default inactivity timeout in seconds (5 minutes)
pub const DEFAULT_INACTIVITY_TIMEOUT_SECS: u64 = 300;

/// Default buffer flush interval in seconds
pub const DEFAULT_BUFFER_FLUSH_INTERVAL_SECS: u64 = 10;

/// Default process polling interval in seconds
pub const DEFAULT_POLLING_INTERVAL_SECS: u64 = 5;
