//! Agent supervisor - manages background agent loops

use std::sync::Arc;

use chrono::Utc;
use nexus_core::config::AgentConfig;
use nexus_llm::LlmClient;
use nexus_storage::repository::{
    MemoryRelationRepository, MemoryRepository, ProcessedFileRepository,
};
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

use crate::consolidate::ConsolidateService;
use crate::error::AgentError;
use crate::inbox::InboxScanner;
use crate::ingest::IngestService;
use crate::pulse;
use crate::query::QueryService;
use crate::types::AgentStatus;

pub struct AgentSupervisor {
    config: AgentConfig,
    llm: Arc<dyn LlmClient>,
    pool: SqlitePool,
    namespace_id: i64,
    status: Arc<RwLock<AgentStatus>>,
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
            pool,
            namespace_id,
            status,
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

        info!("Agent supervisor started with {} tasks", self.tasks.len());
        Ok(())
    }

    pub async fn stop(&mut self) {
        info!("Stopping agent supervisor");

        for task in &self.tasks {
            task.abort();
        }

        // Wait for tasks to complete
        for task in &mut self.tasks {
            let _ = task.await;
        }

        self.tasks.clear();
        info!("Agent supervisor stopped");
    }

    pub async fn get_status(&self) -> AgentStatus {
        self.status.read().await.clone()
    }

    pub fn query_service(&self) -> QueryService {
        QueryService::new(self.llm.clone(), self.config.clone())
    }

    pub fn ingest_service(&self) -> IngestService {
        IngestService::new(self.llm.clone(), self.config.clone())
    }

    pub fn consolidate_service(&self) -> ConsolidateService {
        ConsolidateService::new(self.llm.clone(), self.config.clone())
    }

    /// Get the agent namespace ID
    pub fn namespace_id(&self) -> i64 {
        self.namespace_id
    }

    async fn spawn_inbox_scanner(&self) -> Result<JoinHandle<()>, AgentError> {
        let config = self.config.clone();
        let llm = self.llm.clone();
        let pool = self.pool.clone();
        let namespace_id = self.namespace_id;
        let status = self.status.clone();
        let interval_secs = config.scan_interval_secs;

        let handle = tokio::spawn(async move {
            let scanner = InboxScanner::new(config.clone(), IngestService::new(llm, config));
            let mut ticker = interval(Duration::from_secs(interval_secs));

            loop {
                ticker.tick().await;

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
                        error!(error = %e, "Inbox scan failed");
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
        let interval_mins = config.consolidation_interval_mins;

        let handle = tokio::spawn(async move {
            let service = ConsolidateService::new(llm, config);
            let mut ticker = interval(Duration::from_secs(interval_mins * 60));

            loop {
                ticker.tick().await;

                let memory_repo = MemoryRepository::new(pool.clone());
                let relation_repo = MemoryRelationRepository::new(&pool);

                match service
                    .consolidate(namespace_id, &memory_repo, &relation_repo)
                    .await
                {
                    Ok(Some(count)) => {
                        let mut s = status.write().await;
                        s.last_consolidation = Some(Utc::now());
                        s.memories_consolidated += count as u64;
                        pulse::write_pulse(
                            "consolidation",
                            s.memories_consolidated,
                            s.files_processed,
                        );
                    }
                    Ok(None) => {
                        debug!("No memories to consolidate");
                    }
                    Err(e) => {
                        error!(error = %e, "Consolidation failed");
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
}
