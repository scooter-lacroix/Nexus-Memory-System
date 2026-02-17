//! REST API endpoints for the web dashboard

mod memories;
mod namespaces;
mod stats;
mod health;

pub use memories::*;
pub use namespaces::*;
pub use stats::*;
pub use health::*;
