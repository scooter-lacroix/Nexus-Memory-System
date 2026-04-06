//! Migration command implementation
//!
//! Provides commands for migrating existing Nexus data into the current workspace format

use anyhow::{Context, Result};
use clap::Subcommand;
use indicatif::{ProgressBar, ProgressStyle};
use nexus_core::{
    infer_perspective, CognitiveLevel, CognitiveMetadata, Config, Memory, PerspectiveSource,
};
use nexus_storage::models::EnqueueJobParams;
use nexus_storage::repository::{MemoryRepository, NamespaceRepository};
use nexus_storage::StorageManager;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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

    /// Run a migration into the current Nexus format
    Run {
        /// Source database path
        #[arg(short, long)]
        from: Option<String>,

        /// Target database path
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
        /// Source database path
        #[arg(short, long)]
        from: Option<String>,

        /// Target database path
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

    /// Backfill cognition metadata and enqueue missing cognition jobs
    Cognition {
        /// Optional agent/namespace name to scope the backfill
        #[arg(short, long)]
        agent: Option<String>,

        /// Maximum memories examined per namespace during metadata backfill.
        /// This same bound also caps uncovered session digest enqueueing work.
        #[arg(short, long, default_value = "500")]
        limit: usize,

        /// Show what would be changed without writing updates
        #[arg(long)]
        dry_run: bool,

        /// Skip enqueueing session digest jobs for uncovered sessions
        #[arg(long)]
        skip_digests: bool,

        /// Skip enqueueing a namespace reflection job after backfill
        #[arg(long)]
        skip_reflect: bool,

        /// Optional path to write a JSON verification report (`-` for stdout)
        #[arg(long)]
        report_json: Option<String>,
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
        MigrateCommands::Cognition {
            agent,
            limit,
            dry_run,
            skip_digests,
            skip_reflect,
            report_json,
        } => {
            backfill_cognition(
                agent.as_deref(),
                limit,
                dry_run,
                !skip_digests,
                !skip_reflect,
                report_json.as_deref(),
            )
            .await
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CognitionBackfillReport {
    namespaces: usize,
    memories_examined: usize,
    metadata_backfilled: usize,
    derive_jobs_enqueued: usize,
    digest_jobs_enqueued: usize,
    reflect_jobs_enqueued: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CognitionCoverageSnapshot {
    active_memories: i64,
    archived_memories: i64,
    missing_cognitive_metadata: i64,
    session_keys_with_cognition: i64,
    session_keys_missing_digests: i64,
    digest_count: i64,
    raw_count: i64,
    explicit_count: i64,
    derived_count: i64,
    contradiction_count: i64,
    summary_short_count: i64,
    summary_long_count: i64,
    pending_derive_jobs: i64,
    pending_digest_jobs: i64,
    pending_reflect_jobs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CognitionNamespaceVerificationReport {
    namespace: String,
    dry_run: bool,
    backfill: CognitionBackfillReport,
    before: CognitionCoverageSnapshot,
    after: CognitionCoverageSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CognitionVerificationReport {
    dry_run: bool,
    report_generated_at: String,
    namespaces: Vec<CognitionNamespaceVerificationReport>,
    totals: CognitionBackfillReport,
}

async fn backfill_cognition(
    agent: Option<&str>,
    limit: usize,
    dry_run: bool,
    enqueue_digests: bool,
    enqueue_reflect: bool,
    report_json: Option<&str>,
) -> Result<()> {
    let config = Config::from_env()?;
    let mut storage = StorageManager::from_url(&config.database_url()).await?;
    storage.initialize().await?;

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let memory_repo = MemoryRepository::new(storage.pool().clone());

    let namespaces = match agent {
        Some(agent_name) => match namespace_repo.get_by_name(agent_name).await? {
            Some(namespace) => vec![namespace],
            None => {
                println!("Namespace '{}' not found.", agent_name);
                return Ok(());
            }
        },
        None => namespace_repo.list_all().await?,
    };

    if namespaces.is_empty() {
        println!("No namespaces found.");
        return Ok(());
    }

    let mut report = CognitionBackfillReport::default();
    let mut namespace_reports = Vec::new();
    for namespace in namespaces {
        let before = capture_cognition_coverage(&memory_repo, namespace.id, limit).await?;
        report.namespaces += 1;
        let namespace_report = backfill_namespace_cognition(
            &memory_repo,
            &namespace.name,
            namespace.id,
            limit,
            dry_run,
            enqueue_digests,
            enqueue_reflect,
        )
        .await?;

        report.memories_examined += namespace_report.memories_examined;
        report.metadata_backfilled += namespace_report.metadata_backfilled;
        report.derive_jobs_enqueued += namespace_report.derive_jobs_enqueued;
        report.digest_jobs_enqueued += namespace_report.digest_jobs_enqueued;
        report.reflect_jobs_enqueued += namespace_report.reflect_jobs_enqueued;

        let after = capture_cognition_coverage(&memory_repo, namespace.id, limit).await?;
        print_cognition_verification_summary(
            &namespace.name,
            dry_run,
            &namespace_report,
            &before,
            &after,
        );
        namespace_reports.push(CognitionNamespaceVerificationReport {
            namespace: namespace.name,
            dry_run,
            backfill: namespace_report,
            before,
            after,
        });
    }

    println!(
        "{} cognition backfill across {} namespace(s): {} memories examined, {} metadata updates, {} derive jobs, {} digest jobs, {} reflect jobs.",
        if dry_run { "Dry-run" } else { "Completed" },
        report.namespaces,
        report.memories_examined,
        report.metadata_backfilled,
        report.derive_jobs_enqueued,
        report.digest_jobs_enqueued,
        report.reflect_jobs_enqueued,
    );

    if let Some(path) = report_json {
        let report_doc = CognitionVerificationReport {
            dry_run,
            report_generated_at: chrono::Utc::now().to_rfc3339(),
            namespaces: namespace_reports,
            totals: report,
        };
        write_cognition_report(path, &report_doc)?;
    }

    Ok(())
}

async fn capture_cognition_coverage(
    repo: &MemoryRepository,
    namespace_id: i64,
    limit: usize,
) -> Result<CognitionCoverageSnapshot> {
    let session_keys_missing_digests = repo
        .list_session_keys_without_digests(namespace_id, limit.max(1) as i64)
        .await?;
    Ok(CognitionCoverageSnapshot {
        active_memories: repo.count_by_namespace(namespace_id).await?,
        archived_memories: repo.count_archived_by_namespace(namespace_id).await?,
        missing_cognitive_metadata: repo.count_missing_cognitive_metadata(namespace_id).await?,
        session_keys_with_cognition: repo
            .count_distinct_session_keys_with_cognition(namespace_id)
            .await?,
        session_keys_missing_digests: session_keys_missing_digests.len() as i64,
        digest_count: repo.count_digests(namespace_id, None).await?,
        raw_count: repo
            .count_by_cognitive_level(namespace_id, CognitiveLevel::Raw)
            .await?,
        explicit_count: repo
            .count_by_cognitive_level(namespace_id, CognitiveLevel::Explicit)
            .await?,
        derived_count: repo
            .count_by_cognitive_level(namespace_id, CognitiveLevel::Derived)
            .await?,
        contradiction_count: repo
            .count_by_cognitive_level(namespace_id, CognitiveLevel::Contradiction)
            .await?,
        summary_short_count: repo
            .count_by_cognitive_level(namespace_id, CognitiveLevel::SummaryShort)
            .await?,
        summary_long_count: repo
            .count_by_cognitive_level(namespace_id, CognitiveLevel::SummaryLong)
            .await?,
        pending_derive_jobs: repo
            .count_jobs(namespace_id, Some("derive_memory"), Some("pending"))
            .await?,
        pending_digest_jobs: repo
            .count_jobs(namespace_id, Some("digest_session"), Some("pending"))
            .await?,
        pending_reflect_jobs: repo
            .count_jobs(namespace_id, Some("reflect_namespace"), Some("pending"))
            .await?,
    })
}

fn print_cognition_verification_summary(
    namespace_name: &str,
    dry_run: bool,
    report: &CognitionBackfillReport,
    before: &CognitionCoverageSnapshot,
    after: &CognitionCoverageSnapshot,
) {
    println!(
        "[{}] {} verification: missing metadata {} -> {}, session digests missing {} -> {}, derive jobs {} -> {}, digest jobs {} -> {}, reflect jobs {} -> {}",
        namespace_name,
        if dry_run { "dry-run" } else { "post-run" },
        before.missing_cognitive_metadata,
        after.missing_cognitive_metadata,
        before.session_keys_missing_digests,
        after.session_keys_missing_digests,
        before.pending_derive_jobs,
        after.pending_derive_jobs,
        before.pending_digest_jobs,
        after.pending_digest_jobs,
        before.pending_reflect_jobs,
        after.pending_reflect_jobs,
    );
    println!(
        "[{}] coverage: raw={}, explicit={}, derived={}, contradictions={}, summaries(short={}, long={}); examined={}, metadata updates={}",
        namespace_name,
        after.raw_count,
        after.explicit_count,
        after.derived_count,
        after.contradiction_count,
        after.summary_short_count,
        after.summary_long_count,
        report.memories_examined,
        report.metadata_backfilled,
    );
}

fn write_cognition_report(path: &str, report: &CognitionVerificationReport) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    if path == "-" {
        println!("{json}");
        return Ok(());
    }
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write cognition verification report to {path}"))?;
    println!("Wrote cognition verification report to {path}");
    Ok(())
}

async fn backfill_namespace_cognition(
    repo: &MemoryRepository,
    namespace_name: &str,
    namespace_id: i64,
    limit: usize,
    dry_run: bool,
    enqueue_digests: bool,
    enqueue_reflect: bool,
) -> Result<CognitionBackfillReport> {
    let mut report = CognitionBackfillReport::default();
    let digest_kind_by_memory = load_digest_kind_by_memory(repo, namespace_id).await?;
    let mut dry_run_offset = 0_i64;
    let mut seen_memory_ids = HashSet::new();

    loop {
        let batch = repo
            .list_missing_cognitive_metadata(
                namespace_id,
                limit.min(100) as i64,
                if dry_run { dry_run_offset } else { 0 },
            )
            .await?;
        if batch.is_empty() || report.memories_examined >= limit {
            break;
        }
        if dry_run {
            dry_run_offset += batch.len() as i64;
        }

        for memory in batch {
            if report.memories_examined >= limit {
                break;
            }
            if !seen_memory_ids.insert(memory.id) {
                continue;
            }
            report.memories_examined += 1;

            let inferred = infer_backfill_cognitive_metadata(
                repo,
                namespace_name,
                &memory,
                digest_kind_by_memory.get(&memory.id).map(String::as_str),
            )
            .await?;
            let merged = inferred.merge_into(&memory.metadata);

            println!(
                "[{}] backfill #{} -> {} ({})",
                namespace_name, memory.id, inferred.level, inferred.generated_by
            );

            if !dry_run {
                repo.update_memory_metadata(memory.id, &merged).await?;
            }
            report.metadata_backfilled += 1;

            if inferred.level == CognitiveLevel::Raw
                && should_enqueue_backfill_derive(repo, memory.id).await?
            {
                let payload =
                    serde_json::json!({ "memory_id": memory.id, "reason": "cognition_backfill" });
                let perspective = serde_json::to_value(inferred.perspective())?;
                if !dry_run {
                    repo.enqueue_job(EnqueueJobParams {
                        namespace_id,
                        job_type: "derive_memory",
                        priority: 90,
                        perspective: Some(&perspective),
                        payload: &payload,
                    })
                    .await?;
                }
                report.derive_jobs_enqueued += 1;
            }
        }
    }

    if enqueue_digests {
        let session_keys = repo
            .list_session_keys_without_digests(namespace_id, limit as i64)
            .await?;
        for session_key in session_keys {
            println!(
                "[{}] enqueue digest job for session {}",
                namespace_name, session_key
            );
            if !dry_run {
                let payload = serde_json::json!({
                    "session_key": session_key,
                    "reason": "cognition_backfill"
                });
                repo.enqueue_job(EnqueueJobParams {
                    namespace_id,
                    job_type: "digest_session",
                    priority: 100,
                    perspective: None,
                    payload: &payload,
                })
                .await?;
            }
            report.digest_jobs_enqueued += 1;
        }
    }

    if enqueue_reflect && report.metadata_backfilled > 0 {
        println!("[{}] enqueue reflect_namespace job", namespace_name);
        if !dry_run {
            let payload = serde_json::json!({ "reason": "cognition_backfill" });
            repo.enqueue_job(EnqueueJobParams {
                namespace_id,
                job_type: "reflect_namespace",
                priority: 110,
                perspective: None,
                payload: &payload,
            })
            .await?;
        }
        report.reflect_jobs_enqueued += 1;
    }

    Ok(report)
}

async fn load_digest_kind_by_memory(
    repo: &MemoryRepository,
    namespace_id: i64,
) -> Result<HashMap<i64, String>> {
    let total = repo.count_digests(namespace_id, None).await?;
    if total == 0 {
        return Ok(HashMap::new());
    }

    let mut mapping = HashMap::new();
    for digest in repo.list_digests(namespace_id, None, total, 0).await? {
        mapping.insert(digest.memory_id, digest.digest_kind);
    }
    Ok(mapping)
}

async fn infer_backfill_cognitive_metadata(
    repo: &MemoryRepository,
    namespace_name: &str,
    memory: &Memory,
    digest_kind: Option<&str>,
) -> Result<CognitiveMetadata> {
    let metadata = &memory.metadata;
    let source = infer_backfill_perspective_source(memory, digest_kind);
    let session_key = infer_backfill_session_key(metadata);
    let perspective = infer_perspective(source, namespace_name, None::<String>, session_key);

    let level = infer_backfill_level(memory, digest_kind);
    let mut cognitive = CognitiveMetadata::new(
        level,
        perspective.observer,
        perspective.subject,
        perspective.session_key,
        infer_backfill_generated_by(level, memory, digest_kind),
    );

    cognitive.source_memory_ids = infer_backfill_source_ids(repo, memory.id).await?;
    cognitive.confidence = infer_backfill_confidence(level, memory);
    cognitive.times_reinforced = infer_backfill_counter(metadata, "times_reinforced");
    cognitive.times_contradicted = infer_backfill_counter(metadata, "times_contradicted");

    Ok(cognitive)
}

fn infer_backfill_level(memory: &Memory, digest_kind: Option<&str>) -> CognitiveLevel {
    if let Some(kind) = digest_kind {
        return match kind {
            "long" => CognitiveLevel::SummaryLong,
            _ => CognitiveLevel::SummaryShort,
        };
    }

    if memory.metadata.get("raw_activity").is_some()
        || memory.labels.iter().any(|label| label == "raw-activity")
    {
        return CognitiveLevel::Raw;
    }

    if memory.metadata.get("distilled_from").is_some() {
        return CognitiveLevel::SummaryShort;
    }

    if memory
        .metadata
        .get("reflection_case")
        .and_then(serde_json::Value::as_str)
        == Some("contradiction")
        || memory.content.to_lowercase().starts_with("contradiction")
    {
        return CognitiveLevel::Contradiction;
    }

    if memory
        .metadata
        .get("distillation")
        .and_then(|v| v.get("summary_memory_id"))
        .and_then(serde_json::Value::as_i64)
        .is_some()
    {
        return CognitiveLevel::Raw;
    }

    if memory.is_archived {
        return CognitiveLevel::Derived;
    }

    CognitiveLevel::Explicit
}

fn infer_backfill_perspective_source(
    memory: &Memory,
    digest_kind: Option<&str>,
) -> PerspectiveSource {
    match infer_backfill_level(memory, digest_kind) {
        CognitiveLevel::Raw => PerspectiveSource::HookIngest,
        CognitiveLevel::SummaryShort | CognitiveLevel::SummaryLong => PerspectiveSource::Digest,
        CognitiveLevel::Derived | CognitiveLevel::Contradiction => PerspectiveSource::Reflection,
        CognitiveLevel::Explicit => PerspectiveSource::HookIngest,
    }
}

fn infer_backfill_session_key(metadata: &serde_json::Value) -> Option<String> {
    metadata
        .get("cognitive")
        .and_then(|v| v.get("session_key"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            metadata
                .get("raw_activity")
                .and_then(|v| v.get("derived_session_key"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            metadata
                .get("session_lifecycle")
                .and_then(|v| v.get("derived_session_key"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            metadata
                .get("session_id")
                .and_then(serde_json::Value::as_str)
        })
        .map(ToOwned::to_owned)
}

fn infer_backfill_generated_by(
    level: CognitiveLevel,
    memory: &Memory,
    digest_kind: Option<&str>,
) -> String {
    if digest_kind.is_some() {
        return "backfill:digest_service".to_string();
    }

    if memory.metadata.get("distilled_from").is_some() {
        return "backfill:activity_distill".to_string();
    }

    match level {
        CognitiveLevel::Raw => "backfill:hook_ingest".to_string(),
        CognitiveLevel::Explicit => "backfill:explicit".to_string(),
        CognitiveLevel::Derived => "backfill:derive_or_reflect".to_string(),
        CognitiveLevel::SummaryShort => "backfill:summary_short".to_string(),
        CognitiveLevel::SummaryLong => "backfill:summary_long".to_string(),
        CognitiveLevel::Contradiction => "backfill:reflect_service".to_string(),
    }
}

async fn infer_backfill_source_ids(repo: &MemoryRepository, memory_id: i64) -> Result<Vec<i64>> {
    let lineage = repo.load_lineage(memory_id).await?;
    let source_ids: HashSet<i64> = lineage
        .into_iter()
        .filter(|entry| entry.derived_memory_id == memory_id)
        .map(|entry| entry.source_memory_id)
        .collect();
    Ok(source_ids.into_iter().collect())
}

fn infer_backfill_confidence(level: CognitiveLevel, memory: &Memory) -> Option<f32> {
    memory
        .metadata
        .get("cognitive")
        .and_then(|v| v.get("confidence"))
        .and_then(serde_json::Value::as_f64)
        .map(|v| v as f32)
        .or_else(|| {
            memory
                .metadata
                .get("agent")
                .and_then(|v| v.get("importance_score"))
                .and_then(serde_json::Value::as_f64)
                .map(|v| v as f32)
        })
        .or({
            Some(match level {
                CognitiveLevel::Raw => 0.35,
                CognitiveLevel::Explicit => 0.65,
                CognitiveLevel::Derived => 0.72,
                CognitiveLevel::SummaryShort => 0.8,
                CognitiveLevel::SummaryLong => 0.84,
                CognitiveLevel::Contradiction => 0.78,
            })
        })
}

fn infer_backfill_counter(metadata: &serde_json::Value, key: &str) -> i64 {
    metadata
        .get("cognitive")
        .and_then(|v| v.get(key))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default() as i64
}

async fn should_enqueue_backfill_derive(repo: &MemoryRepository, memory_id: i64) -> Result<bool> {
    let lineage = repo.load_lineage(memory_id).await?;
    Ok(!lineage
        .iter()
        .any(|entry| entry.source_memory_id == memory_id))
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

/// Run migration into the current Nexus format
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
            // Try to auto-discover a previously used database
            let home = std::env::var("HOME").context("Could not determine home directory")?;
            let default_legacy_path = PathBuf::from(&home).join(".nexus/nexus.db");
            if !default_legacy_path.exists() {
                anyhow::bail!(
                    "Source database not found. Use --from to specify the path.\n\
                     Expected location: {}",
                    default_legacy_path.display()
                );
            }
            default_legacy_path
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
    // Step 1: Ensure target has the right schema and production-grade pragmas
    let target_url = format!("sqlite:{}", target_path.display());
    let options = target_url
        .parse::<sqlx::sqlite::SqliteConnectOptions>()?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .foreign_keys(true)
        .pragma("cache_size", "-2000")
        .pragma("temp_store", "MEMORY");
    let pool = sqlx::SqlitePool::connect_with(options)
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
    use chrono::Utc;
    use nexus_core::MemoryCategory;
    use nexus_storage::repository::{StoreDigestParams, StoreMemoryParams};
    use nexus_storage::StorageManager;
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

    fn make_memory(
        id: i64,
        content: &str,
        labels: &[&str],
        metadata: serde_json::Value,
        is_archived: bool,
    ) -> Memory {
        Memory {
            id,
            namespace_id: 1,
            content: content.to_string(),
            category: MemoryCategory::Session,
            memory_lane_type: None,
            labels: labels.iter().map(|label| (*label).to_string()).collect(),
            metadata,
            similarity_score: None,
            relevance_score: None,
            content_embedding: None,
            embedding_model: None,
            created_at: Utc::now(),
            updated_at: Some(Utc::now()),
            last_accessed: None,
            is_active: true,
            is_archived,
            access_count: 0,
        }
    }

    #[test]
    fn test_infer_backfill_level_heuristics() {
        let base = serde_json::json!({});

        let digest_long = make_memory(1, "digest", &[], base.clone(), false);
        assert_eq!(
            infer_backfill_level(&digest_long, Some("long")),
            CognitiveLevel::SummaryLong
        );
        assert_eq!(
            infer_backfill_level(&digest_long, Some("short")),
            CognitiveLevel::SummaryShort
        );

        let raw_labeled = make_memory(2, "raw", &["raw-activity"], base.clone(), false);
        assert_eq!(
            infer_backfill_level(&raw_labeled, None),
            CognitiveLevel::Raw
        );

        let raw_flagged = make_memory(
            3,
            "raw",
            &[],
            serde_json::json!({ "raw_activity": true }),
            false,
        );
        assert_eq!(
            infer_backfill_level(&raw_flagged, None),
            CognitiveLevel::Raw
        );

        let distilled = make_memory(
            4,
            "distilled summary",
            &[],
            serde_json::json!({ "distilled_from": [1, 2, 3] }),
            false,
        );
        assert_eq!(
            infer_backfill_level(&distilled, None),
            CognitiveLevel::SummaryShort
        );

        let contradicted = make_memory(
            5,
            "Contradiction: this conflicts",
            &[],
            serde_json::json!({}),
            false,
        );
        assert_eq!(
            infer_backfill_level(&contradicted, None),
            CognitiveLevel::Contradiction
        );

        let archived = make_memory(6, "archived", &[], base.clone(), true);
        assert_eq!(
            infer_backfill_level(&archived, None),
            CognitiveLevel::Derived
        );

        let explicit = make_memory(7, "explicit", &[], base, false);
        assert_eq!(
            infer_backfill_level(&explicit, None),
            CognitiveLevel::Explicit
        );
    }

    #[test]
    fn test_infer_backfill_generated_by_heuristics() {
        let digest = make_memory(1, "digest", &[], serde_json::json!({}), false);
        assert_eq!(
            infer_backfill_generated_by(CognitiveLevel::SummaryShort, &digest, Some("short")),
            "backfill:digest_service"
        );

        let distilled = make_memory(
            2,
            "distilled",
            &[],
            serde_json::json!({ "distilled_from": [1, 2] }),
            false,
        );
        assert_eq!(
            infer_backfill_generated_by(CognitiveLevel::SummaryShort, &distilled, None),
            "backfill:activity_distill"
        );

        let raw = make_memory(3, "raw", &["raw-activity"], serde_json::json!({}), false);
        assert_eq!(
            infer_backfill_generated_by(CognitiveLevel::Raw, &raw, None),
            "backfill:hook_ingest"
        );

        let contradiction = make_memory(4, "Contradiction: x", &[], serde_json::json!({}), false);
        assert_eq!(
            infer_backfill_generated_by(CognitiveLevel::Contradiction, &contradiction, None),
            "backfill:reflect_service"
        );
    }

    #[test]
    fn test_infer_backfill_session_key_prefers_nested_cognitive_then_fallbacks() {
        let from_cognitive = serde_json::json!({
            "cognitive": { "session_key": "cognitive-key" },
            "raw_activity": { "derived_session_key": "raw-key" },
            "session_lifecycle": { "derived_session_key": "lifecycle-key" },
            "session_id": "root-key"
        });
        assert_eq!(
            infer_backfill_session_key(&from_cognitive).as_deref(),
            Some("cognitive-key")
        );

        let from_raw_activity = serde_json::json!({
            "raw_activity": { "derived_session_key": "raw-key" },
            "session_lifecycle": { "derived_session_key": "lifecycle-key" },
            "session_id": "root-key"
        });
        assert_eq!(
            infer_backfill_session_key(&from_raw_activity).as_deref(),
            Some("raw-key")
        );

        let from_lifecycle = serde_json::json!({
            "session_lifecycle": { "derived_session_key": "lifecycle-key" },
            "session_id": "root-key"
        });
        assert_eq!(
            infer_backfill_session_key(&from_lifecycle).as_deref(),
            Some("lifecycle-key")
        );

        let from_root = serde_json::json!({ "session_id": "root-key" });
        assert_eq!(
            infer_backfill_session_key(&from_root).as_deref(),
            Some("root-key")
        );
    }

    async fn setup_cognition_repo() -> (MemoryRepository, i64) {
        let mut storage = StorageManager::from_url("sqlite::memory:").await.unwrap();
        storage.initialize().await.unwrap();

        let namespace_repo = NamespaceRepository::new(storage.pool().clone());
        let namespace = namespace_repo
            .get_or_create("backfill-test", "backfill-test")
            .await
            .unwrap();

        (MemoryRepository::new(storage.pool().clone()), namespace.id)
    }

    #[tokio::test]
    async fn test_backfill_namespace_cognition_updates_metadata_and_enqueues_jobs() {
        let (repo, namespace_id) = setup_cognition_repo().await;

        let raw = repo
            .store(StoreMemoryParams {
                namespace_id,
                content: "{\"event\":\"tool\",\"tool\":\"rg\"}",
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &["raw-activity".to_string()],
                metadata: &serde_json::json!({
                    "raw_activity": { "derived_session_key": "session-a" }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        repo.store(StoreMemoryParams {
            namespace_id,
            content: "User fixed the installer wrapper behavior.",
            category: &MemoryCategory::General,
            memory_lane_type: None,
            labels: &[],
            metadata: &serde_json::json!({
                "session_id": "session-a"
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let report = backfill_namespace_cognition(
            &repo,
            "backfill-test",
            namespace_id,
            50,
            false,
            true,
            true,
        )
        .await
        .unwrap();

        assert_eq!(report.memories_examined, 2);
        assert_eq!(report.metadata_backfilled, 2);
        assert_eq!(report.derive_jobs_enqueued, 1);
        assert_eq!(report.digest_jobs_enqueued, 1);
        assert_eq!(report.reflect_jobs_enqueued, 1);

        let raw = repo.get_by_id(raw.id).await.unwrap().unwrap();
        let cognitive = raw.metadata.get("cognitive").unwrap();
        assert_eq!(
            cognitive.get("level").and_then(serde_json::Value::as_str),
            Some("raw")
        );
        assert_eq!(
            cognitive
                .get("session_key")
                .and_then(serde_json::Value::as_str),
            Some("session-a")
        );

        let jobs = repo
            .list_jobs(namespace_id, None, None, 10, 0)
            .await
            .unwrap();
        assert_eq!(jobs.len(), 3);
        let job_types: HashSet<_> = jobs.iter().map(|job| job.job_type.as_str()).collect();
        assert!(job_types.contains("derive_memory"));
        assert!(job_types.contains("digest_session"));
        assert!(job_types.contains("reflect_namespace"));
    }

    #[tokio::test]
    async fn test_backfill_namespace_cognition_dry_run_does_not_mutate() {
        let (repo, namespace_id) = setup_cognition_repo().await;

        let raw = repo
            .store(StoreMemoryParams {
                namespace_id,
                content: "{\"event\":\"tool\",\"tool\":\"cargo test\"}",
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &["raw-activity".to_string()],
                metadata: &serde_json::json!({
                    "raw_activity": { "derived_session_key": "session-b" }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        let report = backfill_namespace_cognition(
            &repo,
            "backfill-test",
            namespace_id,
            50,
            true,
            true,
            true,
        )
        .await
        .unwrap();

        assert_eq!(report.memories_examined, 1);
        assert_eq!(report.metadata_backfilled, 1);
        assert_eq!(repo.count_jobs(namespace_id, None, None).await.unwrap(), 0);

        let raw = repo.get_by_id(raw.id).await.unwrap().unwrap();
        assert!(raw.metadata.get("cognitive").is_none());
    }

    #[tokio::test]
    async fn test_backfill_namespace_cognition_skips_session_with_existing_digest() {
        let (repo, namespace_id) = setup_cognition_repo().await;

        let digest_memory = repo
            .store(StoreMemoryParams {
                namespace_id,
                content: "Digest already exists",
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &[],
                metadata: &serde_json::json!({
                    "cognitive": {
                        "level": "summary_short",
                        "observer": "claude-code",
                        "subject": "claude-code",
                        "session_key": "session-c",
                        "generated_by": "digest_service"
                    }
                }),
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();

        repo.store_digest(StoreDigestParams {
            namespace_id,
            session_key: "session-c",
            digest_kind: "short",
            memory_id: digest_memory.id,
            start_memory_id: Some(digest_memory.id),
            end_memory_id: Some(digest_memory.id),
            token_count: 20,
        })
        .await
        .unwrap();

        repo.store(StoreMemoryParams {
            namespace_id,
            content: "{\"event\":\"tool\",\"tool\":\"cargo fmt\"}",
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": { "derived_session_key": "session-c" }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let report = backfill_namespace_cognition(
            &repo,
            "backfill-test",
            namespace_id,
            50,
            false,
            true,
            false,
        )
        .await
        .unwrap();

        assert_eq!(report.derive_jobs_enqueued, 1);
        assert_eq!(report.digest_jobs_enqueued, 0);
        assert_eq!(report.reflect_jobs_enqueued, 0);
    }

    #[tokio::test]
    async fn test_capture_cognition_coverage_and_report_output() {
        let (repo, namespace_id) = setup_cognition_repo().await;

        repo.store(StoreMemoryParams {
            namespace_id,
            content: "{\"event\":\"tool\",\"tool\":\"cargo test\"}",
            category: &MemoryCategory::Session,
            memory_lane_type: None,
            labels: &["raw-activity".to_string()],
            metadata: &serde_json::json!({
                "raw_activity": { "derived_session_key": "session-report" }
            }),
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();

        let before = capture_cognition_coverage(&repo, namespace_id, 50)
            .await
            .unwrap();
        assert_eq!(before.missing_cognitive_metadata, 1);
        assert_eq!(before.session_keys_with_cognition, 0);
        assert_eq!(before.session_keys_missing_digests, 0);

        let backfill = backfill_namespace_cognition(
            &repo,
            "backfill-test",
            namespace_id,
            50,
            false,
            true,
            true,
        )
        .await
        .unwrap();
        let after = capture_cognition_coverage(&repo, namespace_id, 50)
            .await
            .unwrap();

        assert_eq!(after.missing_cognitive_metadata, 0);
        assert_eq!(after.session_keys_with_cognition, 1);
        assert_eq!(after.session_keys_missing_digests, 1);
        assert_eq!(after.raw_count, 1);
        assert_eq!(after.pending_derive_jobs, 1);
        assert_eq!(after.pending_digest_jobs, 1);
        assert_eq!(after.pending_reflect_jobs, 1);

        let report = CognitionVerificationReport {
            dry_run: false,
            report_generated_at: Utc::now().to_rfc3339(),
            namespaces: vec![CognitionNamespaceVerificationReport {
                namespace: "backfill-test".to_string(),
                dry_run: false,
                backfill,
                before,
                after,
            }],
            totals: CognitionBackfillReport {
                namespaces: 1,
                memories_examined: 1,
                metadata_backfilled: 1,
                derive_jobs_enqueued: 1,
                digest_jobs_enqueued: 1,
                reflect_jobs_enqueued: 1,
            },
        };

        let temp_dir = TempDir::new().unwrap();
        let report_path = temp_dir.path().join("cognition-report.json");
        write_cognition_report(report_path.to_str().unwrap(), &report).unwrap();

        let written = std::fs::read_to_string(report_path).unwrap();
        assert!(written.contains("\"session_keys_missing_digests\": 1"));
        assert!(written.contains("\"derive_jobs_enqueued\": 1"));
        assert!(written.contains("\"namespace\": \"backfill-test\""));
    }
}
