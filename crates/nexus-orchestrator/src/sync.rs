//! Cross-agent memory synchronization

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncPolicy {
    Manual,
    Auto,
    Aggressive,
}

impl Default for SyncPolicy {
    fn default() -> Self {
        Self::Manual
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncStatus {
    Pending,
    InProgress,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub source_namespace: String,
    pub target_namespace: String,
    pub status: SyncStatus,
    pub memories_synced: usize,
    pub timestamp: DateTime<Utc>,
}

pub struct SyncCoordinator {
    policy: SyncPolicy,
}

impl SyncCoordinator {
    pub fn new() -> Self {
        Self {
            policy: SyncPolicy::default(),
        }
    }
    pub fn policy(&self) -> SyncPolicy {
        self.policy
    }
}

impl Default for SyncCoordinator {
    fn default() -> Self {
        Self::new()
    }
}
