//! List available models from an LLM provider

use nexus_core::config::LlmConfig;

use crate::error::{LlmError, Result};
use crate::provider::Provider;

/// Response from OpenAI-compatible `GET /models` endpoint.
#[derive(serde::Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModel>,
}

#[derive(serde::Deserialize)]
struct OpenAiModel {
    id: String,
}

/// List available model IDs from the configured provider.
///
/// Validates the API key and base URL by making a live request.
/// Returns model IDs sorted alphabetically.
pub async fn list_models(config: &LlmConfig) -> Result<Vec<String>> {
    let provider = Provider::parse(&config.provider)
        .ok_or_else(|| LlmError::UnsupportedProvider(config.provider.clone()))?;

    let api_key = std::env::var(&config.api_key_env)
        .map_err(|_| LlmError::MissingApiKey(config.api_key_env.clone()))?;

    let base_url = config
        .base_url
        .as_deref()
        .unwrap_or(provider.default_base_url());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_secs.min(15)))
        .build()?;

    if provider.is_anthropic_protocol() {
        list_anthropic_models(&client, base_url, &api_key).await
    } else {
        list_openai_models(&client, base_url, &api_key).await
    }
}

async fn list_openai_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await?;

    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(LlmError::Api {
            status,
            message: body,
        });
    }

    let models: OpenAiModelsResponse = resp.json().await?;
    let mut ids: Vec<String> = models.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

async fn list_anthropic_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));

    let resp = client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?;

    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(LlmError::Api {
            status,
            message: body,
        });
    }

    let models: OpenAiModelsResponse = resp.json().await?;
    let mut ids: Vec<String> = models.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}
