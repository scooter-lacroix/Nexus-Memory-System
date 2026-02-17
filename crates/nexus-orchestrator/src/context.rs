//! Context enhancement for queries

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedContext {
    pub query: String,
    pub memories: Vec<MemoryContext>,
    pub rankings: HashMap<String, f32>,
    pub enhanced_at: DateTime<Utc>,
}

impl EnhancedContext {
    pub fn new(query: impl Into<String>) -> Self {
        Self { query: query.into(), memories: Vec::new(), rankings: HashMap::new(), enhanced_at: Utc::now() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub id: i64,
    pub content_summary: String,
    pub relevance_score: f32,
}

pub struct ContextEnhancer;

impl ContextEnhancer {
    pub fn new() -> Self { Self }
    pub fn enhance(&self, query: impl Into<String>) -> EnhancedContext {
        EnhancedContext::new(query)
    }
}

impl Default for ContextEnhancer {
    fn default() -> Self { Self::new() }
}
