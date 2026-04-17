//! Main orchestrator combining all components

use crate::context::ContextEnhancer;
use crate::event_bus::EventBus;
use crate::session::{Session, SessionId, SessionManager};
use crate::sync::SyncCoordinator;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    pub session_idle_timeout_secs: u64,
    pub max_sessions: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            session_idle_timeout_secs: 300,
            max_sessions: 10000,
        }
    }
}

pub struct Orchestrator {
    config: OrchestratorConfig,
    session_manager: SessionManager,
    pub event_bus: EventBus,
    sync_coordinator: SyncCoordinator,
    context_enhancer: ContextEnhancer,
}

impl Orchestrator {
    pub fn new(config: OrchestratorConfig) -> Self {
        let session_manager = SessionManager::with_idle_timeout(config.session_idle_timeout_secs);

        Self {
            session_manager,
            event_bus: EventBus::new(1024),
            sync_coordinator: SyncCoordinator::new(),
            context_enhancer: ContextEnhancer::new(),
            config,
        }
    }

    pub async fn create_session(&self, agent_type: impl Into<String>) -> Session {
        self.session_manager.create_session(agent_type).await
    }

    pub async fn end_session(&self, id: &SessionId) -> Option<Session> {
        self.session_manager.end_session(id).await
    }

    pub async fn active_session_count(&self) -> usize {
        self.session_manager.active_count().await
    }

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<crate::event_bus::Event> {
        self.event_bus.subscribe()
    }

    pub fn config(&self) -> &OrchestratorConfig {
        &self.config
    }

    pub fn sync_policy(&self) -> crate::sync::SyncPolicy {
        self.sync_coordinator.policy()
    }

    pub fn enhance_context(&self, query: impl Into<String>) -> crate::context::EnhancedContext {
        self.context_enhancer.enhance(query)
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        let cfg = OrchestratorConfig::default();
        let idle_timeout = cfg.session_idle_timeout_secs;
        Self {
            config: cfg,
            session_manager: SessionManager::with_idle_timeout(idle_timeout),
            event_bus: EventBus::global().clone(),
            sync_coordinator: SyncCoordinator::new(),
            context_enhancer: ContextEnhancer::new(),
        }
    }
}
