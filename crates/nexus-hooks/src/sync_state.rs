//! Sync state tracking for incremental subconscious retrieval.
//!
//! Tracks the last-processed position per session so retrieval hooks only
//! surface new information.  Persisted as JSON in `.nexus/sessions/{id}/sync_state.json`.

use nexus_core::fsutil::atomic_write;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Incremental sync position for a single session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    /// Session identifier (matches `.nexus/sessions/{session_id}/`).
    pub session_id: String,
    /// Index of the last-processed transcript entry (None = never synced).
    pub last_processed_index: Option<usize>,
    /// Hash of the last soul.md content seen (detects dream/soul updates).
    pub last_soul_hash: String,
    /// Timestamp of the last successful sync.
    pub last_sync_timestamp: DateTime<Utc>,
    /// Number of hot-cache entries at last sync (detects cache promotions).
    pub last_hot_cache_count: usize,
}

impl SyncState {
    /// Load sync state for a session, creating a fresh one if none exists.
    pub fn load(project_root: &Path, session_id: &str) -> io::Result<Self> {
        let path = sync_state_path(project_root, session_id)?;
        if path.exists() {
            let data = fs::read_to_string(&path)?;
            let mut state: SyncState = serde_json::from_str(&data)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if state.session_id != session_id {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "sync_state session_id does not match requested session",
                ));
            }
            // Use canonical session_id from caller
            state.session_id = session_id.to_string();
            Ok(state)
        } else {
            Ok(Self::new(session_id))
        }
    }

    /// Persist the current sync state to disk.
    pub fn save(&self, project_root: &Path) -> io::Result<()> {
        let path = sync_state_path(project_root, &self.session_id)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        atomic_write(&path, &data)
    }

    /// Create a fresh sync state for a new session.
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            last_processed_index: None,
            last_soul_hash: String::new(),
            last_sync_timestamp: Utc::now(),
            last_hot_cache_count: 0,
        }
    }

    /// Whether there are updates since the last sync (soul changed or cache grew).
    pub fn has_updates(&self, current_soul_hash: &str, current_hot_count: usize) -> bool {
        current_soul_hash != self.last_soul_hash || current_hot_count > self.last_hot_cache_count
    }

    /// Record a successful sync, advancing all watermarks.
    pub fn advance(&mut self, soul_hash: String, hot_cache_count: usize, new_index: Option<usize>) {
        self.last_soul_hash = soul_hash;
        self.last_hot_cache_count = hot_cache_count;
        if let Some(idx) = new_index {
            self.last_processed_index = Some(idx);
        }
        self.last_sync_timestamp = Utc::now();
    }
}

/// Compute the path to a session's sync state file.
fn sync_state_path(project_root: &Path, session_id: &str) -> io::Result<PathBuf> {
    // Validate session_id to prevent path traversal
    if session_id.is_empty()
        || session_id.len() > 128
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session_id contains invalid characters",
        ));
    }
    Ok(project_root
        .join(".nexus")
        .join("sessions")
        .join(session_id)
        .join("sync_state.json"))
}

/// Compute a quick hash of soul.md content for change detection.
/// Uses a simple rolling hash (FxHash-style) instead of pulling in a full
/// crypto hash — we only need to detect *change*, not verify integrity.
pub fn soul_content_hash(content: &str) -> String {
    // FxHash-like: multiply by a large odd constant and XOR with each byte group.
    let mut hash: u64 = 0;
    for chunk in content.as_bytes().chunks(8) {
        let mut buf = [0u8; 8];
        buf[..chunk.len()].copy_from_slice(chunk);
        let val = u64::from_le_bytes(buf);
        hash = hash.wrapping_mul(0x517cc1b727220a95).wrapping_add(val);
    }
    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_state_has_defaults() {
        let state = SyncState::new("test-session");
        assert_eq!(state.session_id, "test-session");
        assert_eq!(state.last_processed_index, None);
        assert!(state.last_soul_hash.is_empty());
        assert_eq!(state.last_hot_cache_count, 0);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();

        let mut state = SyncState::new("roundtrip-test");
        state.last_soul_hash = "abc123".to_string();
        state.last_hot_cache_count = 5;
        state.last_processed_index = Some(42);
        state.save(project_root).unwrap();

        let loaded = SyncState::load(project_root, "roundtrip-test").unwrap();
        assert_eq!(loaded.session_id, "roundtrip-test");
        assert_eq!(loaded.last_soul_hash, "abc123");
        assert_eq!(loaded.last_hot_cache_count, 5);
        assert_eq!(loaded.last_processed_index, Some(42));
    }

    #[test]
    fn load_nonexistent_creates_fresh() {
        let dir = TempDir::new().unwrap();
        let state = SyncState::load(dir.path(), "no-such-session").unwrap();
        assert_eq!(state.session_id, "no-such-session");
        assert_eq!(state.last_processed_index, None);
    }

    #[test]
    fn has_updates_detects_soul_change() {
        let state = SyncState::new("test");
        assert!(state.has_updates("different", 0));
        assert!(!state.has_updates("", 0));
    }

    #[test]
    fn has_updates_detects_cache_growth() {
        let state = SyncState::new("test");
        assert!(state.has_updates("", 3));
    }

    #[test]
    fn advance_updates_watermarks() {
        let mut state = SyncState::new("test");
        state.advance("newhash".to_string(), 7, Some(15));
        assert_eq!(state.last_soul_hash, "newhash");
        assert_eq!(state.last_hot_cache_count, 7);
        assert_eq!(state.last_processed_index, Some(15));
    }

    #[test]
    fn soul_hash_deterministic() {
        let h1 = soul_content_hash("hello world");
        let h2 = soul_content_hash("hello world");
        let h3 = soul_content_hash("hello earth");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }
}
