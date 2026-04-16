//! Event Bus - Event routing and processing using tokio broadcast channels
//!
//! This module provides sub-millisecond event propagation using tokio's
//! broadcast channel for efficient pub/sub messaging.
//!
//! ## Features
//! - Lock-free event propagation using broadcast channels
//! - Event priorities for processing order
//! - Multiple subscriber support (fan-out)
//! - Event type filtering
//!
//! ## Performance Target
//! - Event propagation: <1ms

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::broadcast;
use tracing::{debug, error, trace, warn};
use uuid::Uuid;

use crate::error::{OrchestratorError, Result};

/// Event priority levels for processing order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EventPriority {
    /// Lowest priority, background processing
    #[default]
    Background,
    /// Low priority
    Low,
    /// Normal priority (default)
    Normal,
    /// High priority
    High,
    /// Highest priority, process immediately
    Critical,
}

impl EventPriority {
    /// Get numeric priority for ordering (lower = higher priority)
    pub fn order(&self) -> u8 {
        match self {
            EventPriority::Critical => 0,
            EventPriority::High => 1,
            EventPriority::Normal => 2,
            EventPriority::Low => 3,
            EventPriority::Background => 4,
        }
    }
}

/// Event types for the event bus
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    // Memory events
    MemoryStored,
    MemoryRetrieved,
    MemoryUpdated,
    MemoryDeleted,

    // Session events
    SessionStarted,
    SessionEnded,
    SessionIdle,
    SessionActive,

    // Sync events
    SyncStarted,
    SyncCompleted,
    SyncFailed,
    MemoryShared,

    // System events
    SystemReady,
    SystemError,
    SystemWarning,

    // Cognitive events
    CognitiveDrift,
    DreamCompleted,
    MorningRecall,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::MemoryStored => write!(f, "memory.stored"),
            EventType::MemoryRetrieved => write!(f, "memory.retrieved"),
            EventType::MemoryUpdated => write!(f, "memory.updated"),
            EventType::MemoryDeleted => write!(f, "memory.deleted"),
            EventType::SessionStarted => write!(f, "session.started"),
            EventType::SessionEnded => write!(f, "session.ended"),
            EventType::SessionIdle => write!(f, "session.idle"),
            EventType::SessionActive => write!(f, "session.active"),
            EventType::SyncStarted => write!(f, "sync.started"),
            EventType::SyncCompleted => write!(f, "sync.completed"),
            EventType::SyncFailed => write!(f, "sync.failed"),
            EventType::MemoryShared => write!(f, "memory.shared"),
            EventType::SystemReady => write!(f, "system.ready"),
            EventType::SystemError => write!(f, "system.error"),
            EventType::SystemWarning => write!(f, "system.warning"),
            EventType::CognitiveDrift => write!(f, "cognitive.drift"),
            EventType::DreamCompleted => write!(f, "cognitive.dream_completed"),
            EventType::MorningRecall => write!(f, "cognitive.morning_recall"),
        }
    }
}

/// Event data structure for the event bus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique event identifier
    pub id: Uuid,
    /// Event type
    pub event_type: EventType,
    /// Event payload data
    pub data: HashMap<String, serde_json::Value>,
    /// Source of the event
    pub source: String,
    /// Event priority
    pub priority: EventPriority,
    /// Event creation timestamp
    pub timestamp: DateTime<Utc>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Event {
    /// Create a new event with the given type
    pub fn new(event_type: EventType) -> Self {
        Self {
            id: Uuid::new_v4(),
            event_type,
            data: HashMap::new(),
            source: "unknown".to_string(),
            priority: EventPriority::Normal,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Create an event with data
    pub fn with_data(event_type: EventType, data: HashMap<String, serde_json::Value>) -> Self {
        Self {
            id: Uuid::new_v4(),
            event_type,
            data,
            source: "unknown".to_string(),
            priority: EventPriority::Normal,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Set the source
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Set the priority
    pub fn with_priority(mut self, priority: EventPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Get a data field
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.data
            .get(key)
            .and_then(|v| match serde_json::from_value(v.clone()) {
                Ok(parsed) => Some(parsed),
                Err(e) => {
                    warn!(
                        error = %e,
                        key,
                        "Failed to deserialize event data field; field will be treated as missing"
                    );
                    None
                }
            })
    }
}

/// Event bus configuration
#[derive(Debug, Clone)]
pub struct EventBusConfig {
    /// Maximum number of events to buffer
    pub capacity: usize,
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self { capacity: 1000 }
    }
}

/// Central event bus for routing events to handlers
///
/// Uses tokio broadcast channel for efficient fan-out to multiple subscribers.
/// Provides sub-millisecond event propagation.
///
/// ## Example
///
/// ```rust,ignore
/// let bus = EventBus::new(1000);
///
/// // Subscribe to events
/// let mut rx = bus.subscribe();
///
/// // Publish event
/// bus.publish(Event::new(EventType::SessionStarted)).await;
///
/// // Receive event
/// let event = rx.recv().await?;
/// ```
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
    config: EventBusConfig,
}

/// Global shared EventBus singleton.
///
/// All callers that need to publish or subscribe to orchestrator events
/// should use `EventBus::global()` to ensure they share the same broadcast
/// channel. Creating `EventBus::new(...)` directly is still available for
/// tests or isolated subsystems.
static GLOBAL_EVENT_BUS: OnceLock<EventBus> = OnceLock::new();

impl EventBus {
    /// Get the global shared EventBus instance.
    ///
    /// All calls return the same `EventBus`, ensuring publishers and
    /// subscribers share the same broadcast channel.
    pub fn global() -> &'static Self {
        GLOBAL_EVENT_BUS.get_or_init(|| EventBus::new(256))
    }
    /// Create a new event bus with the specified capacity
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            config: EventBusConfig { capacity },
        }
    }

    /// Create a new event bus with configuration
    pub fn with_config(config: EventBusConfig) -> Self {
        let (sender, _) = broadcast::channel(config.capacity);
        Self { sender, config }
    }

    /// Publish an event to all subscribers
    ///
    /// This is a non-blocking operation that broadcasts the event
    /// to all current subscribers.
    pub fn publish(&self, event: Event) -> Result<()> {
        let event_type = event.event_type.clone();
        let event_id = event.id;

        match self.sender.send(event) {
            Ok(receiver_count) => {
                trace!(
                    event_id = %event_id,
                    event_type = %event_type,
                    receivers = receiver_count,
                    "Event published"
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    event_id = %event_id,
                    event_type = %event_type,
                    error = %e,
                    "Failed to publish event"
                );
                Err(OrchestratorError::EventPublishFailed(e.to_string()))
            }
        }
    }

    /// Subscribe to all events
    ///
    /// Returns a receiver that will receive all published events.
    /// Note: If the receiver falls behind, older events may be dropped.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Get the capacity of the event bus
    pub fn capacity(&self) -> usize {
        self.config.capacity
    }
}

/// Event receiver wrapper with filtering support
pub struct EventReceiver {
    receiver: broadcast::Receiver<Event>,
    filter: Option<Vec<EventType>>,
}

impl EventReceiver {
    /// Create a new event receiver
    pub fn new(receiver: broadcast::Receiver<Event>) -> Self {
        Self {
            receiver,
            filter: None,
        }
    }

    /// Set event type filter
    pub fn with_filter(mut self, event_types: Vec<EventType>) -> Self {
        self.filter = Some(event_types);
        self
    }

    /// Receive the next event (filtered if filter is set)
    pub async fn recv(&mut self) -> Result<Event> {
        loop {
            let event = self
                .receiver
                .recv()
                .await
                .map_err(|e| OrchestratorError::EventBus(format!("Receive error: {}", e)))?;

            // Check filter
            if let Some(ref filter) = self.filter {
                if !filter.contains(&event.event_type) {
                    continue;
                }
            }

            return Ok(event);
        }
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&mut self) -> Result<Option<Event>> {
        loop {
            match self.receiver.try_recv() {
                Ok(event) => {
                    // Check filter
                    if let Some(ref filter) = self.filter {
                        if !filter.contains(&event.event_type) {
                            continue;
                        }
                    }
                    return Ok(Some(event));
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    return Ok(None);
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    return Err(OrchestratorError::EventBus("Channel closed".to_string()));
                }
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    debug!("Event receiver lagged by {} messages", n);
                    continue;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[test]
    fn test_event_priority_order() {
        assert!(EventPriority::Critical.order() < EventPriority::High.order());
        assert!(EventPriority::High.order() < EventPriority::Normal.order());
        assert!(EventPriority::Normal.order() < EventPriority::Low.order());
        assert!(EventPriority::Low.order() < EventPriority::Background.order());
    }

    #[test]
    fn test_event_creation() {
        let event = Event::new(EventType::SessionStarted)
            .with_source("test")
            .with_priority(EventPriority::High);

        assert_eq!(event.event_type, EventType::SessionStarted);
        assert_eq!(event.source, "test");
        assert_eq!(event.priority, EventPriority::High);
    }

    #[test]
    fn test_event_with_data() {
        let mut data = HashMap::new();
        data.insert("session_id".to_string(), serde_json::json!("abc-123"));

        let event = Event::with_data(EventType::SessionStarted, data);
        let session_id: String = event.get("session_id").unwrap();
        assert_eq!(session_id, "abc-123");
    }

    #[tokio::test]
    async fn test_event_bus_basic() {
        let bus = EventBus::new(100);
        let mut rx = bus.subscribe();

        let event = Event::new(EventType::SessionStarted);
        bus.publish(event.clone()).unwrap();

        let received = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("Should receive event")
            .unwrap();

        assert_eq!(received.event_type, EventType::SessionStarted);
    }

    #[tokio::test]
    async fn test_event_bus_multiple_subscribers() {
        let bus = EventBus::new(100);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        let event = Event::new(EventType::MemoryStored);
        bus.publish(event.clone()).unwrap();

        let r1 = timeout(Duration::from_millis(100), rx1.recv())
            .await
            .expect("Subscriber 1 should receive")
            .unwrap();
        let r2 = timeout(Duration::from_millis(100), rx2.recv())
            .await
            .expect("Subscriber 2 should receive")
            .unwrap();

        assert_eq!(r1.event_type, EventType::MemoryStored);
        assert_eq!(r2.event_type, EventType::MemoryStored);
    }

    #[tokio::test]
    async fn test_event_receiver_filter() {
        let bus = EventBus::new(100);
        let mut rx = EventReceiver::new(bus.subscribe())
            .with_filter(vec![EventType::SessionStarted, EventType::SessionEnded]);

        // Publish different event types
        bus.publish(Event::new(EventType::MemoryStored)).unwrap();
        bus.publish(Event::new(EventType::SessionStarted)).unwrap();

        let received = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("Should receive filtered event")
            .unwrap();

        // Should only receive SessionStarted, not MemoryStored
        assert_eq!(received.event_type, EventType::SessionStarted);
    }

    #[tokio::test]
    async fn test_event_propagation_latency() {
        let bus = EventBus::new(1000);
        let mut rx = bus.subscribe();

        let start = std::time::Instant::now();
        let event = Event::new(EventType::SessionStarted);
        bus.publish(event).unwrap();

        let _ = rx.recv().await.unwrap();
        let elapsed = start.elapsed();

        // Should be sub-millisecond
        assert!(
            elapsed < Duration::from_millis(1),
            "Event propagation took {:?}, expected <1ms",
            elapsed
        );
    }
}
