//! Always-on memory agent for Nexus Memory System
//!
//! Provides three core services:
//! - Ingest: Extract structured info from raw text using LLM
//! - Consolidate: Find patterns across memories
//! - Query: Answer questions with memory citations

pub mod consolidate;
pub mod derive;
pub mod digest;
pub mod error;
pub mod identity;
pub mod inbox;
pub mod ingest;
pub mod prompts;
pub mod pulse;
pub mod query;
mod ranking;
pub mod reflect;
pub mod representation;
pub mod runtime;
pub mod supervisor;
pub mod types;
pub mod util;

// Re-exports
pub use consolidate::ConsolidateService;
pub use derive::{DeriveService, DerivedObservation};
pub use digest::{DigestResult, DigestService};
pub use error::{AgentError, Result};
pub use inbox::{InboxScanner, ScanResult};
pub use ingest::IngestService;
pub use query::{introspect_query, QueryService};
pub use reflect::{ReflectService, ReflectionCase, ReflectionOutput, ReflectionResult};
pub use representation::RepresentationService;
pub use runtime::{
    create_embedding_service, derive_session_key, RuntimeController, RuntimeMode,
    RuntimeShutdownReason,
};
pub use supervisor::AgentSupervisor;
pub use types::*;
pub use util::CognitionSnapshot;
