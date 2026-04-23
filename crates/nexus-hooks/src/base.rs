//! AgentHook trait definition
//!
//! This module defines the core AgentHook trait that all agent hooks must implement.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::error::Result;
use crate::session::SessionContext;
use crate::types::{ExtractionSource, SessionActivity, SupportTier};
use nexus_memory_agent::activity_monitor::ActivityMonitor;
use nexus_memory_agent::dream_cycle::run_nap;

/// Callback type for session end events
pub type SessionEndCallback = Arc<dyn Fn(SessionContext) + Send + Sync>;

/// Describes which lifecycle events an agent hook can handle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleCapabilities {
    pub session_start: bool,
    pub session_end: bool,
    pub checkpoint: bool,
    pub error_hook: bool,
    pub compact: bool,
}

impl LifecycleCapabilities {
    pub fn end_only() -> Self {
        Self {
            session_end: true,
            ..Default::default()
        }
    }
    pub fn monitor_only() -> Self {
        Self::default()
    }
}

/// Result of a hook operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResult {
    pub success: bool,
    pub agent_type: String,
    pub source: ExtractionSource,
    pub context: Option<SessionContext>,
    pub error: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl HookResult {
    pub fn success(agent_type: impl Into<String>, source: ExtractionSource) -> Self {
        Self {
            success: true,
            agent_type: agent_type.into(),
            source,
            context: None,
            error: None,
            timestamp: Utc::now(),
        }
    }
    pub fn success_with_context(
        agent_type: impl Into<String>,
        source: ExtractionSource,
        context: SessionContext,
    ) -> Self {
        Self {
            success: true,
            agent_type: agent_type.into(),
            source,
            context: Some(context),
            error: None,
            timestamp: Utc::now(),
        }
    }
    pub fn failure(
        agent_type: impl Into<String>,
        source: ExtractionSource,
        error: impl Into<String>,
    ) -> Self {
        Self {
            success: false,
            agent_type: agent_type.into(),
            source,
            context: None,
            error: Some(error.into()),
            timestamp: Utc::now(),
        }
    }
}

/// AgentHook trait - all agent hooks must implement this
#[async_trait]
pub trait AgentHook: Send + Sync {
    fn agent_type(&self) -> &str;
    async fn install_session_end_hook(&mut self, callback: SessionEndCallback) -> Result<()>;
    async fn install_session_start_hook(&mut self, _callback: SessionEndCallback) -> Result<()> {
        Err(crate::error::HookError::NotSupported(
            "Session start hooks not supported for this agent".to_string(),
        ))
    }
    async fn install_compact_hook(&mut self, _callback: SessionEndCallback) -> Result<()> {
        Err(crate::error::HookError::NotSupported(
            "Compact/checkpoint hooks not supported for this agent".to_string(),
        ))
    }
    async fn detect_session_activity(&self) -> Result<SessionActivity>;
    async fn extract_session_context(&self) -> Result<SessionContext>;
    async fn install_checkpoint_hook(&mut self, _callback: SessionEndCallback) -> Result<()> {
        Err(crate::error::HookError::NotSupported(
            "Checkpoint hooks not supported for this agent".to_string(),
        ))
    }
    async fn install_error_hook(&mut self, _callback: SessionEndCallback) -> Result<()> {
        Err(crate::error::HookError::NotSupported(
            "Error hooks not supported for this agent".to_string(),
        ))
    }
    fn is_hook_installed(&self) -> bool {
        false
    }
    async fn uninstall_hooks(&mut self) -> Result<()> {
        Ok(())
    }
    fn reliability_score(&self) -> f32 {
        1.0
    }
    fn lifecycle_capabilities(&self) -> LifecycleCapabilities {
        LifecycleCapabilities::end_only()
    }
    fn support_tier(&self) -> SupportTier {
        SupportTier::MonitorOnly
    }
    fn record_activity(&self) {}
}

/// Base hook implementation with common functionality
pub struct BaseHook {
    pub agent_type: String,
    pub installed: bool,
    pub callbacks: Vec<SessionEndCallback>,
    pub session_start_callbacks: Vec<SessionEndCallback>,
    pub checkpoint_callbacks: Vec<SessionEndCallback>,
    pub error_callbacks: Vec<SessionEndCallback>,
    pub activity_monitor: std::sync::Mutex<ActivityMonitor>,
    pub rescorer: RwLock<Option<Arc<crate::rescorer::SessionRescorer>>>,
    /// Project root resolved at construction time, used for nap/dream cycles.
    project_root: PathBuf,
}

impl BaseHook {
    pub fn new(agent_type: impl Into<String>) -> Self {
        let agent_type = agent_type.into();
        let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        Self {
            agent_type,
            installed: false,
            callbacks: Vec::new(),
            session_start_callbacks: Vec::new(),
            checkpoint_callbacks: Vec::new(),
            error_callbacks: Vec::new(),
            activity_monitor: std::sync::Mutex::new(ActivityMonitor::load()),
            rescorer: RwLock::new(None),
            project_root,
        }
    }
    /// Lazily initialize the rescorer on first use
    fn ensure_rescorer(&self) {
        let read = self.rescorer.read().unwrap();
        if read.is_none() {
            drop(read);
            let mut write = self.rescorer.write().unwrap();
            if write.is_none() {
                if let Ok(cwd) = std::env::current_dir() {
                    let project = nexus_core::ProjectIdentity::resolve(&cwd);
                    let config = nexus_core::Config::from_env().unwrap_or_default();
                    *write = Some(Arc::new(crate::rescorer::SessionRescorer::new(
                        project,
                        config.cognitive_system.rescore_turn_interval,
                        config.cognitive_system.rescore_drift_threshold,
                    )));
                }
            }
        }
    }

    pub fn record_activity(&self) {
        self.record_activity_with_content("activity recorded")
    }

    pub fn record_activity_with_content(&self, content: &str) {
        // Ensure rescorer is initialized lazily
        self.ensure_rescorer();

        if let Ok(mut monitor) = self.activity_monitor.lock() {
            // Reload full monitor from disk to avoid overwriting concurrent writes
            // to fields like last_deep_dream, deep_dream_cooldown, etc.
            let mut disk = ActivityMonitor::load();
            disk.record_activity();
            *monitor = disk.clone();
            drop(monitor);
            if let Err(e) = disk.save() {
                tracing::debug!("Failed to save activity monitor: {e}");
            }
        }

        // Skip drift/rescore for activity-only sampling (placeholder content)
        if content == "activity recorded" {
            return;
        }
        // Trigger real-time re-scoring if drift detected
        let rescorer = self.rescorer.read().unwrap().clone();
        if let Some(rescorer) = rescorer {
            let content = content.to_string();
            let agent_type = self.agent_type.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let config = nexus_core::Config::from_env().unwrap_or_default();
                    let embeddings = if config.embedding.enabled {
                        nexus_memory_agent::runtime::create_embedding_service(&config).await
                    } else {
                        None
                    };
                    if let Some(similarity) =
                        rescorer.on_turn(&content, embeddings.as_deref()).await
                    {
                        let _ = rescorer.rescore(embeddings.as_deref(), &agent_type).await;

                        // PHASE 11: Notify orchestrator of drift
                        // Only publish drift event for actual topic drift, not interval triggers
                        // (interval triggers return similarity = 1.0)
                        if similarity < rescorer.drift_threshold() {
                            let mut data = std::collections::HashMap::new();
                            data.insert("agent_type".to_string(), serde_json::json!(agent_type));
                            data.insert("drift_detected".to_string(), serde_json::json!(true));
                            data.insert("similarity".to_string(), serde_json::json!(similarity));
                            data.insert(
                                "threshold".to_string(),
                                serde_json::json!(rescorer.drift_threshold()),
                            );

                            let event = nexus_orchestrator::Event::with_data(
                                nexus_orchestrator::EventType::CognitiveDrift,
                                data,
                            )
                            .with_source("base_hook");

                            let event_bus = nexus_orchestrator::EventBus::global();
                            let _ = event_bus.publish(event);
                        }
                    }
                });
            }
        }
    }

    pub fn add_callback(&mut self, callback: SessionEndCallback) {
        self.callbacks.push(callback);
    }

    pub fn add_session_start_callback(&mut self, callback: SessionEndCallback) {
        self.session_start_callbacks.push(callback);
    }

    pub fn add_checkpoint_callback(&mut self, callback: SessionEndCallback) {
        self.checkpoint_callbacks.push(callback);
    }

    pub fn add_error_callback(&mut self, callback: SessionEndCallback) {
        self.error_callbacks.push(callback);
    }

    pub fn trigger_session_start_callbacks(&self, context: SessionContext) {
        for callback in &self.session_start_callbacks {
            callback(context.clone());
        }
    }

    pub fn trigger_checkpoint_callbacks(&self, context: SessionContext) {
        for callback in &self.checkpoint_callbacks {
            callback(context.clone());
        }
    }

    pub fn trigger_error_callbacks(&self, context: SessionContext) {
        for callback in &self.error_callbacks {
            callback(context.clone());
        }
    }

    pub fn trigger_callbacks(&self, context: SessionContext) {
        for callback in &self.callbacks {
            callback(context.clone());
        }

        if let Some(session_id) = context.session_id.as_ref() {
            let session_id = session_id.clone();
            let agent_type = context.agent_type.clone();
            let project_root = self.project_root.clone();

            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let config = nexus_core::Config::from_env().unwrap_or_default();
                    if config.cognitive_system.dream_triggers.nap_on_session_end {
                        let cwd = project_root;
                        let pool_url = config.database_url();
                        if let Some(parent) = config.database.path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Ok(mut storage) =
                            nexus_storage::StorageManager::from_url(&pool_url).await
                        {
                            if let Err(e) = storage.initialize().await {
                                tracing::warn!("Failed to initialize storage for nap: {e}");
                                return;
                            }
                            let ns_repo = nexus_storage::repository::NamespaceRepository::new(
                                storage.pool().clone(),
                            );
                            if let Ok(namespace) =
                                ns_repo.get_or_create(&agent_type, &agent_type).await
                            {
                                let llm_result = nexus_llm::create_client_auto_with_fallback();
                                let llm = match llm_result {
                                    Ok(client) => client,
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to create LLM client for session-end nap: {}",
                                            e
                                        );
                                        return;
                                    }
                                };
                                let embeddings = if config.embedding.enabled {
                                    nexus_memory_agent::runtime::create_embedding_service(&config)
                                        .await
                                } else {
                                    None
                                };
                                let timeout = std::time::Duration::from_secs(
                                    config.cognition.session_end_dream_timeout_secs,
                                );
                                let services = nexus_memory_agent::dream_cycle::DreamServices {
                                    pool: storage.pool().clone(),
                                    cognition: config.cognition.clone(),
                                    agent: config.agent.clone(),
                                    llm,
                                    embeddings,
                                    cognitive_system: config.cognitive_system.clone(),
                                };
                                match run_nap(&session_id, &cwd, namespace.id, &services, timeout)
                                    .await
                                {
                                    Ok(nap_result) => {
                                        if nap_result.timed_out {
                                            tracing::warn!(
                                                session_id = %session_id,
                                                "nap timed out; not publishing DreamCompleted"
                                            );
                                        } else {
                                            // PHASE 11: Notify of dream completion
                                            let mut data = std::collections::HashMap::new();
                                            data.insert(
                                                "agent_type".to_string(),
                                                serde_json::json!(agent_type),
                                            );
                                            data.insert(
                                                "processed".to_string(),
                                                serde_json::json!(nap_result.memories_processed),
                                            );

                                            let event = nexus_orchestrator::Event::with_data(
                                                nexus_orchestrator::EventType::DreamCompleted,
                                                data,
                                            )
                                            .with_source("agent_supervisor");

                                            let event_bus = nexus_orchestrator::EventBus::global();
                                            let _ = event_bus.publish(event);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            session_id = %session_id,
                                            error = %e,
                                            "Session-end nap failed"
                                        );
                                    }
                                }
                            } else {
                                tracing::debug!("Failed to get/create namespace for nap");
                            }
                        } else {
                            tracing::debug!("Failed to create storage for nap");
                        }
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_result_success() {
        let result = HookResult::success("test-agent", ExtractionSource::Manual);
        assert!(result.success);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_hook_result_failure() {
        let result = HookResult::failure(
            "test-agent",
            ExtractionSource::Manual,
            "Something went wrong",
        );
        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(result.error.unwrap(), "Something went wrong");
    }

    #[test]
    fn test_hook_result_with_context() {
        let ctx = SessionContext::new("test");
        let result = HookResult::success_with_context(
            "test-agent",
            ExtractionSource::NativeHook("skill".to_string()),
            ctx,
        );
        assert!(result.success);
        assert!(result.context.is_some());
    }

    #[test]
    fn test_base_hook() {
        let mut hook = BaseHook::new("test");
        assert_eq!(hook.agent_type, "test");
        assert!(!hook.installed);

        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();
        hook.add_callback(Arc::new(move |_ctx| {
            called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        }));

        hook.trigger_callbacks(SessionContext::new("test"));
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_lifecycle_capabilities_default() {
        let caps = LifecycleCapabilities::default();
        assert!(!caps.session_start);
        assert!(!caps.session_end);
        assert!(!caps.checkpoint);
        assert!(!caps.error_hook);
        assert!(!caps.compact);
    }

    #[test]
    fn test_lifecycle_capabilities_end_only() {
        let caps = LifecycleCapabilities::end_only();
        assert!(!caps.session_start);
        assert!(caps.session_end);
        assert!(!caps.checkpoint);
        assert!(!caps.error_hook);
        assert!(!caps.compact);
    }

    #[test]
    fn test_lifecycle_capabilities_monitor_only() {
        let caps = LifecycleCapabilities::monitor_only();
        assert!(!caps.session_end);
    }
}
