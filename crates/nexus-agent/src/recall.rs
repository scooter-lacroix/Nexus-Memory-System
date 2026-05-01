//! Recall service - provides read operations for memory retrieval tools.

use crate::error::AgentError;
use nexus_core::{CognitiveLevel, Memory, PerspectiveKey};
use nexus_storage::{MemoryLineageEntry, MemoryRepository};

/// Service for recall operations used by MCP tools.
pub struct RecallToolService;

impl RecallToolService {
    /// Create a new RecallToolService.
    pub fn new() -> Self {
        Self
    }

    /// Search memories by perspective, optionally filtered by cognitive level.
    pub async fn search_memory(
        &self,
        repo: &MemoryRepository,
        namespace_id: i64,
        perspective: &PerspectiveKey,
        cognitive_level: Option<CognitiveLevel>,
        limit: i64,
    ) -> Result<Vec<Memory>, AgentError> {
        let memories = match cognitive_level {
            Some(level) => repo
                .get_by_cognitive_level_with_perspective(namespace_id, level, perspective, limit)
                .await
                .map_err(|e| AgentError::Storage(e.to_string()))?,
            None => repo
                .get_recent_by_perspective(namespace_id, perspective, limit)
                .await
                .map_err(|e| AgentError::Storage(e.to_string()))?,
        };
        Ok(memories)
    }

    /// Get session digests (short and long) for a given session.
    pub async fn get_session_digest(
        &self,
        repo: &MemoryRepository,
        namespace_id: i64,
        session_key: &str,
    ) -> Result<(Option<Memory>, Option<Memory>), AgentError> {
        let short = repo
            .latest_digest_for_session(namespace_id, session_key, "short")
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        let long = repo
            .latest_digest_for_session(namespace_id, session_key, "long")
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        Ok((short, long))
    }

    /// Get the lineage (evidence links) for a memory.
    pub async fn get_memory_lineage(
        &self,
        repo: &MemoryRepository,
        memory_id: i64,
    ) -> Result<Vec<MemoryLineageEntry>, AgentError> {
        let lineage = repo
            .load_lineage(memory_id)
            .await
            .map_err(|e| AgentError::Storage(e.to_string()))?;
        Ok(lineage)
    }
}

impl Default for RecallToolService {
    fn default() -> Self {
        Self::new()
    }
}
