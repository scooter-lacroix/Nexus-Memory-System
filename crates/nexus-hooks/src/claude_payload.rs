//! Claude Code hook payload normalization
//!
//! Normalizes raw Claude Code hook payloads into a stable internal event schema.
//! Handles both snake_case and camelCase field names.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Normalized hook event from any agent source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedHookEvent {
    pub agent: String,
    pub event_name: String,
    pub observed_at: DateTime<Utc>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub cwd: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_response_text: Option<String>,
    pub assistant_message_text: Option<String>,
    pub user_message_text: Option<String>,
    pub raw_payload: Value,
}

/// Normalize a Claude Code hook payload into a stable event schema.
///
/// Handles both snake_case and camelCase field names from different
/// Claude Code versions.
pub fn normalize_claude_payload(agent: &str, event_name: &str, raw: &Value) -> NormalizedHookEvent {
    // Extract event_name from payload or use provided one
    let extracted_event_name = get_string(
        raw,
        &[
            "hook_event_name",
            "hookEventName",
            "event_name",
            "eventName",
        ],
    )
    .unwrap_or_else(|| event_name.to_string());

    // Extract tool_name
    let tool_name = get_string(raw, &["tool_name", "toolName", "name"]);

    // Extract tool_input (may be an object or already parsed)
    let tool_input = raw
        .get("tool_input")
        .or_else(|| raw.get("toolInput"))
        .or_else(|| raw.get("input"))
        .cloned();

    // Extract session_id from various possible field names
    let session_id = get_string(
        raw,
        &[
            "session_id",
            "sessionId",
            "thread_id",
            "threadId",
            "conversation_id",
            "conversationId",
        ],
    );

    // Extract turn/message ID
    let turn_id = get_string(raw, &["turn_id", "turnId", "message_id", "messageId"]);

    // Extract tool response and stringify if needed
    let tool_response_text = raw
        .get("tool_response")
        .or_else(|| raw.get("toolResponse"))
        .and_then(|v| {
            if v.is_string() {
                v.as_str().map(|s| s.to_string())
            } else {
                Some(v.to_string())
            }
        });

    // Extract assistant message from message.content or direct field
    let assistant_message_text = raw
        .get("message")
        .and_then(|m| m.get("content"))
        .or_else(|| raw.get("content"))
        .or_else(|| raw.get("assistant_message"))
        .or_else(|| raw.get("assistantMessage"))
        .and_then(|c| flatten_message_content(Some(c)));

    // Extract user message if present at top level
    let user_message_text =
        get_string(raw, &["user_message", "userMessage"]).filter(|s| s.len() > 20);

    // Extract cwd
    let cwd = get_string(
        raw,
        &[
            "cwd",
            "directory",
            "workspace",
            "working_directory",
            "workingDirectory",
        ],
    );

    NormalizedHookEvent {
        agent: agent.to_string(),
        event_name: extracted_event_name,
        observed_at: Utc::now(),
        session_id,
        turn_id,
        cwd,
        tool_name,
        tool_input,
        tool_response_text,
        assistant_message_text,
        user_message_text,
        raw_payload: raw.clone(),
    }
}

/// Get a string value from a JSON object, trying multiple possible keys.
///
/// Returns the first non-empty string match, or None if no key matches
/// or the matched value is not a string.
pub fn get_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = value.get(key) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// Flatten message content from Claude's structured format into plain text.
///
/// Handles:
/// - String content: returns trimmed string if non-empty
/// - Array of content blocks: filters for text blocks and joins with newlines
/// - Other types: returns None
pub fn flatten_message_content(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                Some(trimmed.to_string())
            } else {
                None
            }
        }
        Some(Value::Array(arr)) => {
            let text_blocks: Vec<&str> = arr
                .iter()
                .filter_map(|block| {
                    if let Some(obj) = block.as_object() {
                        if obj.get("type").and_then(|t| t.as_str()) == Some("text") {
                            return obj.get("text").and_then(|t| t.as_str());
                        }
                    }
                    None
                })
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            let joined = text_blocks.join("\n\n");
            if !joined.is_empty() {
                Some(joined)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_normalize_claude_payload_with_bash_tool() {
        let raw = json!({
            "hook_event_name": "post-tool-use",
            "tool_name": "Bash",
            "tool_input": {"command": "cargo test"},
            "tool_response": "running 12 tests... test result: ok",
            "session_id": "sess-123",
            "cwd": "/project"
        });

        let normalized = normalize_claude_payload("claude-code", "post-tool-use", &raw);

        assert_eq!(normalized.agent, "claude-code");
        assert_eq!(normalized.event_name, "post-tool-use");
        assert_eq!(normalized.tool_name, Some("Bash".to_string()));
        assert_eq!(normalized.session_id, Some("sess-123".to_string()));
        assert_eq!(normalized.cwd, Some("/project".to_string()));
        assert!(normalized.tool_response_text.is_some());
    }

    #[test]
    fn test_normalize_claude_payload_camelcase_fallback() {
        let raw = json!({
            "hookEventName": "postToolUse",
            "toolName": "Read",
            "toolInput": {"file_path": "src/main.rs"},
            "sessionId": "sess-456"
        });

        let normalized = normalize_claude_payload("claude-code", "post-tool-use", &raw);

        assert_eq!(normalized.event_name, "postToolUse");
        assert_eq!(normalized.tool_name, Some("Read".to_string()));
        assert_eq!(normalized.session_id, Some("sess-456".to_string()));
    }

    #[test]
    fn test_flatten_message_content_string() {
        let content = Value::String("  Hello world  ".to_string());
        let result = flatten_message_content(Some(&content));
        assert_eq!(result, Some("Hello world".to_string()));
    }

    #[test]
    fn test_flatten_message_content_empty_string() {
        let content = Value::String("   ".to_string());
        let result = flatten_message_content(Some(&content));
        assert_eq!(result, None);
    }

    #[test]
    fn test_flatten_message_content_array() {
        let content = json!( [
            {"type": "text", "text": "First paragraph"},
            {"type": "image", "source": "..."},
            {"type": "text", "text": "Second paragraph"}
        ]);

        let result = flatten_message_content(Some(&content));
        assert_eq!(
            result,
            Some("First paragraph\n\nSecond paragraph".to_string())
        );
    }

    #[test]
    fn test_flatten_message_content_non_text_array() {
        let content = json!( [
            {"type": "image", "source": "..."},
            {"type": "tool_use", "id": "..."}
        ]);

        let result = flatten_message_content(Some(&content));
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_string_multiple_keys() {
        let value = json!({
            "first": "",
            "second": "found",
            "third": "unused"
        });

        let result = get_string(&value, &["first", "second", "third"]);
        assert_eq!(result, Some("found".to_string()));
    }

    #[test]
    fn test_get_string_no_match() {
        let value = json!({"other": "value"});
        let result = get_string(&value, &["missing", "also_missing"]);
        assert_eq!(result, None);
    }

    #[test]
    fn test_normalize_minimal_payload() {
        let raw = json!({});

        let normalized = normalize_claude_payload("claude-code", "test-event", &raw);

        assert_eq!(normalized.agent, "claude-code");
        assert_eq!(normalized.event_name, "test-event");
        assert_eq!(normalized.tool_name, None);
        assert_eq!(normalized.session_id, None);
    }

    #[test]
    fn test_normalize_with_message_content() {
        let raw = json!({
            "message": {
                "content": [
                    {"type": "text", "text": "Decision made"}
                ]
            }
        });

        let normalized = normalize_claude_payload("claude-code", "assistant-message", &raw);

        assert_eq!(
            normalized.assistant_message_text,
            Some("Decision made".to_string())
        );
    }

    #[test]
    fn test_normalize_with_user_message() {
        let raw = json!({
            "user_message": "Please implement the feature with proper error handling"
        });

        let normalized = normalize_claude_payload("claude-code", "user-prompt-submit", &raw);

        assert_eq!(
            normalized.user_message_text,
            Some("Please implement the feature with proper error handling".to_string())
        );
    }

    #[test]
    fn test_normalize_user_message_too_short() {
        let raw = json!({
            "userMessage": "short"
        });

        let normalized = normalize_claude_payload("claude-code", "test", &raw);

        assert_eq!(normalized.user_message_text, None);
    }
}
