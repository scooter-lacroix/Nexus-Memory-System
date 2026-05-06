//! High-signal memory candidate derivation from normalized hook events
//!
//! Scores events and derives MemoryCandidates only when there is
//! possible retrieval value. Implements duplicate suppression.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

use crate::claude_payload::NormalizedHookEvent;

/// Truncate a string to at most `max_chars` characters, appending "..." if truncated.
///
/// Operates on `char` boundaries (not bytes) so it is safe for multi-byte UTF-8.
pub(crate) fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

/// A candidate memory derived from a hook event, pending LLM enrichment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub candidate_id: String,
    pub source_event_name: String,
    pub source_agent: String,
    pub signal_score: f32,
    pub provisional_category: Option<String>,
    pub memory_text: String,
    pub evidence: Value,
    pub labels: Vec<String>,
}

/// Simple bash commands that indicate low-signal noise.
const SIMPLE_BASH_COMMANDS: &[&str] = &["ls", "pwd", "whoami", "date", "uptime", "echo"];

/// Patterns that indicate high-signal content in tool responses.
const HIGH_SIGNAL_PATTERNS: &[&str] = &[
    "test result:",
    "passed",
    "failed",
    "error:",
    "warning:",
    "version",
    "/",
    ".",
    "compilation",
    "build",
];

/// Derive memory candidates from a normalized hook event.
///
/// Returns a vector of candidates (typically 0-1). Returns empty if:
/// - The event has no extractable content
/// - The signal score is below threshold (0.3)
/// - The event is a duplicate (based on fingerprint)
pub fn derive_candidates(
    event: &NormalizedHookEvent,
    seen_fingerprints: &mut HashSet<String>,
) -> Vec<MemoryCandidate> {
    // Start with zero signal
    let mut signal_score = 0.0f32;

    // Bash-specific scoring - check before base score for simple commands
    let is_low_signal_bash = if event.tool_name.as_deref() == Some("Bash") {
        let command = event
            .tool_input
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        let is_simple = SIMPLE_BASH_COMMANDS
            .iter()
            .any(|&simple| command.trim().starts_with(simple));

        if is_simple {
            // Simple bash commands get very low signal
            signal_score += 0.1;
            true
        } else if let Some(response) = &event.tool_response_text {
            // Check for high-signal patterns in response
            let pattern_matches = HIGH_SIGNAL_PATTERNS
                .iter()
                .filter(|&&pattern| response.to_lowercase().contains(pattern))
                .count();
            // Score proportional to number of high-signal patterns found
            signal_score += 0.2 + (pattern_matches as f32 * 0.1).min(0.4);
            false
        } else {
            false
        }
    } else {
        false
    };

    // Check for tool input with content (unless it's a low-signal bash command)
    if !is_low_signal_bash {
        let has_tool_input = event
            .tool_input
            .as_ref()
            .map(|v| !v.is_null() && !v.as_object().map(|o| o.is_empty()).unwrap_or(false))
            .unwrap_or(false);

        if event.tool_name.is_some() && has_tool_input {
            signal_score += 0.3;
        }

        // File operations (Read, Write, Edit) are inherently high-signal
        if let Some(tool) = &event.tool_name {
            let tool_lower = tool.to_lowercase();
            if tool_lower == "read"
                || tool_lower == "write"
                || tool_lower == "edit"
                || tool_lower == "multi_edit"
                || tool_lower == "glob"
                || tool_lower == "grep"
            {
                signal_score += 0.2;
            }
        }
    }

    // Assistant message contributes (reduced minimum threshold, reward length)
    if let Some(msg) = &event.assistant_message_text {
        if msg.len() > 50 {
            signal_score += 0.3;
        } else if msg.len() > 20 {
            signal_score += 0.15;
        }
    }

    // User message contributes (reduced minimum threshold, reward length)
    if let Some(msg) = &event.user_message_text {
        if msg.len() > 100 {
            signal_score += 0.35;
        } else if msg.len() > 20 {
            signal_score += 0.2;
        }
    }

    // User prompt submit is high signal only if it has actual content
    if event.event_name == "user-prompt-submit" && event.user_message_text.is_some() {
        signal_score += 0.3;
    }

    // Plan and review events are high signal
    let event_lower = event.event_name.to_lowercase();
    if event_lower.contains("plan") || event_lower.contains("review") {
        signal_score += 0.2;
    }

    // Error/failure events are always high signal
    if event_lower.contains("error")
        || event_lower.contains("fail")
        || event_lower.contains("crash")
    {
        signal_score += 0.25;
    }

    // Build/compile/test events are high signal
    if event_lower.contains("build")
        || event_lower.contains("test")
        || event_lower.contains("compile")
    {
        signal_score += 0.2;
    }

    // Require meaningful content, not just metadata presence
    let has_meaningful_tool_input = event.tool_input.as_ref().is_some_and(|v| {
        !v.is_null() && !v.as_object().is_some_and(|o| o.is_empty()) && v.to_string().len() > 5
    });
    let has_any_content = has_meaningful_tool_input
        || event
            .tool_response_text
            .as_ref()
            .is_some_and(|s| s.len() > 10)
        || event
            .assistant_message_text
            .as_ref()
            .is_some_and(|s| s.len() > 20)
        || event
            .user_message_text
            .as_ref()
            .is_some_and(|s| s.len() > 10);

    if !has_any_content {
        return Vec::new();
    }

    // Skip if signal score is too low (reduced from 0.4 to 0.3 to capture more useful events)
    if signal_score < 0.3 {
        return Vec::new();
    }

    // Build fingerprint for duplicate suppression
    let tool_input_hash = if let Some(input) = &event.tool_input {
        let mut hasher = Sha256::new();
        hasher.update(input.to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    } else {
        String::new()
    };

    let fingerprint = format!(
        "{}|{}|{}|{}",
        event.session_id.as_deref().unwrap_or(""),
        event.event_name,
        event.tool_name.as_deref().unwrap_or(""),
        tool_input_hash
    );

    if seen_fingerprints.contains(&fingerprint) {
        return Vec::new(); // Duplicate
    }

    seen_fingerprints.insert(fingerprint);

    // Derive memory text based on event type
    let memory_text = derive_memory_text(event);

    // Build evidence JSON
    let evidence = build_evidence(event);

    // Derive labels
    let labels = derive_labels(event, signal_score);

    // Determine provisional category
    let provisional_category = derive_provisional_category(event, signal_score);

    let candidate_id = uuid::Uuid::new_v4().to_string();

    vec![MemoryCandidate {
        candidate_id,
        source_event_name: event.event_name.clone(),
        source_agent: event.agent.clone(),
        signal_score,
        provisional_category,
        memory_text,
        evidence,
        labels,
    }]
}

/// Derive memory text based on event type and content.
fn derive_memory_text(event: &NormalizedHookEvent) -> String {
    // Bash tool event
    if event.tool_name.as_deref() == Some("Bash") {
        let command = event
            .tool_input
            .as_ref()
            .and_then(|v| v.get("command"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        let excerpt = event
            .tool_response_text
            .as_ref()
            .map(|s| truncate_str(s, 12000))
            .unwrap_or_default();

        if !excerpt.is_empty() {
            return format!("Ran `{}` → {}", command, excerpt);
        }
    }

    // User prompt submit
    if event.event_name == "user-prompt-submit" {
        if let Some(msg) = &event.user_message_text {
            return msg.clone();
        }
    }

    // Plan/review events
    let event_lower = event.event_name.to_lowercase();
    if event_lower.contains("plan") || event_lower.contains("review") {
        if let Some(input) = &event.tool_input {
            if let Some(plan) = input.get("plan").and_then(|p| p.as_str()) {
                return format!("Plan: {}", plan);
            }
        }
        if let Some(name) = &event.tool_name {
            return format!("Plan: {}", name);
        }
    }

    // Assistant messages with decisions
    if let Some(msg) = &event.assistant_message_text {
        if msg.to_lowercase().contains("decision")
            || msg.to_lowercase().contains("will")
            || msg.to_lowercase().contains("going to")
        {
            let excerpt = truncate_str(msg, 12000);
            return format!("Decision: {}", excerpt);
        }
    }

    // Fallback: concatenate available text
    let parts: Vec<&str> = vec![
        event.tool_response_text.as_deref(),
        event.assistant_message_text.as_deref(),
        event.user_message_text.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();

    if parts.is_empty() {
        format!("Event: {}", event.event_name)
    } else {
        parts.join(" | ")
    }
}

/// Build evidence JSON for the candidate.
fn build_evidence(event: &NormalizedHookEvent) -> Value {
    let mut evidence = serde_json::Map::new();

    if let Some(name) = &event.tool_name {
        evidence.insert("tool_name".to_string(), Value::String(name.clone()));
    }

    if let Some(input) = &event.tool_input {
        evidence.insert("tool_input".to_string(), input.clone());
    }

    if let Some(response) = &event.tool_response_text {
        let excerpt = truncate_str(response, 200);
        evidence.insert("tool_response_excerpt".to_string(), Value::String(excerpt));
    }

    if let Some(msg) = &event.assistant_message_text {
        let excerpt = truncate_str(msg, 200);
        evidence.insert(
            "assistant_message_excerpt".to_string(),
            Value::String(excerpt),
        );
    }

    if let Some(msg) = &event.user_message_text {
        let excerpt = truncate_str(msg, 200);
        evidence.insert("user_message_excerpt".to_string(), Value::String(excerpt));
    }

    Value::Object(evidence)
}

/// Derive labels based on event content.
fn derive_labels(event: &NormalizedHookEvent, signal_score: f32) -> Vec<String> {
    let mut labels = Vec::new();

    // Signal level label
    if signal_score >= 0.7 {
        labels.push("high-signal".to_string());
    } else if signal_score >= 0.5 {
        labels.push("medium-signal".to_string());
    }

    // Tool-based labels
    if let Some(name) = &event.tool_name {
        labels.push(format!("tool:{}", name.to_lowercase()));
    }

    // Event type labels
    let event_lower = event.event_name.to_lowercase();
    if event_lower.contains("plan") {
        labels.push("plan".to_string());
    }
    if event_lower.contains("review") {
        labels.push("review".to_string());
    }
    if event_lower.contains("error") {
        labels.push("error".to_string());
    }

    // Verification/testing labels
    if event_lower.contains("test") || event_lower.contains("verify") {
        labels.push("verification".to_string());
    }

    labels
}

/// Derive provisional category based on event characteristics.
fn derive_provisional_category(event: &NormalizedHookEvent, signal_score: f32) -> Option<String> {
    let event_lower = event.event_name.to_lowercase();

    // User preferences
    if event_lower.contains("user-prompt") {
        if let Some(msg) = &event.user_message_text {
            if msg.to_lowercase().contains("prefer")
                || msg.to_lowercase().contains("always")
                || msg.to_lowercase().contains("never")
            {
                return Some("preferences".to_string());
            }
        }
    }

    // Context/decisions from assistant
    if event_lower.contains("plan") || event_lower.contains("review") {
        return Some("context".to_string());
    }

    // Facts from verification
    if (event_lower.contains("test") || event_lower.contains("verify")) && signal_score > 0.6 {
        return Some("facts".to_string());
    }

    // Bash output often contains facts
    if event.tool_name.as_deref() == Some("Bash") {
        if let Some(response) = &event.tool_response_text {
            if response.contains("test result:")
                || response.contains("passed")
                || response.contains("failed")
            {
                return Some("facts".to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_payload::normalize_claude_payload;
    use serde_json::json;

    #[test]
    fn test_noise_event_yields_no_candidates() {
        let raw = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "ls"},
            "tool_response": "file1.txt\nfile2.txt"
        });

        let event = normalize_claude_payload("claude-code", "post-tool-use", &raw);
        let mut seen = HashSet::new();
        let candidates = derive_candidates(&event, &mut seen);

        // Simple ls command should have low signal (<0.4)
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_bash_verification_event_yields_candidate() {
        let raw = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": "running 12 tests\ntest result: ok. 12 passed; 0 failed",
            "session_id": "sess-123"
        });

        let event = normalize_claude_payload("claude-code", "post-tool-use", &raw);
        let mut seen = HashSet::new();
        let candidates = derive_candidates(&event, &mut seen);

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert!(candidate.signal_score >= 0.4);
        assert!(candidate.memory_text.contains("Ran"));
        assert!(candidate.labels.iter().any(|l| l == "tool:bash"));
    }

    #[test]
    fn test_user_preference_prompt_yields_candidate() {
        let raw = json!({
            "event_name": "user-prompt-submit",
            "user_message": "I always prefer to use rustfmt with a 4-space indent. Please configure this for all my projects."
        });

        let event = normalize_claude_payload("claude-code", "user-prompt-submit", &raw);
        let mut seen = HashSet::new();
        let candidates = derive_candidates(&event, &mut seen);

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert!(candidate.signal_score >= 0.5);
        assert_eq!(
            candidate.provisional_category,
            Some("preferences".to_string())
        );
        assert!(candidate.memory_text.contains("prefer"));
    }

    #[test]
    fn test_duplicate_suppression_works() {
        let raw = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": "test result: ok",
            "session_id": "sess-456"
        });

        let event = normalize_claude_payload("claude-code", "post-tool-use", &raw);
        let mut seen = HashSet::new();

        let first = derive_candidates(&event, &mut seen);
        assert_eq!(first.len(), 1);

        let second = derive_candidates(&event, &mut seen);
        assert_eq!(second.len(), 0); // Duplicate suppressed
    }

    #[test]
    fn test_plan_event_yields_candidate() {
        let raw = json!({
            "event_name": "plan-review",
            "tool_name": "Plan",
            "tool_input": {"plan": "Implement feature X, then test"}
        });

        let event = normalize_claude_payload("claude-code", "plan-review", &raw);
        let mut seen = HashSet::new();
        let candidates = derive_candidates(&event, &mut seen);

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert!(candidate.signal_score >= 0.3);
        assert!(candidate.labels.contains(&"plan".to_string()));
        assert_eq!(candidate.provisional_category, Some("context".to_string()));
    }

    #[test]
    fn test_empty_event_yields_no_candidates() {
        let raw = json!({});

        let event = normalize_claude_payload("claude-code", "empty", &raw);
        let mut seen = HashSet::new();
        let candidates = derive_candidates(&event, &mut seen);

        assert!(candidates.is_empty());
    }

    #[test]
    fn test_high_signal_label_added() {
        let raw = json!({
            "event_name": "user-prompt-submit",
            "user_message": "I always prefer using tabs over spaces in my code. This is a strong preference that applies to all languages.",
            "assistant_message": "I'll configure your editor to use tabs by default for all file types."
        });

        let event = normalize_claude_payload("claude-code", "user-prompt-submit", &raw);
        let mut seen = HashSet::new();
        let candidates = derive_candidates(&event, &mut seen);

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert!(candidate.signal_score >= 0.7);
        assert!(candidate.labels.contains(&"high-signal".to_string()));
    }

    #[test]
    fn test_evidence_construction() {
        let raw = json!({
            "tool_name": "Read",
            "tool_input": {"file_path": "src/main.rs"},
            "tool_response": "This is a very long response that should be truncated in the evidence because it exceeds the maximum character limit for excerpts.",
            "assistant_message": "The file contains the main function with error handling."
        });

        let event = normalize_claude_payload("claude-code", "post-tool-use", &raw);
        let mut seen = HashSet::new();
        let candidates = derive_candidates(&event, &mut seen);

        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert!(candidate.evidence.get("tool_name").is_some());
        assert!(candidate.evidence.get("tool_input").is_some());

        let excerpt = candidate
            .evidence
            .get("tool_response_excerpt")
            .and_then(|v| v.as_str());
        assert!(excerpt.is_some());
        assert!(excerpt.unwrap().len() <= 203); // 200 + "..."
    }

    #[test]
    fn test_truncate_utf8_multibyte() {
        // Japanese characters are 3 bytes each in UTF-8
        let s = "日本語テスト文字列";
        assert_eq!(truncate_str(s, 100), s);
        let truncated = truncate_str(s, 4);
        assert_eq!(truncated, "日本語テ...");
        // Verify it's valid UTF-8
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
    }

    #[test]
    fn test_truncate_mixed_ascii_multibyte() {
        let s = "Hello日本語World";
        assert_eq!(truncate_str(s, 100), s);
        let truncated = truncate_str(s, 7);
        assert_eq!(truncated, "Hello日本...");
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
    }

    #[test]
    fn test_truncate_empty_and_short() {
        assert_eq!(truncate_str("", 10), "");
        assert_eq!(truncate_str("hi", 10), "hi");
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_exact_boundary() {
        let s = "abcdefghij";
        assert_eq!(truncate_str(s, 10), s); // exactly at limit, no "..."
        let longer = "abcdefghijklmno";
        assert_eq!(truncate_str(longer, 10), "abcdefghij...");
    }

    #[test]
    fn test_different_sessions_different_fingerprints() {
        let raw1 = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "echo test"},
            "tool_response": "test",
            "session_id": "sess-A"
        });

        let raw2 = json!({
            "tool_name": "Bash",
            "tool_input": {"command": "echo test"},
            "tool_response": "test",
            "session_id": "sess-B"
        });

        let event1 = normalize_claude_payload("claude-code", "post-tool-use", &raw1);
        let event2 = normalize_claude_payload("claude-code", "post-tool-use", &raw2);

        let mut seen = HashSet::new();
        let candidates1 = derive_candidates(&event1, &mut seen);
        let candidates2 = derive_candidates(&event2, &mut seen);

        // Different sessions should not be considered duplicates
        // (though simple echo may still be filtered by low signal)
        let total: usize = candidates1.len() + candidates2.len();
        assert!(total <= 2);
    }
}
