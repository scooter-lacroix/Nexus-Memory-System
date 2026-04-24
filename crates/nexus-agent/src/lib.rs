//! Always-on memory agent for Nexus Memory System
//!
//! Provides three core services:
//! - Ingest: Extract structured info from raw text using LLM
//! - Consolidate: Find patterns across memories
//! - Query: Answer questions with memory citations

pub mod activity_monitor;
pub mod cognitive_cache;
pub mod consolidate;
pub mod context_builder;
pub mod derive;
pub mod digest;
pub mod distill;
pub mod dream_cycle;
pub mod error;
pub mod identity;
pub mod inbox;
pub mod ingest;
pub mod job_processor;
pub mod prompts;
pub mod pulse;
pub mod query;
mod ranking;
pub mod reflect;
pub mod representation;
pub mod runtime;
pub mod runtime_state;
pub mod session_manager;
pub mod soul;
pub mod supervisor;
pub mod token_budget;
pub mod types;
pub mod util;

// Re-exports
pub use cognitive_cache::{CognitiveCache, ConfidenceTier, HotCache, HotCacheEntry};
pub use consolidate::ConsolidateService;
pub use context_builder::build_context_md;
pub use derive::{DeriveService, DerivedObservation};
pub use digest::{DigestResult, DigestService};
pub use dream_cycle::{run_dream_cycle, DreamCycleRequest};
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
pub use token_budget::TokenBudget;
pub use types::*;
pub use util::CognitionSnapshot;
