//! Shared utility functions for agent services

use nexus_core::traits::EmbeddingService;
use nexus_core::{CognitiveLevel, CognitiveMetadata, Memory, PerspectiveKey};
use nexus_llm::{GenerateResponse, TokenUsage};
use nexus_storage::{MemoryRepository, MetricSample};
use tracing::warn;

#[derive(Debug, Clone)]
pub struct CognitionSnapshot {
    pub level: CognitiveLevel,
    pub confidence: Option<f32>,
    pub perspective: Option<PerspectiveKey>,
    pub generated_by: Option<String>,
    pub times_reinforced: i64,
    pub raw_activity: bool,
}

impl CognitionSnapshot {
    /// Build a cognition snapshot from a memory's metadata.
    ///
    /// Parses `CognitiveMetadata` exactly once and reads all fields from the parsed
    /// struct.  Previous versions called `cognitive_level_from_metadata` and
    /// `perspective_from_metadata` as fallbacks, each of which re-parsed the same
    /// JSON — up to three redundant deserializations per call.
    pub fn from_memory(memory: &Memory) -> Self {
        let cognitive = CognitiveMetadata::from_metadata(&memory.metadata);
        let level = cognitive
            .as_ref()
            .map_or(CognitiveLevel::Raw, |value| value.level);
        let perspective = cognitive.as_ref().map(|value| PerspectiveKey {
            observer: value.observer.clone(),
            subject: value.subject.clone(),
            session_key: value.session_key.clone(),
        });
        let raw_activity = memory.labels.iter().any(|label| label == "raw-activity")
            || memory
                .metadata
                .get("raw_activity")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);

        Self {
            level,
            confidence: cognitive.as_ref().and_then(|value| value.confidence),
            perspective,
            generated_by: cognitive.as_ref().map(|value| value.generated_by.clone()),
            times_reinforced: cognitive
                .as_ref()
                .map(|value| value.times_reinforced)
                .unwrap_or(0),
            raw_activity,
        }
    }

    pub fn confidence_meets_threshold(&self) -> bool {
        let confidence = self.confidence.unwrap_or(1.0);
        match self.level {
            CognitiveLevel::Explicit => confidence >= 0.70,
            CognitiveLevel::Derived => confidence >= 0.75,
            CognitiveLevel::Contradiction => confidence >= 0.80,
            CognitiveLevel::SummaryShort | CognitiveLevel::SummaryLong | CognitiveLevel::Raw => {
                true
            }
        }
    }
}

/// Extract the `agent.summary` field from JSON metadata, falling back to a
/// truncated content excerpt.
pub fn extract_agent_summary(metadata: &str, content: &str, fallback_chars: usize) -> String {
    #[derive(serde::Deserialize)]
    struct AgentMeta {
        summary: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Metadata {
        agent: Option<AgentMeta>,
    }

    serde_json::from_str::<Metadata>(metadata)
        .ok()
        .and_then(|md| md.agent)
        .and_then(|a| a.summary)
        .unwrap_or_else(|| content.chars().take(fallback_chars).collect())
}

/// Persist a best-effort stage timing metric without impacting cognition flow.
pub async fn record_stage_metric(
    repo: &MemoryRepository,
    namespace_id: i64,
    metric_name: &str,
    metric_value_ms: f64,
    stage: &str,
) {
    let labels = serde_json::json!({
        "namespace_id": namespace_id,
        "stage": stage,
        "unit": "ms",
    });

    if let Err(error) = repo
        .record_metric(metric_name, metric_value_ms, &labels)
        .await
    {
        warn!(
            %error,
            namespace_id,
            metric_name,
            stage,
            "Failed to persist cognition stage metric"
        );
    }
}

pub fn stage_metric_sample(
    namespace_id: i64,
    metric_name: &str,
    metric_value_ms: f64,
    stage: &str,
) -> MetricSample {
    MetricSample {
        metric_name: metric_name.to_string(),
        metric_value: metric_value_ms,
        labels: serde_json::json!({
            "namespace_id": namespace_id,
            "stage": stage,
            "unit": "ms",
        }),
    }
}

/// Persist best-effort token usage metrics for a cognition stage.
pub async fn record_token_usage_metrics(
    repo: &MemoryRepository,
    namespace_id: i64,
    metric_prefix: &str,
    stage: &str,
    usage: Option<&TokenUsage>,
) {
    let Some(usage) = usage else {
        return;
    };

    for (suffix, value) in [
        ("prompt_tokens", usage.prompt_tokens as f64),
        ("completion_tokens", usage.completion_tokens as f64),
        ("total_tokens", usage.total_tokens as f64),
    ] {
        let metric_name = format!("{metric_prefix}.{suffix}");
        let labels = serde_json::json!({
            "namespace_id": namespace_id,
            "stage": stage,
            "unit": "tokens",
        });

        if let Err(error) = repo.record_metric(&metric_name, value, &labels).await {
            warn!(
                %error,
                namespace_id,
                metric_name,
                stage,
                "Failed to persist cognition token usage metric"
            );
        }
    }
}

pub fn token_usage_metric_samples(
    namespace_id: i64,
    metric_prefix: &str,
    stage: &str,
    usage: Option<&TokenUsage>,
) -> Vec<MetricSample> {
    let Some(usage) = usage else {
        return Vec::new();
    };

    [
        ("prompt_tokens", usage.prompt_tokens as f64),
        ("completion_tokens", usage.completion_tokens as f64),
        ("total_tokens", usage.total_tokens as f64),
    ]
    .into_iter()
    .map(|(suffix, value)| MetricSample {
        metric_name: format!("{metric_prefix}.{suffix}"),
        metric_value: value,
        labels: serde_json::json!({
            "namespace_id": namespace_id,
            "stage": stage,
            "unit": "tokens",
        }),
    })
    .collect()
}

pub async fn flush_metric_samples(repo: &MemoryRepository, samples: &[MetricSample]) {
    if samples.is_empty() {
        return;
    }

    if let Err(error) = repo.record_metrics_batch(samples).await {
        warn!(%error, count = samples.len(), "Failed to persist cognition metric batch");
    }
}

/// Parse a JSON response using the same fenced-block tolerance as `LlmClientJson`.
pub fn parse_json_response<T: serde::de::DeserializeOwned>(
    response: &GenerateResponse,
) -> Result<T, serde_json::Error> {
    let content = response.content.trim();
    let json_str = if content.starts_with("```") {
        let start = content.find('\n').map(|i| i + 1).unwrap_or(3);
        let end = content[start..]
            .rfind("```")
            .map(|i| start + i)
            .unwrap_or(content.len());
        if start >= end {
            content
        } else {
            &content[start..end]
        }
    } else {
        content
    };

    serde_json::from_str(json_str.trim())
}

/// Attempt to generate an embedding for `content` using the optional service.
///
/// Returns `(Some(vector), Some(model_name))` on success, `(None, None)` when
/// the service is absent or the call fails (graceful degradation).
pub async fn maybe_embed(
    service: Option<&dyn EmbeddingService>,
    content: &str,
) -> (Option<Vec<f32>>, Option<String>) {
    let Some(svc) = service else {
        return (None, None);
    };
    match svc.embed(content).await {
        Ok(vec) => (Some(vec), Some(svc.model_name().to_string())),
        Err(error) => {
            warn!(%error, "Embedding generation failed, storing without embedding");
            (None, None)
        }
    }
}
