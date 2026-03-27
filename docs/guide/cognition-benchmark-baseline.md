# Cognition Benchmark Baseline

Date: 2026-03-27

Command used:

```bash
cargo bench -p nexus-memory-agent --bench cognition -- --sample-size 10
```

Environment:

- local optimized Criterion run
- benchmark harness in `crates/nexus-agent/benches/cognition.rs`
- in-memory SQLite fixture
- mock LLM for deterministic query timing

## Current Baseline

Measured ranges from the current harness:

- `cognition_representation_build_80`
  - `1.2821 ms` to `1.3147 ms`
- `cognition_query_with_representation_80`
  - `1.3485 ms` to `1.3855 ms`
- `cognition_reflect_cycle_40`
  - `16.485 ms` to `16.985 ms`

## What These Numbers Mean

- Working representation assembly remains low-latency on the benchmark fixture while now enforcing the locked bucket allocation and confidence gating rules from the cognition spec.
- Representation-first query orchestration remains close to one millisecond with a mock LLM and bounded context, even after the stricter ranking normalization pass.
- A bounded dream/reflection pass over a 40-memory fixture remains in the tens-of-milliseconds range and stays comfortably inside the shutdown/runtime guardrails.

These measurements still support the design goal of keeping cognition work lightweight and bounded inside the local Rust/SQLite stack, even though they are slower than the earlier pre-tuning baseline. The additional cost comes from stricter representation shaping and normalized scoring rather than unbounded background work.

## Scope Limits

This baseline is intentionally narrow:

- it measures in-memory SQLite, not long-lived on-disk databases
- it uses a mock LLM rather than a network model provider
- it does not yet measure database growth over time
- it does not yet measure end-to-end token usage against live provider responses

## Additional Resource Evidence

- `cargo test -p nexus-memory-agent --test resource_efficiency`
  - validates that a representative cognition fixture stays under `1 MiB` on disk
- `cargo test -p nexus-memory-agent --test runtime_controller`
  - validates bounded runtime startup and shutdown cognition behavior

## Recommended Operator Interpretation

- Keep `representation_max_items = 24` unless benchmark evidence justifies expansion.
- Keep `max_job_batch = 8` and `lease_ttl_secs = 120` as conservative defaults for now.
- Keep bounded shutdown dreaming enabled, but do not widen `session_end_dream_timeout_secs` without a clear latency need.
- Treat the current benchmark numbers as the post-tuning truth for the locked v1 scoring model; optimize only when a change preserves the same cognition behavior.
- Re-run this benchmark after any ranking, digest-window, or reflection-logic change.

## Next Benchmarking Targets

- on-disk SQLite size growth for representative multi-session fixtures
- token usage across digest and query prompts with real provider telemetry
- larger reflection fixtures to find the first meaningful nonlinear cost boundary
