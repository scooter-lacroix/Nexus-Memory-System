//! REST API endpoints for the web dashboard

mod agent;
mod health;
mod memories;
mod namespaces;
mod observability;
mod stats;

pub use agent::*;
pub use health::*;
pub use memories::*;
pub use namespaces::*;
pub use observability::*;
pub use stats::*;
