//! Factory for creating LLM clients from configuration

use crate::anthropic::AnthropicCompatibleClient;
use crate::client::LlmClient;
use crate::error::{LlmError, Result};
use crate::fallback::FallbackClient;
use crate::openai::OpenAiCompatibleClient;
use crate::provider::Provider;
use nexus_core::config::LlmConfig;
use std::sync::Arc;
use tracing::{info, warn};

/// Create an LLM client from configuration
pub fn create_client(config: &LlmConfig) -> Result<Arc<dyn LlmClient>> {
    let provider = Provider::parse(&config.provider)
        .ok_or_else(|| LlmError::UnsupportedProvider(config.provider.clone()))?;

    let api_key = std::env::var(&config.api_key_env)
        .map_err(|_| LlmError::MissingApiKey(config.api_key_env.clone()))?;

    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| provider.default_base_url().to_string());

    let client: Arc<dyn LlmClient> = if provider.is_anthropic_protocol() {
        Arc::new(AnthropicCompatibleClient::new(
            provider,
            base_url,
            api_key,
            config.model.clone(),
            config.timeout_secs,
            config.max_tokens,
            config.temperature,
        )?)
    } else {
        Arc::new(OpenAiCompatibleClient::new(
            provider,
            base_url,
            api_key,
            config.model.clone(),
            config.timeout_secs,
            config.max_tokens,
            config.temperature,
        )?)
    };

    Ok(client)
}

/// Create an LLM client with auto-configuration from environment
pub fn create_client_auto() -> Result<Arc<dyn LlmClient>> {
    let config = nexus_core::Config::from_env()
        .map(|c| c.llm)
        .unwrap_or_default();
    create_client(&config)
}

/// Create an LLM client with automatic fallback on quota/rate-limit errors.
///
/// Reads `NEXUS_LLM_FALLBACK_{1..5}_PROVIDER`, `NEXUS_LLM_FALLBACK_{1..5}_MODEL`,
/// and `NEXUS_LLM_FALLBACK_{1..5}_API_KEY_ENV` environment variables to configure
/// up to 5 fallback providers. If no fallbacks are configured, returns the primary
/// client directly (fully backward compatible).
pub fn create_client_with_fallback(config: &LlmConfig) -> Result<Arc<dyn LlmClient>> {
    let primary = create_client(config)?;

    let mut fallbacks: Vec<Arc<dyn LlmClient>> = Vec::new();

    for i in 1..=5u32 {
        let provider_env = format!("NEXUS_LLM_FALLBACK_{}_PROVIDER", i);
        let model_env = format!("NEXUS_LLM_FALLBACK_{}_MODEL", i);
        let key_env = format!("NEXUS_LLM_FALLBACK_{}_API_KEY_ENV", i);
        let base_url_env = format!("NEXUS_LLM_FALLBACK_{}_BASE_URL", i);

        let provider = match std::env::var(&provider_env) {
            Ok(p) => p,
            Err(_) => break,
        };

        let model = match std::env::var(&model_env) {
            Ok(m) => m,
            Err(_) => break,
        };

        let api_key_env = std::env::var(&key_env).unwrap_or_else(|_| {
            Provider::parse(&provider)
                .map(|p| p.default_api_key_env().to_string())
                .unwrap_or_default()
        });

        if api_key_env.is_empty() {
            break;
        }

        let fb_config = LlmConfig {
            provider: provider.clone(),
            model,
            api_key_env: api_key_env.clone(),
            base_url: std::env::var(&base_url_env).ok().filter(|s| !s.is_empty()),
            timeout_secs: config.timeout_secs,
            max_tokens: config.max_tokens,
            temperature: config.temperature,
        };

        match create_client(&fb_config) {
            Ok(client) => {
                info!(provider = %provider, index = i, "Configured fallback provider");
                fallbacks.push(client);
            }
            Err(e) => {
                warn!(
                    provider = %provider,
                    index = i,
                    error = %e,
                    "Failed to create fallback client, skipping"
                );
            }
        }
    }

    if fallbacks.is_empty() {
        return Ok(primary);
    }

    info!(
        primary = %primary.provider_name(),
        fallbacks = fallbacks.len(),
        "Created fallback client with {} backup providers",
        fallbacks.len()
    );

    let mut all_clients = vec![primary];
    all_clients.extend(fallbacks);

    Ok(Arc::new(FallbackClient::new(all_clients)))
}

/// Create an LLM client with auto-configuration from environment, including
/// automatic fallback on quota/rate-limit errors.
pub fn create_client_auto_with_fallback() -> Result<Arc<dyn LlmClient>> {
    let config = nexus_core::Config::from_env()
        .map(|c| c.llm)
        .unwrap_or_default();
    create_client_with_fallback(&config)
}
