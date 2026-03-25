//! Anthropic-compatible LLM client

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use crate::client::LlmClient;
use crate::error::{LlmError, Result};
use crate::provider::Provider;
use crate::types::{GenerateParams, GenerateResponse, TokenUsage};

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    model: String,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorResponse {
    error: Option<AnthropicErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
}

pub struct AnthropicCompatibleClient {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    provider: Provider,
    max_tokens: u32,
    #[allow(dead_code)]
    temperature: f32,
}

impl AnthropicCompatibleClient {
    pub fn new(
        provider: Provider,
        base_url: String,
        api_key: String,
        model: String,
        timeout_secs: u64,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(LlmError::Http)?;

        Ok(Self {
            client,
            base_url,
            api_key,
            model,
            provider,
            max_tokens,
            temperature,
        })
    }

    fn messages_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/v1/messages", base)
    }
}

#[async_trait]
impl LlmClient for AnthropicCompatibleClient {
    async fn generate(&self, params: GenerateParams) -> Result<GenerateResponse> {
        let system_msg = params
            .messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());

        let messages: Vec<AnthropicMessage> = params
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| AnthropicMessage {
                role: if m.role == "assistant" {
                    "assistant".to_string()
                } else {
                    "user".to_string()
                },
                content: m.content.clone(),
            })
            .collect();

        let request = AnthropicRequest {
            model: self.model.clone(),
            messages,
            max_tokens: if params.max_tokens > 0 {
                params.max_tokens
            } else {
                self.max_tokens
            },
            temperature: Some(params.temperature.max(0.01)),
            system: system_msg,
        };

        let url = self.messages_url();
        debug!(provider = %self.provider, url = %url, model = %self.model, "Sending Anthropic-compatible request");

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request)
            .send()
            .await
            .map_err(LlmError::Http)?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().await.unwrap_or_default();
            let message = serde_json::from_str::<AnthropicErrorResponse>(&body)
                .ok()
                .and_then(|e| e.error)
                .map(|e| format!("{}: {}", e.error_type, e.message))
                .unwrap_or(body);
            return Err(LlmError::Api { status, message });
        }

        let response: AnthropicResponse = resp.json().await.map_err(LlmError::Http)?;

        let content = response
            .content
            .into_iter()
            .find(|c| c.content_type == "text")
            .and_then(|c| c.text)
            .ok_or(LlmError::EmptyResponse)?;

        let usage = response.usage.map(|u| TokenUsage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: u.input_tokens + u.output_tokens,
        });

        Ok(GenerateResponse {
            content,
            model: response.model,
            usage,
        })
    }

    fn provider_name(&self) -> String {
        self.provider.to_string()
    }

    fn model_name(&self) -> String {
        self.model.clone()
    }
}
