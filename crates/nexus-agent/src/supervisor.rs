//! Agent supervisor - manages background agent loops

use std::sync::Arc;

use chrono::Utc;
use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::traits::EmbeddingService;
use nexus_core::{CognitiveLevel, CognitiveMetadata, Config};
use nexus_llm::LlmClient;
use nexus_storage::repository::{ListMemoryFilters, MemoryRepository, ProcessedFileRepository};
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::dream_cycle::{drain_cognition_jobs, run_dream_cycle, DreamCycleRequest};
use crate::error::AgentError;
use crate::inbox::InboxScanner;
use crate::ingest::IngestService;
use crate::pulse;
use crate::query::QueryService;
use crate::types::{AgentStatus, QueryIntrospection};

/// How long to wait for tasks to shut down gracefully before force-aborting.
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AgentSupervisor {
    config: AgentConfig,
    llm: Arc<dyn LlmClient>,
    query_embedder: Option<Arc<dyn EmbeddingService>>,
    pool: SqlitePool,
    namespace_id: i64,
    status: Arc<RwLock<AgentStatus>>,
    cancel_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
}

impl AgentSupervisor {
    pub fn new(
        config: AgentConfig,
        llm: Arc<dyn LlmClient>,
        pool: SqlitePool,
        namespace_id: i64,
    ) -> Self {
        let status = Arc::new(RwLock::new(AgentStatus {
            enabled: config.enabled,
            namespace: config.namespace.clone(),
            inbox_dir: config.inbox_dir.clone(),
            last_scan: None,
            last_consolidation: None,
            files_processed: 0,
            memories_consolidated: 0,
            queries_answered: 0,
            errors: Vec::new(),
        }));

        Self {
            config,
            llm,
            query_embedder: None,
            pool,
            namespace_id,
            status,
            cancel_token: CancellationToken::new(),
            tasks: Vec::new(),
        }
    }

    pub async fn start(&mut self) -> Result<(), AgentError> {
        if !self.config.enabled {
            info!("Agent is disabled, not starting supervisor");
            return Ok(());
        }

        info!("Starting agent supervisor");

        // Spawn inbox scanner task
        let inbox_handle = self.spawn_inbox_scanner().await?;
        self.tasks.push(inbox_handle);

        // Spawn consolidation task
        let consolidation_handle = self.spawn_consolidation_task().await?;
        self.tasks.push(consolidation_handle);

        let cognition_handle = self.spawn_cognition_worker_task().await?;
        self.tasks.push(cognition_handle);

        info!("Agent supervisor started with {} tasks", self.tasks.len());
        Ok(())
    }

    pub async fn stop(&mut self) {
        info!("Stopping agent supervisor (signaling graceful shutdown)");

        self.cancel_token.cancel();

        // Wait for tasks to complete gracefully
        let mut remaining: Vec<JoinHandle<()>> = Vec::new();
        for task in self.tasks.drain(..) {
            if task.is_finished() {
                let _ = task.await;
            } else {
                remaining.push(task);
            }
        }

        if !remaining.is_empty() {
            info!(
                "Waiting up to {}s for {} task(s) to finish gracefully",
                GRACEFUL_SHUTDOWN_TIMEOUT.as_secs(),
                remaining.len()
            );
            match tokio::time::timeout(GRACEFUL_SHUTDOWN_TIMEOUT, async {
                for task in remaining {
                    let _ = task.await;
                }
            })
            .await
            {
                Ok(()) => info!("All tasks shut down gracefully"),
                Err(_) => {
                    // Tasks didn't finish in time — they were already cancelled,
                    // and their JoinHandles will return once the loop exits.
                    info!("Graceful shutdown timed out");
                }
            }
        }

        self.tasks.clear();
        info!("Agent supervisor stopped");
    }

    pub async fn get_status(&self) -> AgentStatus {
        self.status.read().await.clone()
    }

    pub fn with_query_embedder(mut self, embedder: Arc<dyn EmbeddingService>) -> Self {
        self.query_embedder = Some(embedder);
        self
    }

    /// Increment the queries answered counter (for external callers like the web API).
    pub async fn increment_queries_answered(&self) {
        let mut s = self.status.write().await;
        s.queries_answered += 1;
    }

    pub fn query_service(&self) -> QueryService {
        if let Some(embedder) = &self.query_embedder {
            QueryService::with_embedder(self.llm.clone(), self.config.clone(), embedder.clone())
        } else {
            QueryService::new(self.llm.clone(), self.config.clone())
        }
    }

    pub fn ingest_service(&self) -> IngestService {
        IngestService::new(self.llm.clone(), self.config.clone())
    }

    /// Get the agent namespace ID
    pub fn namespace_id(&self) -> i64 {
        self.namespace_id
    }

    /// Compute query introspection (ranking decisions) without calling the LLM.
    pub async fn query_introspection(
        &self,
        question: &str,
        namespace_id: i64,
        memory_repo: &MemoryRepository,
    ) -> Result<QueryIntrospection, AgentError> {
        self.query_service()
            .query_introspection(question, namespace_id, memory_repo)
            .await
    }

    async fn spawn_inbox_scanner(&self) -> Result<JoinHandle<()>, AgentError> {
        let config = self.config.clone();
        let llm = self.llm.clone();
        let pool = self.pool.clone();
        let namespace_id = self.namespace_id;
        let status = self.status.clone();
        let interval_secs = config.scan_interval_secs;
        let cancel = self.cancel_token.clone();

        let handle = tokio::spawn(async move {
            let ingest_service = IngestService::new(llm.clone(), config.clone());
            let scanner = InboxScanner::new(config, ingest_service);
            let mut ticker = interval(Duration::from_secs(interval_secs));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {}
                    _ = cancel.cancelled() => {
                        info!("Inbox scanner received shutdown signal");
                        break;
                    }
                }

                let processed_repo = ProcessedFileRepository::new(&pool);
                let memory_repo = MemoryRepository::new(pool.clone());

                match scanner
                    .run(namespace_id, &processed_repo, &memory_repo)
                    .await
                {
                    Ok(result) => {
                        let mut s = status.write().await;
                        s.last_scan = Some(Utc::now());
                        s.files_processed += result.processed;
                        pulse::write_pulse(
                            "inbox_scan",
                            s.memories_consolidated,
                            s.files_processed,
                        );
                    }
                    Err(e) => {
                        error!(error = %e, namespace_id, "Inbox scan failed");
                        let mut s = status.write().await;
                        s.errors.push(format!("Scan error: {}", e));
                        if s.errors.len() > 10 {
                            s.errors.remove(0);
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    async fn spawn_consolidation_task(&self) -> Result<JoinHandle<()>, AgentError> {
        let config = self.config.clone();
        let llm = self.llm.clone();
        let pool = self.pool.clone();
        let namespace_id = self.namespace_id;
        let status = self.status.clone();
        let base_interval_secs = config.consolidation_interval_mins * 60;
        let cancel = self.cancel_token.clone();
        let cognition = Config::from_env()
            .map(|config| config.cognition)
            .unwrap_or_default();

        let handle = tokio::spawn(async move {
            loop {
                let sleep_duration = if cognition.adaptive_dream_enabled {
                    compute_adaptive_dream_interval(
                        pool.clone(),
                        namespace_id,
                        base_interval_secs,
                        &cognition,
                    )
                    .await
                } else {
                    Duration::from_secs(base_interval_secs)
                };

                tokio::select! {
                    _ = tokio::time::sleep(sleep_duration) => {}
                    _ = cancel.cancelled() => {
                        info!("Consolidation task received shutdown signal");
                        break;
                    }
                }

                let lease_owner = format!("supervisor-dream-{}", namespace_id);
                match run_dream_cycle(
                    pool.clone(),
                    &cognition,
                    &config,
                    llm.clone(),
                    None,
                    DreamCycleRequest {
                        namespace_id,
                        lease_owner: &lease_owner,
                        perspective: None,
                        session_key: None,
                        reflect_reason: "namespace_dream",
                        digest_reason: "dream_digest",
                    },
                )
                .await
                {
                    Ok(processed) if processed > 0 => {
                        let mut s = status.write().await;
                        s.last_consolidation = Some(Utc::now());
                        s.memories_consolidated += processed as u64;
                        pulse::write_pulse(
                            "consolidation",
                            s.memories_consolidated,
                            s.files_processed,
                        );
                    }
                    Ok(_) => {
                        debug!("No memories to consolidate");
                    }
                    Err(e) => {
                        error!(error = %e, namespace_id, "Consolidation failed");
                        let mut s = status.write().await;
                        s.errors.push(format!("Consolidation error: {}", e));
                        if s.errors.len() > 10 {
                            s.errors.remove(0);
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    async fn spawn_cognition_worker_task(&self) -> Result<JoinHandle<()>, AgentError> {
        let config = self.config.clone();
        let pool = self.pool.clone();
        let namespace_id = self.namespace_id;
        let status = self.status.clone();
        let llm = self.llm.clone();
        let cancel = self.cancel_token.clone();
        let cognition = nexus_core::Config::from_env()
            .map(|config| config.cognition)
            .unwrap_or_default();

        let handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(config.scan_interval_secs.max(1)));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {}
                    _ = cancel.cancelled() => {
                        info!("Cognition worker received shutdown signal");
                        break;
                    }
                }

                match drain_cognition_jobs(
                    pool.clone(),
                    namespace_id,
                    &cognition,
                    &config,
                    llm.clone(),
                    None,
                    &format!("supervisor-{}", namespace_id),
                )
                .await
                {
                    Ok(processed) => {
                        if processed > 0 {
                            debug!(namespace_id, processed, "Cognition worker drained jobs");
                            let mut s = status.write().await;
                            s.last_consolidation = Some(Utc::now());
                            s.memories_consolidated += processed as u64;
                            pulse::write_pulse(
                                "cognition",
                                s.memories_consolidated,
                                s.files_processed,
                            );
                        }
                    }
                    Err(error) => {
                        error!(error = %error, namespace_id, "Cognition worker failed");
                        let mut s = status.write().await;
                        s.errors.push(format!("Cognition error: {}", error));
                        if s.errors.len() > 10 {
                            s.errors.remove(0);
                        }
                    }
                }
            }
        });

        Ok(handle)
    }
}

/// Compute the next dream-cycle interval based on contradiction density.
///
/// When contradictions exist in the namespace, the interval is shortened
/// proportionally (down to `adaptive_dream_min_interval_secs`). Otherwise
/// the base interval is used (capped at `adaptive_dream_max_interval_secs`).
async fn compute_adaptive_dream_interval(
    pool: SqlitePool,
    namespace_id: i64,
    base_interval_secs: u64,
    cognition: &CognitionConfig,
) -> Duration {
    let repo = MemoryRepository::new(pool);
    let min = cognition.adaptive_dream_min_interval_secs;
    let max = cognition.adaptive_dream_max_interval_secs;
    let base = base_interval_secs.clamp(min, max);

    let contradiction_count = match repo
        .list_filtered(
            namespace_id,
            ListMemoryFilters {
                category: None,
                since: None,
                until: None,
                content_like: None,
                include_raw: false,
                limit: 256,
                offset: 0,
            },
        )
        .await
    {
        Ok(memories) => memories
            .iter()
            .filter(|m| {
                CognitiveMetadata::from_metadata(&m.metadata)
                    .map(|c| c.level == CognitiveLevel::Contradiction)
                    .unwrap_or(false)
            })
            .count(),
        Err(_) => return Duration::from_secs(base),
    };

    if contradiction_count == 0 {
        return Duration::from_secs(base);
    }

    // Shorten interval proportionally: each contradiction shaves 10% off the base,
    // down to the minimum.
    let factor = 1.0 - ((contradiction_count as f32 * 0.10).min(0.9));
    let adapted = (base as f32 * factor) as u64;
    Duration::from_secs(adapted.clamp(min, max))
}
