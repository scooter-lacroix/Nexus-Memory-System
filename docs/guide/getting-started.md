# Getting Started

## 1. Build the CLI

```bash
cargo build --release -p nexus-cli
```

## 2. Install Nexus

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
nexus stats
```

## 6. Install hooks

```bash
nexus hooks install --agent all
```
