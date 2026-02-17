//! Session and process monitoring
//!
//! This module provides the SECONDARY layer of detection through
//! process monitoring and session state tracking.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Pid, ProcessStatus, System};
use tokio::sync::{broadcast, RwLock};
use tokio::time::interval;

use crate::error::Result;
use crate::types::{AgentType, ProcessInfo, SessionActivity};

/// Process monitor for tracking agent processes
pub struct ProcessMonitor {
    /// sysinfo system handle
    system: System,

    /// Tracked process IDs
    tracked_pids: HashMap<u32, AgentType>,

    /// Previous process states
    previous_states: HashMap<String, bool>,
}

impl Clone for ProcessMonitor {
    fn clone(&self) -> Self {
        Self {
            system: System::new_all(),
            tracked_pids: self.tracked_pids.clone(),
            previous_states: self.previous_states.clone(),
        }
    }
}

impl ProcessMonitor {
    /// Create a new process monitor
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
            tracked_pids: HashMap::new(),
            previous_states: HashMap::new(),
        }
    }

    /// Refresh process information
    pub fn refresh(&mut self) {
        self.system.refresh_all();
    }

    /// Track a process by PID
    pub fn track_process(&mut self, pid: u32, agent_type: AgentType) {
        self.tracked_pids.insert(pid, agent_type);
    }

    /// Stop tracking a process
    pub fn untrack_process(&mut self, pid: u32) {
        self.tracked_pids.remove(&pid);
    }

    /// Check if a process is alive
    pub fn is_process_alive(&self, pid: u32) -> bool {
        self.system
            .process(Pid::from(pid as usize))
            .map(|p| p.status() == ProcessStatus::Run)
            .unwrap_or(false)
    }

    /// Get process information
    pub fn get_process_info(&self, pid: u32) -> Option<ProcessInfo> {
        self.system
            .process(Pid::from(pid as usize))
            .map(|p| ProcessInfo {
                pid,
                name: p.name().to_string_lossy().to_string(),
                status: format!("{:?}", p.status()),
                command: Some(
                    p.cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                working_dir: p.cwd().map(|p| p.to_string_lossy().to_string()),
                create_time: chrono::DateTime::from_timestamp(p.start_time() as i64, 0),
                cpu_percent: Some(p.cpu_usage()),
                memory_bytes: Some(p.memory()),
            })
    }

    /// Find processes matching agent type
    pub fn find_agent_processes(&mut self, agent_type: AgentType) -> Vec<ProcessInfo> {
        self.refresh();

        let process_names = agent_type.process_names();
        let mut found = Vec::new();

        for process in self.system.processes().values() {
            let name = process.name().to_string_lossy().to_lowercase();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            for pattern in process_names {
                if name.contains(pattern) || cmd.contains(pattern) {
                    if let Some(info) = self.get_process_info(process.pid().as_u32()) {
                        found.push(info);
                    }
                    break;
                }
            }
        }

        found
    }

    /// Monitor tracked processes and return terminated ones
    pub fn check_terminated(&mut self) -> Vec<(u32, AgentType)> {
        self.refresh();

        let mut terminated = Vec::new();

        for (pid, agent_type) in &self.tracked_pids {
            if !self.is_process_alive(*pid) {
                terminated.push((*pid, *agent_type));
            }
        }

        // Untrack terminated processes
        for (pid, _) in &terminated {
            self.tracked_pids.remove(pid);
        }

        terminated
    }
}

impl Default for ProcessMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Event emitted by session monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MonitorEvent {
    /// Session started
    SessionStarted {
        agent_type: String,
        pid: Option<u32>,
        timestamp: DateTime<Utc>,
    },

    /// Session ended
    SessionEnded {
        agent_type: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    /// Process detected
    ProcessDetected {
        agent_type: String,
        process: ProcessInfo,
    },

    /// Process terminated
    ProcessTerminated { agent_type: String, pid: u32 },

    /// Inactivity detected
    InactivityDetected {
        agent_type: String,
        inactive_for_secs: u64,
    },
}

/// Session monitor for background process monitoring
///
/// This is the SECONDARY layer of detection with ~95% reliability.
pub struct SessionMonitor {
    /// Process monitor
    process_monitor: Arc<RwLock<ProcessMonitor>>,

    /// Event sender
    event_sender: broadcast::Sender<MonitorEvent>,

    /// Last activity times per agent
    last_activity: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,

    /// Previous session states
    previous_states: Arc<RwLock<HashMap<String, bool>>>,

    /// Polling interval
    polling_interval: Duration,

    /// Inactivity threshold
    inactivity_threshold: Duration,

    /// Whether monitoring is active
    active: Arc<RwLock<bool>>,
}

impl SessionMonitor {
    /// Create a new session monitor
    pub fn new() -> Self {
        let (event_sender, _) = broadcast::channel(100);

        Self {
            process_monitor: Arc::new(RwLock::new(ProcessMonitor::new())),
            event_sender,
            last_activity: Arc::new(RwLock::new(HashMap::new())),
            previous_states: Arc::new(RwLock::new(HashMap::new())),
            polling_interval: Duration::from_secs(5),
            inactivity_threshold: Duration::from_secs(300),
            active: Arc::new(RwLock::new(false)),
        }
    }

    /// Set polling interval
    pub fn with_polling_interval(mut self, interval: Duration) -> Self {
        self.polling_interval = interval;
        self
    }

    /// Set inactivity threshold
    pub fn with_inactivity_threshold(mut self, threshold: Duration) -> Self {
        self.inactivity_threshold = threshold;
        self
    }

    /// Subscribe to monitor events
    pub fn subscribe(&self) -> broadcast::Receiver<MonitorEvent> {
        self.event_sender.subscribe()
    }

    /// Start monitoring for specific agent types
    pub async fn start_monitoring(&self, agent_types: Vec<AgentType>) {
        let mut active = self.active.write().await;
        if *active {
            return;
        }
        *active = true;
        drop(active);

        let process_monitor = self.process_monitor.clone();
        let event_sender = self.event_sender.clone();
        let last_activity = self.last_activity.clone();
        let previous_states = self.previous_states.clone();
        let polling_interval = self.polling_interval;
        let inactivity_threshold = self.inactivity_threshold;
        let active_flag = self.active.clone();

        tokio::spawn(async move {
            let mut ticker = interval(polling_interval);

            loop {
                ticker.tick().await;

                // Check if still active
                {
                    let a = active_flag.read().await;
                    if !*a {
                        break;
                    }
                }

                let mut monitor = process_monitor.write().await;
                monitor.refresh();

                for agent_type in &agent_types {
                    let processes = monitor.find_agent_processes(*agent_type);
                    let is_active = !processes.is_empty();

                    // Get previous state
                    let was_active = {
                        let states = previous_states.read().await;
                        states
                            .get(&agent_type.to_string())
                            .copied()
                            .unwrap_or(false)
                    };

                    // Update activity time
                    if is_active {
                        let mut activity = last_activity.write().await;
                        activity.insert(agent_type.to_string(), Utc::now());

                        // Emit process detected events
                        for process in &processes {
                            let _ = event_sender.send(MonitorEvent::ProcessDetected {
                                agent_type: agent_type.to_string(),
                                process: process.clone(),
                            });
                        }
                    }

                    // Check for state change
                    if was_active && !is_active {
                        // Session ended
                        let _ = event_sender.send(MonitorEvent::SessionEnded {
                            agent_type: agent_type.to_string(),
                            reason: "process_terminated".to_string(),
                            timestamp: Utc::now(),
                        });
                    } else if !was_active && is_active {
                        // Session started
                        let _ = event_sender.send(MonitorEvent::SessionStarted {
                            agent_type: agent_type.to_string(),
                            pid: processes.first().map(|p| p.pid),
                            timestamp: Utc::now(),
                        });
                    }

                    // Check for inactivity
                    if !is_active {
                        let activity = last_activity.read().await;
                        if let Some(last) = activity.get(&agent_type.to_string()) {
                            let elapsed = (Utc::now() - *last).to_std().unwrap_or(Duration::ZERO);

                            if elapsed > inactivity_threshold {
                                let _ = event_sender.send(MonitorEvent::InactivityDetected {
                                    agent_type: agent_type.to_string(),
                                    inactive_for_secs: elapsed.as_secs(),
                                });
                            }
                        }
                    }

                    // Update previous state
                    {
                        let mut states = previous_states.write().await;
                        states.insert(agent_type.to_string(), is_active);
                    }
                }
            }
        });
    }

    /// Stop monitoring
    pub async fn stop_monitoring(&self) {
        let mut active = self.active.write().await;
        *active = false;
    }

    /// Record activity for an agent
    pub async fn record_activity(&self, agent_type: &str) {
        let mut activity = self.last_activity.write().await;
        activity.insert(agent_type.to_string(), Utc::now());
    }

    /// Check if agent has been inactive
    pub async fn is_inactive(&self, agent_type: &str) -> bool {
        let activity = self.last_activity.read().await;

        if let Some(last) = activity.get(agent_type) {
            let elapsed = (Utc::now() - *last).to_std().unwrap_or(Duration::ZERO);
            elapsed > self.inactivity_threshold
        } else {
            false
        }
    }

    /// Get inactive duration for an agent
    pub async fn get_inactive_duration(&self, agent_type: &str) -> Option<Duration> {
        let activity = self.last_activity.read().await;

        activity
            .get(agent_type)
            .map(|last| (Utc::now() - *last).to_std().unwrap_or(Duration::ZERO))
    }

    /// Detect session activity for an agent
    pub async fn detect_activity(&self, agent_type: AgentType) -> Result<SessionActivity> {
        let mut monitor = self.process_monitor.write().await;
        let processes = monitor.find_agent_processes(agent_type);

        let mut activity = SessionActivity::new(agent_type);

        if !processes.is_empty() {
            activity.is_active = true;
            activity.processes = processes;
        }

        Ok(activity)
    }
}

impl Default for SessionMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_monitor_new() {
        let monitor = ProcessMonitor::new();
        assert!(monitor.tracked_pids.is_empty());
    }

    #[test]
    fn test_process_monitor_track() {
        let mut monitor = ProcessMonitor::new();
        monitor.track_process(1234, AgentType::ClaudeCode);

        assert!(monitor.tracked_pids.contains_key(&1234));
    }

    #[test]
    fn test_process_monitor_untrack() {
        let mut monitor = ProcessMonitor::new();
        monitor.track_process(1234, AgentType::ClaudeCode);
        monitor.untrack_process(1234);

        assert!(!monitor.tracked_pids.contains_key(&1234));
    }

    #[tokio::test]
    async fn test_session_monitor_new() {
        let monitor = SessionMonitor::new();
        assert!(!*monitor.active.read().await);
    }

    #[tokio::test]
    async fn test_session_monitor_record_activity() {
        let monitor = SessionMonitor::new();
        monitor.record_activity("claude-code").await;

        let duration = monitor.get_inactive_duration("claude-code").await;
        assert!(duration.is_some());
        assert!(duration.unwrap() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_session_monitor_subscribe() {
        let monitor = SessionMonitor::new();
        let mut receiver = monitor.subscribe();

        // Send test event
        let _ = monitor.event_sender.send(MonitorEvent::SessionStarted {
            agent_type: "test".to_string(),
            pid: None,
            timestamp: Utc::now(),
        });

        // Should receive event
        let event = receiver.try_recv();
        assert!(event.is_ok());
    }
}
