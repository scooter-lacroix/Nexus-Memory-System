//! LLM provider abstraction for Nexus Memory System
//!
//! Supports multiple providers: OpenAI, Anthropic, Gemini, OpenRouter, Groq, Z.ai, Minimax, Mistral

pub mod client;
pub mod error;
pub mod factory;
pub mod models;
pub mod provider;
pub mod types;

// Protocol-specific implementations
mod anthropic;
mod openai;

// Re-exports
pub use client::{LlmClient, LlmClientJson};
pub use error::{LlmError, Result};
pub use factory::{create_client, create_client_auto};
pub use models::list_models;
pub use provider::Provider;
pub use types::{ChatMessage, GenerateParams, GenerateResponse, TokenUsage};
