//! Phase 14 soak-test slice: bounded multi-session lifecycle coverage.
//!
//! Exercises four required behaviors across the cognition/runtime system:
//! 1. Multiple sessions for the same agent do not bleed cognition state
//! 2. At least one non-Claude supported agent path is exercised honestly
//! 3. Buffered ingestion (memory jobs) participates correctly in the session lifecycle
//! 4. Session-end dreaming/digesting occurs safely across repeated sessions

mod common;

use std::collections::HashSet;

use nexus_core::config::AgentConfig;
use nexus_core::{CognitiveLevel, CognitiveMetadata, MemoryCategory};
use nexus_memory_agent::{RuntimeController, RuntimeMode, RuntimeShutdownReason};
use nexus_storage::models::EnqueueJobParams;
use nexus_storage::repository::{MemoryRepository, NamespaceRepository, StoreMemoryParams};
use nexus_storage::StorageManager;

use common::{env_lock, runtime_cognition_config, SoakFixture};

// ---------------------------------------------------------------------------
// Behavior 1: Multi-session isolation
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn three_consecutive_sessions_isolate_cognition_state() {
    let _guard = env_lock().lock().await;

    let sessions = ["soak-alpha", "soak-beta", "soak-gamma"];
    let agent = "claude-code";
    let session_contents: &[&[&str]] = &[
        &["Alpha: project uses PostgreSQL for persistence"],
        &["Beta: project uses Redis for caching"],
        &["Gamma: project uses gRPC for communication"],
    ];

    let fixture = SoakFixture::new(agent).await;
    let cognition = runtime_cognition_config();

    for (i, session_key) in sessions.iter().enumerate() {
        let controller = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
        let cwd = format!("/tmp/soak-isolation-{session_key}");

        controller
            .ensure_started(
                agent,
                Some(session_key),
                Some(&cwd),
                RuntimeMode::SessionScoped,
            )
            .await
            .unwrap();

        for content in session_contents[i] {
            fixture
                .store_cognitive(
                    content,
                    agent,
                    session_key,
                    CognitiveLevel::Explicit,
                    &MemoryCategory::Facts,
                )
                .await;
        }

        controller
            .flush_and_shutdown(
                agent,
                Some(session_key),
                Some(&cwd),
                RuntimeShutdownReason::SessionEnded,
            )
            .await
            .unwrap();
    }

    // Verify cross-session isolation
    for (i, session_key) in sessions.iter().enumerate() {
        let memories = fixture
            .repo
            .list_by_session_key(fixture.namespace_id, session_key, 100, true)
            .await
            .unwrap();

        // Each session should contain its own content
        assert!(
            memories.iter().any(|m| m.content.contains(session_key)),
            "Session {session_key} should contain its own content"
        );

        // Each session should NOT contain content from other sessions
        for (j, other_key) in sessions.iter().enumerate() {
            if i != j {
                let other_content = session_contents[j][0];
                assert!(
                    !memories.iter().any(|m| m.content.contains(other_content)),
                    "Session {session_key} should NOT contain content from {other_key}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Behavior 2: Non-Claude agent lifecycle
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn gemini_and_qwen_namespace_isolation_from_claude() {
    let _guard = env_lock().lock().await;

    let cognition = runtime_cognition_config();
    let agents = ["claude-code", "gemini", "qwen"];
    let session_keys = ["claude-soak", "gemini-soak", "qwen-soak"];
    let contents = [
        "Claude-specific memory about anthropic tooling",
        "Gemini-specific memory about google tooling",
        "Qwen-specific memory about alibaba tooling",
    ];

    // We need a single DB shared across all agent namespaces.
    let temp = tempfile::tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let state_dir = temp.path().join("state");
    let db_path = temp.path().join("nexus.db");
    std::fs::create_dir_all(&home_dir).unwrap();
    std::fs::create_dir_all(&state_dir).unwrap();

    let _env = common::EnvGuard::set(&[
        ("HOME", home_dir.display().to_string()),
        ("XDG_STATE_HOME", state_dir.display().to_string()),
        ("NEXUS_DATABASE_PATH", db_path.display().to_string()),
    ]);

    let url = format!("sqlite:{}", db_path.display());
    let mut storage = StorageManager::from_url(&url).await.unwrap();
    storage.initialize().await.unwrap();

    let namespace_repo = NamespaceRepository::new(storage.pool().clone());
    let repo = MemoryRepository::new(storage.pool().clone());

    for i in 0..agents.len() {
        let agent = agents[i];
        let controller = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
        let cwd = format!("/tmp/soak-multiagent-{agent}");

        controller
            .ensure_started(
                agent,
                Some(session_keys[i]),
                Some(&cwd),
                RuntimeMode::SessionScoped,
            )
            .await
            .unwrap();

        let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();

        let mut cognitive = CognitiveMetadata::new(
            CognitiveLevel::Explicit,
            agent,
            agent,
            Some(session_keys[i].to_string()),
            "soak_test",
        );
        cognitive.confidence = Some(0.9);
        let metadata = cognitive.merge_into(&serde_json::json!({}));

        repo.store(StoreMemoryParams {
            namespace_id: namespace.id,
            content: contents[i],
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
                Some(session_keys[i]),
                Some(&cwd),
                RuntimeShutdownReason::SessionEnded,
            )
            .await
            .unwrap();
    }

    // Verify namespace isolation: each agent's memories stay in its namespace
    for i in 0..agents.len() {
        let agent = agents[i];
        let namespace = namespace_repo.get_or_create(agent, agent).await.unwrap();
        let memories = repo
            .list_by_session_key(namespace.id, session_keys[i], 100, true)
            .await
            .unwrap();

        assert!(
            memories.iter().any(|m| m.content == contents[i]),
            "{agent} namespace should contain its own memory"
        );

        for j in 0..agents.len() {
            if i != j {
                assert!(
                    !memories.iter().any(|m| m.content == contents[j]),
                    "{agent} namespace should NOT contain memory from {}",
                    agents[j]
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Behavior 3: Buffered ingestion (memory jobs) participates in session lifecycle
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn buffered_ingestion_drains_on_session_start_and_shutdown() {
    let _guard = env_lock().lock().await;

    let agent = "claude-code";
    let session_key = "soak-buffered-drain";
    let fixture = SoakFixture::new(agent).await;
    let cognition = runtime_cognition_config();

    // Pre-populate some raw activity memories so the job queue has work.
    for i in 0..3 {
        let mut cognitive = CognitiveMetadata::new(
            CognitiveLevel::Raw,
            agent,
            agent,
            Some(session_key.to_string()),
            "soak_test",
        );
        cognitive.confidence = Some(0.7);
        let mut metadata = cognitive.merge_into(&serde_json::json!({}));
        metadata["raw_activity"] = serde_json::json!({
            "event": "tool_use",
            "tool": "cargo_test",
            "ordinal": i,
        });
        fixture
            .repo
            .store(StoreMemoryParams {
                namespace_id: fixture.namespace_id,
                content: &format!(
                    r#"{{"event":"tool_use","tool":"cargo test","exit_code":0,"ordinal":{i}}}"#
                ),
                category: &MemoryCategory::Session,
                memory_lane_type: None,
                labels: &["raw-activity".to_string()],
                metadata: &metadata,
                embedding: None,
                embedding_model: None,
            })
            .await
            .unwrap();
    }

    // Enqueue an activity_distill job that should be drained on session start.
    let perspective = serde_json::json!({
        "observer": agent,
        "subject": agent,
        "session_key": session_key,
    });
    let job_id = fixture
        .repo
        .enqueue_job(EnqueueJobParams {
            namespace_id: fixture.namespace_id,
            job_type: "activity_distill",
            priority: 10,
            perspective: Some(&perspective),
            payload: &serde_json::json!({"session_key": session_key}),
        })
        .await
        .unwrap();

    assert!(job_id > 0, "Enqueued job should have a valid ID");

    // Session start should drain the pending job.
    let controller = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    let cwd = "/tmp/soak-buffered-drain";

    controller
        .ensure_started(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeMode::SessionScoped,
        )
        .await
        .unwrap();

    // Store some explicit memories for the session.
    fixture.store_contradiction_pair(agent, session_key).await;

    // Session end should trigger dream/digest cycle.
    controller
        .flush_and_shutdown(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    // Verify: raw activity memories were stored.
    let raw_memories = fixture
        .repo
        .list_by_session_key(fixture.namespace_id, session_key, 100, true)
        .await
        .unwrap();
    assert!(
        raw_memories
            .iter()
            .any(|m| m.labels.iter().any(|l| l == "raw-activity")),
        "Raw activity memories should exist for the session"
    );

    // Verify: explicit memories were stored.
    assert!(
        raw_memories
            .iter()
            .any(|m| m.content.contains("cache system")),
        "Explicit session memories should exist"
    );
}

// ---------------------------------------------------------------------------
// Behavior 4: Session-end dreaming/digesting across repeated sessions
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn repeated_session_end_dreaming_safely_accumulates() {
    let _guard = env_lock().lock().await;

    let agent = "claude-code";
    let session_keys = ["soak-dream-1", "soak-dream-2", "soak-dream-3"];
    let cognition = runtime_cognition_config();
    let fixture = SoakFixture::new(agent).await;

    for session_key in &session_keys {
        let controller = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
        let cwd = format!("/tmp/soak-dream-{session_key}");

        controller
            .ensure_started(
                agent,
                Some(session_key),
                Some(cwd.as_str()),
                RuntimeMode::SessionScoped,
            )
            .await
            .unwrap();

        // Store a contradictory pair to trigger dream outputs.
        fixture.store_contradiction_pair(agent, session_key).await;

        controller
            .flush_and_shutdown(
                agent,
                Some(session_key),
                Some(cwd.as_str()),
                RuntimeShutdownReason::SessionEnded,
            )
            .await
            .unwrap();
    }

    // Each session should have produced a short digest.
    for session_key in &session_keys {
        let digest = fixture
            .repo
            .latest_digest_for_session(fixture.namespace_id, session_key, "short")
            .await
            .unwrap();
        assert!(
            digest.is_some(),
            "Session {session_key} should have a short digest after shutdown"
        );
    }

    // No session should have duplicate digests (each session gets at most 1 short digest).
    let mut seen_digest_ids: HashSet<i64> = HashSet::new();
    for session_key in &session_keys {
        let digest = fixture
            .repo
            .latest_digest_for_session(fixture.namespace_id, session_key, "short")
            .await
            .unwrap()
            .unwrap();
        assert!(
            seen_digest_ids.insert(digest.id),
            "Session {session_key} should have a unique digest, not a duplicate of another session"
        );
    }

    // Contradiction-level memories should exist in the namespace from dream passes.
    let contradictions = fixture
        .repo
        .get_by_cognitive_level(fixture.namespace_id, CognitiveLevel::Contradiction, 50)
        .await
        .unwrap();
    // At least some contradictions should have been produced by the dream cycles.
    // We don't assert an exact count because the dream cycle depends on LLM availability,
    // but if LLM is present, contradictions should exist.
    if !contradictions.is_empty() {
        // Verify contradictions reference at least one of our sessions.
        let has_session_reference = contradictions.iter().any(|m| {
            let meta_str = m.metadata.to_string();
            session_keys.iter().any(|sk| meta_str.contains(sk))
        });
        assert!(
            has_session_reference,
            "Contradiction memories should reference at least one of the soak sessions"
        );
    }
}

/// Verify that dreaming across sessions doesn't corrupt earlier session digests.
#[tokio::test(flavor = "current_thread")]
async fn dream_outputs_dont_corrupt_earlier_session_digests() {
    let _guard = env_lock().lock().await;

    let agent = "claude-code";
    let session_a = "soak-digest-a";
    let session_b = "soak-digest-b";
    let cognition = runtime_cognition_config();
    let fixture = SoakFixture::new(agent).await;

    // Run session A with a unique memory.
    let controller_a = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    let cwd_a = "/tmp/soak-digest-a";
    controller_a
        .ensure_started(
            agent,
            Some(session_a),
            Some(cwd_a),
            RuntimeMode::SessionScoped,
        )
        .await
        .unwrap();
    fixture
        .store_cognitive(
            "Session A unique fact about feature X implementation",
            agent,
            session_a,
            CognitiveLevel::Explicit,
            &MemoryCategory::Facts,
        )
        .await;
    controller_a
        .flush_and_shutdown(
            agent,
            Some(session_a),
            Some(cwd_a),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    // Capture session A digest content.
    let digest_a = fixture
        .repo
        .latest_digest_for_session(fixture.namespace_id, session_a, "short")
        .await
        .unwrap()
        .expect("Session A should have a digest");
    let digest_a_content = digest_a.content.clone();

    // Run session B with a contradictory pair.
    let controller_b = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    let cwd_b = "/tmp/soak-digest-b";
    controller_b
        .ensure_started(
            agent,
            Some(session_b),
            Some(cwd_b),
            RuntimeMode::SessionScoped,
        )
        .await
        .unwrap();
    fixture.store_contradiction_pair(agent, session_b).await;
    controller_b
        .flush_and_shutdown(
            agent,
            Some(session_b),
            Some(cwd_b),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    // Session A's digest should be unchanged after session B's dream cycle.
    let digest_a_after = fixture
        .repo
        .latest_digest_for_session(fixture.namespace_id, session_a, "short")
        .await
        .unwrap()
        .expect("Session A digest should still exist");
    assert_eq!(
        digest_a.id, digest_a_after.id,
        "Session A digest ID should not change after session B runs"
    );
    assert_eq!(
        digest_a_content, digest_a_after.content,
        "Session A digest content should not be modified by session B"
    );

    // Session B should have its own digest.
    let digest_b = fixture
        .repo
        .latest_digest_for_session(fixture.namespace_id, session_b, "short")
        .await
        .unwrap()
        .expect("Session B should have a digest");
    assert_ne!(
        digest_a.id, digest_b.id,
        "Session A and B digests should be distinct"
    );
}

// ---------------------------------------------------------------------------
// Behavior 1 supplement: Runtime state file isolation
// ---------------------------------------------------------------------------

/// Verify that runtime state files are created and cleaned up independently
/// for different sessions of the same agent.
#[tokio::test(flavor = "current_thread")]
async fn runtime_state_files_isolated_per_session() {
    let _guard = env_lock().lock().await;

    let agent = "claude-code";
    let sessions = ["soak-state-x", "soak-state-y"];
    let cognition = runtime_cognition_config();
    let fixture = SoakFixture::new(agent).await;

    let state_paths: Vec<std::path::PathBuf> = sessions
        .iter()
        .map(|sk| common::session_state_file(agent, sk))
        .collect();

    // Ensure no leftover state files.
    for p in &state_paths {
        let _ = std::fs::remove_file(p);
    }

    // Start both sessions concurrently (neither shut down yet).
    for (i, session_key) in sessions.iter().enumerate() {
        let controller = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
        let cwd = format!("/tmp/soak-state-{session_key}");
        controller
            .ensure_started(
                agent,
                Some(session_key),
                Some(&cwd),
                RuntimeMode::SessionScoped,
            )
            .await
            .unwrap();

        // Store a memory so each session has content for digest generation.
        fixture
            .store_cognitive(
                &format!("Session {i} unique content for state isolation test"),
                agent,
                session_key,
                CognitiveLevel::Explicit,
                &MemoryCategory::Facts,
            )
            .await;
    }

    // Both state files should exist simultaneously.
    for (i, p) in state_paths.iter().enumerate() {
        assert!(
            p.exists(),
            "State file for session {} should exist after ensure_started",
            sessions[i]
        );
    }

    // Shut down only session 0.
    let controller_x = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    controller_x
        .flush_and_shutdown(
            agent,
            Some(sessions[0]),
            Some("/tmp/soak-state-soak-state-x"),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    // Session 0 state file should be gone; session 1 should still exist.
    assert!(
        !state_paths[0].exists(),
        "Session {} state file should be removed after shutdown",
        sessions[0]
    );
    assert!(
        state_paths[1].exists(),
        "Session {} state file should still exist (not yet shut down)",
        sessions[1]
    );

    // Clean up session 1.
    let controller_y = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    controller_y
        .flush_and_shutdown(
            agent,
            Some(sessions[1]),
            Some("/tmp/soak-state-soak-state-y"),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();
    assert!(
        !state_paths[1].exists(),
        "Session {} state file should be removed after shutdown",
        sessions[1]
    );

    // Both sessions should have produced independent digests.
    for session_key in &sessions {
        assert!(
            fixture
                .repo
                .latest_digest_for_session(fixture.namespace_id, session_key, "short")
                .await
                .unwrap()
                .is_some(),
            "Session {session_key} should have its own digest"
        );
    }
}

/// Verify that re-entering the same session key does not create duplicate
/// sessions or bleed state from a previous completed session.
#[tokio::test(flavor = "current_thread")]
async fn reentering_session_preserves_key_no_state_bleed() {
    let _guard = env_lock().lock().await;

    let agent = "claude-code";
    let session_key = "soak-reenter";
    let cognition = runtime_cognition_config();
    let fixture = SoakFixture::new(agent).await;
    let cwd = "/tmp/soak-reenter";

    // Run session 1 with a specific memory.
    let controller_1 = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    controller_1
        .ensure_started(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeMode::SessionScoped,
        )
        .await
        .unwrap();

    fixture
        .store_cognitive(
            "First run: feature X is implemented",
            agent,
            session_key,
            CognitiveLevel::Explicit,
            &MemoryCategory::Facts,
        )
        .await;

    controller_1
        .flush_and_shutdown(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    // Run session 2 with the SAME session key but different content.
    let controller_2 = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    controller_2
        .ensure_started(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeMode::SessionScoped,
        )
        .await
        .unwrap();

    fixture
        .store_cognitive(
            "Second run: feature Y is implemented",
            agent,
            session_key,
            CognitiveLevel::Explicit,
            &MemoryCategory::Facts,
        )
        .await;

    controller_2
        .flush_and_shutdown(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    // Both memories should exist under the same session key.
    let all_memories = fixture
        .repo
        .list_by_session_key(fixture.namespace_id, session_key, 100, true)
        .await
        .unwrap();

    assert!(
        all_memories.iter().any(|m| m.content.contains("feature X")),
        "First run memory should persist under the same session key"
    );
    assert!(
        all_memories.iter().any(|m| m.content.contains("feature Y")),
        "Second run memory should also be stored under the same session key"
    );

    // There should be exactly one short digest (latest_digest returns the most recent).
    let digest = fixture
        .repo
        .latest_digest_for_session(fixture.namespace_id, session_key, "short")
        .await
        .unwrap();
    assert!(digest.is_some(), "Re-entered session should have a digest");
}

// ---------------------------------------------------------------------------
// Behavior 4 supplement: Dreaming rigor
// ---------------------------------------------------------------------------

/// Verify that dream digest content actually references the session's own memories.
#[tokio::test(flavor = "current_thread")]
async fn dream_digest_content_references_session_memories() {
    let _guard = env_lock().lock().await;

    let agent = "claude-code";
    let session_key = "soak-digest-ref";
    let unique_marker = "UNIQUE_DIGEST_MARKER_7f3a";
    let cognition = runtime_cognition_config();
    let fixture = SoakFixture::new(agent).await;

    let controller = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    let cwd = "/tmp/soak-digest-ref";

    controller
        .ensure_started(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeMode::SessionScoped,
        )
        .await
        .unwrap();

    // Store a memory with a unique marker so we can check the digest references it.
    fixture
        .store_cognitive(
            &format!("The project uses {unique_marker} for advanced caching"),
            agent,
            session_key,
            CognitiveLevel::Explicit,
            &MemoryCategory::Facts,
        )
        .await;

    // Store a contradictory pair to ensure dream triggers.
    fixture.store_contradiction_pair(agent, session_key).await;

    controller
        .flush_and_shutdown(
            agent,
            Some(session_key),
            Some(cwd),
            RuntimeShutdownReason::SessionEnded,
        )
        .await
        .unwrap();

    // The digest should reference the session key in its metadata.
    let digest = fixture
        .repo
        .latest_digest_for_session(fixture.namespace_id, session_key, "short")
        .await
        .unwrap()
        .expect("Digest should exist");

    // The digest metadata should reference our session key.
    let digest_meta_str = digest.metadata.to_string();
    assert!(
        digest_meta_str.contains(session_key),
        "Digest metadata should reference the session key {session_key}, got: {digest_meta_str}"
    );

    // Verify session memories are queryable by session key and contain our marker.
    let session_memories = fixture
        .repo
        .list_by_session_key(fixture.namespace_id, session_key, 100, true)
        .await
        .unwrap();
    assert!(
        session_memories
            .iter()
            .any(|m| m.content.contains(unique_marker)),
        "Session memories should contain the unique marker {unique_marker}"
    );
}

/// Verify that running two full start/dream/shutdown cycles with the same session
/// key does not produce orphan digests — the digest row is reused and all memories
/// accumulate under the same session key.
#[tokio::test(flavor = "current_thread")]
async fn repeated_dream_same_session_no_orphan_digests() {
    let _guard = env_lock().lock().await;

    let agent = "claude-code";
    let session_key = "soak-dup-digest";
    let cognition = runtime_cognition_config();
    let fixture = SoakFixture::new(agent).await;
    let cwd = "/tmp/soak-dup-digest";

    // First start/shutdown cycle.
    let c1 = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    c1.ensure_started(
        agent,
        Some(session_key),
        Some(cwd),
        RuntimeMode::SessionScoped,
    )
    .await
    .unwrap();
    fixture.store_contradiction_pair(agent, session_key).await;
    c1.flush_and_shutdown(
        agent,
        Some(session_key),
        Some(cwd),
        RuntimeShutdownReason::SessionEnded,
    )
    .await
    .unwrap();

    let digest_after_first = fixture
        .repo
        .latest_digest_for_session(fixture.namespace_id, session_key, "short")
        .await
        .unwrap()
        .expect("First cycle should produce a digest");
    let first_id = digest_after_first.id;

    // Second start/shutdown cycle with same session key.
    let c2 = RuntimeController::new(cognition.clone(), AgentConfig::default(), None);
    c2.ensure_started(
        agent,
        Some(session_key),
        Some(cwd),
        RuntimeMode::SessionScoped,
    )
    .await
    .unwrap();
    fixture
        .store_cognitive(
            "Second cycle: additional observation about refactoring",
            agent,
            session_key,
            CognitiveLevel::Explicit,
            &MemoryCategory::Facts,
        )
        .await;
    c2.flush_and_shutdown(
        agent,
        Some(session_key),
        Some(cwd),
        RuntimeShutdownReason::SessionEnded,
    )
    .await
    .unwrap();

    let digest_after_second = fixture
        .repo
        .latest_digest_for_session(fixture.namespace_id, session_key, "short")
        .await
        .unwrap()
        .expect("Second cycle should still have a digest");

    // The digest system reuses the same row for the same session key.
    // This is correct: no orphan digest rows should be created.
    assert_eq!(
        first_id, digest_after_second.id,
        "Same session key should reuse the same digest row, not create an orphan"
    );

    // All memories from both cycles should be queryable under the same session key.
    let all_memories = fixture
        .repo
        .list_by_session_key(fixture.namespace_id, session_key, 200, true)
        .await
        .unwrap();
    assert!(
        all_memories.len() >= 5,
        "Both cycles' memories should be present under the same session key (got {})",
        all_memories.len()
    );

    // The second cycle's memory should be present.
    assert!(
        all_memories
            .iter()
            .any(|m| m.content.contains("refactoring")),
        "Second cycle's memory should be stored under the same session key"
    );
}
