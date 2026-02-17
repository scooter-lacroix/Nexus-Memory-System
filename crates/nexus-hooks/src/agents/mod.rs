//! Agent hook implementations
//!
//! This module contains all agent-specific hook implementations.

mod claude;
mod gemini;
mod qwen;
mod cli;
mod pi_mono;
mod oh_my_pi;
mod pi_skills;

pub use claude::ClaudeCodeHook;
pub use gemini::GeminiHook;
pub use qwen::QwenHook;
pub use cli::CLIHook;
pub use pi_mono::PiMonoHook;
pub use oh_my_pi::OhMyPiHook;
pub use pi_skills::PiSkillsHook;
