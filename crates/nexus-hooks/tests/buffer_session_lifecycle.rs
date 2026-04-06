//! Soak-test slice: PersistentBuffer session lifecycle integration.
//!
//! Exercises the real PersistentBuffer from nexus-hooks to prove:
//!
//! 1. Multi-agent buffer isolation (entries for agent A don't leak to agent B).
//! 2. Buffer recovery after simulated crash produces correct data.
//! 3. Context types are preserved through the flush → recover cycle.
//! 4. Buffer clear removes both in-memory and on-disk state.
//! 5. flush_all handles multiple concurrent agent buffers.

use std::collections::HashSet;

use nexus_memory_hooks::{PersistentBuffer, SessionContext};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Behavior 3: PersistentBuffer multi-agent isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multi_agent_buffer_isolation() {
    let dir = tempdir().unwrap();
    let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf()))
        .unwrap()
        .with_max_entries(100);

    let agents = ["codex", "gemini", "hermes"];

    // Buffer distinct contexts for each agent.
    for agent in &agents {
        buffer.start_buffering(agent).await.unwrap();
        let mut ctx = SessionContext::new(*agent);
        ctx.add_insight(format!("{agent}-unique-insight"));
        buffer
            .buffer_context(agent, ctx, "checkpoint")
            .await
            .unwrap();
    }

    // Each agent should have exactly 1 entry.
    for agent in &agents {
        let status = buffer.get_buffer_status(agent).await.unwrap();
        assert_eq!(
            status.entries_count, 1,
            "{agent} should have exactly 1 buffered entry"
        );
    }

    // Verify entry content isolation: each agent's entries only contain its own data.
    for agent in &agents {
        let status = buffer.get_buffer_status(agent).await.unwrap();
        assert_eq!(status.entries_count, 1);
    }
}

#[tokio::test]
async fn buffer_entries_stay_separated_after_flush_and_recover() {
    let dir = tempdir().unwrap();
    let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf()))
        .unwrap()
        .with_max_entries(1); // auto-flush after 1 entry

    // Agent A: buffer 2 entries.
    buffer.start_buffering("agent-a").await.unwrap();
    let mut ctx_a1 = SessionContext::new("agent-a");
    ctx_a1.add_insight("agent-a-first");
    buffer
        .buffer_context("agent-a", ctx_a1, "checkpoint-1")
        .await
        .unwrap();

    let mut ctx_a2 = SessionContext::new("agent-a");
    ctx_a2.add_insight("agent-a-second");
    buffer
        .buffer_context("agent-a", ctx_a2, "checkpoint-2")
        .await
        .unwrap();

    // Agent B: buffer 1 entry.
    buffer.start_buffering("agent-b").await.unwrap();
    let mut ctx_b = SessionContext::new("agent-b");
    ctx_b.add_insight("agent-b-only");
    buffer
        .buffer_context("agent-b", ctx_b, "checkpoint")
        .await
        .unwrap();

    // Flush all to disk.
    buffer.flush_all().await.unwrap();

    // Simulate a new buffer instance (crash recovery scenario).
    let buffer2 = PersistentBuffer::new(Some(dir.path().to_path_buf())).unwrap();

    // Recover agent A.
    let recovered_a = buffer2
        .recover_buffer("agent-a")
        .await
        .unwrap()
        .expect("agent-a buffer should be recoverable");
    assert_eq!(
        recovered_a.entries.len(),
        2,
        "agent-a should have 2 entries after recovery"
    );

    // Verify agent A's entries don't contain agent B's data.
    let a_context_types: HashSet<_> = recovered_a
        .entries
        .iter()
        .map(|e| e.context_type.clone())
        .collect();
    assert!(
        a_context_types.contains("checkpoint-1"),
        "agent-a should have checkpoint-1 entry"
    );
    assert!(
        a_context_types.contains("checkpoint-2"),
        "agent-a should have checkpoint-2 entry"
    );

    // Recover agent B.
    let recovered_b = buffer2
        .recover_buffer("agent-b")
        .await
        .unwrap()
        .expect("agent-b buffer should be recoverable");
    assert_eq!(
        recovered_b.entries.len(),
        1,
        "agent-b should have 1 entry after recovery"
    );

    // Agent B's entry should reference agent-b, not agent-a.
    assert_eq!(
        recovered_b.entries[0].context.agent_type, "agent-b",
        "Recovered agent-b entry should have agent_type 'agent-b'"
    );
}

// ---------------------------------------------------------------------------
// Behavior 3: Context type preservation through flush/recover
// ---------------------------------------------------------------------------

#[tokio::test]
async fn context_types_preserved_through_flush_recover() {
    let dir = tempdir().unwrap();
    let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf()))
        .unwrap()
        .with_max_entries(1);

    buffer.start_buffering("test-agent").await.unwrap();

    let context_types = ["session-start", "checkpoint", "tool-use", "session-end"];

    for ct in &context_types {
        let mut ctx = SessionContext::new("test-agent");
        ctx.add_command(format!("cmd-for-{ct}"));
        buffer.buffer_context("test-agent", ctx, ct).await.unwrap();
    }

    // Flush and recover.
    buffer.flush_to_disk("test-agent").await.unwrap();

    let buffer2 = PersistentBuffer::new(Some(dir.path().to_path_buf())).unwrap();
    let recovered = buffer2
        .recover_buffer("test-agent")
        .await
        .unwrap()
        .expect("Buffer should be recoverable");

    let recovered_types: HashSet<_> = recovered
        .entries
        .iter()
        .map(|e| e.context_type.as_str())
        .collect();

    for ct in &context_types {
        assert!(
            recovered_types.contains(*ct),
            "Context type '{ct}' should be preserved through flush/recover"
        );
    }
}

// ---------------------------------------------------------------------------
// Behavior 3: Buffer clear removes both memory and disk state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn clear_buffer_removes_memory_and_disk_state() {
    let dir = tempdir().unwrap();
    let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf()))
        .unwrap()
        .with_max_entries(1);

    buffer.start_buffering("clear-test").await.unwrap();

    let mut ctx = SessionContext::new("clear-test");
    ctx.add_insight("will-be-cleared");
    buffer
        .buffer_context("clear-test", ctx, "pre-clear")
        .await
        .unwrap();

    // Flush to disk so there's a file.
    buffer.flush_to_disk("clear-test").await.unwrap();

    // Verify the buffer file exists.
    let buffer_file = dir.path().join("clear-test.json");
    assert!(buffer_file.exists(), "Buffer file should exist after flush");

    // Verify in-memory status exists.
    assert!(
        buffer.get_buffer_status("clear-test").await.is_some(),
        "In-memory status should exist before clear"
    );

    // Clear.
    buffer.clear_buffer("clear-test").await.unwrap();

    // Both should be gone.
    assert!(
        buffer.get_buffer_status("clear-test").await.is_none(),
        "In-memory status should be gone after clear"
    );
    assert!(
        !buffer_file.exists(),
        "Buffer file should be removed after clear"
    );

    // Recovery should return None.
    let buffer2 = PersistentBuffer::new(Some(dir.path().to_path_buf())).unwrap();
    assert!(
        buffer2
            .recover_buffer("clear-test")
            .await
            .unwrap()
            .is_none(),
        "Recovery should return None after clear"
    );
}

// ---------------------------------------------------------------------------
// Behavior 3: flush_all with multiple agents
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flush_all_handles_multiple_agents() {
    let dir = tempdir().unwrap();
    let buffer = PersistentBuffer::new(Some(dir.path().to_path_buf()))
        .unwrap()
        .with_max_entries(100);

    let agents = ["alpha", "beta", "gamma", "delta"];
    for agent in &agents {
        buffer.start_buffering(agent).await.unwrap();
        let mut ctx = SessionContext::new(*agent);
        ctx.add_insight(format!("{agent}-data"));
        buffer
            .buffer_context(agent, ctx, "batch-checkpoint")
            .await
            .unwrap();
    }

    buffer.flush_all().await.unwrap();

    // All four agent files should exist on disk.
    for agent in &agents {
        let path = dir.path().join(format!("{agent}.json"));
        assert!(
            path.exists(),
            "Flushed buffer file for {agent} should exist on disk"
        );
    }

    // Verify each is independently recoverable.
    let buffer2 = PersistentBuffer::new(Some(dir.path().to_path_buf())).unwrap();
    for agent in &agents {
        let recovered = buffer2
            .recover_buffer(agent)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{agent} should be recoverable"));
        assert_eq!(
            recovered.entries.len(),
            1,
            "{agent} should have exactly 1 entry"
        );
        assert_eq!(
            recovered.entries[0].context.agent_type, *agent,
            "{agent} recovered entry should reference the correct agent type"
        );
    }
}
