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
