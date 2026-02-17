//! Session lifecycle management

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Unique session identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl Default for SessionId {
    fn default() -> Self { Self::new() }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState { Active, Idle, Ended }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub agent_type: String,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub state: SessionState,
}

impl Session {
    pub fn new(agent_type: impl Into<String>) -> Self {
        let now = Utc::now();
        Self { id: SessionId::new(), agent_type: agent_type.into(), created_at: now, last_activity: now, state: SessionState::Active }
    }
    pub fn touch(&mut self) { self.last_activity = Utc::now(); }
}

pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<SessionId, Session>>>,
    idle_timeout_secs: u64,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { sessions: Arc::new(RwLock::new(HashMap::new())), idle_timeout_secs: 300 }
    }
    pub async fn create_session(&self, agent_type: impl Into<String>) -> Session {
        let session = Session::new(agent_type);
        self.sessions.write().await.insert(session.id.clone(), session.clone());
        session
    }
    pub async fn end_session(&self, id: &SessionId) -> Option<Session> {
        self.sessions.write().await.remove(id)
    }
    pub async fn active_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

impl Default for SessionManager {
    fn default() -> Self { Self::new() }
}
