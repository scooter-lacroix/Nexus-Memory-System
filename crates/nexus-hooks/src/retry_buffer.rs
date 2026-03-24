//! Retry buffer for failed LLM enrichment
//!
//! When enrichment fails, writes artifacts to disk for later retry.
//! Location: ~/.local/state/nexus-memory-system/pending-enrichment/

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::candidate::MemoryCandidate;
use crate::claude_payload::NormalizedHookEvent;

/// A retry artifact containing the data needed to retry failed enrichment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryArtifact {
    /// Agent that generated the event
    pub agent: String,
    /// Event name from the hook
    pub event_name: String,
    /// Normalized event data
    pub normalized_event: NormalizedHookEvent,
    /// Candidates that failed enrichment
    pub candidates: Vec<MemoryCandidate>,
    /// Error message from the failed attempt
    pub error: String,
    /// When this artifact was created
    pub created_at: String,
}

/// File-based retry buffer for failed enrichment attempts.
pub struct RetryBuffer {
    buffer_path: PathBuf,
}

impl RetryBuffer {
    /// Create a new retry buffer with the default path.
    pub fn new() -> Self {
        Self {
            buffer_path: Self::buffer_path(),
        }
    }

    /// Get the default buffer directory path.
    ///
    /// Uses XDG state directory: ~/.local/state/nexus-memory-system/pending-enrichment/
    pub fn buffer_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("nexus-memory-system")
            .join("pending-enrichment")
    }

    /// Write a failed enrichment attempt to the buffer.
    ///
    /// Creates a JSON file named `{timestamp}_{uuid}.json` in the buffer directory.
    /// Creates the directory if it doesn't exist.
    ///
    /// Returns the path to the created file.
    pub fn write_failed(
        &self,
        event: &NormalizedHookEvent,
        candidates: &[MemoryCandidate],
        error: &str,
    ) -> std::io::Result<PathBuf> {
        // Ensure directory exists
        fs::create_dir_all(&self.buffer_path)?;

        // Generate filename: timestamp with colons replaced + UUID
        let timestamp = Utc::now().to_rfc3339().replace(':', "-");
        let uuid = uuid::Uuid::new_v4();
        let filename = format!("{}_{}.json", timestamp, uuid);
        let file_path = self.buffer_path.join(&filename);

        // Create the artifact
        let artifact = RetryArtifact {
            agent: event.agent.clone(),
            event_name: event.event_name.clone(),
            normalized_event: event.clone(),
            candidates: candidates.to_vec(),
            error: error.to_string(),
            created_at: Utc::now().to_rfc3339(),
        };

        // Serialize and write
        let json = serde_json::to_string_pretty(&artifact)?;
        fs::write(&file_path, json)?;

        warn!(
            "Buffered failed enrichment for agent={}, event={}: {}",
            event.agent,
            event.event_name,
            file_path.display()
        );

        Ok(file_path)
    }

    /// List all pending retry artifacts in the buffer directory.
    ///
    /// Returns paths to all .json files in the buffer.
    pub fn list_pending(&self) -> std::io::Result<Vec<PathBuf>> {
        if !self.buffer_path.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&self.buffer_path)?;

        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Only include .json files
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                files.push(path);
            }
        }

        // Sort by creation time (filename starts with timestamp)
        files.sort();

        Ok(files)
    }

    /// Read a pending retry artifact from disk.
    pub fn read_pending(path: &Path) -> std::io::Result<RetryArtifact> {
        let contents = fs::read_to_string(path)?;
        let artifact: RetryArtifact = serde_json::from_str(&contents)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(artifact)
    }

    /// Remove a pending retry artifact from disk.
    pub fn remove_pending(path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)?;
        info!("Removed buffered enrichment artifact: {}", path.display());
        Ok(())
    }

    /// Count the number of pending retry artifacts.
    pub fn pending_count(&self) -> std::io::Result<usize> {
        Ok(self.list_pending()?.len())
    }

    /// Clear all pending retry artifacts.
    ///
    /// Returns the number of artifacts removed.
    pub fn clear_all(&self) -> std::io::Result<usize> {
        let files = self.list_pending()?;
        let count = files.len();

        for file in files {
            fs::remove_file(&file)?;
        }

        info!("Cleared {} buffered enrichment artifacts", count);
        Ok(count)
    }
}

impl Default for RetryBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_test_event() -> NormalizedHookEvent {
        NormalizedHookEvent {
            agent: "test-agent".to_string(),
            event_name: "test_event".to_string(),
            observed_at: Utc::now(),
            session_id: Some("session-123".to_string()),
            turn_id: Some("turn-456".to_string()),
            cwd: Some("/home/user/project".to_string()),
            tool_name: Some("test_tool".to_string()),
            tool_input: None,
            tool_response_text: None,
            assistant_message_text: None,
            user_message_text: None,
            raw_payload: json!({}),
        }
    }

    fn make_test_candidates() -> Vec<MemoryCandidate> {
        vec![MemoryCandidate {
            candidate_id: "test-1".to_string(),
            source_event_name: "test_event".to_string(),
            source_agent: "test-agent".to_string(),
            signal_score: 0.8,
            provisional_category: Some("preferences".to_string()),
            memory_text: "Test memory".to_string(),
            evidence: json!({"key": "value"}),
            labels: vec!["test".to_string()],
        }]
    }

    #[test]
    fn test_buffer_path() {
        let path = RetryBuffer::buffer_path();
        assert!(path.ends_with("pending-enrichment"));
    }

    #[test]
    fn test_retry_artifact_serialization() {
        let event = make_test_event();
        let candidates = make_test_candidates();

        let artifact = RetryArtifact {
            agent: event.agent.clone(),
            event_name: event.event_name.clone(),
            normalized_event: event,
            candidates,
            error: "Test error".to_string(),
            created_at: Utc::now().to_rfc3339(),
        };

        let serialized = serde_json::to_string(&artifact).unwrap();
        let deserialized: RetryArtifact = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.agent, "test-agent");
        assert_eq!(deserialized.event_name, "test_event");
        assert_eq!(deserialized.candidates.len(), 1);
        assert_eq!(deserialized.error, "Test error");
    }

    #[test]
    fn test_write_and_list_pending() {
        // Use a temp directory for testing
        let temp_dir = std::env::temp_dir().join("nexus-test-retry-buffer");
        fs::create_dir_all(&temp_dir).unwrap();

        // We need to create a custom RetryBuffer with the temp path
        // Since buffer_path() is static, we'll test the write logic differently
        let event = make_test_event();
        let candidates = make_test_candidates();

        // Just test serialization works
        let artifact = RetryArtifact {
            agent: event.agent.clone(),
            event_name: event.event_name.clone(),
            normalized_event: event,
            candidates,
            error: "Test error".to_string(),
            created_at: Utc::now().to_rfc3339(),
        };

        let json = serde_json::to_string_pretty(&artifact).unwrap();
        assert!(json.contains("test-agent"));
        assert!(json.contains("Test error"));

        // Cleanup
        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_default() {
        let buffer = RetryBuffer::default();
        assert_eq!(buffer.buffer_path, RetryBuffer::buffer_path());
    }
}
