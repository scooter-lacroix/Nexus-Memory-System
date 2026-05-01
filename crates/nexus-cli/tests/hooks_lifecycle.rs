use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use chrono::Utc;
use nexus_core::{CognitiveLevel, CognitiveMetadata, MemoryCategory};
use nexus_hooks::{NormalizedHookEvent, RetryArtifact};
use nexus_storage::repository::{MemoryRepository, NamespaceRepository, StoreMemoryParams};
use nexus_storage::StorageManager;
use tempfile::tempdir;

fn run_nexus(
    bin: &Path,
    args: &[&str],
    stdin: Option<&str>,
    home_dir: &Path,
    state_dir: &Path,
    db_path: &Path,
) -> Output {
    let mut command = Command::new(bin);
    command
        .args(args)
        .env("HOME", home_dir)
        .env("XDG_STATE_HOME", state_dir)
        .env("NEXUS_DATABASE_PATH", db_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(stdin) = stdin {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn runtime_db_url(db_path: &Path) -> String {
    format!("sqlite:{}", db_path.display())
}

async fn seed_explicit_memory(
    repo: &MemoryRepository,
    namespace_id: i64,
    agent: &str,
    session_key: &str,
    content: &str,
) {
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
        namespace_id,
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

fn session_key_for(memory: &nexus_core::Memory) -> Option<String> {
    CognitiveMetadata::from_metadata(&memory.metadata).and_then(|meta| meta.session_key)
}

#[test]
fn session_lifecycle_works_without_manual_serve_and_persists_cognition_outputs() {
    let bin = Path::new(env!("CARGO_BIN_EXE_nexus"));
    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let state_dir = temp.path().join("state");
    let db_path = temp.path().join("nexus.db");
    std::fs::create_dir_all(&home_dir).unwrap();
    std::fs::create_dir_all(&state_dir).unwrap();

    let init = run_nexus(bin, &["init"], None, &home_dir, &state_dir, &db_path);
    assert_success(&init, "nexus init");

    let agent = "claude-code";
    let session_key = "cli-e2e-session";
    let cwd = "/tmp/nexus-hooks-lifecycle";

    let start = run_nexus(
        bin,
        &[
            "session",
            "start",
            "--agent",
            agent,
            "--session-key",
            session_key,
            "--cwd",
            cwd,
        ],
        None,
        &home_dir,
        &state_dir,
        &db_path,
    );
    assert_success(&start, "session start");

    let low_signal_payload = serde_json::json!({
        "hook_event_name": "post-tool-use",
        "session_id": session_key,
        "cwd": cwd,
    })
    .to_string();
    let ingest = run_nexus(
        bin,
        &[
            "ingest-hook-event",
            "--agent",
            agent,
            "--event",
            "post-tool-use",
            "--format",
            "claude-code",
            "--session-key",
            session_key,
            "--cwd",
            cwd,
        ],
        Some(&low_signal_payload),
        &home_dir,
        &state_dir,
        &db_path,
    );
    assert_success(&ingest, "ingest-hook-event");

    let runtime_db_url = runtime_db_url(&db_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut storage = StorageManager::from_url(&runtime_db_url).await.unwrap();
        storage.initialize().await.unwrap();
        let namespace_repo = NamespaceRepository::new(storage.pool().clone());
        let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
        let repo = MemoryRepository::new(storage.pool().clone());

        for content in [
            "The cache system is enabled and improves performance",
            "The cache system is not enabled and degrades performance",
        ] {
            seed_explicit_memory(&repo, namespace.id, agent, session_key, content).await;
        }
    });

    let checkpoint = run_nexus(
        bin,
        &[
            "session",
            "event",
            "--agent",
            agent,
            "--session-key",
            session_key,
            "--cwd",
            cwd,
            "--kind",
            "checkpoint",
        ],
        None,
        &home_dir,
        &state_dir,
        &db_path,
    );
    assert_success(&checkpoint, "session event checkpoint");

    let end = run_nexus(
        bin,
        &[
            "session",
            "end",
            "--agent",
            agent,
            "--session-key",
            session_key,
            "--cwd",
            cwd,
            "--reason",
            "integration-test",
        ],
        None,
        &home_dir,
        &state_dir,
        &db_path,
    );
    assert_success(&end, "session end");

    rt.block_on(async {
        let mut storage = StorageManager::from_url(&runtime_db_url).await.unwrap();
        storage.initialize().await.unwrap();
        let namespace_repo = NamespaceRepository::new(storage.pool().clone());
        let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
        let repo = MemoryRepository::new(storage.pool().clone());

        let session_memories = repo
            .list_by_session_key(namespace.id, session_key, 200, true)
            .await
            .unwrap();
        assert!(
            session_memories
                .iter()
                .any(|memory| memory.content.contains("session start")),
            "session start memory should exist"
        );
        assert!(
            session_memories
                .iter()
                .any(|memory| memory.content.contains("session end")),
            "session end memory should exist"
        );
        assert!(
            session_memories
                .iter()
                .any(|memory| memory.labels.iter().any(|label| label == "raw-activity")),
            "raw activity from hook ingest should be captured"
        );
        assert!(
            repo.latest_digest_for_session(namespace.id, session_key, "short")
                .await
                .unwrap()
                .is_some(),
            "short digest should exist after session end"
        );
        assert!(
            repo.latest_digest_for_session(namespace.id, session_key, "long")
                .await
                .unwrap()
                .is_some(),
            "long digest should exist after session end"
        );
        assert!(
            !repo
                .get_by_cognitive_level(namespace.id, CognitiveLevel::Contradiction, 20)
                .await
                .unwrap()
                .is_empty(),
            "dream outputs should exist after shutdown"
        );
    });
}

#[test]
fn repeated_sessions_keep_cognition_scoped_to_each_session() {
    let bin = Path::new(env!("CARGO_BIN_EXE_nexus"));
    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let state_dir = temp.path().join("state");
    let db_path = temp.path().join("nexus.db");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&state_dir).unwrap();

    let init = run_nexus(bin, &["init"], None, &home_dir, &state_dir, &db_path);
    assert_success(&init, "nexus init");

    let agent = "claude-code";
    let cwd = "/tmp/nexus-hooks-lifecycle-repeated";
    let session_alpha = "cli-soak-alpha";
    let session_beta = "cli-soak-beta";

    for (session_key, facts) in [
        (
            session_alpha,
            [
                "alpha-marker cache mode is enabled for this session",
                "alpha-marker cache mode is disabled for this session",
            ],
        ),
        (
            session_beta,
            [
                "beta-marker vector recall is enabled for this session",
                "beta-marker vector recall is disabled for this session",
            ],
        ),
    ] {
        let start = run_nexus(
            bin,
            &[
                "session",
                "start",
                "--agent",
                agent,
                "--session-key",
                session_key,
                "--cwd",
                cwd,
            ],
            None,
            &home_dir,
            &state_dir,
            &db_path,
        );
        assert_success(&start, "session start");

        let payload = serde_json::json!({
            "hook_event_name": "post-tool-use",
            "session_id": session_key,
            "cwd": cwd,
            "tool_name": "Bash",
        })
        .to_string();
        let ingest = run_nexus(
            bin,
            &[
                "ingest-hook-event",
                "--agent",
                agent,
                "--event",
                "post-tool-use",
                "--format",
                "claude-code",
                "--session-key",
                session_key,
                "--cwd",
                cwd,
            ],
            Some(&payload),
            &home_dir,
            &state_dir,
            &db_path,
        );
        assert_success(&ingest, "ingest-hook-event");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let runtime_db_url = runtime_db_url(&db_path);
        rt.block_on(async {
            let mut storage = StorageManager::from_url(&runtime_db_url).await.unwrap();
            storage.initialize().await.unwrap();
            let namespace_repo = NamespaceRepository::new(storage.pool().clone());
            let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
            let repo = MemoryRepository::new(storage.pool().clone());
            for fact in facts {
                seed_explicit_memory(&repo, namespace.id, agent, session_key, fact).await;
            }
        });

        let checkpoint = run_nexus(
            bin,
            &[
                "session",
                "event",
                "--agent",
                agent,
                "--session-key",
                session_key,
                "--cwd",
                cwd,
                "--kind",
                "checkpoint",
            ],
            None,
            &home_dir,
            &state_dir,
            &db_path,
        );
        assert_success(&checkpoint, "session event checkpoint");

        let end = run_nexus(
            bin,
            &[
                "session",
                "end",
                "--agent",
                agent,
                "--session-key",
                session_key,
                "--cwd",
                cwd,
                "--reason",
                "repeated-session-soak",
            ],
            None,
            &home_dir,
            &state_dir,
            &db_path,
        );
        assert_success(&end, "session end");
    }

    let runtime_db_url = runtime_db_url(&db_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut storage = StorageManager::from_url(&runtime_db_url).await.unwrap();
        storage.initialize().await.unwrap();
        let namespace_repo = NamespaceRepository::new(storage.pool().clone());
        let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
        let repo = MemoryRepository::new(storage.pool().clone());

        let alpha_memories = repo
            .list_by_session_key(namespace.id, session_alpha, 200, true)
            .await
            .unwrap();
        let beta_memories = repo
            .list_by_session_key(namespace.id, session_beta, 200, true)
            .await
            .unwrap();

        assert!(
            !alpha_memories.is_empty() && !beta_memories.is_empty(),
            "both sessions should have scoped memories"
        );
        assert!(
            alpha_memories
                .iter()
                .all(|memory| { session_key_for(memory).as_deref() == Some(session_alpha) }),
            "alpha session retrieval should only return alpha-scoped memories"
        );
        assert!(
            beta_memories
                .iter()
                .all(|memory| { session_key_for(memory).as_deref() == Some(session_beta) }),
            "beta session retrieval should only return beta-scoped memories"
        );
        assert!(
            alpha_memories
                .iter()
                .any(|memory| memory.content.contains("alpha-marker")),
            "alpha session should retain alpha explicit memories"
        );
        assert!(
            beta_memories
                .iter()
                .any(|memory| memory.content.contains("beta-marker")),
            "beta session should retain beta explicit memories"
        );
        assert!(
            alpha_memories
                .iter()
                .all(|memory| !memory.content.contains("beta-marker")),
            "alpha session retrieval must not bleed beta content"
        );
        assert!(
            beta_memories
                .iter()
                .all(|memory| !memory.content.contains("alpha-marker")),
            "beta session retrieval must not bleed alpha content"
        );
        assert!(
            repo.latest_digest_for_session(namespace.id, session_alpha, "short")
                .await
                .unwrap()
                .is_some(),
            "alpha session should have a short digest"
        );
        assert!(
            repo.latest_digest_for_session(namespace.id, session_beta, "short")
                .await
                .unwrap()
                .is_some(),
            "beta session should have a short digest"
        );
    });
}

#[test]
fn session_checkpoint_replays_retry_buffer_artifacts_into_raw_activity() {
    let bin = Path::new(env!("CARGO_BIN_EXE_nexus"));
    let temp = tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let state_dir = temp.path().join("state");
    let db_path = temp.path().join("nexus.db");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&state_dir).unwrap();

    let init = run_nexus(bin, &["init"], None, &home_dir, &state_dir, &db_path);
    assert_success(&init, "nexus init");

    let agent = "claude-code";
    let session_key = "cli-retry-replay";
    let cwd = "/tmp/nexus-hooks-lifecycle-retry";

    let start = run_nexus(
        bin,
        &[
            "session",
            "start",
            "--agent",
            agent,
            "--session-key",
            session_key,
            "--cwd",
            cwd,
        ],
        None,
        &home_dir,
        &state_dir,
        &db_path,
    );
    assert_success(&start, "session start");

    let pending_dir = state_dir
        .join("nexus-memory-system")
        .join("pending-enrichment");
    fs::create_dir_all(&pending_dir).unwrap();
    let artifact_path = pending_dir.join("2026-03-27T11-00-00Z_retry-artifact.json");
    let artifact = RetryArtifact {
        agent: agent.to_string(),
        event_name: "post-tool-use".to_string(),
        normalized_event: NormalizedHookEvent {
            agent: agent.to_string(),
            event_name: "post-tool-use".to_string(),
            observed_at: Utc::now(),
            session_id: Some(session_key.to_string()),
            turn_id: Some("replay-turn-1".to_string()),
            cwd: Some(cwd.to_string()),
            tool_name: Some("Bash".to_string()),
            tool_input: None,
            tool_response_text: Some("ok".to_string()),
            assistant_message_text: None,
            user_message_text: None,
            observer: Some(agent.to_string()),
            subject: Some(agent.to_string()),
            session_key: Some(session_key.to_string()),
            raw_payload: serde_json::json!({
                "hook_event_name": "post-tool-use",
                "session_id": session_key,
                "cwd": cwd,
                "tool_name": "Bash"
            }),
        },
        candidates: Vec::new(),
        error: "synthetic test artifact".to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    fs::write(
        &artifact_path,
        serde_json::to_vec_pretty(&artifact).unwrap(),
    )
    .unwrap();

    let checkpoint = run_nexus(
        bin,
        &[
            "session",
            "event",
            "--agent",
            agent,
            "--session-key",
            session_key,
            "--cwd",
            cwd,
            "--kind",
            "checkpoint",
        ],
        None,
        &home_dir,
        &state_dir,
        &db_path,
    );
    assert_success(&checkpoint, "session event checkpoint");

    assert!(
        !artifact_path.exists(),
        "checkpoint replay should remove processed retry artifacts"
    );

    let runtime_db_url = runtime_db_url(&db_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut storage = StorageManager::from_url(&runtime_db_url).await.unwrap();
        storage.initialize().await.unwrap();
        let namespace_repo = NamespaceRepository::new(storage.pool().clone());
        let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
        let repo = MemoryRepository::new(storage.pool().clone());
        let session_memories = repo
            .list_by_session_key(namespace.id, session_key, 200, true)
            .await
            .unwrap();

        let replayed_raw = session_memories.iter().find(|memory| {
            memory.labels.iter().any(|label| label == "raw-activity")
                && memory.content.contains("[event:replay-turn-1]")
        });
        let replayed_raw = replayed_raw.expect("retry replay should persist raw activity");

        assert_eq!(
            session_key_for(replayed_raw).as_deref(),
            Some(session_key),
            "replayed raw activity should stay scoped to the active session"
        );
    });
}
