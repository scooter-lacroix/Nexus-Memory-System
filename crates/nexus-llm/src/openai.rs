//! OpenAI-compatible LLM client

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
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    model: Option<String>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OpenAiErrorResponse {
    error: Option<OpenAiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct OpenAiErrorDetail {
    message: Option<String>,
}

pub struct OpenAiCompatibleClient {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    provider: Provider,
    max_tokens: u32,
    #[allow(dead_code)]
    temperature: f32,
}

impl OpenAiCompatibleClient {
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

    fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/chat/completions", base)
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn generate(&self, params: GenerateParams) -> Result<GenerateResponse> {
        let messages: Vec<OpenAiMessage> = params
            .messages
            .iter()
            .map(|m| OpenAiMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let response_format = if params.json_mode {
            Some(ResponseFormat {
                format_type: "json_object".to_string(),
            })
        } else {
            None
        };

        let request = OpenAiRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(if params.max_tokens > 0 {
                params.max_tokens
            } else {
                self.max_tokens
            }),
            temperature: Some(params.temperature.max(0.01)),
            response_format,
        };

        let url = self.chat_completions_url();
        debug!(provider = %self.provider, url = %url, model = %self.model, "Sending OpenAI-compatible request");

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(LlmError::Http)?;

        let status = resp.status().as_u16();
        if status >= 400 {
            let body = resp.text().await.unwrap_or_default();
            let message = serde_json::from_str::<OpenAiErrorResponse>(&body)
                .ok()
                .and_then(|e| e.error)
                .and_then(|e| e.message)
                .unwrap_or(body);
            return Err(LlmError::Api { status, message });
        }

        let response: OpenAiResponse = resp.json().await.map_err(LlmError::Http)?;

        let content = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or(LlmError::EmptyResponse)?;

        let usage = response.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens.unwrap_or(0),
            completion_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        });

        Ok(GenerateResponse {
            content,
            model: response.model.unwrap_or_else(|| self.model.clone()),
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
