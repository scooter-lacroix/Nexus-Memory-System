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
        }
    }

    pub fn is_anthropic_protocol(&self) -> bool {
        matches!(self, Provider::Anthropic | Provider::Zai)
    }

    pub fn is_openai_protocol(&self) -> bool {
        !self.is_anthropic_protocol()
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

    /// Supplemental model IDs known to exist for this provider but not always
    /// returned by the standard `/models` listing endpoint.
    pub fn supplemental_models(&self) -> &'static [&'static str] {
        match self {
            Provider::Zai => &[
                "glm-4.7-flash",
                "glm-4-flash",
                "glm-4-flash-250414",
                "glm-4-plus",
                "glm-4-long",
                "glm-4-air",
                "glm-4-airx",
                "glm-4v",
                "glm-4v-plus",
                "glm-z1-air",
                "glm-z1-airx",
                "glm-z1-flash",
                "cogview-4",
            ],
            Provider::Gemini => &[
                "gemini-2.0-flash",
                "gemini-2.5-pro-preview-05-06",
                "gemini-2.5-flash-preview-05-20",
            ],
            Provider::OpenAi => &[
                "o3-mini",
                "o4-mini",
                "gpt-4.1",
                "gpt-4.1-mini",
                "gpt-4.1-nano",
            ],
            _ => &[],
        }
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
