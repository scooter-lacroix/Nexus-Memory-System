# Getting Started

This is the fastest path from clone to a working local memory runtime.

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
nexus store --content "first memory" --agent codex --category session
```

## 5. Query the system

```bash
nexus search --query "first memory"
nexus recall --agent codex --query "first memory"
nexus stats
```

## 6. Install hooks and wrappers

```bash
nexus hooks install --agent all
nexus hooks status
```

## 7. Peek under the hood

```bash
nexus represent --agent codex --query "first memory" --introspect
```
