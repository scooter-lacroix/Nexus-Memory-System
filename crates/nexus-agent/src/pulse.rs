//! Agent pulse file for cross-process status communication
//!
//! Writes a lightweight JSON file that the Claude Code hook shim can read
//! to display a visual indicator when the always-on agent is actively
//! dreaming (consolidating memories).

use chrono::Utc;
use serde::Serialize;
use std::path::PathBuf;
use tracing::debug;

/// State directory for nexus agent runtime files
fn state_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NEXUS_STATE_DIR") {
        PathBuf::from(dir)
    } else {
        let xdg = std::env::var("XDG_STATE_HOME")
            .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
        PathBuf::from(xdg).join("nexus-memory-system")
    }
}

fn pulse_path() -> PathBuf {
    state_dir().join("agent-pulse.json")
}

/// Pulse data written to disk for the hook shim to read.
#[derive(Debug, Serialize)]
struct AgentPulse {
    /// ISO 8601 timestamp of the last agent activity
    last_activity: String,
    /// What the agent last did
    last_action: String,
    /// Total memories consolidated so far
    memories_consolidated: u64,
    /// Total files ingested so far
    files_processed: u64,
}

/// Write a pulse update to the state directory.
///
/// Called after consolidation, ingestion, or other agent activities.
/// The hook shim reads this file to decide whether to show a visual indicator.
pub fn write_pulse(action: &str, memories_consolidated: u64, files_processed: u64) {
    let path = pulse_path();

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let pulse = AgentPulse {
        last_activity: Utc::now().to_rfc3339(),
        last_action: action.to_string(),
        memories_consolidated,
        files_processed,
    };

    match serde_json::to_string_pretty(&pulse) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                debug!(error = %e, path = %path.display(), "Failed to write pulse file");
            }
        }
        Err(e) => {
            debug!(error = %e, "Failed to serialize pulse data");
        }
    }
}
