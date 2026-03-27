//! Hook payload normalization
//!
//! Normalizes raw hook payloads from any agent into a stable internal event
//! schema.  Provides a Claude-specific normalizer (handles snake_case and
//! camelCase), a generic normalizer (common field names across agents), and a
//! unified dispatcher that selects the right one automatically.

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

/// Agent type strings that should use the Claude-specific normalizer.
const CLAUDE_AGENT_TYPES: &[&str] = &["claude-code", "claude"];

/// Flatten a JSON value to a plain-text string.
///
/// Handles strings directly, arrays of strings (joined with newlines),
/// and objects that contain a "text" key.  Returns `None` for null or empty.
pub fn flatten_text_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let flattened: Vec<String> = items.iter().filter_map(flatten_text_value).collect();
            if flattened.is_empty() {
                None
            } else {
                Some(flattened.join("\n"))
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                if text.is_empty() {
                    None
                } else {
                    Some(text.to_string())
                }
            } else {
                Some(value.to_string())
            }
        }
        _ => Some(value.to_string()),
    }
}

/// Normalize a generic (non-Claude) hook payload into the stable event schema.
///
/// Tries common field names across different agent formats (Gemini, Qwen,
/// Codex, Amp, Droid, etc.) for tool_name, tool_input, session_id, cwd,
/// and message content.
///
/// Field coverage:
/// - **Gemini**: `functionCall.name`, `functionCall.args`, `functionResponse.response`
/// - **Codex/Amp**: `function.name`, `arguments`, `choices[0].message.content`
/// - **Qwen/Droid/Hermes**: `name`, `input`, `output`, `sessionKey`
pub fn normalize_generic_payload(agent: &str, event: &str, raw: &Value) -> NormalizedHookEvent {
    let obj = raw.as_object().cloned().unwrap_or_default();

    // tool_name: handles Gemini functionCall, Codex/Amp function, generic name
    let tool_name = obj
        .get("tool_name")
        .or_else(|| obj.get("toolName"))
        .or_else(|| obj.get("name"))
        .or_else(|| obj.get("functionCall").and_then(|fc| fc.get("name")))
        .or_else(|| obj.get("function").and_then(|f| f.get("name")))
        .cloned();

    // tool_input: handles Gemini functionCall.args, Codex/Amp arguments/input
    let tool_input = obj
        .get("tool_input")
        .or_else(|| obj.get("toolInput"))
        .or_else(|| obj.get("input"))
        .or_else(|| obj.get("arguments"))
        .or_else(|| obj.get("functionCall").and_then(|fc| fc.get("args")))
        .or_else(|| obj.get("function").and_then(|f| f.get("arguments")))
        .cloned();

    // tool_response: handles Gemini functionResponse, generic output/result
    let tool_response_text = obj
        .get("tool_response_text")
        .or_else(|| obj.get("toolResponseText"))
        .or_else(|| obj.get("output"))
        .or_else(|| obj.get("result"))
        .or_else(|| {
            obj.get("functionResponse")
                .and_then(|fr| fr.get("response"))
        })
        .and_then(flatten_text_value);

    // assistant_message: handles Codex/Amp choices[0].message.content, generic response
    let assistant_message_text = obj
        .get("assistant_message_text")
        .or_else(|| obj.get("assistantMessageText"))
        .or_else(|| obj.get("assistant_message"))
        .or_else(|| obj.get("response"))
        .or_else(|| {
            obj.get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
        })
        .and_then(flatten_text_value);

    let user_message_text = obj
        .get("user_message_text")
        .or_else(|| obj.get("userMessageText"))
        .or_else(|| obj.get("user_message"))
        .or_else(|| obj.get("prompt"))
        .and_then(flatten_text_value);

    // session_id: handles Gemini sessionId, Qwen sessionKey, generic variants
    let session_id = obj
        .get("session_id")
        .or_else(|| obj.get("sessionId"))
        .or_else(|| obj.get("sessionKey"))
        .or_else(|| obj.get("thread_id"))
        .and_then(|v| v.as_str().map(String::from));

    let turn_id = obj
        .get("turn_id")
        .or_else(|| obj.get("turnId"))
        .and_then(|v| v.as_str().map(String::from));

    // cwd: handles generic cwd, workingDirectory, Gemini sandbox/workingDir
    let cwd = obj
        .get("cwd")
        .or_else(|| obj.get("working_directory"))
        .or_else(|| obj.get("workingDirectory"))
        .or_else(|| obj.get("workingDir"))
        .and_then(|v| v.as_str().map(String::from));

    NormalizedHookEvent {
        agent: agent.to_string(),
        event_name: event.to_string(),
        observed_at: chrono::Utc::now(),
        session_id,
        turn_id,
        cwd,
        tool_name: tool_name.and_then(|v| v.as_str().map(String::from)),
        tool_input,
        tool_response_text,
        assistant_message_text,
        user_message_text,
        raw_payload: raw.clone(),
    }
}

/// Normalize a hook payload into the stable event schema.
///
/// Automatically selects the Claude-specific or generic normalizer based
/// on the agent type string.
pub fn normalize_payload(agent: &str, event: &str, raw: &Value) -> NormalizedHookEvent {
    let agent_lower = agent.to_lowercase();
    if CLAUDE_AGENT_TYPES.contains(&agent_lower.as_str()) {
        normalize_claude_payload(agent, event, raw)
    } else {
        normalize_generic_payload(agent, event, raw)
    }
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

    // ── Generic normalizer tests ─────────────────────────────────────

    #[test]
    fn test_normalize_generic_gemini_payload() {
        let raw = json!({
            "toolName": "Bash",
            "toolInput": {"command": "npm test"},
            "output": "12 tests passed",
            "sessionId": "gem-sess-1",
            "workingDirectory": "/home/user/project"
        });

        let normalized = normalize_generic_payload("gemini", "post-tool-use", &raw);

        assert_eq!(normalized.agent, "gemini");
        assert_eq!(normalized.event_name, "post-tool-use");
        assert_eq!(normalized.tool_name, Some("Bash".to_string()));
        assert_eq!(normalized.session_id, Some("gem-sess-1".to_string()));
        assert_eq!(normalized.cwd, Some("/home/user/project".to_string()));
        assert_eq!(
            normalized.tool_response_text,
            Some("12 tests passed".to_string())
        );
    }

    #[test]
    fn test_normalize_generic_qwen_payload() {
        let raw = json!({
            "name": "Read",
            "input": {"file_path": "src/main.rs"},
            "sessionKey": "qw-sess-1",
            "cwd": "/project"
        });

        let normalized = normalize_generic_payload("qwen", "tool-use", &raw);

        assert_eq!(normalized.agent, "qwen");
        assert_eq!(normalized.tool_name, Some("Read".to_string()));
        assert_eq!(normalized.session_id, Some("qw-sess-1".to_string()));
        assert_eq!(normalized.cwd, Some("/project".to_string()));
    }

    #[test]
    fn test_normalize_generic_minimal_payload() {
        let raw = json!({});

        let normalized = normalize_generic_payload("codex", "event", &raw);

        assert_eq!(normalized.agent, "codex");
        assert_eq!(normalized.event_name, "event");
        assert_eq!(normalized.tool_name, None);
        assert_eq!(normalized.session_id, None);
        assert_eq!(normalized.cwd, None);
    }

    #[test]
    fn test_normalize_generic_camelcase_fields() {
        let raw = json!({
            "toolName": "Write",
            "toolInput": {"path": "foo.rs"},
            "toolResponseText": "Written 42 bytes",
            "assistantMessageText": "File created successfully",
            "userMessageText": "Create a new file",
            "turnId": "turn-1",
            "workingDirectory": "/workspace"
        });

        let normalized = normalize_generic_payload("amp", "post-tool-use", &raw);

        assert_eq!(normalized.tool_name, Some("Write".to_string()));
        assert!(normalized.tool_input.is_some());
        assert_eq!(
            normalized.tool_response_text,
            Some("Written 42 bytes".to_string())
        );
        assert_eq!(
            normalized.assistant_message_text,
            Some("File created successfully".to_string())
        );
        assert_eq!(
            normalized.user_message_text,
            Some("Create a new file".to_string())
        );
        assert_eq!(normalized.turn_id, Some("turn-1".to_string()));
        assert_eq!(normalized.cwd, Some("/workspace".to_string()));
    }

    #[test]
    fn test_flatten_text_value_null() {
        assert_eq!(flatten_text_value(&Value::Null), None);
    }

    #[test]
    fn test_flatten_text_value_empty_string() {
        assert_eq!(flatten_text_value(&Value::String(String::new())), None);
    }

    #[test]
    fn test_flatten_text_value_string() {
        assert_eq!(
            flatten_text_value(&Value::String("hello".to_string())),
            Some("hello".to_string())
        );
    }

    #[test]
    fn test_flatten_text_value_array() {
        let arr = json!(["line1", "line2", "line3"]);
        assert_eq!(
            flatten_text_value(&arr),
            Some("line1\nline2\nline3".to_string())
        );
    }

    #[test]
    fn test_flatten_text_value_object_with_text() {
        let obj = json!({"text": "content"});
        assert_eq!(flatten_text_value(&obj), Some("content".to_string()));
    }

    // ── Unified dispatcher tests ──────────────────────────────────────

    #[test]
    fn test_normalize_payload_dispatches_claude() {
        let raw = json!({
            "tool_name": "Bash",
            "session_id": "sess-1"
        });

        let normalized = normalize_payload("claude-code", "event", &raw);
        assert_eq!(normalized.agent, "claude-code");
        // Claude normalizer picks up session_id from "session_id"
        assert_eq!(normalized.session_id, Some("sess-1".to_string()));
    }

    #[test]
    fn test_normalize_payload_dispatches_generic() {
        let raw = json!({
            "toolName": "Read",
            "sessionId": "sess-2"
        });

        let normalized = normalize_payload("gemini", "event", &raw);
        assert_eq!(normalized.agent, "gemini");
        // Generic normalizer picks up session_id from "sessionId"
        assert_eq!(normalized.session_id, Some("sess-2".to_string()));
    }

    #[test]
    fn test_normalize_payload_dispatches_by_alias() {
        let raw = json!({
            "tool_name": "Bash",
        });

        let normalized = normalize_payload("claude", "event", &raw);
        assert_eq!(normalized.agent, "claude");
        assert_eq!(normalized.tool_name, Some("Bash".to_string()));
    }

    // ── Cross-agent consistency tests ───────────────────────────────

    #[test]
    fn test_normalize_codex_payload() {
        let raw = json!({
            "toolName": "Bash",
            "toolInput": {"command": "go test ./..."},
            "toolResponseText": "PASS",
            "sessionId": "cx-1",
            "turnId": "t-1",
            "workingDirectory": "/project"
        });

        let normalized = normalize_payload("codex", "post-tool-use", &raw);
        assert_eq!(normalized.agent, "codex");
        assert_eq!(normalized.tool_name, Some("Bash".to_string()));
        assert_eq!(normalized.session_id, Some("cx-1".to_string()));
        assert_eq!(normalized.turn_id, Some("t-1".to_string()));
        assert_eq!(normalized.cwd, Some("/project".to_string()));
    }

    #[test]
    fn test_normalize_amp_payload() {
        let raw = json!({
            "tool_name": "Edit",
            "tool_input": {"file": "src/lib.rs"},
            "tool_response_text": "Updated 3 lines",
            "assistant_message_text": "Fixed the off-by-one error",
            "session_id": "amp-sess",
            "cwd": "/workspace"
        });

        let normalized = normalize_payload("amp", "post-tool-use", &raw);
        assert_eq!(normalized.agent, "amp");
        assert_eq!(normalized.tool_name, Some("Edit".to_string()));
        assert_eq!(normalized.session_id, Some("amp-sess".to_string()));
        assert_eq!(normalized.cwd, Some("/workspace".to_string()));
        assert_eq!(
            normalized.assistant_message_text,
            Some("Fixed the off-by-one error".to_string())
        );
    }

    #[test]
    fn test_normalize_droid_payload_minimal() {
        let raw = json!({
            "name": "Read",
            "input": {"file_path": "README.md"}
        });

        let normalized = normalize_payload("droid", "tool-use", &raw);
        assert_eq!(normalized.agent, "droid");
        assert_eq!(normalized.tool_name, Some("Read".to_string()));
        assert_eq!(normalized.session_id, None);
        assert_eq!(normalized.cwd, None);
    }

    #[test]
    fn test_all_agents_produce_stable_schema() {
        let agents = [
            "claude-code",
            "claude",
            "gemini",
            "qwen",
            "codex",
            "amp",
            "droid",
            "opencode",
        ];
        for agent in agents {
            let raw = json!({
                "tool_name": "Test",
                "session_id": "s-1",
            });
            let normalized = normalize_payload(agent, "test-event", &raw);
            assert_eq!(normalized.agent, agent, "agent name mismatch for {}", agent);
            assert_eq!(
                normalized.event_name, "test-event",
                "event_name mismatch for {}",
                agent
            );
            // All agents should produce a non-empty raw_payload
            assert!(
                !normalized.raw_payload.is_null(),
                "raw_payload should not be null for {}",
                agent
            );
        }
    }

    // ── Agent-specific field format tests ────────────────────────────

    #[test]
    fn test_normalize_gemini_function_call() {
        let raw = json!({
            "functionCall": {
                "name": "run_command",
                "args": {"command": "npm test"}
            },
            "sessionId": "gem-fc-1",
            "workingDir": "/home/user/project"
        });

        let normalized = normalize_generic_payload("gemini", "function-call", &raw);

        assert_eq!(normalized.agent, "gemini");
        assert_eq!(normalized.tool_name, Some("run_command".to_string()));
        assert!(normalized.tool_input.is_some());
        assert_eq!(normalized.session_id, Some("gem-fc-1".to_string()));
        assert_eq!(normalized.cwd, Some("/home/user/project".to_string()));
    }

    #[test]
    fn test_normalize_gemini_function_response() {
        let raw = json!({
            "functionResponse": {
                "response": "All 24 tests passed successfully"
            },
            "sessionId": "gem-fr-1"
        });

        let normalized = normalize_generic_payload("gemini", "function-response", &raw);

        assert_eq!(
            normalized.tool_response_text,
            Some("All 24 tests passed successfully".to_string())
        );
        assert_eq!(normalized.session_id, Some("gem-fr-1".to_string()));
    }

    #[test]
    fn test_normalize_codex_openai_function_format() {
        let raw = json!({
            "function": {
                "name": "shell",
                "arguments": "{\"command\": \"go build\"}"
            },
            "sessionId": "cx-fn-1",
            "turnId": "cx-turn-1",
            "workingDirectory": "/repo"
        });

        let normalized = normalize_generic_payload("codex", "tool-call", &raw);

        assert_eq!(normalized.tool_name, Some("shell".to_string()));
        assert!(normalized.tool_input.is_some());
        assert_eq!(normalized.session_id, Some("cx-fn-1".to_string()));
        assert_eq!(normalized.turn_id, Some("cx-turn-1".to_string()));
        assert_eq!(normalized.cwd, Some("/repo".to_string()));
    }

    #[test]
    fn test_normalize_codex_choices_message_content() {
        let raw = json!({
            "choices": [
                {
                    "message": {
                        "content": "I've implemented the feature"
                    }
                }
            ]
        });

        let normalized = normalize_generic_payload("codex", "response", &raw);

        assert_eq!(
            normalized.assistant_message_text,
            Some("I've implemented the feature".to_string())
        );
    }

    #[test]
    fn test_normalize_amp_function_format() {
        let raw = json!({
            "function": {
                "name": "edit_file",
                "arguments": "{\"path\": \"src/main.rs\", \"content\": \"...\"}"
            },
            "sessionKey": "amp-fn-1"
        });

        let normalized = normalize_generic_payload("amp", "function-call", &raw);

        assert_eq!(normalized.tool_name, Some("edit_file".to_string()));
        assert!(normalized.tool_input.is_some());
        assert_eq!(normalized.session_id, Some("amp-fn-1".to_string()));
    }
}
