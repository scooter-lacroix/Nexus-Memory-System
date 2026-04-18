//! Transcript streamer — reads Claude Code JSONL transcripts and formats
//! them for Nexus ingest.
//!
//! Ported from `letta-ai/claude-subconscious/scripts/transcript_utils.ts`.

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Maximum bytes per line — prevents OOM from malformed transcripts.
const MAX_LINE_BYTES: usize = 10 * 1024 * 1024; // 10 MB

/// Maximum bytes for tool result content stored in memory.
const MAX_TOOL_RESULT_CONTENT: usize = 2000;

/// A parsed entry from a Claude Code JSONL transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    /// Role: "user", "assistant", "system"
    pub role: String,
    /// Extracted text content
    pub text: String,
    /// Thinking content (if any)
    pub thinking: Option<String>,
    /// Tool calls made in this entry
    pub tool_calls: Vec<ToolCall>,
    /// Tool results received in this entry
    pub tool_results: Vec<ToolResult>,
    /// Original message type from JSONL
    pub message_type: String,
    /// Timestamp
    pub timestamp: Option<DateTime<Utc>>,
    /// Position index in the transcript (for sync state tracking)
    pub index: usize,
}

/// Truncate an owned string to a maximum byte length, respecting UTF-8 boundaries.
fn truncate_owned(s: String, max_len: usize) -> String {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// Formatted entry ready for Nexus ingest-hook-event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestEntry {
    pub role: String,
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
}

/// Read a Claude Code JSONL transcript file and parse entries.
pub fn read_transcript(path: &Path) -> io::Result<Vec<TranscriptEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                debug!("Failed to read line {index}: {e}");
                continue;
            }
        };

        if line.len() > MAX_LINE_BYTES {
            debug!("Skipping oversized line {index}: {} bytes", line.len());
            continue;
        }

        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(msg) => {
                if let Some(entry) = parse_transcript_message(&msg, index) {
                    entries.push(entry);
                }
            }
            Err(e) => {
                debug!("Failed to parse line {index}: {e}");
            }
        }
    }

    Ok(entries)
}

/// Read only entries from `start_index` onward (for incremental sync).
pub fn read_transcript_from(path: &Path, start_index: usize) -> io::Result<Vec<TranscriptEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    if start_index == 0 {
        return read_transcript(path);
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        if index <= start_index {
            continue;
        }
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                debug!("Failed to read line {index}: {e}");
                continue;
            }
        };

        if line.len() > MAX_LINE_BYTES {
            debug!("Skipping oversized line {index}: {} bytes", line.len());
            continue;
        }

        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(msg) => {
                if let Some(entry) = parse_transcript_message(&msg, index) {
                    entries.push(entry);
                }
            }
            Err(e) => {
                debug!("Failed to parse line {index}: {e}");
            }
        }
    }

    Ok(entries)
}

/// Format transcript entries for Nexus ingest.
/// Produces a single concatenated string suitable for `nexus ingest-hook-event`.
pub fn format_for_ingest(
    entries: &[TranscriptEntry],
    max_chars_per_entry: usize,
) -> Vec<IngestEntry> {
    let mut formatted = Vec::new();
    let mut tool_name_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for entry in entries {
        // Skip internal message types
        if entry.message_type == "file-history-snapshot" || entry.message_type == "system" {
            continue;
        }

        // Handle summary messages
        if entry.message_type == "summary" && !entry.text.is_empty() {
            formatted.push(IngestEntry {
                role: "system".to_string(),
                content: format!(
                    "[Session Summary]: {}",
                    truncate(&entry.text, max_chars_per_entry)
                ),
                timestamp: entry.timestamp,
            });
            continue;
        }

        let mut parts = Vec::new();

        // Add thinking (summarized)
        if let Some(ref thinking) = entry.thinking {
            parts.push(format!("[Thinking]: {}", truncate(thinking, 300)));
        }

        // Add tool calls
        for tc in &entry.tool_calls {
            tool_name_map.insert(tc.id.clone(), tc.name.clone());
            parts.push(format!(
                "[Tool: {}] {}",
                tc.name,
                truncate(&tc.input_summary, 150)
            ));
        }

        // Add tool results
        for tr in &entry.tool_results {
            let tool_name = tool_name_map
                .get(&tr.tool_use_id)
                .cloned()
                .unwrap_or_else(|| tr.tool_use_id.clone());
            let prefix = if tr.is_error {
                "[Tool Error"
            } else {
                "[Tool Result"
            };
            let truncated = truncate(&tr.content, 1000);
            parts.push(format!("{prefix}: {tool_name}]\n{truncated}"));
        }

        // Add main text
        if !entry.text.is_empty() {
            parts.push(truncate(&entry.text, max_chars_per_entry).to_string());
        }

        if !parts.is_empty() {
            formatted.push(IngestEntry {
                role: entry.role.clone(),
                content: parts.join("\n"),
                timestamp: entry.timestamp,
            });
        }
    }

    formatted
}

/// Build a JSON payload for `nexus ingest-hook-event` from transcript entries.
pub fn build_ingest_payload(
    entries: &[IngestEntry],
    agent: &str,
    session_id: &str,
    cwd: &str,
) -> serde_json::Value {
    let mut parts = Vec::new();
    for entry in entries {
        parts.push(format!("[{}]: {}", entry.role, entry.content));
    }

    serde_json::json!({
        "transcript": {
            "entries": entries.iter().map(|e| serde_json::json!({
                "role": e.role,
                "content": e.content,
                "timestamp": e.timestamp.map(|t| t.to_rfc3339()),
            })).collect::<Vec<_>>(),
            "session_id": session_id,
            "agent": agent,
            "cwd": cwd,
        },
        "content": parts.join("\n\n"),
        "session_id": session_id,
        "cwd": cwd,
    })
}

/// Parse a single JSONL message into a TranscriptEntry.
fn parse_transcript_message(msg: &serde_json::Value, index: usize) -> Option<TranscriptEntry> {
    let msg_type = msg.get("type")?.as_str()?;

    match msg_type {
        "summary" => {
            let summary = msg
                .get("summary")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            Some(TranscriptEntry {
                role: "system".to_string(),
                text: summary,
                thinking: None,
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                message_type: "summary".to_string(),
                timestamp: parse_timestamp(msg.get("timestamp")),
                index,
            })
        }
        "user" => {
            let (text, tool_results) = extract_content(msg);
            Some(TranscriptEntry {
                role: "user".to_string(),
                text,
                thinking: None,
                tool_calls: Vec::new(),
                tool_results,
                message_type: "user".to_string(),
                timestamp: parse_timestamp(msg.get("timestamp")),
                index,
            })
        }
        "assistant" => {
            let (text, thinking, tool_calls) = extract_assistant_content(msg);
            Some(TranscriptEntry {
                role: "assistant".to_string(),
                text,
                thinking,
                tool_calls,
                tool_results: Vec::new(),
                message_type: "assistant".to_string(),
                timestamp: parse_timestamp(msg.get("timestamp")),
                index,
            })
        }
        "system" | "file-history-snapshot" => {
            // Include but will be filtered during formatting
            Some(TranscriptEntry {
                role: "system".to_string(),
                text: String::new(),
                thinking: None,
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
                message_type: msg_type.to_string(),
                timestamp: parse_timestamp(msg.get("timestamp")),
                index,
            })
        }
        _ => None,
    }
}

/// Extract text content and tool results from a user message.
fn extract_content(msg: &serde_json::Value) -> (String, Vec<ToolResult>) {
    let content = msg
        .get("message")
        .and_then(|m| m.get("content"))
        .or_else(|| msg.get("content"));

    match content {
        Some(serde_json::Value::String(s)) => (s.clone(), Vec::new()),
        Some(serde_json::Value::Array(blocks)) => {
            let mut text_parts = Vec::new();
            let mut results = Vec::new();

            for block in blocks {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(text.to_string());
                        }
                    }
                    "tool_result" => {
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let is_error = block
                            .get("is_error")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let content_str = block
                            .get("content")
                            .map(|c| {
                                c.as_str()
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| serde_json::to_string(c).unwrap_or_default())
                            })
                            .unwrap_or_default();
                        let content_str = truncate_owned(content_str, MAX_TOOL_RESULT_CONTENT);
                        results.push(ToolResult {
                            tool_use_id,
                            content: content_str,
                            is_error,
                        });
                    }
                    _ => {}
                }
            }

            (text_parts.join("\n"), results)
        }
        _ => (String::new(), Vec::new()),
    }
}

/// Extract text, thinking, and tool calls from an assistant message.
/// Tool results only appear in user messages.
fn extract_assistant_content(msg: &serde_json::Value) -> (String, Option<String>, Vec<ToolCall>) {
    let content = msg
        .get("message")
        .and_then(|m| m.get("content"))
        .or_else(|| msg.get("content"));

    match content {
        Some(serde_json::Value::String(s)) => (s.clone(), None, Vec::new()),
        Some(serde_json::Value::Array(blocks)) => {
            let mut text_parts = Vec::new();
            let mut thinking_parts = Vec::new();
            let mut tool_calls = Vec::new();

            for block in blocks {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(text.to_string());
                        }
                    }
                    "thinking" => {
                        if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                            thinking_parts.push(thinking.to_string());
                        }
                    }
                    "tool_use" => {
                        let id = block
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let input_summary = summarize_tool_input(
                            &name,
                            block.get("input").unwrap_or(&serde_json::Value::Null),
                        );
                        tool_calls.push(ToolCall {
                            id,
                            name,
                            input_summary,
                        });
                    }
                    _ => {}
                }
            }

            let thinking = if thinking_parts.is_empty() {
                None
            } else {
                Some(thinking_parts.join("\n"))
            };

            (text_parts.join("\n"), thinking, tool_calls)
        }
        _ => (String::new(), None, Vec::new()),
    }
}

/// Produce a concise summary of a tool's input, mirroring Letta's formatToolInput.
fn summarize_tool_input(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Read" | "Edit" | "Write" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "Bash" => input
            .get("command")
            .and_then(|c| c.as_str())
            .map(|s| truncate(s, 100).to_string())
            .unwrap_or_default(),
        "Glob" => input
            .get("pattern")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "Grep" => input
            .get("pattern")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "WebFetch" => input
            .get("url")
            .and_then(|u| u.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "WebSearch" => input
            .get("query")
            .and_then(|q| q.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "Task" => input
            .get("description")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        _ => {
            let s = serde_json::to_string(input).unwrap_or_default();
            truncate(&s, 100).to_string()
        }
    }
}

fn parse_timestamp(val: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    val.and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

/// Truncate text to a maximum length.
fn truncate(text: &str, max_len: usize) -> &str {
    if text.len() <= max_len {
        return text;
    }
    // Find a safe UTF-8 boundary
    let mut end = max_len;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn read_empty_transcript() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("transcript.jsonl");
        File::create(&path).unwrap();
        let entries = read_transcript(&path).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn read_nonexistent_transcript() {
        let entries = read_transcript(Path::new("/no/such/file.jsonl")).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_user_message() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"Hello world"}}}}"#
        )
        .unwrap();

        let entries = read_transcript(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, "user");
        assert_eq!(entries[0].text, "Hello world");
        assert_eq!(entries[0].index, 0);
    }

    #[test]
    fn parse_assistant_with_tool_use() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"Let me read that file"}},{{"type":"tool_use","name":"Read","id":"call_123","input":{{"file_path":"/src/main.rs"}}}}]}}}}"#
        )
        .unwrap();

        let entries = read_transcript(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, "assistant");
        assert_eq!(entries[0].text, "Let me read that file");
        assert_eq!(entries[0].tool_calls.len(), 1);
        assert_eq!(entries[0].tool_calls[0].name, "Read");
        assert_eq!(entries[0].tool_calls[0].id, "call_123");
        assert_eq!(entries[0].tool_calls[0].input_summary, "/src/main.rs");
    }

    #[test]
    fn skip_system_and_file_history() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(file, r#"{{"type":"system","subtype":"init"}}"#).unwrap();
        writeln!(
            file,
            r#"{{"type":"file-history-snapshot","snapshot":{{}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"user","message":{{"role":"user","content":"real content"}}}}"#
        )
        .unwrap();

        let entries = read_transcript(&path).unwrap();
        assert_eq!(entries.len(), 3);

        // Formatting should skip system/file-history
        let ingest = format_for_ingest(&entries, 500);
        assert_eq!(ingest.len(), 1);
        assert_eq!(ingest[0].role, "user");
    }

    #[test]
    fn read_from_index() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let mut file = File::create(&path).unwrap();
        for i in 0..5 {
            writeln!(
                file,
                r#"{{"type":"user","message":{{"role":"user","content":"msg {i}"}}}}"#
            )
            .unwrap();
        }

        let entries = read_transcript_from(&path, 2).unwrap();
        assert_eq!(entries.len(), 2); // indices 3, 4 (filter > 2)
        assert!(entries[0].index > 2);
    }

    #[test]
    fn format_for_ingest_truncates() {
        let entry = TranscriptEntry {
            role: "user".to_string(),
            text: "a".repeat(1000),
            thinking: None,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            message_type: "user".to_string(),
            timestamp: None,
            index: 0,
        };
        let result = format_for_ingest(&[entry], 50);
        assert_eq!(result.len(), 1);
        assert!(result[0].content.len() < 100);
    }

    #[test]
    fn summarize_tool_input_known_tools() {
        assert_eq!(
            summarize_tool_input("Read", &serde_json::json!({"file_path": "/src/main.rs"})),
            "/src/main.rs"
        );
        assert_eq!(
            summarize_tool_input("Bash", &serde_json::json!({"command": "cargo build"})),
            "cargo build"
        );
    }

    #[test]
    fn build_ingest_payload_structure() {
        let entries = vec![IngestEntry {
            role: "user".to_string(),
            content: "hello".to_string(),
            timestamp: None,
        }];
        let payload = build_ingest_payload(&entries, "claude-code", "sess-1", "/home/user");
        assert_eq!(payload["session_id"], "sess-1");
        assert_eq!(payload["cwd"], "/home/user");
    }

    #[test]
    fn truncate_handles_multibyte() {
        let text = "héllo wörld";
        let result = truncate(text, 5);
        assert!(result.len() <= 5);
    }
}
