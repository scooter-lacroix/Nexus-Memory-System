//! Stats command implementation

use anyhow::Result;
use nexus_core::{Config, MemoryCategory};
use nexus_storage::{NamespaceRepository, StorageManager};
use sqlx::Row;
use std::collections::BTreeMap;

/// Execute the stats command
pub async fn execute(agent: Option<String>) -> Result<()> {
    tracing::info!("Fetching statistics");

    if let Some(ref agent_name) = agent {
        tracing::debug!("Agent filter: {}", agent_name);
    }

    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    println!("Nexus Memory System Statistics");
    println!("================================");
    println!();

    if let Some(agent_name) = agent {
        let namespace_repo = NamespaceRepository::new(storage.pool().clone());
        let namespace = namespace_repo.get_by_name(&agent_name).await?;

        if let Some(namespace) = namespace {
            let totals = sqlx::query(
                r#"
                SELECT
                    COUNT(*) AS total_memories,
                    COALESCE(SUM(CASE WHEN is_active = 1 THEN 1 ELSE 0 END), 0) AS active_memories,
                    COALESCE(SUM(CASE WHEN is_archived = 1 THEN 1 ELSE 0 END), 0) AS archived_memories
                FROM memories
                WHERE namespace_id = ?
                "#,
            )
            .bind(namespace.id)
            .fetch_one(storage.pool())
            .await?;

            println!("Namespace: {}", agent_name);
            println!(
                "  Total memories: {}",
                totals.get::<i64, _>("total_memories")
            );
            println!(
                "  Active memories: {}",
                totals.get::<i64, _>("active_memories")
            );
            println!(
                "  Archived memories: {}",
                totals.get::<i64, _>("archived_memories")
            );
        } else {
            println!("Namespace: {}", agent_name);
            println!("  Total memories: 0");
            println!("  Active memories: 0");
            println!("  Archived memories: 0");
        }
    } else {
        let global = sqlx::query(
            r#"
            SELECT
                (SELECT COUNT(*) FROM agent_namespaces) AS total_namespaces,
                (SELECT COUNT(*) FROM memories) AS total_memories,
                (SELECT COUNT(*) FROM memories WHERE is_active = 1) AS active_memories,
                (SELECT COUNT(*) FROM memories WHERE is_archived = 1) AS archived_memories
            "#,
        )
        .fetch_one(storage.pool())
        .await?;

        let mut by_category = BTreeMap::from([
            (MemoryCategory::General.to_string(), 0_i64),
            (MemoryCategory::Facts.to_string(), 0_i64),
            (MemoryCategory::Preferences.to_string(), 0_i64),
            (MemoryCategory::Context.to_string(), 0_i64),
            (MemoryCategory::Specifications.to_string(), 0_i64),
            (MemoryCategory::Session.to_string(), 0_i64),
        ]);

        let category_rows = sqlx::query(
            r#"
            SELECT category, COUNT(*) AS count
            FROM memories
            GROUP BY category
            "#,
        )
        .fetch_all(storage.pool())
        .await?;

        for row in category_rows {
            let category: String = row.get("category");
            let count: i64 = row.get("count");
            by_category.insert(category, count);
        }

        println!("Global Statistics:");
        println!(
            "  Total namespaces: {}",
            global.get::<i64, _>("total_namespaces")
        );
        println!(
            "  Total memories: {}",
            global.get::<i64, _>("total_memories")
        );
        println!(
            "  Active memories: {}",
            global.get::<i64, _>("active_memories")
        );
        println!(
            "  Archived memories: {}",
            global.get::<i64, _>("archived_memories")
        );
        println!();
        println!("By Category:");
        for (category, count) in by_category {
            println!("  {}: {}", category, count);
        }
    }

    tracing::info!("Statistics retrieved");
    Ok(())
}
