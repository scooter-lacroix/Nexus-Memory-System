//! Agent hook implementations
//!
//! This module contains all agent-specific hook implementations.

mod claude;
mod cli;
mod gemini;
mod oh_my_pi;
mod pi_mono;
mod pi_skills;
mod qwen;

pub use claude::ClaudeCodeHook;
pub use cli::CLIHook;
pub use gemini::GeminiHook;
pub use oh_my_pi::OhMyPiHook;
pub use pi_mono::PiMonoHook;
pub use pi_skills::PiSkillsHook;
pub use qwen::QwenHook;
