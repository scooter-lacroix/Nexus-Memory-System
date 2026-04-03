//! OpenAI-compatible HTTP embedding service implementation

use crate::config::EmbeddingConfig;
use crate::error::{EmbeddingError, Result};
use async_trait::async_trait;
use nexus_core::traits::EmbeddingService;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug)]
pub struct HttpEmbeddingService {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
    dimension: usize,
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: EmbeddingInput<'a>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum EmbeddingInput<'a> {
    Single(&'a str),
    Batch(&'a [String]),
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingDatum>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: Option<ErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    message: Option<String>,
}

impl HttpEmbeddingService {
    pub fn new(config: EmbeddingConfig) -> Result<Self> {
        let base_url = config.base_url.ok_or_else(|| {
            EmbeddingError::ConfigurationError(
                "remote embeddings require NEXUS_EMBEDDING_BASE_URL".to_string(),
            )
        })?;

        let api_key = match config.api_key_env.as_deref() {
            Some(env) if !env.trim().is_empty() => {
                std::env::var(env).ok().filter(|v| !v.is_empty())
            }
            _ => None,
        };

        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| EmbeddingError::RemoteError(e.to_string()))?;

        Ok(Self {
            client,
            base_url,
            api_key,
            model: config.model,
            dimension: config.dimension,
        })
    }

    fn embeddings_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/embeddings", base)
    }

    async fn post_embeddings(&self, input: EmbeddingInput<'_>) -> Result<Vec<Vec<f32>>> {
        let request = EmbeddingRequest {
            model: &self.model,
            input,
        };

        let mut builder = self
            .client
            .post(self.embeddings_url())
            .header("Content-Type", "application/json");
        if let Some(api_key) = &self.api_key {
            builder = builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = builder
            .json(&request)
            .send()
            .await
            .map_err(|e| EmbeddingError::RemoteError(e.to_string()))?;

        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            let body = response.text().await.unwrap_or_default();
            let message = serde_json::from_str::<ErrorEnvelope>(&body)
                .ok()
                .and_then(|env| env.error)
                .and_then(|error| error.message)
                .filter(|message| !message.is_empty())
                .unwrap_or(body);
            return Err(EmbeddingError::RemoteError(message));
        }

        let response: EmbeddingResponse = response
            .json()
            .await
            .map_err(|e| EmbeddingError::RemoteError(e.to_string()))?;

        let _model = response.model.unwrap_or_else(|| self.model.clone());
        Ok(response
            .data
            .into_iter()
            .map(|entry| entry.embedding)
            .collect())
    }
}

#[async_trait]
impl EmbeddingService for HttpEmbeddingService {
    async fn embed(&self, text: &str) -> nexus_core::Result<Vec<f32>> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::InvalidInput("Cannot embed empty text".to_string()).into());
        }
        let embeddings = self.post_embeddings(EmbeddingInput::Single(text)).await?;
        embeddings.into_iter().next().ok_or_else(|| {
            nexus_core::NexusError::Embedding("Remote provider returned no embeddings".to_string())
        })
    }

    async fn embed_batch(&self, texts: &[String]) -> nexus_core::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        self.post_embeddings(EmbeddingInput::Batch(texts))
            .await
            .map_err(Into::into)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::sync::oneshot;

    #[derive(Clone)]
    struct TestState;

    async fn embed_handler(
        State(_state): State<Arc<TestState>>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        let input = payload.get("input").cloned().unwrap_or(Value::Null);
        let data = match input {
            Value::Array(items) => items
                .into_iter()
                .map(|_| json!({ "embedding": vec![0.1_f32, 0.2_f32, 0.3_f32] }))
                .collect::<Vec<_>>(),
            _ => vec![json!({ "embedding": vec![0.4_f32, 0.5_f32, 0.6_f32] })],
        };
        Json(json!({ "data": data, "model": payload.get("model").cloned().unwrap_or(Value::Null) }))
    }

    async fn spawn_test_server() -> (SocketAddr, oneshot::Sender<()>) {
        let state = Arc::new(TestState);
        let app = Router::new()
            .route("/embeddings", post(embed_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .unwrap();
        });
        (addr, tx)
    }

    /// Flaky under CI due to ephemeral port binding race.
    /// Can be run manually with `--include-ignored` when needed.
    #[tokio::test]
    #[ignore = "flaky under CI: TcpListener bind race on ephemeral port"]
    async fn test_http_embedding_service_single_and_batch() {
        let (addr, shutdown) = spawn_test_server().await;
        let config = EmbeddingConfig {
            backend: "openai-compatible".to_string(),
            provider: "custom".to_string(),
            model: "text-embedding-test".to_string(),
            api_key_env: None,
            base_url: Some(format!("http://{}", addr)),
            dimension: 3,
            ..EmbeddingConfig::default()
        };

        let service = HttpEmbeddingService::new(config).unwrap();
        let single = service.embed("hello").await.unwrap();
        assert_eq!(single, vec![0.4_f32, 0.5_f32, 0.6_f32]);

        let batch = service
            .embed_batch(&["one".to_string(), "two".to_string()])
            .await
            .unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0], vec![0.1_f32, 0.2_f32, 0.3_f32]);

        let _ = shutdown.send(());
    }
}
