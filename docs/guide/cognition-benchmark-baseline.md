# Cognition Benchmark Baseline

Date: 2026-03-27

Command used:

```bash
cargo bench -p nexus-memory-agent --bench cognition -- --sample-size 10
```

Environment:

- local optimized Criterion run
- benchmark harness in `crates/nexus-agent/benches/cognition.rs`
- in-memory and file-backed SQLite fixtures
- mock LLM for deterministic query timing

## Current Baseline

Measured ranges from the current harness:

- `cognition_representation_build_80`
  - `1.2122 ms` to `1.2728 ms`
- `cognition_query_with_representation_80`
  - `1.3905 ms` to `1.4193 ms`
- `cognition_reflect_cycle_40`
  - `13.128 ms` to `13.385 ms`
- `cognition_representation_build_ondisk_80`
  - `6.7485 ms` to `6.7938 ms`
- `cognition_representation_build_ondisk_200`
  - `7.1663 ms` to `7.1986 ms`
- `cognition_representation_build_ondisk_500`
  - `8.1072 ms` to `11.703 ms`
- `cognition_query_with_representation_ondisk_80`
  - `12.478 ms` to `12.627 ms`
- `cognition_query_with_representation_ondisk_200`
  - `12.975 ms` to `20.126 ms`
- `cognition_query_with_representation_ondisk_500`
  - `13.802 ms` to `22.125 ms`
- `cognition_reflect_cycle_ondisk_40`
  - `770.65 ms` to `782.63 ms`

## What These Numbers Mean

- Working representation assembly remains low-latency on the benchmark fixture while now enforcing the locked bucket allocation and confidence gating rules from the cognition spec.
- Representation-first query orchestration remains low-latency with a mock LLM and bounded context, and the metric-batching optimization pulled the in-memory query path meaningfully closer to the earlier baseline.
- A bounded dream/reflection pass over a 40-memory in-memory fixture remains in the low tens-of-milliseconds range, with only minor movement after the telemetry refactor.
- File-backed SQLite is now dramatically healthier than the first telemetry pass: representation and query dropped from the mid-tens-of-milliseconds band into roughly `7-22 ms` depending on fixture size, while full reflection still stays under a second for the verified 40-memory fixture.

These measurements still support the design goal of keeping cognition work lightweight and bounded inside the local Rust/SQLite stack. The most important finding from this pass is that per-metric insert overhead was a real hot-path cost: batching those writes produced clear wins for representation and query, especially on file-backed fixtures. The remaining work is now much more likely to live in reflection logic and row/metadata handling than in observability persistence itself.

## Scope Limits

This baseline is still intentionally incomplete:

- it now measures both in-memory and on-disk SQLite fixtures, but not long-lived multi-session growth curves
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
- Treat the current benchmark numbers as the post-telemetry truth for the locked v1 scoring model; optimize only when a change preserves the same cognition behavior.
- Treat the batched-metrics write path as the required observability baseline; do not revert to one-insert-per-metric in hot cognition paths.
- Re-run this benchmark after any ranking, digest-window, or reflection-logic change.

## Next Benchmarking Targets

- on-disk SQLite size growth for representative multi-session fixtures
- token usage across digest and query prompts with real provider telemetry
- larger reflection fixtures to find the first meaningful nonlinear cost boundary
