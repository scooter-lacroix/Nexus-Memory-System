# Hooks

Nexus hooks are the automatic capture layer for supported coding agents. They give the system its “always-on” feel by collecting lifecycle and activity context and feeding it into the shared memory runtime.

The important detail is that hooks are only part of the story. Depending on the tool, Nexus can use native hooks, wrapper lifecycle boundaries, or monitor-aware fallback. The system reports that honestly.

## Support Tiers

### `native-lifecycle`

Dedicated lifecycle integration with real tool-specific installation.

Current examples:

- Claude Code
- pi-mono
- oh-my-pi
- pi-skills

### `wrapper-lifecycle`

Lifecycle boundaries are provided by the installed Nexus wrapper around the target CLI.

Current examples:

- Codex
- Amp
- OpenCode
- Droid
- Hermes

### `monitor-only`

No native hook install is available. Nexus can observe process activity, but lifecycle fidelity is intentionally lower.

Current examples:

- Gemini
- Qwen

## Common Commands

### Install integrations for everything available

```bash
nexus hooks install --agent all
```

### Install one integration

```bash
nexus hooks install --agent claude-code
```

### Check status

```bash
nexus hooks status
nexus hooks status --verbose
```

### Remove an integration

```bash
nexus hooks uninstall --agent codex
```

## What Gets Captured

Depending on the support tier and tool, Nexus can capture:

- session start and end
- checkpoints and compact-style lifecycle markers
- tool activity and command context
- bounded retry-buffer replay when direct enrichment is unavailable
- enough session metadata to build digests, derive observations, and run dreaming later

Low-signal activity is not the end state. Nexus can later distill it into more useful memory instead of leaving it as raw operational clutter.

## Hook Flow

1. You install integrations with `nexus hooks install`.
2. The target tool triggers a hook, wrapper boundary, or observable lifecycle event.
3. Nexus normalizes the event and stores raw activity with cognitive metadata when appropriate.
4. Derivation, digests, and dreaming improve the memory over time.
5. Recall surfaces the useful result rather than forcing you to read raw event noise.

## Operational Notes

- Hooks rely on the target tool being present in `PATH`.
- The shared database path is controlled by the installed Nexus environment.
- `nexus hooks status --verbose` is the first command to run when troubleshooting.
- Status checks are designed to be honest about support depth rather than overstating integration quality.

## Claude Code Notes

Claude Code is the most complete native-lifecycle integration today.

Nexus manages Claude’s hook configuration so that:

- hook entries use the correct `matcher + hooks[]` schema
- invalid or duplicate Nexus-managed entries are removed
- Nexus runtime environment values are propagated into Claude settings

That keeps hook-triggered ingestion aligned with the same live config used by the main `nexus` binary.

## Related Docs

- [Installation Guide](INSTALLATION.md)
- [Getting Started](docs/guide/getting-started.md)
- [Cognition Rollout Guide](docs/guide/cognition-rollout.md)
