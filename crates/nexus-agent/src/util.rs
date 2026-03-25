//! Shared utility functions for agent services

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
