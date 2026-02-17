//! REST API endpoints for the web dashboard

mod health;
mod memories;
mod namespaces;
mod stats;

pub use health::*;
pub use memories::*;
pub use namespaces::*;
pub use stats::*;
