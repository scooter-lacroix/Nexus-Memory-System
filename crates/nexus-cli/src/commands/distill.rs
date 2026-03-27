//! Distill command - process raw hook events into meaningful session summaries
//!
//! Takes accumulated raw JSON hook events and uses LLM to produce
//! human-readable session summaries, then replaces the raw noise.

use anyhow::Result;
use nexus_core::{
    infer_perspective, CognitiveLevel, CognitiveMetadata, Config, MemoryLaneCognitiveType,
    MemoryLaneType, PerspectiveSource,
};
use nexus_llm::{create_client_auto_with_fallback, ChatMessage, GenerateParams, LlmClientJson};
use nexus_storage::repository::ListMemoryFilters;
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
struct DistilledSession {
    pub summary: String,
    pub category: String,
    pub labels: Vec<String>,
    pub key_activities: Vec<String>,
    pub files_touched: Vec<String>,
    pub tools_used: Vec<String>,
    pub decisions_made: Vec<String>,
}

#[derive(Debug, Clone)]
struct DistillEvent {
    memory_id: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    session_key: String,
    event_name: String,
    cwd: Option<String>,
    raw_payload: Value,
}

const DISTILL_SYSTEM_PROMPT: &str = r#"You are distilling a batch of raw agent hook events into a meaningful session summary.

Given a set of raw hook events (JSON with timestamps, tool names, CWD, session IDs), produce a structured summary of what happened in the session.

Focus on:
- What the user/agent was working on (project, directory, task)
- Which tools were used and how often
- Key actions taken (tests run, files edited, commands executed)
- Any patterns (repeated test runs, debugging cycles, etc.)

Return strict JSON with these fields:
- summary: A 1-3 sentence human-readable summary of the session
- category: One of "session", "context", "facts"
- labels: 2-5 descriptive labels
- key_activities: List of notable activities
- files_touched: List of files/directories mentioned
- tools_used: List of unique tools used
- decisions_made: Any decisions evident from the event sequence

Return strict JSON only. No markdown fences."#;

pub async fn execute(agent: String, batch_size: usize, dry_run: bool) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespace = match namespace_repo.get_by_name(&agent).await? {
        Some(ns) => ns,
        None => {
            println!("No namespace found for agent '{}'", agent);
            return Ok(());
        }
    };

    // Find raw activity candidates by category, then extract payloads from metadata or legacy content.
    let candidate_events = memory_repo
        .list_filtered(
            namespace.id,
            ListMemoryFilters {
                category: Some("session"),
                since: None,
                until: None,
                content_like: None,
                include_raw: true,
                limit: (batch_size * 20) as i64,
                offset: 0,
            },
        )
        .await?;
    let raw_events: Vec<_> = candidate_events
        .into_iter()
        .filter_map(|memory| distill_event_from_memory(&agent, memory))
        .collect();

    if raw_events.is_empty() {
        println!("No raw hook events found to distill.");
        return Ok(());
    }

    println!("Found {} raw hook events to distill.", raw_events.len());

    // Group by session_id
    let mut sessions: HashMap<String, Vec<&DistillEvent>> = HashMap::new();

    for event in &raw_events {
        sessions
            .entry(event.session_key.clone())
            .or_default()
            .push(event);
    }

    println!("Grouped into {} sessions.", sessions.len());

    if dry_run {
        for (session_id, events) in &sessions {
            let first_ts = events.iter().map(|e| e.created_at).min();
            let last_ts = events.iter().map(|e| e.created_at).max();
            let short_id: String = session_id.chars().take(12).collect();
            println!(
                "  Session {}: {} events ({:?} - {:?})",
                short_id,
                events.len(),
                first_ts,
                last_ts,
            );
        }
        println!(
            "\n[dry-run] Would distill {} sessions. Run without --dry-run to execute.",
            sessions.len()
        );
        return Ok(());
    }

    let llm = create_client_auto_with_fallback()?;
    let mut total_distilled = 0u64;
    let mut total_removed = 0u64;

    for (session_id, events) in &sessions {
        if events.len() < 3 {
            continue; // Not enough events to distill meaningfully
        }

        // Build a condensed representation of the events for the LLM
        let event_summaries: Vec<String> = events
            .iter()
            .take(batch_size)
            .map(|e| {
                let ts = e
                    .raw_payload
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| e.created_at.to_rfc3339());
                let event_type = e
                    .raw_payload
                    .get("event")
                    .or_else(|| e.raw_payload.get("hook_event_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&e.event_name);
                let tool = e
                    .raw_payload
                    .get("tool")
                    .or_else(|| e.raw_payload.get("tool_name"))
                    .or_else(|| e.raw_payload.get("toolName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                let cwd = e
                    .raw_payload
                    .get("cwd")
                    .or_else(|| e.raw_payload.get("working_directory"))
                    .and_then(|v| v.as_str())
                    .or(e.cwd.as_deref())
                    .unwrap_or("-");
                format!("{} | {} | tool={} | cwd={}", ts, event_type, tool, cwd)
            })
            .collect();

        if event_summaries.is_empty() {
            continue;
        }

        let user_prompt = format!(
            "Session ID: {}\nEvent count: {}\n\nEvents:\n{}",
            session_id,
            events.len(),
            event_summaries.join("\n")
        );

        let params = GenerateParams {
            messages: vec![
                ChatMessage::system(DISTILL_SYSTEM_PROMPT),
                ChatMessage::user(user_prompt),
            ],
            max_tokens: 2048,
            temperature: 0.3,
            json_mode: true,
        };

        match llm.generate_json::<DistilledSession>(params).await {
            Ok(distilled) => {
                // Store the distilled summary with cognition-aware metadata
                let category = nexus_core::MemoryCategory::parse(&distilled.category)
                    .unwrap_or(nexus_core::MemoryCategory::Session);
                let source_ids: Vec<i64> = events.iter().map(|event| event.memory_id).collect();
                let cognitive = build_distill_cognitive_metadata(&agent, session_id, &source_ids);
                let metadata = cognitive.merge_into(&serde_json::json!({
                    "distilled_from": events.len(),
                    "session_id": session_id,
                    "key_activities": distilled.key_activities,
                    "files_touched": distilled.files_touched,
                    "tools_used": distilled.tools_used,
                    "decisions_made": distilled.decisions_made,
                    "pipeline": "distill-v1",
                }));
                let labels = distilled.labels.clone();
                let lane_type = MemoryLaneType::Cognitive(MemoryLaneCognitiveType::Explicit);

                let store_result = memory_repo
                    .store_distilled_summary(
                        nexus_storage::repository::StoreMemoryParams {
                            namespace_id: namespace.id,
                            content: &distilled.summary,
                            category: &category,
                            memory_lane_type: Some(&lane_type),
                            labels: &labels,
                            metadata: &metadata,
                            embedding: None,
                            embedding_model: None,
                        },
                        &source_ids,
                    )
                    .await;

                match store_result {
                    Ok(memory) => {
                        let short_id: String = session_id.chars().take(12).collect();
                        println!(
                            "✓ Distilled session {} ({} events) → memory #{}: {}",
                            short_id,
                            events.len(),
                            memory.id,
                            distilled.summary.chars().take(80).collect::<String>()
                        );
                        total_distilled += 1;

                        total_removed += source_ids.len() as u64;
                    }
                    Err(e) => {
                        eprintln!("  Failed to store distilled memory: {}", e);
                    }
                }
            }
            Err(e) => {
                let short_id: String = session_id.chars().take(12).collect();
                eprintln!("  Failed to distill session {}: {}", short_id, e);
            }
        }
    }

    println!(
        "\nDistilled {} sessions, removed {} raw events.",
        total_distilled, total_removed
    );

    Ok(())
}

/// Build a [`CognitiveMetadata`] envelope for a distilled summary memory.
///
/// The perspective is inferred via [`infer_perspective`] with [`PerspectiveSource::Digest`],
/// using the agent name as observer and the session key for scoping.
fn build_distill_cognitive_metadata(
    agent: &str,
    session_key: &str,
    source_memory_ids: &[i64],
) -> CognitiveMetadata {
    let perspective = infer_perspective(
        PerspectiveSource::Digest,
        agent,
        None::<String>,
        Some(session_key.to_string()),
    );
    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::SummaryShort,
        perspective.observer,
        perspective.subject,
        perspective.session_key,
        "nexus:distill-v1",
    );
    cognitive.source_memory_ids = source_memory_ids.to_vec();
    cognitive.confidence = Some(0.8);
    cognitive
}

fn distill_event_from_memory(agent: &str, memory: nexus_core::Memory) -> Option<DistillEvent> {
    let raw_payload = memory
        .metadata
        .get("raw_payload")
        .cloned()
        .filter(|value| value.is_object())
        .or_else(|| parse_legacy_raw_payload(&memory.content))?;

    if !looks_like_raw_activity(&raw_payload) {
        return None;
    }

    let derived_session_key = memory
        .metadata
        .pointer("/session_lifecycle/derived_session_key")
        .and_then(|v| v.as_str())
        .or_else(|| {
            memory
                .metadata
                .pointer("/raw_activity/derived_session_key")
                .and_then(|v| v.as_str())
        })
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            nexus_agent::derive_session_key(
                agent,
                raw_payload
                    .get("session_id")
                    .or_else(|| raw_payload.get("sessionId"))
                    .and_then(|v| v.as_str()),
                raw_payload
                    .get("cwd")
                    .or_else(|| raw_payload.get("working_directory"))
                    .and_then(|v| v.as_str()),
            )
        });
    let event_name = memory
        .metadata
        .pointer("/session_lifecycle/event")
        .and_then(|v| v.as_str())
        .or_else(|| {
            memory
                .metadata
                .pointer("/raw_activity/event_name")
                .and_then(|v| v.as_str())
        })
        .or_else(|| raw_payload.get("event").and_then(|v| v.as_str()))
        .unwrap_or("hook_event")
        .to_string();
    let cwd = memory
        .metadata
        .pointer("/session_lifecycle/cwd")
        .and_then(|v| v.as_str())
        .or_else(|| {
            memory
                .metadata
                .pointer("/raw_activity/cwd")
                .and_then(|v| v.as_str())
        })
        .or_else(|| raw_payload.get("cwd").and_then(|v| v.as_str()))
        .map(ToOwned::to_owned);

    Some(DistillEvent {
        memory_id: memory.id,
        created_at: memory.created_at,
        session_key: derived_session_key,
        event_name,
        cwd,
        raw_payload,
    })
}

fn parse_legacy_raw_payload(content: &str) -> Option<Value> {
    let trimmed = content.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

fn looks_like_raw_activity(raw_payload: &Value) -> bool {
    raw_payload.get("timestamp").is_some()
        || raw_payload.get("event").is_some()
        || raw_payload.get("tool").is_some()
        || raw_payload.get("tool_name").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_distill_cognitive_metadata_fields() {
        let ids = vec![10, 20, 30];
        let cognitive = build_distill_cognitive_metadata("claude", "sess-abc-123", &ids);

        assert_eq!(cognitive.level, CognitiveLevel::SummaryShort);
        assert_eq!(cognitive.observer, "claude");
        assert_eq!(cognitive.subject, "claude");
        assert_eq!(cognitive.session_key, Some("sess-abc-123".to_string()));
        assert_eq!(cognitive.source_memory_ids, ids);
        assert!((cognitive.confidence.unwrap() - 0.8).abs() < f32::EPSILON);
        assert_eq!(cognitive.generated_by, "nexus:distill-v1");
        assert_eq!(cognitive.times_reinforced, 0);
        assert_eq!(cognitive.times_contradicted, 0);
        assert!(cognitive.derived_at.is_some());
    }

    #[test]
    fn test_build_distill_cognitive_metadata_perspective_defaults_subject_to_observer() {
        let cognitive = build_distill_cognitive_metadata("codex", "sess-xyz-789", &[1]);

        // Digest source defaults subject to observer when no subject_hint is provided
        assert_eq!(cognitive.observer, "codex");
        assert_eq!(cognitive.subject, "codex");
        assert_eq!(cognitive.session_key, Some("sess-xyz-789".to_string()));

        let perspective = cognitive.perspective();
        assert_eq!(perspective.observer, "codex");
        assert_eq!(perspective.subject, "codex");
    }

    #[test]
    fn test_cognitive_metadata_merge_into_preserves_existing_keys() {
        let cognitive = build_distill_cognitive_metadata("gemini", "sess-merge-001", &[42]);
        let existing = serde_json::json!({
            "pipeline": "distill-v1",
            "distilled_from": 5,
            "session_id": "sess-merge-001"
        });
        let merged = cognitive.merge_into(&existing);

        // Existing keys preserved
        assert_eq!(merged["pipeline"], "distill-v1");
        assert_eq!(merged["distilled_from"], 5);
        assert_eq!(merged["session_id"], "sess-merge-001");

        // Cognitive envelope nested under "cognitive"
        let cog = &merged["cognitive"];
        assert_eq!(cog["level"], "summary_short");
        assert_eq!(cog["observer"], "gemini");
        assert_eq!(cog["generated_by"], "nexus:distill-v1");
        assert!((cog["confidence"].as_f64().unwrap() - 0.8).abs() < 1e-6);
        assert_eq!(cog["source_memory_ids"], serde_json::json!([42]));

        // Round-trip: CognitiveMetadata::from_metadata recovers the struct
        let recovered = CognitiveMetadata::from_metadata(&merged).unwrap();
        assert_eq!(recovered.level, CognitiveLevel::SummaryShort);
        assert_eq!(recovered.observer, "gemini");
    }
}
