//! Manages session scratch files and learning extraction.

use chrono::Utc;
use regex::Regex;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cognitive_cache::{ConfidenceTier, HotCache, HotCacheEntry};
use crate::error::AgentError;

/// Manages session-scoped scratch files for memory ingestion.
pub struct SessionManager {
    nexus_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ScratchLearning {
    pub content: String,
    pub confidence: f32,
}

impl SessionManager {
    /// Reject session IDs that could escape the sessions directory.
    fn validate_session_id(id: &str) -> io::Result<()> {
        if id.is_empty() || id.len() > 128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session_id must be 1-128 chars",
            ));
        }
        if id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session_id contains invalid characters",
            ));
        }
        Ok(())
    }

    /// Create a new session manager.
    pub fn new(project_root: &Path) -> Self {
        Self {
            nexus_dir: project_root.join(".nexus"),
        }
    }

    /// Start a new agent session and create a scratch file.
    pub fn start_session(&self, session_id: &str, agent_type: &str) -> io::Result<PathBuf> {
        Self::validate_session_id(session_id)?;

        let sessions_dir = self.nexus_dir.join("sessions");
        fs::create_dir_all(&sessions_dir)?;

        let scratch_path = sessions_dir.join(format!("{}.md", session_id));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&scratch_path)?;

        let header = format!(
            "---\nid: {}\nagent: {}\nstarted: {}\nstatus: active\n---\n\n# Session Learnings\n\n",
            session_id,
            agent_type,
            Utc::now().to_rfc3339()
        );
        file.write_all(header.as_bytes())?;

        Ok(scratch_path)
    }

    /// Append a learning entry to a session scratch file.
    pub fn append_learning(
        &self,
        session_id: &str,
        content: &str,
        confidence: f32,
    ) -> io::Result<()> {
        let confidence = if confidence.is_finite() {
            confidence.clamp(0.0, 1.0)
        } else {
            tracing::warn!("Non-finite confidence value in append_learning, defaulting to 0.5");
            0.5
        };

        Self::validate_session_id(session_id)?;

        let scratch_path = self
            .nexus_dir
            .join("sessions")
            .join(format!("{}.md", session_id));
        let mut file = fs::OpenOptions::new().append(true).open(scratch_path)?;

        let entry = format!(
            "- [confidence: {:.2}] {}\n",
            confidence,
            content.replace('\n', " ")
        );
        file.write_all(entry.as_bytes())?;

        Ok(())
    }

    /// Merge learnings from a scratch file into the hot cache.
    /// Does NOT rename the scratch file — caller must call mark_session_merged()
    /// after persisting the cache to avoid data loss on save failure.
    pub fn merge_session(
        &self,
        session_id: &str,
        hot_cache: &mut HotCache,
        max_entries: usize,
    ) -> Result<usize, AgentError> {
        Self::validate_session_id(session_id)?;

        let sessions_dir = self.nexus_dir.join("sessions");
        let scratch_path = sessions_dir.join(format!("{}.md", session_id));

        if !scratch_path.exists() {
            return Ok(0);
        }

        let content = fs::read_to_string(&scratch_path).map_err(AgentError::Io)?;

        let learnings = parse_scratch_learnings(&content);
        let mut inserted = 0;

        for learning in learnings {
            if promote_to_hot_cache(hot_cache, learning, max_entries) {
                inserted += 1;
            }
        }

        Ok(inserted)
    }

    /// Mark a session scratch file as merged (rename .md → .merged.md).
    /// Call this AFTER cache persistence succeeds to avoid data loss.
    pub fn mark_session_merged(&self, session_id: &str) -> Result<(), AgentError> {
        Self::validate_session_id(session_id)?;
        let sessions_dir = self.nexus_dir.join("sessions");
        let scratch_path = sessions_dir.join(format!("{}.md", session_id));
        if scratch_path.exists() {
            let merged_path = sessions_dir.join(format!("{}.merged.md", session_id));
            fs::rename(&scratch_path, &merged_path).map_err(AgentError::Io)?;
        }
        Ok(())
    }

    /// Clean up merged session files older than 7 days.
    pub fn cleanup_old_sessions(&self) -> io::Result<usize> {
        let sessions_dir = self.nexus_dir.join("sessions");
        if !sessions_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        let now = Utc::now();
        let week_ago = now - chrono::Duration::days(7);

        for entry in fs::read_dir(sessions_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".merged.md"))
            {
                let metadata = entry.metadata()?;
                let modified: chrono::DateTime<Utc> = metadata.modified()?.into();
                if modified < week_ago {
                    fs::remove_file(path)?;
                    count += 1;
                }
            }
        }

        Ok(count)
    }
}

/// Parse learnings from scratch file content.
pub fn parse_scratch_learnings(content: &str) -> Vec<ScratchLearning> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"- \[confidence: ([\d.]+)\] (.*)").expect("valid regex"));
    let mut learnings = Vec::new();

    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            if let Some(conf_str) = caps.get(1) {
                if let Ok(conf) = conf_str.as_str().parse::<f32>() {
                    if let Some(text_match) = caps.get(2) {
                        learnings.push(ScratchLearning {
                            content: text_match.as_str().to_string(),
                            confidence: conf,
                        });
                    }
                }
            }
        }
    }
    learnings
}

/// Promote a single learning to the hot cache.
/// Uses UUID-based negative IDs that survive process restarts without collision.
/// Returns true if the entry was inserted, false if the cache was full.
pub fn promote_to_hot_cache(
    hot: &mut HotCache,
    learning: ScratchLearning,
    max_entries: usize,
) -> bool {
    let entry = HotCacheEntry {
        memory_id: {
            let raw = (uuid::Uuid::new_v4().as_u128() & (i64::MAX as u128)) as i64;
            -(raw.max(1))
        },
        content: learning.content,
        relevance_score: learning.confidence,
        tier: ConfidenceTier::from_score(learning.confidence),
        promoted_at: Utc::now(),
        last_surfaced: Utc::now(),
        hot_streak: 1,
        pinned: false,
        source_agent: None,
    };
    hot.promote(entry, max_entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_session_lifecycle() {
        let dir = tempdir().unwrap();
        let manager = SessionManager::new(dir.path());
        let session_id = "test-session";

        // 1. Start
        let path = manager.start_session(session_id, "claude-code").unwrap();
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("agent: claude-code"));

        // 2. Append
        manager
            .append_learning(session_id, "Found a pattern", 0.9)
            .unwrap();
        manager
            .append_learning(session_id, "Another insight", 0.75)
            .unwrap();

        // 3. Merge
        let mut hot = HotCache::default();
        let count = manager.merge_session(session_id, &mut hot, 10).unwrap();
        assert_eq!(count, 2);
        assert_eq!(hot.entries.len(), 2);
        assert!(hot.entries.iter().any(|e| e.content == "Found a pattern"));

        // 4. Mark merged (normally done after cache save)
        manager.mark_session_merged(session_id).unwrap();

        // 5. Verification
        assert!(!path.exists()); // Original scratch should be gone
        let merged_path = dir
            .path()
            .join(".nexus/sessions")
            .join(format!("{}.merged.md", session_id));
        assert!(merged_path.exists());
    }
    #[test]
    fn test_parse_scratch_learnings() {
        let content = r#"---
header: ignored
---
- [confidence: 0.95] Valid entry 1
- [confidence: 0.50] Valid entry 2
- malformed entry
- [confidence: invalid] entry 3
"#;
        let learnings = parse_scratch_learnings(content);
        assert_eq!(learnings.len(), 2);
        assert_eq!(learnings[0].content, "Valid entry 1");
        assert_eq!(learnings[0].confidence, 0.95);
    }

    #[test]
    fn test_concurrent_sessions() {
        let dir = tempdir().unwrap();
        let manager = SessionManager::new(dir.path());

        manager.start_session("s1", "a1").unwrap();
        manager.start_session("s2", "a2").unwrap();

        manager.append_learning("s1", "l1", 0.9).unwrap();
        manager.append_learning("s2", "l2", 0.8).unwrap();

        let mut hot = HotCache::default();
        manager.merge_session("s1", &mut hot, 10).unwrap();
        assert_eq!(hot.entries.len(), 1);
        assert_eq!(hot.entries[0].content, "l1");

        manager.merge_session("s2", &mut hot, 10).unwrap();
        assert_eq!(hot.entries.len(), 2);
        assert!(hot.entries.iter().any(|e| e.content == "l2"));
    }
}
