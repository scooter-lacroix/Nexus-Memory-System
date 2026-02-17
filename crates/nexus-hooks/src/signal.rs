//! Signal handling for graceful shutdown
//!
//! This module provides cross-platform signal handling to ensure
//! buffer flush before exit on SIGTERM/SIGINT.

use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

use crate::error::{HookError, Result};

/// Signal event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalEvent {
    /// SIGINT (Ctrl+C)
    Interrupt,

    /// SIGTERM
    Terminate,

    /// SIGHUP (hangup)
    Hangup,

    /// User-defined signal 1
    User1,

    /// User-defined signal 2
    User2,
}

/// Signal handler configuration
pub struct SignalConfig {
    /// Handle SIGINT
    pub handle_interrupt: bool,

    /// Handle SIGTERM
    pub handle_terminate: bool,

    /// Handle SIGHUP
    pub handle_hangup: bool,
}

impl Default for SignalConfig {
    fn default() -> Self {
        Self {
            handle_interrupt: true,
            handle_terminate: true,
            handle_hangup: false,
        }
    }
}

/// Signal handler for graceful shutdown
pub struct SignalHandler {
    /// Event sender
    event_sender: broadcast::Sender<SignalEvent>,

    /// Configuration
    config: SignalConfig,

    /// Whether handler is installed
    installed: Arc<Mutex<bool>>,
}

impl SignalHandler {
    /// Create a new signal handler
    pub fn new() -> Self {
        let (event_sender, _) = broadcast::channel(16);

        Self {
            event_sender,
            config: SignalConfig::default(),
            installed: Arc::new(Mutex::new(false)),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: SignalConfig) -> Self {
        let (event_sender, _) = broadcast::channel(16);

        Self {
            event_sender,
            config,
            installed: Arc::new(Mutex::new(false)),
        }
    }

    /// Subscribe to signal events
    pub fn subscribe(&self) -> broadcast::Receiver<SignalEvent> {
        self.event_sender.subscribe()
    }

    /// Install signal handlers
    #[cfg(unix)]
    pub async fn install(&self) -> Result<()> {
        let mut installed = self.installed.lock().await;
        if *installed {
            return Ok(());
        }

        use signal_hook::consts::*;
        use signal_hook_tokio::Signals;

        let mut signals_to_handle = Vec::new();

        if self.config.handle_interrupt {
            signals_to_handle.push(SIGINT);
        }
        if self.config.handle_terminate {
            signals_to_handle.push(SIGTERM);
        }
        if self.config.handle_hangup {
            signals_to_handle.push(SIGHUP);
        }

        let mut signals = Signals::new(signals_to_handle).map_err(|e| {
            HookError::SignalError(format!("Failed to create signal handler: {}", e))
        })?;

        let event_sender = self.event_sender.clone();
        let _installed_flag = self.installed.clone();

        tokio::spawn(async move {
            while let Some(signal) = signals.next().await {
                let event = match signal {
                    SIGINT => SignalEvent::Interrupt,
                    SIGTERM => SignalEvent::Terminate,
                    SIGHUP => SignalEvent::Hangup,
                    SIGUSR1 => SignalEvent::User1,
                    SIGUSR2 => SignalEvent::User2,
                    _ => continue,
                };

                let _ = event_sender.send(event);
            }
        });

        *installed = true;
        Ok(())
    }

    /// Install signal handlers (Windows)
    #[cfg(windows)]
    pub async fn install(&self) -> Result<()> {
        let mut installed = self.installed.lock().await;
        if *installed {
            return Ok(());
        }

        use tokio::signal;

        let event_sender = self.event_sender.clone();
        let event_sender_ctrl = event_sender.clone();

        // Handle Ctrl+C
        if self.config.handle_interrupt {
            tokio::spawn(async move {
                match signal::ctrl_c().await {
                    Ok(()) => {
                        let _ = event_sender_ctrl.send(SignalEvent::Interrupt);
                    }
                    Err(e) => {
                        tracing::error!("Ctrl+C handler error: {}", e);
                    }
                }
            });
        }

        *installed = true;
        Ok(())
    }

    /// Check if handler is installed
    pub async fn is_installed(&self) -> bool {
        *self.installed.lock().await
    }

    /// Send a signal event manually
    pub fn send(&self, event: SignalEvent) -> Result<()> {
        self.event_sender
            .send(event)
            .map_err(|e| HookError::SignalError(format!("Failed to send signal: {}", e)))?;
        Ok(())
    }
}

impl Default for SignalHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a callback on signal
pub async fn on_signal<F>(handler: &SignalHandler, callback: F) -> Result<()>
where
    F: FnOnce() + Send + 'static,
{
    let mut receiver = handler.subscribe();

    tokio::spawn(async move {
        if let Ok(_signal) = receiver.recv().await {
            callback();
        }
    });

    Ok(())
}

/// Run an async callback on signal
pub async fn on_signal_async<F, Fut>(handler: &SignalHandler, callback: F) -> Result<()>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let mut receiver = handler.subscribe();

    tokio::spawn(async move {
        if let Ok(_signal) = receiver.recv().await {
            callback().await;
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_signal_handler_new() {
        let handler = SignalHandler::new();
        assert!(!handler.is_installed().await);
    }

    #[tokio::test]
    async fn test_signal_handler_subscribe() {
        let handler = SignalHandler::new();
        let mut receiver = handler.subscribe();

        // Send test event
        handler.send(SignalEvent::Interrupt).unwrap();

        let event = receiver.try_recv();
        assert!(event.is_ok());
        assert_eq!(event.unwrap(), SignalEvent::Interrupt);
    }

    #[tokio::test]
    async fn test_signal_config_default() {
        let config = SignalConfig::default();
        assert!(config.handle_interrupt);
        assert!(config.handle_terminate);
        assert!(!config.handle_hangup);
    }
}
