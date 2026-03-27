# CLI Reference

The `nexus` binary is the main operator surface for the entire system: install validation, memory storage, recall, dreaming, hook management, serving, migration, and observability.

## Core Command Groups

### Setup and runtime

- `init`
- `serve`
- `config`
- `session`

### Memory and cognition

- `store`
- `search`
- `list`
- `recall`
- `represent`
- `digest`
- `dream`
- `lineage`
- `stats`

### Integrations and migration

- `hooks`
- `migrate`

## Common Workflows

### Initialize the database

```bash
nexus init
nexus init --reset
```

### Store a memory

```bash
nexus store --content "release completed" --agent codex --category session
```

### Search memories

```bash
nexus search --query "release completed" --agent codex --limit 10
```

### List memories with time filters

```bash
nexus list --agent claude-code --since 24h --limit 20
```

### Recall relevant context

```bash
nexus recall --agent claude-code --query "provider rollout timeline"
```

### Inspect the working representation

```bash
nexus represent --agent claude-code --query "provider rollout timeline" --introspect
```

### Inspect a digest or run a dream cycle

```bash
nexus digest latest --agent claude-code --session-key <session-key>
nexus dream run --agent claude-code
```

### Explain memory lineage

```bash
nexus lineage show --memory-id <id>
```

### Show statistics

```bash
nexus stats
```

### Serve the API and dashboard

```bash
nexus serve --transport http --port 8768
nexus serve --transport web --port 8768 --agent
nexus serve --transport stdio
```

### Manage hooks

```bash
nexus hooks install --agent all
nexus hooks status --verbose
nexus hooks uninstall --agent codex
```

### Inspect tool help and schemas

```bash
nexus tools help
nexus tools help store_memory
nexus tools schema store_memory
```

### Run cognition migration and backfill

```bash
nexus migrate discover
nexus migrate status
nexus migrate run
nexus migrate validate
nexus migrate rollback
nexus migrate cognition --dry-run
```

## Practical Operator Notes

- `search` is useful for direct retrieval.
- `recall` is the better first stop when you want the cognition engine to assemble relevant context.
- `represent --introspect` is the best way to see what the subconscious is actually surfacing.
- `dream` and `digest` are the quickest commands for inspecting the background cognition layer directly.
- `hooks status --verbose` is the first troubleshooting command for lifecycle capture issues.
