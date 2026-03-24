//! Always-on memory agent for Nexus Memory System
//!
//! Provides three core services:
//! - Ingest: Extract structured info from raw text using LLM
//! - Consolidate: Find patterns across memories
//! - Query: Answer questions with memory citations

pub mod consolidate;
pub mod error;
pub mod inbox;
pub mod ingest;
pub mod prompts;
pub mod query;
pub mod supervisor;
pub mod types;

// Re-exports
pub use consolidate::ConsolidateService;
pub use error::{AgentError, Result};
pub use inbox::{InboxScanner, ScanResult};
pub use ingest::IngestService;
pub use query::QueryService;
pub use supervisor::AgentSupervisor;
pub use types::*;
