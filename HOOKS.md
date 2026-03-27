# Hooks

Nexus hooks provide automatic memory capture for supported agent tools by installing tool-specific lifecycle integrations that write into the shared Nexus store.

## Supported Agents

- Claude Code
- Gemini
- Qwen
- Codex
- OpenCode
- Amp
- Droid
- Hermes

## Common Commands

### Install hooks for everything supported on the machine

```bash
nexus hooks install --agent all
```

### Install hooks for one tool

```bash
nexus hooks install --agent codex
```

### Check hook status

```bash
nexus hooks status
```

### Remove hooks

```bash
nexus hooks uninstall --agent codex
```

## Hook Model

The hooks system is implemented by the `nexus-hooks` crate and includes:

- agent-specific installers under `crates/nexus-hooks/src/agents/`
- common hook traits and types
- extraction helpers
- session and signal handling
- monitoring and buffering support

## Flow

1. Install hooks with `nexus hooks install`.
2. The target tool triggers its configured lifecycle callback.
3. Nexus extracts session context.
4. The memory is stored through the shared database path.

## Operational Notes

- Hooks rely on the installed tool being present in `PATH`.
- The shared database path is controlled by the installed Nexus environment.
- `nexus hooks status` is the first command to run when troubleshooting.
