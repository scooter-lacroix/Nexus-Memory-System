//! Activity distillation pipeline — summarises raw hook events into
//! structured session memories using LLM or a deterministic fallback.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use nexus_core::config::CognitionConfig;
use nexus_core::{
    infer_perspective, CognitiveLevel, CognitiveMetadata, Memory, MemoryCategory,
    MemoryLaneCognitiveType, MemoryLaneType, PerspectiveSource,
};
use nexus_llm::{ChatMessage, GenerateParams, LlmClient, LlmClientJson};
use nexus_storage::repository::{MemoryRepository, StoreMemoryParams};

use tracing::warn;

use crate::error::AgentError;

pub(crate) const ACTIVITY_DISTILL_JOB: &str = "activity_distill";

const ACTIVITY_DISTILL_SYSTEM_PROMPT: &str = r#"You are distilling a batch of raw agent hook events into a meaningful session summary.

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

// ── Data types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DistilledSession {
    pub summary: String,
    pub category: String,
    pub labels: Vec<String>,
    pub key_activities: Vec<String>,
    pub files_touched: Vec<String>,
    pub tools_used: Vec<String>,
    pub decisions_made: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DistillEvent {
    pub memory_id: i64,
    pub created_at: DateTime<Utc>,
    pub session_key: String,
    pub event_name: String,
    pub cwd: Option<String>,
    pub raw_payload: serde_json::Value,
}

// ── Job processing ────────────────────────────────────────────────────

pub(crate) async fn process_activity_distill_jobs(
    repo: &MemoryRepository,
    namespace_id: i64,
    cognition: &CognitionConfig,
    llm: Arc<dyn LlmClient>,
    lease_owner: &str,
) -> Result<usize, AgentError> {
    let jobs = repo
        .claim_jobs(
            namespace_id,
            ACTIVITY_DISTILL_JOB,
            lease_owner,
            cognition.lease_ttl_secs,
            cognition.max_job_batch as i64,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    let mut processed = 0usize;
    let mut seen_sessions = HashSet::new();
    for job in jobs {
        let session_key = match job
            .payload
            .get("session_key")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                job.perspective
                    .as_ref()
                    .and_then(|perspective| perspective.session_key.clone())
            }) {
            Some(key) => key,
            None => {
                let error =
                    AgentError::Ingest("activity_distill job missing session_key".to_string());
                repo.fail_job(&job, &error.to_string())
                    .await
                    .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
                continue;
            }
        };
        let agent_name = job
            .payload
            .get("agent")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown-agent")
            .to_string();

        if !seen_sessions.insert(session_key.clone()) {
            repo.complete_job(&job)
                .await
                .map_err(|error| AgentError::Storage(error.to_string()))?;
            continue;
        }

        match distill_raw_activity_session(
            repo,
            namespace_id,
            &agent_name,
            &session_key,
            cognition,
            llm.clone(),
        )
        .await
        {
            Ok(_) => {
                repo.complete_job(&job)
                    .await
                    .map_err(|error| AgentError::Storage(error.to_string()))?;
                processed += 1;
            }
            Err(error) => {
                repo.fail_job(&job, &error.to_string())
                    .await
                    .map_err(|storage_error| AgentError::Storage(storage_error.to_string()))?;
            }
        }
    }

    Ok(processed)
}

// ── Distillation pipeline ─────────────────────────────────────────────

pub(crate) async fn distill_raw_activity_session(
    repo: &MemoryRepository,
    namespace_id: i64,
    agent: &str,
    session_key: &str,
    cognition: &CognitionConfig,
    llm: Arc<dyn LlmClient>,
) -> Result<Option<i64>, AgentError> {
    let events: Vec<DistillEvent> = repo
        .list_by_session_key(
            namespace_id,
            session_key,
            cognition.activity_distill_max_events as i64,
            true,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?
        .into_iter()
        .filter_map(distill_event_from_memory)
        .collect();

    if events.len() < cognition.activity_distill_min_events {
        return Ok(None);
    }

    let event_summaries: Vec<String> = events.iter().map(summarize_distill_event).collect();
    let source_ids: Vec<i64> = events.iter().map(|event| event.memory_id).collect();
    let distilled = match distill_with_llm(&llm, session_key, event_summaries.as_slice()).await {
        Ok(result) => result,
        Err(error) => {
            warn!(%error, session_key, "LLM distillation failed, using deterministic fallback");
            fallback_distilled_session(&events)
        }
    };

    let category = MemoryCategory::parse(&distilled.category).unwrap_or(MemoryCategory::Session);
    let lane_type = MemoryLaneType::Cognitive(MemoryLaneCognitiveType::Explicit);
    let cognitive = build_distill_cognitive_metadata(agent, session_key, &source_ids);
    let metadata = cognitive.merge_into(&serde_json::json!({
        "distilled_from": events.len(),
        "session_id": session_key,
        "key_activities": distilled.key_activities,
        "files_touched": distilled.files_touched,
        "tools_used": distilled.tools_used,
        "decisions_made": distilled.decisions_made,
        "pipeline": "distill-v1",
    }));

    let memory = repo
        .store_distilled_summary(
            StoreMemoryParams {
                namespace_id,
                content: &distilled.summary,
                category: &category,
                memory_lane_type: Some(&lane_type),
                labels: &distilled.labels,
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            },
            &source_ids,
        )
        .await
        .map_err(|error| AgentError::Storage(error.to_string()))?;

    Ok(Some(memory.id))
}

async fn distill_with_llm(
    llm: &Arc<dyn LlmClient>,
    session_key: &str,
    event_summaries: &[String],
) -> Result<DistilledSession, AgentError> {
    let user_prompt = format!(
        "Session ID: {}\nEvent count: {}\n\nEvents:\n{}",
        session_key,
        event_summaries.len(),
        event_summaries.join("\n")
    );
    llm.generate_json(GenerateParams {
        messages: vec![
            ChatMessage::system(ACTIVITY_DISTILL_SYSTEM_PROMPT),
            ChatMessage::user(user_prompt),
        ],
        max_tokens: 2048,
        temperature: 0.3,
        json_mode: true,
    })
    .await
    .map_err(|error| AgentError::Llm(error.to_string()))
}

fn distill_event_from_memory(memory: Memory) -> Option<DistillEvent> {
    let raw_activity = memory.metadata.get("raw_activity")?;
    if !raw_activity
        .get("distill_pending")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    let session_key = raw_activity
        .get("derived_session_key")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            memory
                .metadata
                .pointer("/cognitive/session_key")
                .and_then(serde_json::Value::as_str)
        })?
        .to_string();
    let event_name = raw_activity
        .get("event_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("hook_event")
        .to_string();
    let cwd = raw_activity
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string);
    let raw_payload = memory
        .metadata
        .get("raw_payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    Some(DistillEvent {
        memory_id: memory.id,
        created_at: memory.created_at,
        session_key,
        event_name,
        cwd,
        raw_payload,
    })
}

fn summarize_distill_event(event: &DistillEvent) -> String {
    let ts = event
        .raw_payload
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| event.created_at.to_rfc3339());
    let event_type = event
        .raw_payload
        .get("event")
        .or_else(|| event.raw_payload.get("hook_event_name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&event.event_name);
    let tool = event
        .raw_payload
        .get("tool")
        .or_else(|| event.raw_payload.get("tool_name"))
        .or_else(|| event.raw_payload.get("toolName"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("-");
    let cwd = event
        .raw_payload
        .get("cwd")
        .or_else(|| event.raw_payload.get("working_directory"))
        .and_then(serde_json::Value::as_str)
        .or(event.cwd.as_deref())
        .unwrap_or("-");
    format!("{ts} | {event_type} | tool={tool} | cwd={cwd}")
}

fn fallback_distilled_session(events: &[DistillEvent]) -> DistilledSession {
    let mut tools = BTreeSet::new();
    let mut workspaces = BTreeSet::new();
    let mut activities = Vec::new();

    for event in events {
        if let Some(tool) = event
            .raw_payload
            .get("tool")
            .or_else(|| event.raw_payload.get("tool_name"))
            .or_else(|| event.raw_payload.get("toolName"))
            .and_then(serde_json::Value::as_str)
        {
            tools.insert(tool.to_string());
        }
        if let Some(cwd) = event.cwd.as_deref() {
            workspaces.insert(cwd.to_string());
        }
        activities.push(event.event_name.clone());
    }

    DistilledSession {
        summary: format!(
            "Session {} produced {} low-signal hook events across {} tool(s), primarily in {}.",
            events
                .first()
                .map(|event| event.session_key.as_str())
                .unwrap_or("unknown-session"),
            events.len(),
            tools.len(),
            workspaces
                .iter()
                .next()
                .cloned()
                .unwrap_or_else(|| "an unknown workspace".to_string())
        ),
        category: "session".to_string(),
        labels: vec!["activity-summary".to_string(), "auto-distilled".to_string()],
        key_activities: activities.into_iter().take(5).collect(),
        files_touched: workspaces.into_iter().collect(),
        tools_used: tools.into_iter().collect(),
        decisions_made: Vec::new(),
    }
}

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
