//! Init command implementation

use anyhow::Result;
use nexus_core::Config;
use nexus_storage::StorageManager;
use std::path::Path;

/// Execute the init command
pub async fn execute(reset: bool) -> Result<()> {
    let config = Config::from_env()?;

    // Ensure parent directory exists
    if let Some(parent) = Path::new(&config.database.path).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
            tracing::info!("Created directory: {:?}", parent);
        }
    }

    // Check if database exists
    let db_exists = config.database.path.exists();

    if db_exists && reset {
        tracing::info!("Resetting database at {:?}", config.database.path);
        std::fs::remove_file(&config.database.path)?;
    } else if db_exists {
        tracing::info!("Database already exists at {:?}", config.database.path);
        tracing::info!("Use --reset to reinitialize");
        return Ok(());
    }

    // Initialize database
    tracing::info!("Initializing database at {:?}", config.database.path);
    if !config.database.path.exists() {
        std::fs::File::create(&config.database.path)?;
    }
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    tracing::info!("Database initialized successfully");
    Ok(())
}
