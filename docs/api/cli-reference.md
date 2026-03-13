# CLI Reference

The `nexus` binary is published in the `nexus-memory` package and implemented by the CLI crate in `crates/nexus-cli`.

## Top-Level Commands

- `init`
- `serve`
- `store`
- `search`
- `stats`
- `hooks`
- `migrate`

## Examples

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

### Show statistics

```bash
nexus stats
```

### Serve over HTTP

```bash
nexus serve --transport http --port 8768
```

### Serve over stdio

```bash
nexus serve --transport stdio
```

### Manage hooks

```bash
nexus hooks install --agent all
nexus hooks status
nexus hooks uninstall --agent codex
```

### Inspect tool help and schemas

```bash
nexus tools help
nexus tools help store_memory
nexus tools schema store_memory
nexus tool help store_memory
```

### Migration commands

```bash
nexus migrate discover
nexus migrate status
nexus migrate run
nexus migrate validate
nexus migrate rollback
```
