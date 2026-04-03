//! Nexus Storage - Database operations and storage layer
//!
//! This crate provides SQLite-based storage using SQLx.

pub mod migrations;
pub mod models;
pub mod repository;

pub use migrations::create_processed_files_table;
pub use models::*;
pub use repository::{
    ListMemoryFilters, MemoryRelationRepository, MemoryRepository, MetricSample,
    NamespaceRepository, ProcessedFileRepository, SemanticCandidateParams, StoreDigestParams,
    StoreMemoryParams, StoreMemoryWithLineageParams, WorkingSetParams,
};

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;

/// Convert sqlx::Error to NexusError
pub fn db_error(err: sqlx::Error) -> nexus_core::NexusError {
    nexus_core::NexusError::Database(err.to_string())
}

/// Storage manager for database operations
pub struct StorageManager {
    pool: SqlitePool,
    initialized: bool,
}

impl StorageManager {
    /// Create a new storage manager
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            initialized: false,
        }
    }

    /// Create a new storage manager from database URL
    pub async fn from_url(url: &str) -> crate::Result<Self> {
        let options = url
            .parse::<SqliteConnectOptions>()
            .map_err(|err| nexus_core::NexusError::Database(err.to_string()))?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);

        let pool = SqlitePool::connect_with(options).await.map_err(db_error)?;

        // Additional pragmas that must run per-connection
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .map_err(db_error)?;
        sqlx::query("PRAGMA cache_size = -2000")
            .execute(&pool)
            .await
            .map_err(db_error)?;
        sqlx::query("PRAGMA temp_store = MEMORY")
            .execute(&pool)
            .await
            .map_err(db_error)?;

        Ok(Self::new(pool))
    }

    /// Initialize the database schema
    pub async fn initialize(&mut self) -> crate::Result<()> {
        if self.initialized {
            return Ok(());
        }

        migrations::run_migrations(&self.pool).await?;
        self.initialized = true;
        Ok(())
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Check if initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

pub type Result<T> = std::result::Result<T, nexus_core::NexusError>;
