//! Core LLM client trait

use crate::error::Result;
use crate::types::{GenerateParams, GenerateResponse};
use async_trait::async_trait;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn generate(&self, params: GenerateParams) -> Result<GenerateResponse>;

    fn provider_name(&self) -> String;
    fn model_name(&self) -> String;
}

/// Extension trait for JSON generation
#[async_trait]
pub trait LlmClientJson: LlmClient {
    async fn generate_json<T: serde::de::DeserializeOwned + Send>(
        &self,
        params: GenerateParams,
    ) -> Result<T> {
        let mut params = params;
        params.json_mode = true;
        let response = self.generate(params).await?;

        let content = response.content.trim();
        let json_str = if content.starts_with("```") {
            let start = content.find('\n').unwrap_or(3) + 1;
            let end = content.rfind("```").unwrap_or(content.len());
            &content[start..end]
        } else {
            content
        };

        serde_json::from_str(json_str.trim()).map_err(|e| {
            crate::error::LlmError::InvalidJsonResponse(format!(
                "Failed to parse: {}. Raw: {}",
                e,
                &json_str[..json_str.len().min(200)]
            ))
        })
    }
}

// Blanket implementation for all LlmClient types
#[async_trait]
impl<T: LlmClient + ?Sized> LlmClientJson for T {}
