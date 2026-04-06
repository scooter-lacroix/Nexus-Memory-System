//! Database migrations — versioned, idempotent, forward-only.
//!
//! Each migration records its version in the `schema_migrations` table before
//! returning.  On startup `run_migrations()` skips any version that is already
//! recorded, so calling it multiple times is safe.
//!
//! **Pre-migration upgrade path**: if the `schema_migrations` table does not
//! exist we probe the database for tables that belong to each migration.
//! Tables that are already present cause the corresponding migration to be
//! recorded as applied without re-running the DDL.  This preserves existing
//! user databases while still allowing new databases to start from migration 1.

use crate::db_error;
use sqlx::SqlitePool;
use tracing::debug;

// ---------------------------------------------------------------------------
// Migration registry
// ---------------------------------------------------------------------------

/// Metadata for a single migration.
struct MigrationMeta {
    version: i64,
    description: &'static str,
}

const MIGRATIONS: &[MigrationMeta] = &[
    MigrationMeta {
        version: 1,
        description: "agent_namespaces table",
    },
    MigrationMeta {
        version: 2,
        description: "memories table and basic indexes",
    },
    MigrationMeta {
        version: 3,
        description: "task_specifications table",
    },
    MigrationMeta {
        version: 4,
        description: "memory_relations table",
    },
    MigrationMeta {
        version: 5,
        description: "system_metrics table",
    },
    MigrationMeta {
        version: 6,
        description: "memory_jobs table and indexes",
    },
    MigrationMeta {
        version: 7,
        description: "session_digests table and indexes",
    },
    MigrationMeta {
        version: 8,
        description: "memory_evidence table and indexes",
    },
    MigrationMeta {
        version: 9,
        description: "cognitive indexes on memories",
    },
    MigrationMeta {
        version: 10,
        description: "processed_files table and indexes",
    },
];

/// Dispatch a migration by version number.
async fn apply_migration(pool: &SqlitePool, version: i64) -> crate::Result<()> {
    match version {
        1 => migration_001_agent_namespaces(pool).await,
        2 => migration_002_memories(pool).await,
        3 => migration_003_task_specifications(pool).await,
        4 => migration_004_memory_relations(pool).await,
        5 => migration_005_system_metrics(pool).await,
        6 => migration_006_memory_jobs(pool).await,
        7 => migration_007_session_digests(pool).await,
        8 => migration_008_memory_evidence(pool).await,
        9 => migration_009_cognitive_indexes(pool).await,
        10 => migration_010_processed_files(pool).await,
        _ => panic!("unknown migration version: {version}"),
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run all pending migrations in order.
pub async fn run_migrations(pool: &SqlitePool) -> crate::Result<()> {
    ensure_schema_migrations_table(pool).await?;
    upgrade_pre_migration_databases(pool).await?;

    for migration in MIGRATIONS {
        if is_migration_applied(pool, migration.version).await? {
            debug!(
                version = migration.version,
                "migration already applied, skipping"
            );
            continue;
        }
        debug!(
            version = migration.version,
            description = migration.description,
            "applying migration"
        );
        apply_migration(pool, migration.version).await?;
        record_migration(pool, migration.version, migration.description).await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// schema_migrations bookkeeping
// ---------------------------------------------------------------------------

async fn ensure_schema_migrations_table(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            description TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn is_migration_applied(pool: &SqlitePool, version: i64) -> crate::Result<bool> {
    let row =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = ?")
            .bind(version)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    Ok(row > 0)
}

async fn record_migration(pool: &SqlitePool, version: i64, description: &str) -> crate::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO schema_migrations (version, description) VALUES (?, ?)")
        .bind(version)
        .bind(description)
        .execute(pool)
        .await
        .map_err(db_error)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pre-migration upgrade detection
// ---------------------------------------------------------------------------

/// If the database was created by a previous version that did not use versioned
/// migrations, detect the existing schema and record every migration whose
/// table(s) are already present.  This is forward-only: once recorded, a
/// migration is never un-recorded.
async fn upgrade_pre_migration_databases(pool: &SqlitePool) -> crate::Result<()> {
    // If the migrations table is empty we still need to check whether tables
    // exist — the table might have been created above but have zero rows.
    let any_applied = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations")
        .fetch_one(pool)
        .await
        .map_err(db_error)?;
    if any_applied > 0 {
        // Already running in versioned mode; nothing to backfill.
        return Ok(());
    }

    // Probe each migration's defining table(s).  We check tables in reverse
    // order (newest first) so that if a database is partially migrated we
    // still record the right subset.
    let probes: &[(i64, &[&str])] = &[
        (10, &["processed_files"]),
        (9, &[]), // indexes only; covered by migration 2's table
        (8, &["memory_evidence"]),
        (7, &["session_digests"]),
        (6, &["memory_jobs"]),
        (5, &["system_metrics"]),
        (4, &["memory_relations"]),
        (3, &["task_specifications"]),
        (2, &["memories"]),
        (1, &["agent_namespaces"]),
    ];

    for &(version, tables) in probes {
        if tables.is_empty() {
            // Migration 9 adds indexes only — safe to re-run via IF NOT EXISTS.
            continue;
        }
        let mut all_exist = true;
        for t in tables {
            if !table_exists(pool, t).await? {
                all_exist = false;
                break;
            }
        }
        if all_exist {
            record_migration(
                pool,
                version,
                MIGRATIONS
                    .iter()
                    .find(|m| m.version == version)
                    .map(|m| m.description)
                    .unwrap_or("unknown"),
            )
            .await?;
        }
    }

    Ok(())
}

/// Check whether a table exists in the SQLite database.
///
/// Returns `Err` on query failure so that real database problems are not
/// silently treated as "table absent".
async fn table_exists(pool: &SqlitePool, name: &str) -> crate::Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?")
            .bind(name)
            .fetch_one(pool)
            .await
            .map_err(crate::db_error)?;
    Ok(count > 0)
}

// ---------------------------------------------------------------------------
// Individual migrations (ordered, forward-only)
// ---------------------------------------------------------------------------

async fn migration_001_agent_namespaces(pool: &SqlitePool) -> crate::Result<()> {
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

async fn migration_002_memories(pool: &SqlitePool) -> crate::Result<()> {
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

async fn migration_003_task_specifications(pool: &SqlitePool) -> crate::Result<()> {
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

async fn migration_004_memory_relations(pool: &SqlitePool) -> crate::Result<()> {
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
            UNIQUE(source_memory_id, target_memory_id, relation_type),
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

async fn migration_005_system_metrics(pool: &SqlitePool) -> crate::Result<()> {
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

async fn migration_006_memory_jobs(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace_id INTEGER NOT NULL,
            job_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            priority INTEGER NOT NULL DEFAULT 100,
            perspective_json TEXT,
            payload_json TEXT NOT NULL,
            lease_owner TEXT,
            claim_token TEXT,
            lease_expires_at TEXT,
            attempts INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_memory_jobs_ready
            ON memory_jobs(status, priority, created_at);
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    ensure_column_exists(pool, "memory_jobs", "claim_token", "TEXT").await?;

    Ok(())
}

async fn migration_007_session_digests(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session_digests (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            namespace_id INTEGER NOT NULL,
            session_key TEXT NOT NULL,
            digest_kind TEXT NOT NULL,
            memory_id INTEGER NOT NULL,
            start_memory_id INTEGER,
            end_memory_id INTEGER,
            token_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_session_digests_unique
            ON session_digests(namespace_id, session_key, digest_kind, end_memory_id);
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    Ok(())
}

async fn migration_008_memory_evidence(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_evidence (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            derived_memory_id INTEGER NOT NULL,
            source_memory_id INTEGER NOT NULL,
            evidence_role TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_evidence_unique
            ON memory_evidence(derived_memory_id, source_memory_id, evidence_role);
        CREATE INDEX IF NOT EXISTS idx_memory_evidence_derived
            ON memory_evidence(derived_memory_id);
        CREATE INDEX IF NOT EXISTS idx_memory_evidence_source
            ON memory_evidence(source_memory_id);
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    Ok(())
}

async fn migration_009_cognitive_indexes(pool: &SqlitePool) -> crate::Result<()> {
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_memories_cognitive_level ON memories(json_extract(metadata, '$.cognitive.level'))"
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memories_cognitive_perspective
            ON memories(
                namespace_id,
                json_extract(metadata, '$.cognitive.observer'),
                json_extract(metadata, '$.cognitive.subject'),
                json_extract(metadata, '$.cognitive.session_key')
            )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memories_cognitive_reinforcement
            ON memories(
                namespace_id,
                json_extract(metadata, '$.cognitive.level'),
                json_extract(metadata, '$.cognitive.times_reinforced')
            )
        "#,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;

    Ok(())
}

async fn migration_010_processed_files(pool: &SqlitePool) -> crate::Result<()> {
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
            FOREIGN KEY (namespace_id) REFERENCES agent_namespaces(id),
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn ensure_column_exists(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    definition: &str,
) -> crate::Result<()> {
    let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    match sqlx::query(&sql).execute(pool).await {
        Ok(_) => Ok(()),
        Err(error) => {
            let message = error.to_string().to_lowercase();
            if message.contains("duplicate column name") {
                Ok(())
            } else {
                Err(db_error(error))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Backward-compatible public exports
// ---------------------------------------------------------------------------

/// Create the processed_files table (legacy public entry point).
/// Calls the full migration suite so that all dependencies are in place.
pub async fn create_processed_files_table(pool: &SqlitePool) -> crate::Result<()> {
    run_migrations(pool).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn new_empty_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    /// Test: fresh database — all migrations apply in order.
    #[tokio::test]
    async fn test_fresh_database_all_migrations_apply() {
        let pool = new_empty_pool().await;
        run_migrations(&pool).await.unwrap();

        // All 10 migrations should be recorded.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 10);

        // Verify every expected table exists.
        for table in &[
            "agent_namespaces",
            "memories",
            "task_specifications",
            "memory_relations",
            "system_metrics",
            "memory_jobs",
            "session_digests",
            "memory_evidence",
            "processed_files",
        ] {
            let exists = table_exists(&pool, table).await.unwrap();
            assert!(exists, "table {table} should exist after migrations");
        }
    }

    /// Test: running migrations twice is idempotent.
    #[tokio::test]
    async fn test_migrations_idempotent() {
        let pool = new_empty_pool().await;
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 10);
    }

    /// Test: pre-migration database — existing tables detected and backfilled.
    #[tokio::test]
    async fn test_upgrade_from_pre_migration_database() {
        let pool = new_empty_pool().await;

        // Simulate the old ad-hoc schema creation (exactly what the previous
        // run_migrations did, but WITHOUT a schema_migrations table).
        migration_001_agent_namespaces(&pool).await.unwrap();
        migration_002_memories(&pool).await.unwrap();
        migration_003_task_specifications(&pool).await.unwrap();
        migration_004_memory_relations(&pool).await.unwrap();
        migration_005_system_metrics(&pool).await.unwrap();
        migration_006_memory_jobs(&pool).await.unwrap();
        migration_007_session_digests(&pool).await.unwrap();
        migration_008_memory_evidence(&pool).await.unwrap();
        migration_009_cognitive_indexes(&pool).await.unwrap();
        migration_010_processed_files(&pool).await.unwrap();

        // Verify schema_migrations does NOT exist yet.
        let exists = table_exists(&pool, "schema_migrations").await.unwrap();
        assert!(!exists, "schema_migrations should not exist before upgrade");

        // Now run the versioned migration system — it should detect
        // existing tables and backfill without errors.
        run_migrations(&pool).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 10);
    }

    /// Test: partially migrated database — only pending migrations run.
    #[tokio::test]
    async fn test_partially_migrated_database() {
        let pool = new_empty_pool().await;

        // Simulate old ad-hoc creation of just the first 5 tables.
        migration_001_agent_namespaces(&pool).await.unwrap();
        migration_002_memories(&pool).await.unwrap();
        migration_003_task_specifications(&pool).await.unwrap();
        migration_004_memory_relations(&pool).await.unwrap();
        migration_005_system_metrics(&pool).await.unwrap();

        run_migrations(&pool).await.unwrap();

        // Migrations 1-5 should be backfilled, 6-10 newly applied.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 10);

        // Verify later tables now exist too.
        assert!(table_exists(&pool, "memory_jobs").await.unwrap());
        assert!(table_exists(&pool, "session_digests").await.unwrap());
        assert!(table_exists(&pool, "memory_evidence").await.unwrap());
        assert!(table_exists(&pool, "processed_files").await.unwrap());
    }

    #[tokio::test]
    async fn test_existing_memories_table_still_runs_cognitive_index_migration() {
        let pool = new_empty_pool().await;

        migration_001_agent_namespaces(&pool).await.unwrap();
        migration_002_memories(&pool).await.unwrap();

        let index_count_before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_memories_cognitive_level'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(index_count_before, 0);

        run_migrations(&pool).await.unwrap();

        let index_count_after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_memories_cognitive_level'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(index_count_after, 1);

        let recorded: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations WHERE version = 9")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(recorded, 1);
    }

    /// Test: brand-new database with no pre-existing tables starts from 1.
    #[tokio::test]
    async fn test_new_database_starts_from_scratch() {
        let pool = new_empty_pool().await;
        run_migrations(&pool).await.unwrap();

        // Verify migration versions are 1..=10 in order.
        let versions: Vec<i64> =
            sqlx::query_scalar("SELECT version FROM schema_migrations ORDER BY version")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(versions, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }
}
