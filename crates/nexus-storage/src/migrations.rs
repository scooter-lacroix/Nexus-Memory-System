//! Database migrations

use crate::db_error;
use sqlx::SqlitePool;

/// Run all migrations
pub async fn run_migrations(pool: &SqlitePool) -> crate::Result<()> {
    create_namespaces_table(pool).await?;
    create_memories_table(pool).await?;
    create_task_specifications_table(pool).await?;
    create_memory_relations_table(pool).await?;
    create_system_metrics_table(pool).await?;
    Ok(())
}

async fn create_namespaces_table(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_namespaces (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            agent_type TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn create_memories_table(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace_id INTEGER NOT NULL,
            content TEXT NOT NULL,
            category TEXT NOT NULL DEFAULT 'general',
            memory_lane_type TEXT,
            labels TEXT DEFAULT '[]',
            metadata TEXT DEFAULT '{}',
            similarity_score REAL,
            relevance_score REAL,
            content_embedding TEXT,
            embedding_model TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME,
            last_accessed DATETIME,
            is_active BOOLEAN DEFAULT 1,
            is_archived BOOLEAN DEFAULT 0,
            access_count INTEGER DEFAULT 0,
            FOREIGN KEY (namespace_id) REFERENCES agent_namespaces(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    // Create indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_namespace ON memories(namespace_id)")
        .execute(pool)
        .await
        .map_err(db_error)?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category)")
        .execute(pool)
        .await
        .map_err(db_error)?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at)")
        .execute(pool)
        .await
        .map_err(db_error)?;

    Ok(())
}

async fn create_task_specifications_table(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS task_specifications (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace_id INTEGER NOT NULL,
            spec_id TEXT NOT NULL,
            task_description TEXT NOT NULL,
            spec_content TEXT NOT NULL,
            complexity_score REAL DEFAULT 0.0,
            usage_count INTEGER DEFAULT 0,
            success_rate REAL DEFAULT 0.0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME,
            FOREIGN KEY (namespace_id) REFERENCES agent_namespaces(id),
            UNIQUE(namespace_id, spec_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn create_memory_relations_table(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_relations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_memory_id INTEGER NOT NULL,
            target_memory_id INTEGER NOT NULL,
            relation_type TEXT NOT NULL,
            strength REAL DEFAULT 1.0,
            metadata TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (source_memory_id) REFERENCES memories(id),
            FOREIGN KEY (target_memory_id) REFERENCES memories(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn create_system_metrics_table(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS system_metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            metric_name TEXT NOT NULL,
            metric_value REAL NOT NULL,
            labels TEXT DEFAULT '{}',
            recorded_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;
    Ok(())
}

/// Create the processed_files table for inbox file tracking
pub async fn create_processed_files_table(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS processed_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace_id INTEGER NOT NULL,
            path TEXT NOT NULL,
            content_hash TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            memory_id INTEGER,
            last_error TEXT,
            processed_at DATETIME,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME,
            FOREIGN KEY (namespace_id) REFERENCES namespaces(id),
            FOREIGN KEY (memory_id) REFERENCES memories(id),
            UNIQUE(namespace_id, path)
        );
        CREATE INDEX IF NOT EXISTS idx_processed_files_namespace ON processed_files(namespace_id);
        CREATE INDEX IF NOT EXISTS idx_processed_files_status ON processed_files(status);
        CREATE INDEX IF NOT EXISTS idx_processed_files_path ON processed_files(path);
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    Ok(())
}
