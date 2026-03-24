//! Fallback LLM client with automatic provider failover on quota/rate-limit errors

use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{info, warn};

use crate::client::LlmClient;
use crate::error::{LlmError, Result};
use crate::types::{GenerateParams, GenerateResponse};

/// An LLM client that automatically falls back to alternative providers
/// when the primary provider returns quota/rate-limit errors.
pub struct FallbackClient {
    clients: Vec<std::sync::Arc<dyn LlmClient>>,
    current: AtomicUsize,
}

impl FallbackClient {
    /// Create a new fallback client from an ordered list of providers.
    ///
    /// The first provider in the list is the preferred primary; subsequent
    /// providers are tried in order when the primary (or a previous fallback)
    /// returns a quota/rate-limit error.
    ///
    /// # Panics
    ///
    /// Panics if `clients` is empty.
    pub fn new(clients: Vec<std::sync::Arc<dyn LlmClient>>) -> Self {
        assert!(
            !clients.is_empty(),
            "FallbackClient requires at least one client"
        );
        Self {
            clients,
            current: AtomicUsize::new(0),
        }
    }

    /// Check if an error is a quota/rate-limit error that should trigger fallback.
    fn is_quota_error(err: &LlmError) -> bool {
        match err {
            LlmError::Api { status: 429, .. } => true,
            LlmError::Api { message, .. } => {
                let lower = message.to_lowercase();
                lower.contains("rate_limit")
                    || lower.contains("rate limit")
                    || lower.contains("quota_exceeded")
                    || lower.contains("insufficient_quota")
                    || lower.contains("too many requests")
                    || lower.contains("resource_exhausted")
            }
            _ => false,
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for FallbackClient {
    async fn generate(&self, params: GenerateParams) -> Result<GenerateResponse> {
        let start = self.current.load(Ordering::Relaxed);
        let total = self.clients.len();

        for offset in 0..total {
            let idx = (start + offset) % total;
            match self.clients[idx].generate(params.clone()).await {
                Ok(response) => {
                    if offset > 0 {
                        // Successfully used a fallback, update preferred
                        self.current.store(idx, Ordering::Relaxed);
                        info!(
                            provider = self.clients[idx].provider_name(),
                            model = self.clients[idx].model_name(),
                            "Fallback succeeded (was provider {})",
                            self.clients[start].provider_name()
                        );
                    }
                    return Ok(response);
                }
                Err(e) if Self::is_quota_error(&e) && offset + 1 < total => {
                    warn!(
                        provider = self.clients[idx].provider_name(),
                        error = %e,
                        "Provider hit quota limit, trying next fallback ({}/{})",
                        offset + 1,
                        total - 1
                    );
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        // All providers exhausted -- return the last error.
        // This is reached when the last provider also fails with a quota error
        // (since we only `continue` when `offset + 1 < total`).
        // But the loop's last iteration returns via the `Err(e)` branch above,
        // so this is truly unreachable. Keeping it as a safety net.
        unreachable!("FallbackClient: all providers exhausted without returning an error")
    }

    fn provider_name(&self) -> String {
        let idx = self.current.load(Ordering::Relaxed);
        self.clients[idx].provider_name().to_string()
    }

    fn model_name(&self) -> String {
        let idx = self.current.load(Ordering::Relaxed);
        self.clients[idx].model_name().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TokenUsage;

    /// A mock LLM client that returns a configurable result.
    struct MockClient {
        provider: &'static str,
        model: &'static str,
        result: std::sync::Mutex<Option<Result<GenerateResponse>>>,
    }

    impl MockClient {
        fn new(
            provider: &'static str,
            model: &'static str,
            result: Result<GenerateResponse>,
        ) -> Self {
            Self {
                provider,
                model,
                result: std::sync::Mutex::new(Some(result)),
            }
        }

        fn ok_response(
            _provider: &'static str,
            model: &'static str,
            content: &str,
        ) -> Result<GenerateResponse> {
            Ok(GenerateResponse {
                content: content.to_string(),
                model: model.to_string(),
                usage: Some(TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                }),
            })
        }

        fn quota_error() -> Result<GenerateResponse> {
            Err(LlmError::Api {
                status: 429,
                message: "rate_limit exceeded".to_string(),
            })
        }

        fn other_error() -> Result<GenerateResponse> {
            Err(LlmError::EmptyResponse)
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for MockClient {
        async fn generate(&self, _params: GenerateParams) -> Result<GenerateResponse> {
            let mut guard = self.result.lock().unwrap();
            guard
                .take()
                .expect("MockClient::generate called more than once")
        }

        fn provider_name(&self) -> String {
            self.provider.to_string()
        }

        fn model_name(&self) -> String {
            self.model.to_string()
        }
    }

    fn make_params() -> GenerateParams {
        GenerateParams::default()
    }

    #[tokio::test]
    async fn test_primary_succeeds() {
        let client = FallbackClient::new(vec![std::sync::Arc::new(MockClient::new(
            "primary",
            "model-a",
            MockClient::ok_response("primary", "model-a", "hello"),
        ))]);
        let resp = client.generate(make_params()).await.unwrap();
        assert_eq!(resp.content, "hello");
        assert_eq!(client.provider_name(), "primary");
    }

    #[tokio::test]
    async fn test_fallback_on_quota_error() {
        let client = FallbackClient::new(vec![
            std::sync::Arc::new(MockClient::new(
                "primary",
                "model-a",
                MockClient::quota_error(),
            )),
            std::sync::Arc::new(MockClient::new(
                "backup",
                "model-b",
                MockClient::ok_response("backup", "model-b", "fallback-ok"),
            )),
        ]);
        let resp = client.generate(make_params()).await.unwrap();
        assert_eq!(resp.content, "fallback-ok");
        // After successful fallback, preferred should switch to backup
        assert_eq!(client.provider_name(), "backup");
    }

    #[tokio::test]
    async fn test_non_quota_error_does_not_fallback() {
        let client = FallbackClient::new(vec![
            std::sync::Arc::new(MockClient::new(
                "primary",
                "model-a",
                MockClient::other_error(),
            )),
            std::sync::Arc::new(MockClient::new(
                "backup",
                "model-b",
                MockClient::ok_response("backup", "model-b", "should-not-reach"),
            )),
        ]);
        let err = client.generate(make_params()).await.unwrap_err();
        assert!(matches!(err, LlmError::EmptyResponse));
    }

    #[tokio::test]
    async fn test_all_quota_errors_returns_last_error() {
        let client = FallbackClient::new(vec![
            std::sync::Arc::new(MockClient::new("p1", "m1", MockClient::quota_error())),
            std::sync::Arc::new(MockClient::new("p2", "m2", MockClient::quota_error())),
        ]);
        let err = client.generate(make_params()).await.unwrap_err();
        match err {
            LlmError::Api { status: 429, .. } => {}
            other => panic!("Expected 429 error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_quota_error_message_detection() {
        let client = FallbackClient::new(vec![
            std::sync::Arc::new(MockClient::new(
                "p1",
                "m1",
                Err(LlmError::Api {
                    status: 500,
                    message: "insufficient_quota for this account".to_string(),
                }),
            )),
            std::sync::Arc::new(MockClient::new(
                "p2",
                "m2",
                MockClient::ok_response("p2", "m2", "recovered"),
            )),
        ]);
        let resp = client.generate(make_params()).await.unwrap();
        assert_eq!(resp.content, "recovered");
    }
}
