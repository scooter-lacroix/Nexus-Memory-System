//! Nexus Core - Core types, traits, and business logic
//!
//! This crate provides the foundational types and traits used throughout
//! the Nexus Memory System.

pub mod types;
pub mod traits;
pub mod error;
pub mod config;

pub use types::*;
pub use traits::*;
pub use error::{NexusError, Result};
pub use config::Config;

/// Nexus Memory System version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
