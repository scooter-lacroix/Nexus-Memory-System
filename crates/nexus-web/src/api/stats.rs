//! Statistics API endpoints

use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    error::Result,
    models::{AgentStats, StatsResponse, SystemInfo},
    state::AppState,
};

/// Get global statistics
pub async fn get_stats(State(state): State<Arc<RwLock<AppState>>>) -> Result<Json<StatsResponse>> {
    let state = state.read().await;

    // Get all namespaces
    let namespaces = state.namespace_repo.list_all().await?;

    let mut total_memories: i64 = 0;
    let mut active_memories: i64 = 0;
    let mut archived_memories: i64 = 0;
    let mut categories: HashMap<String, i64> = HashMap::new();
    let mut agent_stats: Vec<AgentStats> = Vec::new();

    for namespace in &namespaces {
        // Count memories using unfiltered queries for accurate totals
        let ns_total = state
            .memory_repo
            .count_all_by_namespace(namespace.id)
            .await?;
        let ns_active = state.memory_repo.count_by_namespace(namespace.id).await?;
        let ns_archived = state
            .memory_repo
            .count_archived_by_namespace(namespace.id)
            .await?;

        // Get active memories for category breakdown
        let memories = state
            .memory_repo
            .search_by_namespace(namespace.id, 10000, 0)
            .await?;

        // Count categories from active memories
        let mut ns_categories: HashMap<String, i64> = HashMap::new();
        for memory in &memories {
            let cat = memory.category.to_string();
            *ns_categories.entry(cat.clone()).or_insert(0) += 1;
            *categories.entry(cat).or_insert(0) += 1;
        }

        // Find oldest and newest memories
        let oldest = memories.iter().map(|m| m.created_at).min();
        let newest = memories.iter().map(|m| m.created_at).max();

        agent_stats.push(AgentStats {
            agent_type: namespace.agent_type.clone(),
            namespace_name: namespace.name.clone(),
            total_memories: ns_total,
            active_memories: ns_active,
            archived_memories: ns_archived,
            categories: json!(ns_categories),
            oldest_memory: oldest.map(|d| d.to_rfc3339()),
            newest_memory: newest.map(|d| d.to_rfc3339()),
        });

        total_memories += ns_total;
        active_memories += ns_active;
        archived_memories += ns_archived;
    }

    let system_info = SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: state.uptime_seconds(),
        active_sessions: state.orchestrator.active_session_count().await,
    };

    Ok(Json(StatsResponse {
        success: true,
        total_memories,
        active_memories,
        archived_memories,
        categories: json!(categories),
        agents: agent_stats,
        system_info: Some(system_info),
    }))
}

/// Get statistics for a specific agent
pub async fn get_agent_stats(
    State(state): State<Arc<RwLock<AppState>>>,
    Path(agent_type): Path<String>,
) -> Result<Json<AgentStats>> {
    let state = state.read().await;

    // Get namespace for this agent
    let namespace = state
        .namespace_repo
        .get_or_create(&agent_type, &agent_type)
        .await?;

    // Count memories using unfiltered queries for accurate totals
    let count = state
        .memory_repo
        .count_all_by_namespace(namespace.id)
        .await?;
    let active = state.memory_repo.count_by_namespace(namespace.id).await?;
    let archived = state
        .memory_repo
        .count_archived_by_namespace(namespace.id)
        .await?;

    // Get active memories for category breakdown
    let memories = state
        .memory_repo
        .search_by_namespace(namespace.id, 10000, 0)
        .await?;

    // Count categories
    let mut ns_categories: HashMap<String, i64> = HashMap::new();
    for memory in &memories {
        let cat = memory.category.to_string();
        *ns_categories.entry(cat).or_insert(0) += 1;
    }

    // Find oldest and newest memories
    let oldest = memories.iter().map(|m| m.created_at).min();
    let newest = memories.iter().map(|m| m.created_at).max();

    Ok(Json(AgentStats {
        agent_type: namespace.agent_type,
        namespace_name: namespace.name,
        total_memories: count,
        active_memories: active,
        archived_memories: archived,
        categories: json!(ns_categories),
        oldest_memory: oldest.map(|d| d.to_rfc3339()),
        newest_memory: newest.map(|d| d.to_rfc3339()),
    }))
}
