use std::path::PathBuf;
use std::sync::OnceLock;

use nexus_core::config::{AgentConfig, CognitionConfig};
use nexus_core::{CognitiveLevel, CognitiveMetadata, MemoryCategory};
use nexus_memory_agent::{RuntimeController, RuntimeMode, RuntimeShutdownReason};
use nexus_storage::repository::{MemoryRepository, NamespaceRepository, StoreMemoryParams};
use nexus_storage::StorageManager;
use tempfile::tempdir;
use tokio::sync::Mutex;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn set(vars: &[(&'static str, String)]) -> Self {
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
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

fn session_state_file(agent: &str, session_key: &str) -> PathBuf {
    RuntimeController::state_root()
        .join("sessions")
        .join(format!("{agent}__{session_key}.json"))
}

#[tokio::test(flavor = "current_thread")]
async fn ensure_started_and_flush_shutdown_manage_runtime_state_and_cognition_outputs() {
    let _guard = env_lock().lock().await;
    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let state_dir = temp.path().join("state");
    let db_path = temp.path().join("nexus.db");
    std::fs::create_dir_all(&home_dir).unwrap();
    std::fs::create_dir_all(&state_dir).unwrap();

    let _env = EnvGuard::set(&[
        ("HOME", home_dir.display().to_string()),
        ("XDG_STATE_HOME", state_dir.display().to_string()),
        ("NEXUS_DATABASE_PATH", db_path.display().to_string()),
    ]);

    let controller =
        RuntimeController::new(CognitionConfig::default(), AgentConfig::default(), None);
    let agent = "claude-code";
    let session_key = "runtime-test-session";
    let cwd = "/tmp/runtime-controller";

    controller
        .ensure_started(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeMode::SessionScoped,
        )
        .await
        .unwrap();

    let state_file = session_state_file(agent, session_key);
    assert!(
        state_file.exists(),
        "runtime state file should exist after start"
    );

    let mut storage = StorageManager::from_url(&format!("sqlite:{}", db_path.display()))
        .await
        .unwrap();
    storage.initialize().await.unwrap();
    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
    let repo = MemoryRepository::new(storage.pool().clone());

    for content in [
        "The cache system is enabled and improves performance",
        "The cache system is not enabled and degrades performance",
    ] {
        let mut cognitive = CognitiveMetadata::new(
            CognitiveLevel::Explicit,
            agent,
            agent,
            Some(session_key.to_string()),
            "test_seed",
        );
        cognitive.confidence = Some(0.9);
        let metadata = cognitive.merge_into(&serde_json::json!({}));
        repo.store(StoreMemoryParams {
            namespace_id: namespace.id,
            content,
            category: &MemoryCategory::Facts,
            memory_lane_type: None,
            labels: &[],
            metadata: &metadata,
            embedding: None,
            embedding_model: None,
        })
        .await
        .unwrap();
    }

    controller
        .flush_and_shutdown(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    assert!(
        !state_file.exists(),
        "runtime state file should be removed after shutdown"
    );
    assert!(
        repo.latest_digest_for_session(namespace.id, session_key, "short")
            .await
            .unwrap()
            .is_some(),
        "shutdown should create a short digest for the session"
    );
    assert!(
        !repo
            .get_by_cognitive_level(namespace.id, CognitiveLevel::Contradiction, 10)
            .await
            .unwrap()
            .is_empty(),
        "shutdown dream pass should create contradiction outputs"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn flush_shutdown_session_end_executes_digest_only_work_before_returning() {
    let _guard = env_lock().lock().await;
    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let state_dir = temp.path().join("state");
    let db_path = temp.path().join("nexus.db");
    std::fs::create_dir_all(&home_dir).unwrap();
    std::fs::create_dir_all(&state_dir).unwrap();

    let _env = EnvGuard::set(&[
        ("HOME", home_dir.display().to_string()),
        ("XDG_STATE_HOME", state_dir.display().to_string()),
        ("NEXUS_DATABASE_PATH", db_path.display().to_string()),
    ]);

    let controller =
        RuntimeController::new(CognitionConfig::default(), AgentConfig::default(), None);
    let agent = "claude-code";
    let session_key = "runtime-digest-only";
    let cwd = "/tmp/runtime-controller-digest-only";

    controller
        .ensure_started(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeMode::SessionScoped,
        )
        .await
        .unwrap();

    let mut storage = StorageManager::from_url(&format!("sqlite:{}", db_path.display()))
        .await
        .unwrap();
    storage.initialize().await.unwrap();
    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
    let repo = MemoryRepository::new(storage.pool().clone());

    let mut cognitive = CognitiveMetadata::new(
        CognitiveLevel::Explicit,
        agent,
        agent,
        Some(session_key.to_string()),
        "test_seed",
    );
    cognitive.confidence = Some(0.9);
    let metadata = cognitive.merge_into(&serde_json::json!({}));
    repo.store(StoreMemoryParams {
        namespace_id: namespace.id,
        content: "A single explicit memory should still produce a shutdown digest",
        category: &MemoryCategory::Facts,
        memory_lane_type: None,
        labels: &[],
        metadata: &metadata,
        embedding: None,
        embedding_model: None,
    })
    .await
    .unwrap();

    controller
        .flush_and_shutdown(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    assert!(
        repo.latest_digest_for_session(namespace.id, session_key, "short")
            .await
            .unwrap()
            .is_some(),
        "session-end digest-only path should flush the digest before returning"
    );
}
