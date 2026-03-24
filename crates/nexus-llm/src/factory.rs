//! Factory for creating LLM clients from configuration

use crate::anthropic::AnthropicCompatibleClient;
use crate::client::LlmClient;
use crate::error::{LlmError, Result};
use crate::openai::OpenAiCompatibleClient;
use crate::provider::Provider;
use nexus_core::config::LlmConfig;
use std::sync::Arc;

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
    let config = LlmConfig::default();
    create_client(&config)
}
