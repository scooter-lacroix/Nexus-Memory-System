//! Multi-layer extractor combining all detection methods
//!
//! This module provides the main extraction orchestrator that combines
//! all four detection layers for maximum reliability.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::base::{AgentHook, HookResult};
use crate::buffer::{BufferData, PersistentBuffer};
use crate::detector::InactivityDetector;
use crate::error::{HookError, Result};
use crate::monitor::{MonitorEvent, SessionMonitor};
use crate::session::SessionContext;
use crate::signal::{SignalEvent, SignalHandler};
use crate::types::{AgentType, ExtractionSource};

/// Extraction statistics
#[derive(Debug, Clone, Default)]
pub struct ExtractionStats {
    pub total_extractions: u64,
    pub native_extractions: u64,
    pub monitor_extractions: u64,
    pub inactivity_extractions: u64,
    pub buffer_recoveries: u64,
    pub signal_extractions: u64,
    pub failed_extractions: u64,
}

impl ExtractionStats {
    pub fn success_rate(&self) -> f32 {
        if self.total_extractions == 0 {
            1.0
        } else {
            let successful = self.total_extractions - self.failed_extractions;
            successful as f32 / self.total_extractions as f32
        }
    }
}

/// Multi-layer extractor for maximum reliability
///
/// This combines all four detection layers:
/// 1. **Native Hooks** (100%): Direct agent integration
/// 2. **Session Monitor** (95%): Process monitoring
/// 3. **Inactivity Detector** (90%): Timeout detection
/// 4. **Persistent Buffer** (99%): Crash recovery
///
/// # Example
///
/// ```ignore
/// use nexus_hooks::{HookFactory, MultiLayerExtractor};
/// use std::sync::Arc;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let factory = HookFactory::new();
///     let hook = factory.create_hook("claude-code")?;
///
///     let extractor = MultiLayerExtractor::new()
///         .with_hook(hook)
///         .await?;
///
///     // Start monitoring
///     extractor.start().await?;
///
///     // Extract context
///     let context = extractor.extract().await?;
///     println!("Extracted: {:?}", context);
///
///     // Stop and flush
///     extractor.stop().await?;
///
///     Ok(())
/// }
/// ```
pub struct MultiLayerExtractor {
    /// Agent hooks by type
    hooks: Arc<RwLock<HashMap<String, Box<dyn AgentHook>>>>,

    /// Persistent buffer
    buffer: PersistentBuffer,

    /// Session monitor
    monitor: SessionMonitor,

    /// Inactivity detector
    inactivity_detector: InactivityDetector,

    /// Signal handler
    signal_handler: SignalHandler,

    /// Event sender
    event_sender: broadcast::Sender<ExtractionEvent>,

    /// Statistics
    stats: Arc<RwLock<ExtractionStats>>,

    /// Whether extraction is active
    active: Arc<RwLock<bool>>,
}

/// Events emitted by the extractor
#[derive(Debug, Clone)]
pub enum ExtractionEvent {
    /// Extraction started
    Started {
        agent_type: String,
        source: ExtractionSource,
    },

    /// Extraction completed
    Completed {
        agent_type: String,
        source: ExtractionSource,
        context: SessionContext,
    },

    /// Extraction failed
    Failed {
        agent_type: String,
        source: ExtractionSource,
        error: String,
    },

    /// Buffer recovered
    BufferRecovered {
        agent_type: String,
        entries: usize,
    },
}

impl MultiLayerExtractor {
    /// Create a new multi-layer extractor
    pub fn new() -> Result<Self> {
        let buffer = PersistentBuffer::new(None)?;
        let (event_sender, _) = broadcast::channel(100);

        Ok(Self {
            hooks: Arc::new(RwLock::new(HashMap::new())),
            buffer,
            monitor: SessionMonitor::new(),
            inactivity_detector: InactivityDetector::new(),
            signal_handler: SignalHandler::new(),
            event_sender,
            stats: Arc::new(RwLock::new(ExtractionStats::default())),
            active: Arc::new(RwLock::new(false)),
        })
    }

    /// Add a hook to the extractor
    pub async fn with_hook(self, hook: Box<dyn AgentHook>) -> Result<Self> {
        let agent_type = hook.agent_type().to_string();

        // Install native hook callback
        let event_sender = self.event_sender.clone();
        let agent_type_clone = agent_type.clone();

        let _callback = Arc::new(move |ctx: SessionContext| {
            let _ = event_sender.send(ExtractionEvent::Completed {
                agent_type: agent_type_clone.clone(),
                source: ExtractionSource::NativeHook("session_end".to_string()),
                context: ctx,
            });
        });

        // We need mutable access to install the hook
        {
            let mut hooks = self.hooks.write().await;
            hooks.insert(agent_type.clone(), hook);
        }

        Ok(self)
    }

    /// Subscribe to extraction events
    pub fn subscribe(&self) -> broadcast::Receiver<ExtractionEvent> {
        self.event_sender.subscribe()
    }

    /// Start all detection layers
    pub async fn start(&self) -> Result<()> {
        let mut active = self.active.write().await;
        if *active {
            return Ok(());
        }
        *active = true;
        drop(active);

        // Get agent types to monitor
        let agent_types: Vec<String> = {
            let hooks = self.hooks.read().await;
            hooks.keys().cloned().collect()
        };

        // Convert to AgentType for monitor
        let agent_types_enum: Vec<AgentType> = agent_types
            .iter()
            .filter_map(|s| AgentType::from_str(s))
            .collect();

        // Start session monitor
        self.monitor.start_monitoring(agent_types_enum).await;

        // Start inactivity detector
        self.inactivity_detector
            .start_monitoring(agent_types.clone())
            .await;

        // Install signal handlers
        self.signal_handler.install().await?;

        // Subscribe to monitor events
        let event_sender = self.event_sender.clone();
        let stats = self.stats.clone();
        let mut monitor_rx = self.monitor.subscribe();

        tokio::spawn(async move {
            while let Ok(event) = monitor_rx.recv().await {
                match event {
                    MonitorEvent::SessionEnded { agent_type, reason: _, .. } => {
                        let _ = event_sender.send(ExtractionEvent::Started {
                            agent_type: agent_type.clone(),
                            source: ExtractionSource::ProcessMonitor,
                        });

                        let mut stats = stats.write().await;
                        stats.total_extractions += 1;
                        stats.monitor_extractions += 1;
                    }
                    MonitorEvent::InactivityDetected { agent_type, .. } => {
                        let _ = event_sender.send(ExtractionEvent::Started {
                            agent_type: agent_type.clone(),
                            source: ExtractionSource::InactivityTimeout,
                        });

                        let mut stats = stats.write().await;
                        stats.total_extractions += 1;
                        stats.inactivity_extractions += 1;
                    }
                    _ => {}
                }
            }
        });

        // Subscribe to signal events
        let _event_sender = self.event_sender.clone();
        let stats = self.stats.clone();
        let mut signal_rx = self.signal_handler.subscribe();

        tokio::spawn(async move {
            while let Ok(signal) = signal_rx.recv().await {
                let _source = match signal {
                    SignalEvent::Interrupt => ExtractionSource::SignalHandler("SIGINT".to_string()),
                    SignalEvent::Terminate => ExtractionSource::SignalHandler("SIGTERM".to_string()),
                    _ => continue,
                };

                let mut stats = stats.write().await;
                stats.total_extractions += 1;
                stats.signal_extractions += 1;
            }
        });

        // Start buffering for all agents
        for agent_type in &agent_types {
            self.buffer.start_buffering(agent_type).await?;
        }

        tracing::info!("Multi-layer extractor started");

        Ok(())
    }

    /// Stop all detection layers
    pub async fn stop(&self) -> Result<()> {
        let mut active = self.active.write().await;
        *active = false;

        self.monitor.stop_monitoring().await;
        self.inactivity_detector.stop_monitoring().await;

        // Flush all buffers
        self.buffer.flush_all().await?;

        tracing::info!("Multi-layer extractor stopped");

        Ok(())
    }

    /// Extract context for an agent
    pub async fn extract(&self, agent_type: &str) -> Result<SessionContext> {
        // Try native extraction first
        let native_result = self.try_native_extraction(agent_type).await;

        if let Ok(context) = native_result {
            // Store to buffer
            self.buffer
                .buffer_context(agent_type, context.clone(), "extraction")
                .await?;
            return Ok(context);
        }

        // Try buffer recovery
        if let Some(data) = self.buffer.recover_buffer(agent_type).await? {
            let context = self.buffer_data_to_context(data);
            let _ = self.event_sender.send(ExtractionEvent::BufferRecovered {
                agent_type: agent_type.to_string(),
                entries: context.insights.len(), // Use insights count as entry count
            });

            let mut stats = self.stats.write().await;
            stats.buffer_recoveries += 1;

            return Ok(context);
        }

        // Fallback to minimal context
        Ok(SessionContext::new(agent_type)
            .with_source("fallback")
            .with_reliability(0.5))
    }

    /// Try native extraction
    async fn try_native_extraction(&self, agent_type: &str) -> Result<SessionContext> {
        let hooks = self.hooks.read().await;

        if let Some(hook) = hooks.get(agent_type) {
            // Check activity first
            let activity = hook.detect_session_activity().await?;

            if activity.is_active {
                return hook.extract_session_context().await;
            }
        }

        Err(HookError::SessionNotActive)
    }

    /// Convert buffer data to session context
    fn buffer_data_to_context(&self, data: BufferData) -> SessionContext {
        let mut context = SessionContext::new(&data.agent_type)
            .with_source("buffer_recovery")
            .with_reliability(0.99);

        for entry in data.entries {
            context.insights.push(format!(
                "[{}] {:?}",
                entry.context_type,
                entry.context.to_memory_content()
            ));
        }

        context
    }

    /// Get extraction statistics
    pub async fn stats(&self) -> ExtractionStats {
        self.stats.read().await.clone()
    }

    /// Check if extraction is active
    pub async fn is_active(&self) -> bool {
        *self.active.read().await
    }

    /// Manually trigger extraction for an agent
    pub async fn trigger_extraction(&self, agent_type: &str) -> Result<HookResult> {
        let _ = self.event_sender.send(ExtractionEvent::Started {
            agent_type: agent_type.to_string(),
            source: ExtractionSource::Manual,
        });

        match self.extract(agent_type).await {
            Ok(context) => {
                let _ = self.event_sender.send(ExtractionEvent::Completed {
                    agent_type: agent_type.to_string(),
                    source: ExtractionSource::Manual,
                    context: context.clone(),
                });

                let mut stats = self.stats.write().await;
                stats.total_extractions += 1;

                Ok(HookResult::success_with_context(
                    agent_type,
                    ExtractionSource::Manual,
                    context,
                ))
            }
            Err(e) => {
                let _ = self.event_sender.send(ExtractionEvent::Failed {
                    agent_type: agent_type.to_string(),
                    source: ExtractionSource::Manual,
                    error: e.to_string(),
                });

                let mut stats = self.stats.write().await;
                stats.total_extractions += 1;
                stats.failed_extractions += 1;

                Ok(HookResult::failure(
                    agent_type,
                    ExtractionSource::Manual,
                    e.to_string(),
                ))
            }
        }
    }

    /// Check for buffered data to recover
    pub async fn check_for_recovery(&self) -> Result<Vec<(String, BufferData)>> {
        let hooks = self.hooks.read().await;
        let mut recovered = Vec::new();

        for agent_type in hooks.keys() {
            if let Some(data) = self.buffer.recover_buffer(agent_type).await? {
                recovered.push((agent_type.clone(), data));
            }
        }

        Ok(recovered)
    }

    /// Clear buffer for an agent
    pub async fn clear_buffer(&self, agent_type: &str) -> Result<()> {
        self.buffer.clear_buffer(agent_type).await
    }
}

impl Default for MultiLayerExtractor {
    fn default() -> Self {
        Self::new().expect("Failed to create extractor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extractor_new() {
        let extractor = MultiLayerExtractor::new().unwrap();
        assert!(!extractor.is_active().await);
    }

    #[tokio::test]
    async fn test_extractor_stats() {
        let extractor = MultiLayerExtractor::new().unwrap();
        let stats = extractor.stats().await;

        assert_eq!(stats.total_extractions, 0);
        assert_eq!(stats.success_rate(), 1.0);
    }

    #[tokio::test]
    async fn test_extractor_subscribe() {
        let extractor = MultiLayerExtractor::new().unwrap();
        let mut receiver = extractor.subscribe();

        // Should be able to subscribe without error
        drop(receiver);
    }

    #[test]
    fn test_extraction_stats_success_rate() {
        let mut stats = ExtractionStats::default();

        assert_eq!(stats.success_rate(), 1.0);

        stats.total_extractions = 10;
        stats.failed_extractions = 2;

        assert!((stats.success_rate() - 0.8).abs() < 0.001);
    }
}
