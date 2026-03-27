# Getting Started

This is the fastest path from clone to a working local memory runtime that can store memory, recall it, and show you the cognition stack in action.

## 1. Build the CLI

```bash
cargo build --release -p nexus-memory
```

## 2. Install or upgrade Nexus

```bash
./scripts/install.sh --binary ./target/release/nexus
```

## 3. Initialize the database

```bash
nexus init
```

## 4. Store a first memory

```bash
nexus store \
  --content "Switched embeddings to a provider-backed model and revalidated serve" \
  --agent codex \
  --category session \
  --labels embeddings,serve
```

## 5. Ask the system what it knows

```bash
nexus search --query "serve"
nexus recall --agent codex --query "What changed in serve and embeddings?"
nexus stats
```

## 6. Install hooks and wrappers

```bash
nexus hooks install --agent all
nexus hooks status --verbose
```

## 7. Peek under the hood

```bash
nexus represent --agent codex --query "What changed in serve and embeddings?" --introspect
```

## 8. Run a dream cycle

```bash
nexus dream --agent codex
```

## 9. Start the web surface

```bash
NEXUS_AGENT_ENABLED=true nexus serve --transport web --port 8768 --agent
```

Then visit the API or dashboard and ask the same kinds of questions through the served runtime.

## Where To Go Next

- [Architecture](../../ARCHITECTURE.md)
- [Hooks](../../HOOKS.md)
- [CLI Reference](../api/cli-reference.md)
- [Cognition Rollout Guide](cognition-rollout.md)
