//! Inactivity detector for session timeout detection
//!
//! This module provides the TERTIARY layer of detection with ~90% reliability.

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio::time::interval;

use crate::monitor::MonitorEvent;

/// Inactivity detector for detecting session timeouts
///
/// This is the TERTIARY layer with ~90% reliability. It triggers
/// when an agent session has been inactive for too long.
#[derive(Clone)]
pub struct InactivityDetector {
    /// Inactivity threshold
    threshold: Duration,

    /// Last activity times
    last_activity: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,

    /// Event sender
    event_sender: broadcast::Sender<MonitorEvent>,

    /// Check interval
    check_interval: Duration,

    /// Active flag
    active: Arc<RwLock<bool>>,
}

impl InactivityDetector {
    /// Create a new inactivity detector with default threshold (5 minutes)
    pub fn new() -> Self {
        Self::with_threshold(Duration::from_secs(300))
    }

    /// Create with custom threshold
    pub fn with_threshold(threshold: Duration) -> Self {
        let (event_sender, _) = broadcast::channel(100);

        Self {
            threshold,
            last_activity: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            check_interval: Duration::from_secs(30),
            active: Arc::new(RwLock::new(false)),
        }
    }

    /// Set check interval
    pub fn with_check_interval(mut self, interval: Duration) -> Self {
        self.check_interval = interval;
        self
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<MonitorEvent> {
        self.event_sender.subscribe()
    }

    /// Record activity for an agent
    pub async fn record_activity(&self, agent_type: &str) {
        let mut activity = self.last_activity.write().await;
        activity.insert(agent_type.to_string(), Utc::now());
    }

    /// Check if agent is inactive
    pub async fn is_inactive(&self, agent_type: &str) -> bool {
        let activity = self.last_activity.read().await;

        if let Some(last) = activity.get(agent_type) {
            let elapsed = (Utc::now() - *last).to_std().unwrap_or(Duration::ZERO);
            elapsed > self.threshold
        } else {
            false
        }
    }

    /// Get inactive duration
    pub async fn get_inactive_duration(&self, agent_type: &str) -> Option<Duration> {
        let activity = self.last_activity.read().await;

        activity
            .get(agent_type)
            .map(|last| (Utc::now() - *last).to_std().unwrap_or(Duration::ZERO))
    }

    /// Check if agent has been inactive and trigger event
    async fn check_and_notify(&self, agent_type: &str) -> Option<Duration> {
        if let Some(duration) = self.get_inactive_duration(agent_type).await {
            if duration > self.threshold {
                let _ = self.event_sender.send(MonitorEvent::InactivityDetected {
                    agent_type: agent_type.to_string(),
                    inactive_for_secs: duration.as_secs(),
                });
                return Some(duration);
            }
        }
        None
    }

    /// Start monitoring for inactivity
    pub async fn start_monitoring(&self, agent_types: Vec<String>) {
        let mut active = self.active.write().await;
        if *active {
            return;
        }
        *active = true;
        drop(active);

        let detector = self.clone();

        tokio::spawn(async move {
            let mut ticker = interval(detector.check_interval);

            loop {
                ticker.tick().await;

                // Check if still active
                {
                    let a = detector.active.read().await;
                    if !*a {
                        break;
                    }
                }

                for agent_type in &agent_types {
                    let _ = detector.check_and_notify(agent_type).await;
                }
            }
        });
    }

    /// Stop monitoring
    pub async fn stop_monitoring(&self) {
        let mut active = self.active.write().await;
        *active = false;
    }

    /// Clear activity for an agent
    pub async fn clear_activity(&self, agent_type: &str) {
        let mut activity = self.last_activity.write().await;
        activity.remove(agent_type);
    }
}

impl Default for InactivityDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inactivity_detector_new() {
        let detector = InactivityDetector::new();
        assert_eq!(detector.threshold, Duration::from_secs(300));
    }

    #[tokio::test]
    async fn test_record_and_check_activity() {
        let detector = InactivityDetector::new();

        detector.record_activity("test-agent").await;

        // Should not be inactive immediately
        let is_inactive = detector.is_inactive("test-agent").await;
        assert!(!is_inactive);

        // Duration should be small
        let duration = detector.get_inactive_duration("test-agent").await;
        assert!(duration.is_some());
        assert!(duration.unwrap() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_clear_activity() {
        let detector = InactivityDetector::new();

        detector.record_activity("test-agent").await;
        detector.clear_activity("test-agent").await;

        let duration = detector.get_inactive_duration("test-agent").await;
        assert!(duration.is_none());
    }

    #[tokio::test]
    async fn test_subscribe_events() {
        let detector = InactivityDetector::new();
        let mut receiver = detector.subscribe();

        // Send test event
        let _ = detector
            .event_sender
            .send(MonitorEvent::InactivityDetected {
                agent_type: "test".to_string(),
                inactive_for_secs: 300,
            });

        let event = receiver.try_recv();
        assert!(event.is_ok());
    }
}
