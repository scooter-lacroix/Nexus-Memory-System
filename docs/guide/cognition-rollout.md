# Cognition Rollout Guide

This guide covers the shipped Nexus cognition stack: automatic session lifecycle capture, explicit derivation, session digests, bounded dreaming, representation-first recall, and the operator knobs that keep the system fast, honest, and resource-aware.

## What Ships

The current cognition system includes:

- automatic session lifecycle capture through the local runtime, hooks, and installer wrappers
- raw activity capture with later distillation instead of immediate discard
- explicit derivation with evidence lineage
- short and long session digests
- bounded reflective dreaming for reinforcement and contradiction handling
- representation-first query recall with digest, recent, derived, semantic, and contradiction blending
- observability surfaces for jobs, digests, lineage, runtime health, and recall composition

The system is designed to work without manually starting `nexus serve` for normal CLI capture flows. The server exists for API, dashboard, and MCP-style surfaces, not as a prerequisite for ordinary CLI memory capture.

## Recommended Rollout Order

1. Install or reinstall the launcher and wrappers.
2. Initialize or migrate the database.
3. Verify hook and wrapper status.
4. Configure generation and embeddings.
5. Backfill cognition metadata for older memories.
6. Let normal sessions generate new cognition artifacts.
7. Run targeted benchmark and validation commands before wider rollout.

## Install And Verify

Recommended commands:

```bash
./scripts/install.sh
nexus init
nexus config
nexus hooks status
nexus stats
```

If you launch supported tools through installed wrappers, Nexus will best-effort issue `session start` and `session end` around the wrapped CLI. That gives the system reliable boundaries for digest refresh, bounded shutdown dreaming, and session-scoped recall.

## Backfill Existing Memory

To upgrade older memories and enqueue missing cognition work:

```bash
nexus migrate cognition
```

Useful variants:

```bash
nexus migrate cognition --dry-run
nexus migrate cognition --agent claude-code
nexus migrate cognition --skip-digests
nexus migrate cognition --skip-reflect
```

What this does:

- infers missing cognitive metadata
- enqueues derive jobs for raw memories that need explicit observations
- enqueues digest jobs for uncovered sessions
- enqueues a bounded namespace reflection pass when useful

## Dreaming

In user-facing language, consolidation and reflection are branded as dreaming. This is not just a label change. It describes the behavior of the subsystem: bounded background work that improves memory quality between active moments of interaction.

Current bounded dream behavior includes:

- reinforcement detection
- contradiction detection
- contradiction memory creation with evidence lineage
- replay-safe, resumable background job execution
- best-effort bounded dream work during session shutdown for the ending session's perspective

Automatic shutdown dreaming now queues:

- a perspective-scoped reflection pass for the ending session
- a forced digest refresh for that same session

Namespace-wide dreaming is still available, but it is an explicit manual or backfill-oriented operation rather than the default shutdown path.

Useful commands:

```bash
nexus dream run --agent claude-code
nexus digest latest --agent claude-code --session-key <session>
nexus lineage show --memory-id <id>
```

## Resource Guardrails

The main cognition guardrails ship in `CognitionConfig`.

Important defaults:

- `auto_runtime_enabled = true`
- `derive_enabled = true`
- `digest_enabled = true`
- `reflect_enabled = true`
- `activity_distill_enabled = true`
- `dream_on_session_end = true`
- `checkpoint_flush_enabled = true`
- `runtime_idle_timeout_secs = 900`
- `max_job_batch = 8`
- `lease_ttl_secs = 120`
- `representation_max_items = 24`
- `digest_short_target_tokens = 600`
- `digest_long_target_tokens = 1800`
- `direct_enrichment_timeout_secs = 8`
- `activity_distill_min_events = 8`
- `activity_distill_max_events = 60`
- `include_raw_by_default = false`
- `session_end_dream_timeout_secs = 8`
- `retry_buffer_drain_limit = 8`

These can be tuned through environment variables under the `NEXUS_COGNITION_*` namespace. The defaults are intentionally conservative so the subconscious remains useful without quietly turning into a background resource hog.

## Benchmarking

A cognition benchmark harness is available in the agent crate.

Run it with:

```bash
cargo bench -p nexus-memory-agent --bench cognition
```

The harness currently measures:

- working representation assembly
- query orchestration over a representation-backed context
- bounded reflection cycle cost

Current baseline measurements are recorded in [Cognition Benchmark Baseline](cognition-benchmark-baseline.md).

This is benchmark scaffolding, not a full production performance report. Use it as a repeatable baseline when adjusting guardrails, ranking constants, and dream intensity.

## Validation Commands

Recommended validation set:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Targeted cognition validation:

```bash
cargo test -p nexus-memory-agent
cargo test -p nexus-memory-hooks -p nexus-memory
cargo check --benches -p nexus-memory-agent
```

## Current Limitations

- lifecycle breadth is stronger for Claude and wrapper-launched CLI tools than for purely native non-Claude integrations
- some agents are intentionally monitor-only and should not be presented as native hook installs
- live-provider token telemetry and very long-horizon on-disk growth studies still benefit from environment-specific follow-up
- cross-session recall relies on canonical namespace identity, so badly fragmented historical namespace naming should be backfilled first

## Operator Notes

- Do not enable raw-memory inclusion by default unless you explicitly want operational noise in recall.
- Prefer wrapper-launched sessions for reliable lifecycle capture where native hooks are unavailable.
- Treat `nexus migrate cognition --dry-run` as the safest first step on older databases.
- If shutdown latency matters more than immediate consolidation, keep bounded dream work enabled but conservative.
