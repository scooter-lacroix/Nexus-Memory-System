//! Shared helpers for nexus-agent integration tests.
//!
//! This module is imported by test files via `mod common;`.
//! It contains no `#[test]` functions so Cargo runs zero tests from it.

use std::path::PathBuf;
use std::sync::OnceLock;

use nexus_core::config::CognitionConfig;
use nexus_core::{CognitiveLevel, CognitiveMetadata, MemoryCategory};
use nexus_memory_agent::RuntimeController;
use nexus_storage::repository::{MemoryRepository, NamespaceRepository, StoreMemoryParams};
use nexus_storage::StorageManager;
use tempfile::TempDir;
use tokio::sync::Mutex;

/// Serializes env-var-dependent tests so they don't clobber each other.
pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// RAII guard that saves/restores environment variables.
pub struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    pub fn set(vars: &[(&'static str, String)]) -> Self {
        let mut saved = Vec::with_capacity(vars.len());
        for (key, value) in vars {
            saved.push((*key, std::env::var(key).ok()));
            unsafe { std::env::set_var(key, value) };
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                Some(v) => unsafe { std::env::set_var(key, v) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

/// Pre-wired test fixture: tempdir + DB + namespace + repos.
///
/// Owns the `EnvGuard` so environment variables remain set for the
/// lifetime of the fixture (and thus the test function).
#[allow(dead_code)]
pub struct SoakFixture {
    _env: EnvGuard,
    _temp: TempDir,
    pub home_dir: PathBuf,
    pub state_dir: PathBuf,
    pub db_path: PathBuf,
    pub namespace_id: i64,
    pub repo: MemoryRepository,
}

impl SoakFixture {
    /// Creates a fresh SQLite database and namespace for `agent`.
    pub async fn new(agent: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home_dir = temp.path().join("home");
        let state_dir = temp.path().join("state");
        let db_path = temp.path().join("nexus.db");
        std::fs::create_dir_all(&home_dir).unwrap();
        std::fs::create_dir_all(&state_dir).unwrap();

        let env = EnvGuard::set(&[
            ("HOME", home_dir.display().to_string()),
            ("XDG_STATE_HOME", state_dir.display().to_string()),
            ("NEXUS_DATABASE_PATH", db_path.display().to_string()),
        ]);

        let url = format!("sqlite:{}", db_path.display());
        let mut storage = StorageManager::from_url(&url).await.unwrap();
        storage.initialize().await.unwrap();

        let namespace_repo = NamespaceRepository::new(storage.pool().clone());
        let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
        let repo = MemoryRepository::new(storage.pool().clone());

        Self {
            _env: env,
            _temp: temp,
            home_dir,
            state_dir,
            db_path,
            namespace_id: namespace.id,
            repo,
        }
    }

    /// Convenience: store a single memory with cognitive metadata for a session.
    pub async fn store_cognitive(
        &self,
        content: &str,
        agent: &str,
        session_key: &str,
        level: CognitiveLevel,
        category: &MemoryCategory,
    ) {
        let mut cognitive = CognitiveMetadata::new(
            level,
            agent,
            agent,
            Some(session_key.to_string()),
            "soak_test",
        );
        cognitive.confidence = Some(0.9);
        let metadata = cognitive.merge_into(&serde_json::json!({}));
        self.repo
            .store(StoreMemoryParams {
                namespace_id: self.namespace_id,
                content,
                category,
                memory_lane_type: None,
                labels: &[],
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
    }

    /// Store a contradictory pair of memories for a session.
    pub async fn store_contradiction_pair(&self, agent: &str, session_key: &str) {
        self.store_cognitive(
            "The cache system is enabled and improves performance",
            agent,
            session_key,
            CognitiveLevel::Explicit,
            &MemoryCategory::Facts,
        )
        .await;
        self.store_cognitive(
            "The cache system is not enabled and degrades performance",
            agent,
            session_key,
            CognitiveLevel::Explicit,
            &MemoryCategory::Facts,
        )
        .await;
    }
}

/// Builds a `CognitionConfig` with auto-runtime enabled and dream-on-shutdown
/// set so the RuntimeController actually exercises the full lifecycle.
pub fn runtime_cognition_config() -> CognitionConfig {
    CognitionConfig {
        auto_runtime_enabled: true,
        dream_on_session_end: true,
        session_end_dream_timeout_secs: 5,
        ..CognitionConfig::default()
    }
}

/// Returns the path to the runtime state file for a given agent + session key.
/// Mirrors the logic in `RuntimeController::state_file_path`.
pub fn session_state_file(agent: &str, session_key: &str) -> PathBuf {
    RuntimeController::state_root()
        .join("sessions")
        .join(format!("{agent}__{session_key}.json"))
}
