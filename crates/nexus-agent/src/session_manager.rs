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
    /// Create a new session manager.
    pub fn new(project_root: &Path) -> Self {
        Self {
            nexus_dir: project_root.join(".nexus"),
        }
    }

    /// Start a new agent session and create a scratch file.
    pub fn start_session(&self, session_id: &str, agent_type: &str) -> io::Result<PathBuf> {
        let sessions_dir = self.nexus_dir.join("sessions");
        fs::create_dir_all(&sessions_dir)?;

        let scratch_path = sessions_dir.join(format!("{}.md", session_id));
        let mut file = fs::File::create(&scratch_path)?;

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
    pub fn merge_session(
        &self,
        session_id: &str,
        hot_cache: &mut HotCache,
        max_entries: usize,
    ) -> Result<usize, AgentError> {
        let sessions_dir = self.nexus_dir.join("sessions");
        let scratch_path = sessions_dir.join(format!("{}.md", session_id));

        if !scratch_path.exists() {
            return Ok(0);
        }

        let content = fs::read_to_string(&scratch_path).map_err(AgentError::Io)?;

        let learnings = parse_scratch_learnings(&content);
        let count = learnings.len();

        for learning in learnings {
            promote_to_hot_cache(hot_cache, learning, max_entries);
        }

        // Mark as merged
        let merged_path = sessions_dir.join(format!("{}.merged.md", session_id));
        fs::rename(&scratch_path, &merged_path).map_err(AgentError::Io)?;

        Ok(count)
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
            if path.extension().is_some_and(|ext| ext == "md")
                && path.to_string_lossy().contains(".merged.")
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
    let mut learnings = Vec::new();
    let re = Regex::new(r"- \[confidence: ([\d.]+)\] (.*)").expect("valid regex");

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
pub fn promote_to_hot_cache(hot: &mut HotCache, learning: ScratchLearning, max_entries: usize) {
    let entry = HotCacheEntry {
        memory_id: Utc::now().timestamp_nanos_opt().unwrap_or(0), // Temporary ID for session learnings
        content: learning.content,
        relevance_score: learning.confidence,
        tier: ConfidenceTier::from_score(learning.confidence),
        promoted_at: Utc::now(),
        last_surfaced: Utc::now(),
        hot_streak: 1,
        pinned: false,
        source_agent: None,
    };
    hot.promote(entry, max_entries);
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

        // 4. Verification
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
