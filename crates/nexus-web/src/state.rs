//! Application state for the web dashboard

use crate::error::Result;
use crate::WebError;
use nexus_agent::AgentSupervisor;
use nexus_orchestrator::{Event, EventType, Orchestrator};
use nexus_storage::{MemoryRepository, NamespaceRepository, StorageManager};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info};

/// Shared application state
pub struct AppState {
    /// Storage manager for database operations
    pub storage: StorageManager,
    /// Orchestrator for event handling
    pub orchestrator: Orchestrator,
    /// Memory repository
    pub memory_repo: MemoryRepository,
    /// Namespace repository
    pub namespace_repo: NamespaceRepository,
    /// WebSocket broadcaster
    pub ws_sender: broadcast::Sender<crate::models::WebSocketMessage>,
    /// Server start time for uptime calculation
    pub start_time: std::time::Instant,
    /// Optional agent supervisor (set when --agent flag is used)
    pub agent_supervisor: Option<AgentSupervisor>,
}

impl AppState {
    /// Create a new application state
    pub async fn new(storage: StorageManager, orchestrator: Orchestrator) -> Result<Self> {
        let pool = storage.pool().clone();
        let memory_repo = MemoryRepository::new(pool.clone());
        let namespace_repo = NamespaceRepository::new(pool.clone());

        // Create WebSocket broadcast channel
        let (ws_sender, _) = broadcast::channel(1000);

        // Initialize agent supervisor if enabled
        let agent_supervisor = match Self::create_agent_supervisor(&pool, &namespace_repo) {
            Ok(Some(supervisor)) => {
                info!("Agent supervisor initialized");
                Some(supervisor)
            }
            Ok(None) => None,
            Err(e) => {
                error!("Failed to initialize agent supervisor: {}", e);
                None
            }
        };

        let state = Self {
            storage,
            orchestrator,
            memory_repo,
            namespace_repo,
            ws_sender,
            start_time: std::time::Instant::now(),
            agent_supervisor,
        };

        // Start event forwarding from orchestrator to WebSocket
        state.start_event_forwarding().await?;

        Ok(state)
    }

    /// Start forwarding events from orchestrator to WebSocket clients
    async fn start_event_forwarding(&self) -> Result<()> {
        let mut rx = self.orchestrator.subscribe_events();
        let ws_sender = self.ws_sender.clone();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Some(msg) = Self::convert_event_to_ws_message(&event) {
                            let _ = ws_sender.send(msg);
                        }
                    }
                    Err(e) => {
                        error!("Event receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Convert orchestrator event to WebSocket message
    fn convert_event_to_ws_message(event: &Event) -> Option<crate::models::WebSocketMessage> {
        use crate::models::{WebSocketMessage, WebSocketMessageType};

        match event.event_type {
            EventType::MemoryStored => {
                let memory_id = event.get::<i64>("memory_id").unwrap_or(0);
                let agent_type = event.get::<String>("agent_type").unwrap_or_default();
                // Note: We can't construct full MemoryResponse here without DB lookup
                // The client may need to fetch the full memory
                let data = serde_json::json!({
                    "memory_id": memory_id,
                    "agent_type": agent_type,
                });
                Some(WebSocketMessage::new(
                    WebSocketMessageType::MemoryStored,
                    data,
                ))
            }
            EventType::MemoryUpdated => {
                let memory_id = event.get::<i64>("memory_id").unwrap_or(0);
                Some(WebSocketMessage::memory_updated(memory_id))
            }
            EventType::MemoryDeleted => {
                let memory_id = event.get::<i64>("memory_id").unwrap_or(0);
                Some(WebSocketMessage::memory_deleted(memory_id))
            }
            EventType::SessionStarted => {
                let session_id = event.get::<String>("session_id").unwrap_or_default();
                let data = serde_json::json!({
                    "session_id": session_id,
                });
                Some(WebSocketMessage::new(
                    WebSocketMessageType::SessionStarted,
                    data,
                ))
            }
            EventType::SessionEnded => {
                let session_id = event.get::<String>("session_id").unwrap_or_default();
                let data = serde_json::json!({
                    "session_id": session_id,
                });
                Some(WebSocketMessage::new(
                    WebSocketMessageType::SessionEnded,
                    data,
                ))
            }
            _ => None,
        }
    }

    /// Get the database pool
    pub fn pool(&self) -> &SqlitePool {
        self.storage.pool()
    }

    /// Create an agent supervisor if agent mode is enabled in the config.
    fn create_agent_supervisor(
        pool: &SqlitePool,
        namespace_repo: &NamespaceRepository,
    ) -> Result<Option<AgentSupervisor>> {
        let config = nexus_core::Config::from_env().map_err(|e| WebError::Config(e.to_string()))?;

        if !config.agent.enabled {
            return Ok(None);
        }

        let llm = nexus_llm::create_client_auto_with_fallback()
            .map_err(|e| WebError::Config(format!("Failed to create LLM client: {}", e)))?;

        // Ensure the agent namespace exists
        let namespace = tokio::runtime::Handle::current()
            .block_on(namespace_repo.get_or_create(&config.agent.namespace, "nexus-agent"))
            .map_err(|e| WebError::Storage(e.to_string()))?;

        let mut supervisor = AgentSupervisor::new(config.agent, llm, pool.clone(), namespace.id);
        tokio::runtime::Handle::current()
            .block_on(supervisor.start())
            .map_err(|e| WebError::Config(format!("Failed to start agent supervisor: {}", e)))?;

        Ok(Some(supervisor))
    }

    /// Get uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Subscribe to WebSocket messages
    pub fn subscribe_ws(&self) -> broadcast::Receiver<crate::models::WebSocketMessage> {
        self.ws_sender.subscribe()
    }

    /// Broadcast a message to all WebSocket clients
    pub fn broadcast_ws(&self, msg: crate::models::WebSocketMessage) -> Result<()> {
        let _ = self.ws_sender.send(msg);
        Ok(())
    }
}

/// Shared state type alias
pub type SharedState = Arc<RwLock<AppState>>;
