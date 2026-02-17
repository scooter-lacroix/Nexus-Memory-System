# Master Track Orchestration Protocol

**Version:** 1.0
**Created:** 2025-02-16

---

## Overview

This document defines the protocol for master track orchestration in the Maestro framework. Master tracks coordinate multiple sub-tracks in dependency order, spawning background agents for parallel execution where possible.

---

## Master Track Definition

A **master track** is a special track type (`type: "orchestration"`) that:
1. Coordinates multiple sub-tracks
2. Manages dependencies between sub-tracks
3. Spawns background agents for parallel execution
4. Tracks overall migration/feature completion
5. Creates checkpoints at phase boundaries

---

## Required Files

For a master track to function, the following must exist:

```
maestro/
├── master-track-protocol.md    # This file
├── tracks.md                   # Track registry
├── workflow.md                 # Development workflow
├── workflow-config.json        # Workflow configuration
└── tracks/
    └── <master-track-id>/
        ├── metadata.json       # Track metadata (type: "orchestration")
        ├── spec.md             # Sub-track catalog, dependency graph
        └── plan.md             # Orchestration phases
```

---

## Orchestration Rules

### Rule 1: Dependency Resolution

Before executing any sub-track, verify all dependencies are satisfied:

```
DEPENDENCY_CHECK(sub_track):
  FOR EACH dependency IN sub_track.dependencies:
    IF dependency.status != "completed":
      RETURN BLOCKED
  RETURN READY
```

### Rule 2: Parallel Execution

Sub-tracks at the same dependency level CAN run in parallel:

```
EXECUTE_PHASE(phase):
  parallel_tracks = GET_PARALLEL_TRACKS(phase)
  FOR EACH track IN parallel_tracks:
    IF DEPENDENCY_CHECK(track) == READY:
      SPAWN_BACKGROUND_AGENT(track)
  WAIT_FOR_ALL_AGENTS()
  VERIFY_ALL_COMPLETED()
```

### Rule 3: Sequential Execution

Sub-tracks with dependencies MUST run sequentially:

```
EXECUTE_SEQUENTIAL(track):
  WAIT_UNTIL(DEPENDENCY_CHECK(track) == READY)
  SPAWN_BACKGROUND_AGENT(track)
  WAIT_FOR_COMPLETION(track)
```

### Rule 4: Checkpoint Creation

After each phase completion:

```
CREATE_CHECKPOINT(phase):
  GIT_ADD_ALL()
  COMMIT("maestro(checkpoint): Phase {phase} complete")
  TAG("checkpoint-phase-{phase}")
  UPDATE_TRACKS_MD()
```

### Rule 5: Failure Handling

If a sub-track fails:

```
HANDLE_FAILURE(track, error):
  LOG_ERROR(error)
  MARK_TRACK(track, "blocked")
  NOTIFY_USER("Track {track} failed: {error}")
  OFFER_OPTIONS:
    - RETRY track
    - ROLLBACK to last checkpoint
    - SKIP track (with warning)
    - ABORT master track
```

---

## Background Agent Protocol

### Spawning Agents

Use the Task tool to spawn background agents for sub-track implementation:

```
SPAWN_BACKGROUND_AGENT(track_id):
  Task(
    subagent_type: "general-purpose",
    description: "Implement {track_id}",
    prompt: """
    You are implementing track: {track_id}

    Read the spec and plan:
    - maestro/tracks/{track_id}/spec.md
    - maestro/tracks/{track_id}/plan.md

    Follow the workflow in maestro/workflow.md:
    - Use TDD (Red-Green-Refactor)
    - Achieve 95%+ test coverage
    - Run codex-reviewer before completion

    After completion:
    - Update tracks.md status
    - Create checkpoint commit

    Report completion to master orchestrator.
    """,
    run_in_background: true
  )
```

### Monitoring Agents

Track agent progress using TaskOutput:

```
MONITOR_AGENT(task_id):
  WHILE NOT COMPLETE:
    status = TaskOutput(task_id, block=false)
    IF status == "error":
      HANDLE_FAILURE(track, status.error)
    ELSE IF status == "complete":
      VERIFY_COMPLETION(track)
      PROCEED_TO_NEXT()
    SLEEP(30 seconds)
```

---

## Status Tracking

### Master Track Status

The master track status reflects overall progress:

| Status | Meaning |
|--------|---------|
| `new` | Master track created, not started |
| `in_progress` | One or more sub-tracks in progress |
| `blocked` | A sub-track is blocked |
| `checkpoint` | At a checkpoint, awaiting user verification |
| `completed` | All sub-tracks completed successfully |
| `failed` | A sub-track failed and was not recovered |

### Sub-Track Status

Each sub-track has independent status:

| Status | Meaning |
|--------|---------|
| `pending` | Track to be created (listed in spec) |
| `new` | Track created, not started |
| `in_progress` | Track being implemented |
| `completed` | Track finished and verified |
| `blocked` | Waiting for dependencies |
| `failed` | Implementation failed |

---

## Handoff Protocol

### Sub-Track to Master

When a sub-track completes:

1. **Mark Complete:** Update sub-track status to `completed`
2. **Create Checkpoint:** Commit and tag
3. **Notify Master:** Update master track progress
4. **Trigger Dependent:** Check if any blocked tracks are now ready

```
SUB_TRACK_COMPLETE(track_id):
  UPDATE_METADATA(track_id, status: "completed")
  GIT_COMMIT("maestro(track): Complete {track_id}")
  UPDATE_MASTER_PROGRESS()
  FOR EACH dependent IN GET_DEPENDENT_TRACKS(track_id):
    IF DEPENDENCY_CHECK(dependent) == READY:
      SPAWN_BACKGROUND_AGENT(dependent)
```

### Master to User

At checkpoints or on completion:

```
MASTER_NOTIFY_USER(event):
  summary = GENERATE_PROGRESS_SUMMARY()
  SEND_MESSAGE("""
  {event}

  Progress Summary:
  {summary}

  Completed: {completed_count}/{total_count}
  In Progress: {in_progress_count}
  Blocked: {blocked_count}
  Remaining: {remaining_count}

  Next: {next_action}
  """)
```

---

## Checkpoint Verification

### Automatic Verification (Autonomous Mode)

After each phase, automatically verify:

```bash
# Build verification
cargo build --workspace --all-features

# Test verification
cargo test --workspace --all-features

# Coverage verification
cargo llvm-cov --workspace --html

# Lint verification
cargo clippy --workspace --all-targets -- -D warnings
```

### Tzar of Excellence Review

Use `/codex-reviewer` skill before checkpoint:

```
TZAR_REVIEW(phase):
  INVOKE_SKILL("/codex-reviewer", """
  Review Phase {phase} of master track {master_id}.

  Review ALL sub-tracks completed in this phase:
  {sub_track_list}

  Zero tolerance for:
  - Security vulnerabilities
  - Unhandled edge cases
  - Missing error handling
  - Performance issues
  - Incomplete implementations

  Output: PASS/FAIL with detailed findings.
  """)
```

---

## Rollback Protocol

### Per-Track Rollback

```
ROLLBACK_TRACK(track_id):
  checkpoint = GET_LAST_CHECKPOINT(track_id)
  IF checkpoint:
    GIT_RESET(checkpoint.commit_hash)
    UPDATE_METADATA(track_id, status: "new")
    CLEAR_TRACK_PROGRESS(track_id)
  ELSE:
    ERROR("No checkpoint available for rollback")
```

### Full Master Rollback

```
ROLLBACK_MASTER(master_id):
  FOR EACH track IN GET_ALL_SUB_TRACKS(master_id):
    ROLLBACK_TRACK(track.id)
  RESET_MASTER_STATUS(master_id)
  GIT_RESET(master_initial_commit)
```

---

## Progress Calculation

```
CALCULATE_PROGRESS(master_id):
  sub_tracks = GET_SUB_TRACKS(master_id)
  completed = COUNT(sub_tracks, status == "completed")
  total = COUNT(sub_tracks)
  percentage = (completed / total) * 100

  RETURN {
    completed: completed,
    total: total,
    percentage: percentage,
    status: DETERMINE_STATUS(percentage, sub_tracks)
  }
```

---

## Example Orchestration Flow

```
MASTER_TRACK: rust-migration-master_20250216

PHASE 1: Foundation
  └─ Track 1: rust-core-foundation (NO DEPENDENCIES)
     └─ SPAWN_AGENT → WAIT → VERIFY → CHECKPOINT

PHASE 2: Core Services (PARALLEL)
  ├─ Track 2: rust-embedding-service (depends on 1)
  ├─ Track 3: rust-hooks-system (depends on 1)
  └─ Track 4: rust-orchestrator-core (depends on 1)
     └─ SPAWN_ALL_PARALLEL → WAIT_ALL → VERIFY_ALL → CHECKPOINT

PHASE 3: Server Layer
  └─ Track 5: rust-mcp-server (depends on 1, 4)
     └─ SPAWN_AGENT → WAIT → VERIFY → CHECKPOINT

PHASE 4: User Interfaces (PARALLEL)
  ├─ Track 6: rust-web-dashboard (depends on 4, 5)
  └─ Track 7: rust-cli-app (depends on 5, 6)
     └─ SPAWN_ALL_PARALLEL → WAIT_ALL → VERIFY_ALL → CHECKPOINT

PHASE 5: Integration
  └─ Track 8: rust-migration-integration (depends on ALL)
     └─ SPAWN_AGENT → WAIT → VERIFY → TZAR_REVIEW → FINAL_CHECKPOINT

PHASE 6: Handoff
  └─ USER_VERIFICATION → MARK_MASTER_COMPLETE → NOTIFY
```

---

## Configuration

Master track behavior can be configured in `workflow-config.json`:

```json
{
  "master_track": {
    "auto_start_sub_tracks": true,
    "parallel_execution": true,
    "max_parallel_agents": 3,
    "checkpoint_interval": "phase",
    "tzar_review_enabled": true,
    "rollback_on_failure": false,
    "notify_on_phase_complete": true
  }
}
```

---

## References

- `maestro/workflow.md` - Development workflow
- `maestro/workflow-config.json` - Workflow configuration
- `maestro/tracks.md` - Track registry
- `maestro/critical_think/templates/` - Critical think templates

---

**Version:** 1.0
**Last Updated:** 2025-02-16
