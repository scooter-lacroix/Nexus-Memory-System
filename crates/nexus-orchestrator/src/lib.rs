//! Nexus Orchestrator - Core coordination layer for Nexus Memory System
//!
//! This crate provides the orchestration layer including:
//! - **Session lifecycle management**: Track active sessions per agent, detect idle sessions
//! - **Event bus**: Pub/sub system using tokio broadcast channels for sub-millisecond propagation
//! - **Cross-agent synchronization**: Share memories between agent namespaces
//! - **Context enhancement**: Enhance queries with session context and memory ranking
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      ORCHESTRATOR CORE                      │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
//! │  │   Session    │  │   Event      │  │    Sync      │       │
//! │  │   Manager    │  │    Bus       │  │  Coordinator │       │
//! │  └──────────────┘  └──────────────┘  └──────────────┘       │
//! │          │                 │                  │             │
//! │          └─────────────────┼──────────────────┘             │
//! │                            ▼                                │
//! │                   ┌──────────────┐                          │
//! │                   │   Context    │                          │
//! │                   │  Enhancer    │                          │
//! │                   └──────────────┘                          │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Performance Targets
//!
//! | Metric | Target |
//! |--------|--------|
//! | Event propagation | <1ms |
//! | Concurrent sessions | 10,000+ |
//! | Session creation | <100μs |
//! | Context retrieval | <10ms |
//!
//! ## Example
//!
//! ```rust,ignore
//! use nexus_orchestrator::{Orchestrator, OrchestratorConfig};
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = OrchestratorConfig::default();
//!     let orchestrator = Orchestrator::new(config);
//!     orchestrator.initialize().await?;
//!
//!     // Create a session
//!     let session = orchestrator.create_session("claude-code").await?;
//!     println!("Session created: {}", session.id);
//!
//!     // Subscribe to events
//!     let mut rx = orchestrator.subscribe_events();
//!
//!     // Publish an event
//!     orchestrator.publish(Event::SessionStarted(session.id.clone())).await;
//!
//!     // End session
//!     orchestrator.end_session(&session.id).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod error;
pub mod event_bus;
pub mod session;
pub mod sync;
pub mod context;
pub mod orchestrator;

// Re-exports
pub use error::{OrchestratorError, Result};
pub use event_bus::{EventBus, Event, EventPriority, EventType};
pub use session::{SessionManager, Session, SessionId, SessionState};
pub use sync::{SyncCoordinator, SyncPolicy, SyncResult, SyncStatus};
pub use context::{ContextEnhancer, EnhancedContext};
pub use orchestrator::{Orchestrator, OrchestratorConfig};

/// Prelude for commonly used types
pub mod prelude {
    pub use crate::{EventBus, Event, EventType, EventPriority};
    pub use crate::{SessionManager, Session, SessionId, SessionState};
    pub use crate::{SyncCoordinator, SyncPolicy, SyncResult};
    pub use crate::{ContextEnhancer, EnhancedContext};
    pub use crate::{Orchestrator, OrchestratorConfig};
    pub use crate::Result;
}
