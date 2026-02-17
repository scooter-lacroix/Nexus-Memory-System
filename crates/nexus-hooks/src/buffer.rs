//! Persistent buffer for crash recovery
//!
//! The persistent buffer acts as a safety net to prevent memory loss
//! even if all hooks fail. It continuously buffers session context
//! for recovery after crashes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

use crate::error::{HookError, Result};
use crate::session::SessionContext;

/// Default buffer directory
pub fn default_buffer_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nexus")
        .join("buffer")
}

/// Buffer entry stored in memory and on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferEntry {
    /// Timestamp when entry was created
    pub timestamp: DateTime<Utc>,

    /// Type of context
    pub context_type: String,

    /// The context data
    pub context: SessionContext,
}

/// Buffer data structure stored on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferData {
    /// When buffering started
    pub started_at: DateTime<Utc>,

    /// Buffer entries
    pub entries: Vec<BufferEntry>,

    /// When last flushed to disk
    pub last_flush: Option<DateTime<Utc>>,

    /// Agent type
    pub agent_type: String,
}

impl BufferData {
    pub fn new(agent_type: impl Into<String>) -> Self {
        Self {
            started_at: Utc::now(),
            entries: Vec::new(),
            last_flush: None,
            agent_type: agent_type.into(),
        }
    }
}

/// Persistent buffer for crash recovery
///
/// This buffer continuously stores session context entries. If all other
/// detection methods fail, we can recover from this buffer.
///
/// # Buffer Lifecycle
///
/// 1. Start buffering when session starts
/// 2. Continuously append context entries
/// 3. Periodically flush to disk
/// 4. Recover after crash
/// 5. Clear buffer after successful storage
///
/// # Example
///
/// ```rust,no_run
/// use nexus_hooks::buffer::PersistentBuffer;
/// use nexus_hooks::session::SessionContext;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let mut buffer = PersistentBuffer::new(None)?;
///
///     // Start buffering for an agent
///     buffer.start_buffering("claude-code").await?;
///
///     // Buffer context periodically
///     let ctx = SessionContext::new("claude-code");
///     buffer.buffer_context("claude-code", ctx, "checkpoint").await?;
///
///     // Recover after crash
///     if let Some(data) = buffer.recover_buffer("claude-code").await? {
///         println!("Recovered {} entries", data.entries.len());
///     }
///
///     // Clear after successful storage
///     buffer.clear_buffer("claude-code").await?;
///
///     Ok(())
/// }
/// ```
pub struct PersistentBuffer {
    /// Buffer directory
    buffer_dir: PathBuf,

    /// In-memory buffers by agent type
    buffers: Arc<RwLock<HashMap<String, BufferData>>>,

    /// Flush interval in seconds
    flush_interval_secs: u64,

    /// Maximum entries before auto-flush
    max_entries: usize,
}

impl PersistentBuffer {
    /// Create a new persistent buffer
    ///
    /// # Arguments
    ///
    /// * `buffer_dir` - Directory for buffer files (default: ~/.local/share/nexus/buffer)
    pub fn new(buffer_dir: Option<PathBuf>) -> Result<Self> {
        let buffer_dir = buffer_dir.unwrap_or_else(default_buffer_dir);

        // Create directory if it doesn't exist
        std::fs::create_dir_all(&buffer_dir)
            .map_err(|e| HookError::BufferError(format!("Failed to create buffer dir: {}", e)))?;

        Ok(Self {
            buffer_dir,
            buffers: Arc::new(RwLock::new(HashMap::new())),
            flush_interval_secs: 10,
            max_entries: 10,
        })
    }

    /// Set flush interval
    pub fn with_flush_interval(mut self, secs: u64) -> Self {
        self.flush_interval_secs = secs;
        self
    }

    /// Set max entries before auto-flush
    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max;
        self
    }

    /// Start buffering for an agent
    pub async fn start_buffering(&self, agent_type: &str) -> Result<()> {
        let mut buffers = self.buffers.write().await;

        if !buffers.contains_key(agent_type) {
            buffers.insert(agent_type.to_string(), BufferData::new(agent_type));
        }

        Ok(())
    }

    /// Buffer a context entry
    pub async fn buffer_context(
        &self,
        agent_type: &str,
        context: SessionContext,
        context_type: &str,
    ) -> Result<()> {
        // Ensure buffering is started
        {
            let mut buffers = self.buffers.write().await;
            if !buffers.contains_key(agent_type) {
                buffers.insert(agent_type.to_string(), BufferData::new(agent_type));
            }
        }

        let entry = BufferEntry {
            timestamp: Utc::now(),
            context_type: context_type.to_string(),
            context,
        };

        // Add to memory buffer
        let should_flush = {
            let mut buffers = self.buffers.write().await;
            if let Some(buffer) = buffers.get_mut(agent_type) {
                buffer.entries.push(entry);
                buffer.entries.len() >= self.max_entries
            } else {
                false
            }
        };

        // Auto-flush if buffer is large
        if should_flush {
            self.flush_to_disk(agent_type).await?;
        }

        Ok(())
    }

    /// Flush buffer to disk
    pub async fn flush_to_disk(&self, agent_type: &str) -> Result<()> {
        let buffers = self.buffers.read().await;

        if let Some(buffer) = buffers.get(agent_type) {
            let buffer_file = self.buffer_dir.join(format!("{}.json", agent_type));
            let json = serde_json::to_string_pretty(buffer)
                .map_err(|e| HookError::BufferError(format!("Failed to serialize: {}", e)))?;

            let mut file = fs::File::create(&buffer_file)
                .await
                .map_err(|e| HookError::BufferError(format!("Failed to create file: {}", e)))?;

            file.write_all(json.as_bytes())
                .await
                .map_err(|e| HookError::BufferError(format!("Failed to write: {}", e)))?;

            // Update last_flush time
            drop(buffers);
            let mut buffers = self.buffers.write().await;
            if let Some(buffer) = buffers.get_mut(agent_type) {
                buffer.last_flush = Some(Utc::now());
            }
        }

        Ok(())
    }

    /// Flush all buffers to disk
    pub async fn flush_all(&self) -> Result<()> {
        let buffers = self.buffers.read().await;
        let agent_types: Vec<String> = buffers.keys().cloned().collect();
        drop(buffers);

        for agent_type in agent_types {
            self.flush_to_disk(&agent_type).await?;
        }

        Ok(())
    }

    /// Recover buffered context after crash
    pub async fn recover_buffer(&self, agent_type: &str) -> Result<Option<BufferData>> {
        let buffer_file = self.buffer_dir.join(format!("{}.json", agent_type));

        if !buffer_file.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&buffer_file)
            .await
            .map_err(|e| HookError::BufferError(format!("Failed to read buffer: {}", e)))?;

        let data: BufferData = serde_json::from_str(&content)
            .map_err(|e| HookError::BufferError(format!("Failed to parse buffer: {}", e)))?;

        tracing::info!(
            "Recovered buffer for {}: {} entries",
            agent_type,
            data.entries.len()
        );

        Ok(Some(data))
    }

    /// Clear buffer after successful storage
    pub async fn clear_buffer(&self, agent_type: &str) -> Result<()> {
        // Clear from memory
        {
            let mut buffers = self.buffers.write().await;
            buffers.remove(agent_type);
        }

        // Clear from disk
        let buffer_file = self.buffer_dir.join(format!("{}.json", agent_type));
        if buffer_file.exists() {
            fs::remove_file(&buffer_file)
                .await
                .map_err(|e| HookError::BufferError(format!("Failed to remove buffer: {}", e)))?;
        }

        Ok(())
    }

    /// Get buffer status
    pub async fn get_buffer_status(&self, agent_type: &str) -> Option<BufferStatus> {
        let buffers = self.buffers.read().await;

        buffers.get(agent_type).map(|buffer| BufferStatus {
            agent_type: agent_type.to_string(),
            started_at: buffer.started_at,
            entries_count: buffer.entries.len(),
            last_flush: buffer.last_flush,
        })
    }

    /// List all active buffers
    pub async fn list_buffers(&self) -> Vec<BufferStatus> {
        let buffers = self.buffers.read().await;

        buffers
            .iter()
            .map(|(agent_type, buffer)| BufferStatus {
                agent_type: agent_type.clone(),
                started_at: buffer.started_at,
                entries_count: buffer.entries.len(),
                last_flush: buffer.last_flush,
            })
            .collect()
    }

    /// Check if buffer exists for agent
    pub async fn has_buffer(&self, agent_type: &str) -> bool {
        let buffers = self.buffers.read().await;
        buffers.contains_key(agent_type)
            || self
                .buffer_dir
                .join(format!("{}.json", agent_type))
                .exists()
    }
}

/// Buffer status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferStatus {
    pub agent_type: String,
    pub started_at: DateTime<Utc>,
    pub entries_count: usize,
    pub last_flush: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_buffer_context() {
        let dir = tempdir().unwrap();
        let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf())).unwrap();

        buffer.start_buffering("test-agent").await.unwrap();

        let ctx = SessionContext::new("test-agent");
        buffer
            .buffer_context("test-agent", ctx, "checkpoint")
            .await
            .unwrap();

        let status = buffer.get_buffer_status("test-agent").await.unwrap();
        assert_eq!(status.entries_count, 1);
    }

    #[tokio::test]
    async fn test_flush_and_recover() {
        let dir = tempdir().unwrap();
        let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf()))
            .unwrap()
            .with_max_entries(1);

        let ctx = SessionContext::new("test-agent");

        // This should auto-flush due to max_entries = 1
        buffer.start_buffering("test-agent").await.unwrap();
        buffer
            .buffer_context("test-agent", ctx.clone(), "test")
            .await
            .unwrap();

        // Recover
        let recovered = buffer.recover_buffer("test-agent").await.unwrap();
        assert!(recovered.is_some());

        let data = recovered.unwrap();
        assert_eq!(data.entries.len(), 1);
    }

    #[tokio::test]
    async fn test_clear_buffer() {
        let dir = tempdir().unwrap();
        let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf())).unwrap();

        buffer.start_buffering("test-agent").await.unwrap();

        let ctx = SessionContext::new("test-agent");
        buffer
            .buffer_context("test-agent", ctx, "test")
            .await
            .unwrap();

        buffer.flush_to_disk("test-agent").await.unwrap();
        buffer.clear_buffer("test-agent").await.unwrap();

        let status = buffer.get_buffer_status("test-agent").await;
        assert!(status.is_none());

        let recovered = buffer.recover_buffer("test-agent").await.unwrap();
        assert!(recovered.is_none());
    }

    #[tokio::test]
    async fn test_list_buffers() {
        let dir = tempdir().unwrap();
        let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf())).unwrap();

        buffer.start_buffering("agent1").await.unwrap();
        buffer.start_buffering("agent2").await.unwrap();

        let buffers = buffer.list_buffers().await;
        assert_eq!(buffers.len(), 2);
    }
}
