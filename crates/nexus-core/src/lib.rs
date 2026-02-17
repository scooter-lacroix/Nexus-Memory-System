//! Nexus Core - Core types, traits, and business logic
//!
//! This crate provides the foundational types and traits used throughout
//! the Nexus Memory System.

pub mod config;
pub mod error;
pub mod traits;
pub mod types;

pub use config::Config;
pub use error::{NexusError, Result};
pub use traits::*;
pub use types::*;

/// Nexus Memory System version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
