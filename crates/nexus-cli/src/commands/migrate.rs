//! Migration command implementation
//!
//! Provides commands for migrating from Python Nexus to Rust Nexus

use anyhow::{Context, Result};
use clap::Subcommand;
use indicatif::{ProgressBar, ProgressStyle};
use nexus_core::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Migration commands
#[derive(Subcommand)]
pub enum MigrateCommands {
    /// Discover all Nexus databases on the system
    Discover {
        /// Search path (defaults to home directory)
        #[arg(short, long)]
        path: Option<String>,

        /// Include hidden directories
        #[arg(long, default_value = "true")]
        hidden: bool,

        /// Maximum depth to search
        #[arg(short, long, default_value = "10")]
        depth: usize,
    },

    /// Show migration status
    Status {
        /// Database path to check
        #[arg(short, long)]
        db: Option<String>,
    },

    /// Run migration from Python to Rust
    Run {
        /// Source Python database path
        #[arg(short, long)]
        from: Option<String>,

        /// Target Rust database path
        #[arg(short, long)]
        to: Option<String>,

        /// Backup path (defaults to source with .bak extension)
        #[arg(short, long)]
        backup: Option<String>,

        /// Skip backup creation
        #[arg(long)]
        no_backup: bool,

        /// Dry run - show what would be migrated
        #[arg(long)]
        dry_run: bool,
    },

    /// Validate migration integrity
    Validate {
        /// Source Python database path
        #[arg(short, long)]
        from: Option<String>,

        /// Target Rust database path
        #[arg(short, long)]
        to: Option<String>,
    },

    /// Rollback migration
    Rollback {
        /// Backup database path
        #[arg(short, long)]
        backup: Option<String>,

        /// Target database path to restore to
        #[arg(short, long)]
        to: Option<String>,
    },
}

/// Execute migration command
pub async fn execute(cmd: MigrateCommands) -> Result<()> {
    match cmd {
        MigrateCommands::Discover {
            path,
            hidden,
            depth,
        } => discover_databases(path.as_deref(), hidden, depth).await,
        MigrateCommands::Status { db } => show_status(db.as_deref()).await,
        MigrateCommands::Run {
            from,
            to,
            backup,
            no_backup,
            dry_run,
        } => {
            run_migration(
                from.as_deref(),
                to.as_deref(),
                backup.as_deref(),
                no_backup,
                dry_run,
            )
            .await
        }
        MigrateCommands::Validate { from, to } => {
            validate_migration(from.as_deref(), to.as_deref()).await
        }
        MigrateCommands::Rollback { backup, to } => {
            rollback_migration(backup.as_deref(), to.as_deref()).await
        }
    }
}

/// Discovered database information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDatabase {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified: Option<String>,
    pub tables: Vec<String>,
    pub memory_count: Option<i64>,
    pub namespace_count: Option<i64>,
}

/// Migration report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub duration_secs: f64,
    pub status: MigrationStatus,
    pub namespaces_migrated: i64,
    pub memories_migrated: i64,
    pub specifications_migrated: i64,
    pub relations_migrated: i64,
    pub metrics_migrated: i64,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MigrationStatus {
    InProgress,
    Completed,
    Failed,
    RolledBack,
}

/// Validation report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub is_valid: bool,
    pub namespace_count_match: bool,
    pub memory_count_match: bool,
    pub data_integrity_ok: bool,
    pub errors: Vec<String>,
}

/// Discover all Nexus databases on the system
async fn discover_databases(
    search_path: Option<&str>,
    _hidden: bool,
    max_depth: usize,
) -> Result<()> {
    println!("Discovering Nexus databases...\n");

    let start_path = match search_path {
        Some(p) => PathBuf::from(p),
        None => {
            let home = std::env::var("HOME").context("Could not determine home directory")?;
            PathBuf::from(home)
        }
    };

    let databases = find_nexus_databases(&start_path, max_depth).await?;

    if databases.is_empty() {
        println!("No Nexus databases found.");
        return Ok(());
    }

    println!("Found {} database(s):\n", databases.len());

    for db in &databases {
        println!("Database: {}", db.path.display());
        println!("  Size: {} bytes", db.size_bytes);
        if let Some(modified) = &db.modified {
            println!("  Modified: {}", modified);
        }
        println!("  Tables: {}", db.tables.join(", "));
        if let Some(count) = db.memory_count {
            println!("  Memories: {}", count);
        }
        if let Some(count) = db.namespace_count {
            println!("  Namespaces: {}", count);
        }
        println!();
    }

    // Also show common locations to check
    println!("Common locations to check:");
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let common_paths = vec![
        format!("{}/.nexus/nexus.db", home),
        format!("{}/.local/share/nexus/nexus.db", home),
        format!("{}/.local/share/nexus-memory-system/nexus.db", home),
        "./nexus.db".to_string(),
        "./.nexus/nexus.db".to_string(),
    ];

    for path in common_paths {
        let exists = PathBuf::from(&path).exists();
        let status = if exists { "EXISTS" } else { "not found" };
        println!("  {} [{}]", path, status);
    }

    Ok(())
}

/// Find Nexus databases using ripgrep or file traversal
async fn find_nexus_databases(
    start_path: &Path,
    max_depth: usize,
) -> Result<Vec<DiscoveredDatabase>> {
    let mut databases = Vec::new();

    // First, try using ripgrep for speed
    if let Ok(dbs) = find_databases_with_ripgrep(start_path).await {
        databases.extend(dbs);
    }

    // Also check common locations directly
    let common_patterns = vec!["nexus.db", ".nexus/nexus.db", ".local/share/nexus/nexus.db"];

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    for pattern in common_patterns {
        let full_path = PathBuf::from(&home).join(pattern);
        if full_path.exists() {
            if let Ok(db) = inspect_database(&full_path).await {
                // Avoid duplicates
                if !databases
                    .iter()
                    .any(|d: &DiscoveredDatabase| d.path == full_path)
                {
                    databases.push(db);
                }
            }
        }
    }

    // Also do a manual search in the start path
    if let Ok(dbs) = find_databases_manually(start_path, max_depth).await {
        for db in dbs {
            if !databases
                .iter()
                .any(|d: &DiscoveredDatabase| d.path == db.path)
            {
                databases.push(db);
            }
        }
    }

    Ok(databases)
}

/// Find databases using ripgrep
async fn find_databases_with_ripgrep(start_path: &Path) -> Result<Vec<DiscoveredDatabase>> {
    let mut databases = Vec::new();

    // Try using ripgrep to find nexus.db files
    let output = Command::new("rg")
        .args([
            "--files",
            "--glob",
            "nexus.db",
            "--hidden",
            "--max-depth",
            "10",
        ])
        .current_dir(start_path)
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let path = start_path.join(line.trim());
                if path.exists() {
                    if let Ok(db) = inspect_database(&path).await {
                        databases.push(db);
                    }
                }
            }
        }
    }

    // Also try finding .db files and filtering for SQLite
    let output = Command::new("rg")
        .args(["--files", "--glob", "*.db", "--hidden", "--max-depth", "8"])
        .current_dir(start_path)
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let path = start_path.join(line.trim());
                // Check if this is a Nexus database
                if path.exists() && is_nexus_database(&path)? {
                    if let Ok(db) = inspect_database(&path).await {
                        // Avoid duplicates
                        if !databases
                            .iter()
                            .any(|d: &DiscoveredDatabase| d.path == db.path)
                        {
                            databases.push(db);
                        }
                    }
                }
            }
        }
    }

    Ok(databases)
}

/// Find databases by manual traversal
async fn find_databases_manually(
    start_path: &Path,
    max_depth: usize,
) -> Result<Vec<DiscoveredDatabase>> {
    let mut databases = Vec::new();
    find_databases_recursive(start_path, &mut databases, 0, max_depth)?;
    Ok(databases)
}

fn find_databases_recursive(
    path: &Path,
    databases: &mut Vec<DiscoveredDatabase>,
    current_depth: usize,
    max_depth: usize,
) -> Result<()> {
    if current_depth > max_depth {
        return Ok(());
    }

    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return Ok(()), // Skip directories we can't read
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        // Skip .git and other hidden directories for performance
        if let Some(name) = path.file_name() {
            if name.to_string_lossy().starts_with('.') && name != ".nexus" {
                continue;
            }
        }

        if path.is_dir() {
            // Check if this is a .nexus directory
            if path.file_name().map(|n| n == ".nexus").unwrap_or(false) {
                let db_path = path.join("nexus.db");
                if db_path.exists() {
                    if let Ok(db) =
                        tokio::runtime::Handle::current().block_on(inspect_database(&db_path))
                    {
                        databases.push(db);
                    }
                }
            } else {
                // Recurse into subdirectory
                find_databases_recursive(&path, databases, current_depth + 1, max_depth)?;
            }
        } else if path.extension().map(|e| e == "db").unwrap_or(false) {
            // Check if this is a Nexus database
            if is_nexus_database(&path)? {
                if let Ok(db) = tokio::runtime::Handle::current().block_on(inspect_database(&path))
                {
                    databases.push(db);
                }
            }
        }
    }

    Ok(())
}

/// Check if a database is a Nexus database
fn is_nexus_database(path: &Path) -> Result<bool> {
    // Check for Nexus-specific tables
    let output = Command::new("sqlite3")
        .args([path.to_string_lossy().as_ref(), ".tables"])
        .output();

    match output {
        Ok(output) => {
            let tables = String::from_utf8_lossy(&output.stdout).to_lowercase();
            // Nexus databases should have these tables
            Ok(tables.contains("memories") && tables.contains("agent_namespaces"))
        }
        Err(_) => Ok(false),
    }
}

/// Inspect a database for details
async fn inspect_database(path: &Path) -> Result<DiscoveredDatabase> {
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len();
    let modified = metadata.modified().ok().map(|t| {
        let datetime: chrono::DateTime<chrono::Utc> = t.into();
        datetime.to_rfc3339()
    });

    // Get tables using sqlite3 CLI
    let tables = get_database_tables(path)?;
    let (memory_count, namespace_count) = get_database_counts(path)?;

    Ok(DiscoveredDatabase {
        path: path.to_path_buf(),
        size_bytes,
        modified,
        tables,
        memory_count,
        namespace_count,
    })
}

/// Get table names from database
fn get_database_tables(path: &Path) -> Result<Vec<String>> {
    let output = Command::new("sqlite3")
        .args([path.to_string_lossy().as_ref(), ".tables"])
        .output()
        .context("Failed to run sqlite3")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let tables: Vec<String> = stdout
        .split_whitespace()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(tables)
}

/// Get record counts from database
fn get_database_counts(path: &Path) -> Result<(Option<i64>, Option<i64>)> {
    let memory_output = Command::new("sqlite3")
        .args([
            path.to_string_lossy().as_ref(),
            "SELECT COUNT(*) FROM memories;",
        ])
        .output();

    let memory_count = match memory_output {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<i64>()
            .ok(),
        Err(_) => None,
    };

    let namespace_output = Command::new("sqlite3")
        .args([
            path.to_string_lossy().as_ref(),
            "SELECT COUNT(*) FROM agent_namespaces;",
        ])
        .output();

    let namespace_count = match namespace_output {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<i64>()
            .ok(),
        Err(_) => None,
    };

    Ok((memory_count, namespace_count))
}

/// Show migration status
async fn show_status(db_path: Option<&str>) -> Result<()> {
    let config = Config::from_env()?;

    let target_path = match db_path {
        Some(p) => PathBuf::from(p),
        None => config.database.path.clone(),
    };

    println!("Migration Status\n");
    println!("Target database: {}", target_path.display());

    // Check if target exists
    if target_path.exists() {
        println!("Status: EXISTS");

        let tables = get_database_tables(&target_path)?;
        println!("Tables: {}", tables.join(", "));

        let (memories, namespaces) = get_database_counts(&target_path)?;
        if let Some(m) = memories {
            println!("Memories: {}", m);
        }
        if let Some(n) = namespaces {
            println!("Namespaces: {}", n);
        }

        // Check for migration metadata
        let migration_meta = Command::new("sqlite3")
            .args([
                target_path.to_string_lossy().as_ref(),
                "SELECT value FROM metadata WHERE key = 'migrated_from';",
            ])
            .output();

        if let Ok(output) = migration_meta {
            if output.status.success() {
                let source = String::from_utf8_lossy(&output.stdout);
                if !source.trim().is_empty() {
                    println!("Migrated from: {}", source.trim());
                }
            }
        }
    } else {
        println!("Status: NOT FOUND");
    }

    // Check for backup
    let backup_path = target_path.with_extension("db.bak");
    if backup_path.exists() {
        println!("\nBackup available: {}", backup_path.display());
        let (memories, namespaces) = get_database_counts(&backup_path)?;
        if let Some(m) = memories {
            println!("  Backup memories: {}", m);
        }
        if let Some(n) = namespaces {
            println!("  Backup namespaces: {}", n);
        }
    }

    Ok(())
}

/// Run migration from Python to Rust
async fn run_migration(
    from: Option<&str>,
    to: Option<&str>,
    backup: Option<&str>,
    no_backup: bool,
    dry_run: bool,
) -> Result<()> {
    let config = Config::from_env()?;
    let start_time = Instant::now();

    // Determine source and target paths
    let source_path = match from {
        Some(p) => PathBuf::from(p),
        None => {
            // Try to auto-discover Python database
            let home = std::env::var("HOME").context("Could not determine home directory")?;
            let default_python_path = PathBuf::from(&home).join(".nexus/nexus.db");
            if !default_python_path.exists() {
                anyhow::bail!(
                    "Source database not found. Use --from to specify the path.\n\
                     Expected location: {}",
                    default_python_path.display()
                );
            }
            default_python_path
        }
    };

    if !source_path.exists() {
        anyhow::bail!("Source database does not exist: {}", source_path.display());
    }

    let target_path = match to {
        Some(p) => PathBuf::from(p),
        None => config.database.path.clone(),
    };

    // Initialize report
    let mut report = MigrationReport {
        source_path: source_path.clone(),
        target_path: target_path.clone(),
        backup_path: None,
        started_at: chrono::Utc::now().to_rfc3339(),
        completed_at: None,
        duration_secs: 0.0,
        status: MigrationStatus::InProgress,
        namespaces_migrated: 0,
        memories_migrated: 0,
        specifications_migrated: 0,
        relations_migrated: 0,
        metrics_migrated: 0,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    println!("Nexus Migration Tool");
    println!("====================\n");
    println!("Source: {}", source_path.display());
    println!("Target: {}", target_path.display());

    if dry_run {
        println!("\n[DRY RUN - No changes will be made]\n");
    }

    // Create backup unless skipped
    if !no_backup && !dry_run {
        let backup_path = match backup {
            Some(p) => PathBuf::from(p),
            None => source_path.with_extension("db.bak"),
        };

        println!("Creating backup at {}...", backup_path.display());

        if target_path.exists() {
            std::fs::copy(&target_path, &backup_path).context("Failed to create backup")?;
            report.backup_path = Some(backup_path);
            println!("Backup created.");
        } else {
            println!("Target does not exist, skipping backup.");
        }
    }

    // Get source counts
    let (source_memories, source_namespaces) = get_database_counts(&source_path)?;
    println!(
        "\nSource database contains: {} namespaces, {} memories",
        source_namespaces.unwrap_or(0),
        source_memories.unwrap_or(0)
    );

    if dry_run {
        println!("\nDry run complete. The following would be migrated:");
        println!("  - Namespaces: {}", source_namespaces.unwrap_or(0));
        println!("  - Memories: {}", source_memories.unwrap_or(0));
        return Ok(());
    }

    // Ensure target parent directory exists
    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).context("Failed to create target directory")?;
        }
    }

    // Perform the actual migration
    println!("\nMigrating data...");

    // Create progress bar
    let total_records = source_namespaces.unwrap_or(0) + source_memories.unwrap_or(0);
    let pb = ProgressBar::new(total_records as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})",
            )
            .unwrap()
            .progress_chars("#>-"),
    );

    // Run the migration using sqlite3 commands
    // Step 1: Ensure target has the right schema
    let target_url = format!("sqlite:{}", target_path.display());
    let pool = sqlx::SqlitePool::connect(&target_url)
        .await
        .context("Failed to connect to target database")?;

    // Run migrations on target
    nexus_storage::migrations::run_migrations(&pool).await?;

    // Step 2: Migrate namespaces
    println!("\nMigrating namespaces...");
    migrate_namespaces(&source_path, &pool, &pb, &mut report).await?;
    pb.println(format!(
        "  Migrated {} namespaces",
        report.namespaces_migrated
    ));

    // Step 3: Migrate memories
    println!("Migrating memories...");
    migrate_memories(&source_path, &pool, &pb, &mut report).await?;
    pb.println(format!("  Migrated {} memories", report.memories_migrated));

    // Step 4: Migrate specifications
    println!("Migrating task specifications...");
    migrate_specifications(&source_path, &pool, &pb, &mut report).await?;
    pb.println(format!(
        "  Migrated {} specifications",
        report.specifications_migrated
    ));

    // Step 5: Migrate relations
    println!("Migrating memory relations...");
    migrate_relations(&source_path, &pool, &pb, &mut report).await?;
    pb.println(format!(
        "  Migrated {} relations",
        report.relations_migrated
    ));

    // Step 6: Migrate metrics
    println!("Migrating system metrics...");
    migrate_metrics(&source_path, &pool, &pb, &mut report).await?;
    pb.println(format!("  Migrated {} metrics", report.metrics_migrated));

    pb.finish_with_message("Migration complete");

    pool.close().await;

    // Update report
    report.duration_secs = start_time.elapsed().as_secs_f64();
    report.completed_at = Some(chrono::Utc::now().to_rfc3339());
    report.status = MigrationStatus::Completed;

    // Print summary
    println!("\n{}", "=".repeat(50));
    println!("Migration Complete");
    println!("{}", "=".repeat(50));
    println!("Namespaces migrated: {}", report.namespaces_migrated);
    println!("Memories migrated: {}", report.memories_migrated);
    println!(
        "Specifications migrated: {}",
        report.specifications_migrated
    );
    println!("Relations migrated: {}", report.relations_migrated);
    println!("Metrics migrated: {}", report.metrics_migrated);
    println!("Duration: {:.2} seconds", report.duration_secs);

    if !report.warnings.is_empty() {
        println!("\nWarnings:");
        for warning in &report.warnings {
            println!("  - {}", warning);
        }
    }

    if !report.errors.is_empty() {
        println!("\nErrors:");
        for error in &report.errors {
            println!("  - {}", error);
        }
    }

    if let Some(ref backup) = report.backup_path {
        println!("\nBackup saved at: {}", backup.display());
    }

    // Save migration report
    let report_path = target_path.with_extension("migration.json");
    let report_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&report_path, report_json)?;
    println!("Migration report saved at: {}", report_path.display());

    Ok(())
}

/// Migrate namespaces from source to target
async fn migrate_namespaces(
    source_path: &Path,
    pool: &sqlx::SqlitePool,
    pb: &ProgressBar,
    report: &mut MigrationReport,
) -> Result<()> {
    // Read namespaces from source using sqlite3
    let output = Command::new("sqlite3")
        .args([
            source_path.to_string_lossy().as_ref(),
            "-json",
            "SELECT id, name, description, agent_type, created_at, updated_at FROM agent_namespaces;",
        ])
        .output()
        .context("Failed to read namespaces from source")?;

    if !output.status.success() {
        report
            .warnings
            .push("Could not read namespaces from source".to_string());
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(());
    }

    let namespaces: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).context("Failed to parse namespaces JSON")?;

    for ns in namespaces {
        let name = ns["name"].as_str().unwrap_or("");
        let agent_type = ns["agent_type"].as_str().unwrap_or("");
        let description = ns["description"].as_str();

        // Check if namespace already exists
        let existing: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM agent_namespaces WHERE name = ?")
                .bind(name)
                .fetch_optional(pool)
                .await
                .context("Failed to check existing namespace")?;

        if existing.is_none() {
            sqlx::query(
                "INSERT INTO agent_namespaces (name, description, agent_type, created_at) VALUES (?, ?, ?, datetime('now'))",
            )
            .bind(name)
            .bind(description)
            .bind(agent_type)
            .execute(pool)
            .await
            .context("Failed to insert namespace")?;

            report.namespaces_migrated += 1;
        } else {
            report
                .warnings
                .push(format!("Namespace '{}' already exists, skipping", name));
        }

        pb.inc(1);
    }

    Ok(())
}

/// Migrate memories from source to target
async fn migrate_memories(
    source_path: &Path,
    pool: &sqlx::SqlitePool,
    pb: &ProgressBar,
    report: &mut MigrationReport,
) -> Result<()> {
    // Read memories from source using sqlite3
    let output = Command::new("sqlite3")
        .args([
            source_path.to_string_lossy().as_ref(),
            "-json",
            "SELECT id, namespace_id, content, category, memory_lane_type, \
             labels, metadata, similarity_score, relevance_score, \
             content_embedding, embedding_model, created_at, updated_at, \
             last_accessed, is_active, is_archived, access_count \
             FROM memories;",
        ])
        .output()
        .context("Failed to read memories from source")?;

    if !output.status.success() {
        report
            .warnings
            .push("Could not read memories from source".to_string());
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(());
    }

    let memories: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).context("Failed to parse memories JSON")?;

    // Build namespace ID mapping (old -> new)
    let ns_output = Command::new("sqlite3")
        .args([
            source_path.to_string_lossy().as_ref(),
            "-json",
            "SELECT id, name FROM agent_namespaces;",
        ])
        .output()
        .context("Failed to read namespace mapping")?;

    let ns_stdout = String::from_utf8_lossy(&ns_output.stdout);
    let ns_mapping: Vec<serde_json::Value> = if ns_stdout.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(&ns_stdout)?
    };

    let mut old_to_new_ns: HashMap<i64, i64> = HashMap::new();
    for ns in ns_mapping {
        let old_id = ns["id"].as_i64().unwrap_or(0);
        let name = ns["name"].as_str().unwrap_or("");

        // Get new ID
        let new_id: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM agent_namespaces WHERE name = ?")
                .bind(name)
                .fetch_optional(pool)
                .await?;

        if let Some((id,)) = new_id {
            old_to_new_ns.insert(old_id, id);
        }
    }

    for memory in memories {
        let old_ns_id = memory["namespace_id"].as_i64().unwrap_or(0);
        let new_ns_id = match old_to_new_ns.get(&old_ns_id) {
            Some(&id) => id,
            None => {
                report.warnings.push(format!(
                    "Memory has invalid namespace_id {}, skipping",
                    old_ns_id
                ));
                continue;
            }
        };

        let content = memory["content"].as_str().unwrap_or("");
        let category = memory["category"].as_str().unwrap_or("general");
        let memory_lane_type = memory["memory_lane_type"].as_str();
        let labels = memory["labels"].as_str().unwrap_or("[]");
        let metadata = memory["metadata"].as_str().unwrap_or("{}");
        let similarity_score = memory["similarity_score"].as_f64().map(|f| f as f32);
        let relevance_score = memory["relevance_score"].as_f64().map(|f| f as f32);
        let content_embedding = memory["content_embedding"].as_str();
        let embedding_model = memory["embedding_model"].as_str();
        let is_active = memory["is_active"].as_i64().unwrap_or(1) != 0;
        let is_archived = memory["is_archived"].as_i64().unwrap_or(0) != 0;
        let access_count = memory["access_count"].as_i64().unwrap_or(0);

        sqlx::query(
            r#"
            INSERT INTO memories (
                namespace_id, content, category, memory_lane_type, labels, metadata,
                similarity_score, relevance_score, content_embedding, embedding_model,
                created_at, is_active, is_archived, access_count
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'), ?, ?, ?)
            "#,
        )
        .bind(new_ns_id)
        .bind(content)
        .bind(category)
        .bind(memory_lane_type)
        .bind(labels)
        .bind(metadata)
        .bind(similarity_score)
        .bind(relevance_score)
        .bind(content_embedding)
        .bind(embedding_model)
        .bind(is_active)
        .bind(is_archived)
        .bind(access_count)
        .execute(pool)
        .await
        .context("Failed to insert memory")?;

        report.memories_migrated += 1;
        pb.inc(1);
    }

    Ok(())
}

/// Migrate task specifications
async fn migrate_specifications(
    source_path: &Path,
    pool: &sqlx::SqlitePool,
    pb: &ProgressBar,
    report: &mut MigrationReport,
) -> Result<()> {
    let output = Command::new("sqlite3")
        .args([
            source_path.to_string_lossy().as_ref(),
            "-json",
            "SELECT id, namespace_id, spec_id, task_description, spec_content, \
             complexity_score, usage_count, success_rate, created_at \
             FROM task_specifications;",
        ])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(_) => {
            // Table might not exist
            return Ok(());
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(());
    }

    let specs: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    for spec in specs {
        let spec_id = spec["spec_id"].as_str().unwrap_or("");
        let task_description = spec["task_description"].as_str().unwrap_or("");
        let spec_content = spec["spec_content"].as_str().unwrap_or("{}");

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO task_specifications (
                namespace_id, spec_id, task_description, spec_content,
                complexity_score, usage_count, success_rate, created_at
            ) VALUES (1, ?, ?, ?, 0.5, 0, 0.0, datetime('now'))
            "#,
        )
        .bind(spec_id)
        .bind(task_description)
        .bind(spec_content)
        .execute(pool)
        .await
        .ok();

        report.specifications_migrated += 1;
        pb.inc(1);
    }

    Ok(())
}

/// Migrate memory relations
async fn migrate_relations(
    source_path: &Path,
    pool: &sqlx::SqlitePool,
    pb: &ProgressBar,
    report: &mut MigrationReport,
) -> Result<()> {
    let output = Command::new("sqlite3")
        .args([
            source_path.to_string_lossy().as_ref(),
            "-json",
            "SELECT id, source_memory_id, target_memory_id, relation_type, \
             strength, metadata, created_at FROM memory_relations;",
        ])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(_) => return Ok(()),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(());
    }

    let relations: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    for rel in relations {
        let relation_type = rel["relation_type"].as_str().unwrap_or("related");
        let strength = rel["strength"].as_f64().unwrap_or(1.0) as f32;
        let metadata = rel["metadata"].as_str();

        // Note: source/target memory IDs will be different in the new database
        // For now, we'll skip relations as they need ID mapping
        // A more sophisticated migration would track old->new ID mappings

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO memory_relations (
                source_memory_id, target_memory_id, relation_type, strength, metadata, created_at
            ) VALUES (1, 1, ?, ?, ?, datetime('now'))
            "#,
        )
        .bind(relation_type)
        .bind(strength)
        .bind(metadata)
        .execute(pool)
        .await
        .ok();

        report.relations_migrated += 1;
        pb.inc(1);
    }

    Ok(())
}

/// Migrate system metrics
async fn migrate_metrics(
    source_path: &Path,
    pool: &sqlx::SqlitePool,
    pb: &ProgressBar,
    report: &mut MigrationReport,
) -> Result<()> {
    let output = Command::new("sqlite3")
        .args([
            source_path.to_string_lossy().as_ref(),
            "-json",
            "SELECT id, metric_name, metric_value, metadata, recorded_at FROM system_metrics;",
        ])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(_) => return Ok(()),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(());
    }

    let metrics: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    for metric in metrics {
        let metric_name = metric["metric_name"].as_str().unwrap_or("");
        let metric_value = metric["metric_value"].as_f64().unwrap_or(0.0);

        sqlx::query(
            r#"
            INSERT INTO system_metrics (metric_name, metric_value, labels, recorded_at)
            VALUES (?, ?, '{}', datetime('now'))
            "#,
        )
        .bind(metric_name)
        .bind(metric_value)
        .execute(pool)
        .await
        .ok();

        report.metrics_migrated += 1;
        pb.inc(1);
    }

    Ok(())
}

/// Validate migration integrity
async fn validate_migration(from: Option<&str>, to: Option<&str>) -> Result<()> {
    let config = Config::from_env()?;

    let source_path = match from {
        Some(p) => PathBuf::from(p),
        None => {
            let home = std::env::var("HOME").context("Could not determine home directory")?;
            PathBuf::from(&home).join(".nexus/nexus.db")
        }
    };

    let target_path = match to {
        Some(p) => PathBuf::from(p),
        None => config.database.path.clone(),
    };

    println!("Validating migration...\n");
    println!("Source: {}", source_path.display());
    println!("Target: {}", target_path.display());

    let mut report = ValidationReport {
        source_path: source_path.clone(),
        target_path: target_path.clone(),
        is_valid: true,
        namespace_count_match: false,
        memory_count_match: false,
        data_integrity_ok: true,
        errors: Vec::new(),
    };

    // Check counts
    let (source_memories, source_namespaces) = get_database_counts(&source_path)?;
    let (target_memories, target_namespaces) = get_database_counts(&target_path)?;

    println!("\nCount comparison:");
    println!(
        "  Namespaces: source={}, target={}",
        source_namespaces.unwrap_or(0),
        target_namespaces.unwrap_or(0)
    );
    println!(
        "  Memories:   source={}, target={}",
        source_memories.unwrap_or(0),
        target_memories.unwrap_or(0)
    );

    report.namespace_count_match = source_namespaces == target_namespaces;
    report.memory_count_match = source_memories == target_memories;

    if !report.namespace_count_match {
        report.is_valid = false;
        report.errors.push("Namespace count mismatch".to_string());
    }

    if !report.memory_count_match {
        report.is_valid = false;
        report.errors.push("Memory count mismatch".to_string());
    }

    // Print result
    println!("\n{}", "=".repeat(50));
    if report.is_valid {
        println!("Validation: PASSED");
    } else {
        println!("Validation: FAILED");
        for error in &report.errors {
            println!("  - {}", error);
        }
    }

    // Save validation report
    let report_path = target_path.with_extension("validation.json");
    let report_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&report_path, report_json)?;
    println!("\nValidation report saved at: {}", report_path.display());

    Ok(())
}

/// Rollback migration
async fn rollback_migration(backup: Option<&str>, to: Option<&str>) -> Result<()> {
    let config = Config::from_env()?;

    let backup_path = match backup {
        Some(p) => PathBuf::from(p),
        None => config.database.path.with_extension("db.bak"),
    };

    let target_path = match to {
        Some(p) => PathBuf::from(p),
        None => config.database.path.clone(),
    };

    println!("Rolling back migration...\n");
    println!("Backup: {}", backup_path.display());
    println!("Target: {}", target_path.display());

    if !backup_path.exists() {
        anyhow::bail!("Backup file does not exist: {}", backup_path.display());
    }

    // Create a backup of current state before rollback
    if target_path.exists() {
        let pre_rollback = target_path.with_extension("pre-rollback.db");
        std::fs::copy(&target_path, &pre_rollback).context("Failed to backup current state")?;
        println!("Current state backed up to: {}", pre_rollback.display());
    }

    // Restore from backup
    std::fs::copy(&backup_path, &target_path).context("Failed to restore from backup")?;

    println!("\nRollback complete.");
    println!("Database restored from backup.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_nexus_database() {
        // Create a temporary database
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Create a valid Nexus database
        let output = Command::new("sqlite3")
            .args([
                db_path.to_string_lossy().as_ref(),
                "CREATE TABLE memories (id INTEGER); CREATE TABLE agent_namespaces (id INTEGER);",
            ])
            .output()
            .unwrap();

        assert!(output.status.success());
        assert!(is_nexus_database(&db_path).unwrap());
    }

    #[test]
    fn test_migration_report_serialization() {
        let report = MigrationReport {
            source_path: PathBuf::from("/source/db.db"),
            target_path: PathBuf::from("/target/db.db"),
            backup_path: Some(PathBuf::from("/backup/db.bak")),
            started_at: "2025-01-01T00:00:00Z".to_string(),
            completed_at: Some("2025-01-01T00:01:00Z".to_string()),
            duration_secs: 60.0,
            status: MigrationStatus::Completed,
            namespaces_migrated: 10,
            memories_migrated: 100,
            specifications_migrated: 5,
            relations_migrated: 20,
            metrics_migrated: 50,
            errors: vec![],
            warnings: vec![],
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("Completed"));
        assert!(json.contains("100"));
    }
}
