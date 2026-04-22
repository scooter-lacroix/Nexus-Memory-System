//! Provider definitions and default configurations

use serde::{Deserialize, Serialize};

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
    Nvidia,
    OpenAiCompatible,
    AnthropicCompatible,
}

/// All supported providers in display order.
pub const ALL_PROVIDERS: &[Provider] = &[
    Provider::OpenAi,
    Provider::Anthropic,
    Provider::Gemini,
    Provider::OpenRouter,
    Provider::Groq,
    Provider::Zai,
    Provider::Minimax,
    Provider::Mistral,
    Provider::Nvidia,
    Provider::OpenAiCompatible,
    Provider::AnthropicCompatible,
];

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" | "open_ai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            "gemini" | "google" => Some(Self::Gemini),
            "openrouter" | "open_router" => Some(Self::OpenRouter),
            "groq" => Some(Self::Groq),
            "zai" | "z.ai" | "zhipu" | "bigmodel" => Some(Self::Zai),
            "minimax" => Some(Self::Minimax),
            "mistral" => Some(Self::Mistral),
            "nvidia" => Some(Self::Nvidia),
            "openai_compatible" | "openai-compatible" | "openai_compat" => {
                Some(Self::OpenAiCompatible)
            }
            "anthropic_compatible" | "anthropic-compatible" | "anthropic_compat" => {
                Some(Self::AnthropicCompatible)
            }
            _ => None,
        }
    }

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
            Provider::Nvidia => "https://integrate.api.nvidia.com/v1",
            // Generic compatible providers have no meaningful default — user must supply one.
            Provider::OpenAiCompatible => "",
            Provider::AnthropicCompatible => "",
        }
    }

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
            Provider::Nvidia => "NVIDIA_API_KEY",
            Provider::OpenAiCompatible => "LLM_API_KEY",
            Provider::AnthropicCompatible => "LLM_API_KEY",
        }
    }

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
            Provider::Nvidia => "nvidia/llama-3.1-nemotron-70b-instruct",
            Provider::OpenAiCompatible => "",
            Provider::AnthropicCompatible => "",
        }
    }

    pub fn is_anthropic_protocol(&self) -> bool {
        matches!(
            self,
            Provider::Anthropic | Provider::Zai | Provider::AnthropicCompatible
        )
    }

    pub fn is_openai_protocol(&self) -> bool {
        !self.is_anthropic_protocol()
    }

    /// Whether the base URL must be provided by the user (no sensible default).
    pub fn requires_base_url(&self) -> bool {
        matches!(
            self,
            Provider::OpenAiCompatible | Provider::AnthropicCompatible
        )
    }

    /// Human-readable label for interactive prompts.
    pub fn display_label(&self) -> &'static str {
        match self {
            Provider::OpenAi => "OpenAI",
            Provider::Anthropic => "Anthropic (Claude)",
            Provider::Gemini => "Google Gemini",
            Provider::OpenRouter => "OpenRouter",
            Provider::Groq => "Groq",
            Provider::Zai => "Z.ai",
            Provider::Minimax => "Minimax",
            Provider::Mistral => "Mistral",
            Provider::Nvidia => "NVIDIA NIM",
            Provider::OpenAiCompatible => "OpenAI-compatible (bring your own endpoint)",
            Provider::AnthropicCompatible => "Anthropic-compatible (bring your own endpoint)",
        }
    }

    /// Resolve a user-supplied provider name (id or label fragment) to a Provider.
    pub fn resolve(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        ALL_PROVIDERS
            .iter()
            .find(|p| p.to_string() == lower || p.display_label().to_lowercase().contains(&lower))
            .copied()
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
            Provider::Nvidia => write!(f, "nvidia"),
            Provider::OpenAiCompatible => write!(f, "openai_compatible"),
            Provider::AnthropicCompatible => write!(f, "anthropic_compatible"),
        }
    }
}
