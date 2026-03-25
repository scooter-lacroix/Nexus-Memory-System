//! Inbox scanner - watches directory for new files

use std::path::Path;
use tokio::fs;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};
use walkdir::WalkDir;

use nexus_core::config::AgentConfig;
use nexus_storage::repository::{MemoryRepository, ProcessedFileRepository};

use crate::error::AgentError;
use crate::ingest::IngestService;

pub struct InboxScanner {
    config: AgentConfig,
    ingest_service: IngestService,
}

/// Maximum file size to read from inbox (10 MB)
const MAX_INBOX_FILE_SIZE: u64 = 10 * 1024 * 1024;

impl InboxScanner {
    pub fn new(config: AgentConfig, ingest_service: IngestService) -> Self {
        Self {
            config,
            ingest_service,
        }
    }

    pub async fn run(
        &self,
        namespace_id: i64,
        processed_repo: &ProcessedFileRepository<'_>,
        memory_repo: &MemoryRepository,
    ) -> Result<ScanResult, AgentError> {
        let inbox_path = Path::new(&self.config.inbox_dir);

        if !inbox_path.exists() {
            info!(path = %inbox_path.display(), "Inbox directory does not exist, creating");
            fs::create_dir_all(inbox_path)
                .await
                .map_err(AgentError::Io)?;
            return Ok(ScanResult::default());
        }

        debug!(path = %inbox_path.display(), "Scanning inbox");

        let mut processed = 0;
        let mut failed = 0;

        // Batch-fetch all completed file paths to avoid N+1 queries
        let completed_paths = match processed_repo.get_completed_paths(namespace_id).await {
            Ok(paths) => paths,
            Err(e) => {
                error!(error = %e, "Failed to fetch completed paths, falling back to per-file checks");
                std::collections::HashSet::new()
            }
        };

        // Walk the inbox directory
        for entry in WalkDir::new(inbox_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let path_str = path.to_string_lossy().to_string();

            // Skip already-processed files (in-memory check)
            if completed_paths.contains(&path_str) {
                continue;
            }

            // Mark as processing
            let file_id = match processed_repo
                .mark_processing(namespace_id, &path_str, None)
                .await
            {
                Ok(id) => id,
                Err(e) => {
                    error!(path = %path.display(), error = %e, "Failed to mark file as processing");
                    continue;
                }
            };

            // Check file size before reading
            let metadata = match fs::metadata(path).await {
                Ok(m) => m,
                Err(e) => {
                    error!(path = %path.display(), error = %e, "Failed to read file metadata");
                    continue;
                }
            };
            if metadata.len() > MAX_INBOX_FILE_SIZE {
                warn!(
                    path = %path.display(),
                    size = metadata.len(),
                    max = MAX_INBOX_FILE_SIZE,
                    "Skipping file exceeding size limit"
                );
                if let Err(e) = processed_repo
                    .mark_failed(
                        file_id,
                        &format!(
                            "File too large ({} bytes, max {})",
                            metadata.len(),
                            MAX_INBOX_FILE_SIZE
                        ),
                    )
                    .await
                {
                    warn!(file_id = file_id, error = %e, "Failed to mark file as failed");
                }
                failed += 1;
                continue;
            }

            // Read and process file
            match fs::read_to_string(path).await {
                Ok(content) => {
                    let source = format!(
                        "inbox/{}",
                        path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "unknown".to_string())
                    );

                    match self
                        .ingest_service
                        .ingest(&content, &source, namespace_id, memory_repo)
                        .await
                    {
                        Ok(memory_id) => {
                            if let Err(e) = processed_repo.mark_processed(file_id, memory_id).await
                            {
                                warn!(
                                    file_id = file_id,
                                    error = %e,
                                    "Failed to mark file as processed"
                                );
                            }
                            processed += 1;
                        }
                        Err(e) => {
                            error!(path = %path.display(), error = %e, "Failed to ingest file");
                            if let Err(e2) =
                                processed_repo.mark_failed(file_id, &e.to_string()).await
                            {
                                warn!(
                                    file_id = file_id,
                                    error = %e2,
                                    "Failed to mark file as failed"
                                );
                            }
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    error!(path = %path.display(), error = %e, "Failed to read file");
                    if let Err(e2) = processed_repo.mark_failed(file_id, &e.to_string()).await {
                        warn!(
                            file_id = file_id,
                            error = %e2,
                            "Failed to mark file as failed"
                        );
                    }
                    failed += 1;
                }
            }
        }

        info!(processed, failed, "Inbox scan complete");
        Ok(ScanResult { processed, failed })
    }

    pub fn scan_interval(&self) -> Duration {
        Duration::from_secs(self.config.scan_interval_secs)
    }
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub processed: u64,
    pub failed: u64,
}
