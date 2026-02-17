# Migration Guide: Python to Rust

This guide explains how to migrate from the Python implementation of Nexus Memory System to the Rust implementation.

## Overview

The Rust implementation provides significant performance improvements:

| Operation | Python | Rust Target |
|-----------|--------|-------------|
| Embedding | ~10ms | <5ms |
| Vector search (1k) | ~50ms | <10ms |
| Memory store | ~5ms | <1ms |
| Concurrent load | ~100 | 10,000+ |

## Prerequisites

Before migrating, ensure you have:

1. **Rust 1.75+** installed
2. **Existing Python Nexus database** (typically at `~/.nexus/nexus.db`)
3. **sqlite3 CLI** (for database inspection)
4. **ripgrep (rg)** - optional, for faster database discovery

## Migration Steps

### Step 1: Discover Existing Databases

Find all Nexus databases on your system:

```bash
nexus migrate discover
```

This will search your home directory for `nexus.db` files and display:

- Database paths
- Sizes
- Memory and namespace counts
- Last modified dates

### Step 2: Check Migration Status

Before migrating, check the current status:

```bash
nexus migrate status
```

This shows:
- Whether a target database exists
- Current memory and namespace counts
- Available backups

### Step 3: Dry Run (Recommended)

Perform a dry run to see what would be migrated without making changes:

```bash
nexus migrate run --dry-run
```

### Step 4: Run Migration

Run the migration with automatic backup:

```bash
nexus migrate run
```

Or specify paths explicitly:

```bash
nexus migrate run \
  --from ~/.nexus/nexus.db \
  --to ~/.nexus/nexus-rust.db \
  --backup ~/.nexus/nexus-backup.db
```

### Step 5: Validate Migration

After migration, validate the data integrity:

```bash
nexus migrate validate
```

This compares:
- Namespace counts
- Memory counts
- Data integrity checks

## Migration Commands Reference

### `nexus migrate discover`

Find all Nexus databases on the system.

```bash
# Search default home directory
nexus migrate discover

# Search specific path
nexus migrate discover --path /path/to/search

# Limit search depth
nexus migrate discover --depth 5
```

### `nexus migrate status`

Show migration status for a database.

```bash
# Check default database
nexus migrate status

# Check specific database
nexus migrate status --db /path/to/nexus.db
```

### `nexus migrate run`

Run the migration from Python to Rust.

```bash
# Basic migration with automatic backup
nexus migrate run

# Dry run (no changes)
nexus migrate run --dry-run

# Skip backup (not recommended)
nexus migrate run --no-backup

# Custom paths
nexus migrate run --from /path/to/source.db --to /path/to/target.db
```

Options:
- `--from` - Source Python database path
- `--to` - Target Rust database path
- `--backup` - Custom backup path
- `--no-backup` - Skip creating backup
- `--dry-run` - Show what would be migrated

### `nexus migrate validate`

Validate migration integrity.

```bash
# Validate default databases
nexus migrate validate

# Validate specific databases
nexus migrate validate --from /path/to/source.db --to /path/to/target.db
```

### `nexus migrate rollback`

Rollback a migration using the backup.

```bash
# Rollback using default backup location
nexus migrate rollback

# Rollback with custom paths
nexus migrate rollback --backup /path/to/backup.db --to /path/to/target.db
```

## What Gets Migrated

### Tables Migrated

1. **agent_namespaces** - Agent namespace configurations
2. **memories** - All stored memories with embeddings
3. **task_specifications** - Reusable task specifications
4. **memory_relations** - Relationships between memories
5. **system_metrics** - System monitoring data

### Data Transformations

The migration handles these transformations automatically:

1. **Datetime Conversion** - Python datetime strings to Rust chrono types
2. **Embeddings** - JSON arrays to vector format
3. **JSON Metadata** - Preserved as-is
4. **Memory Categories** - Preserved exactly
5. **Memory Lane Types** - Preserved exactly

## Rollback Procedure

If you need to rollback to the Python version:

### Option 1: Using the CLI

```bash
nexus migrate rollback
```

### Option 2: Manual Rollback

1. Stop the Rust server
2. Restore the backup:
   ```bash
   cp ~/.nexus/nexus.db.bak ~/.nexus/nexus.db
   ```
3. Start the Python server

### Option 3: Full Restore

If you have a pre-rollback backup:

```bash
cp ~/.nexus/pre-rollback.db ~/.nexus/nexus.db
```

## Troubleshooting

### "Source database does not exist"

The default Python database location was not found. Use `--from` to specify the path:

```bash
nexus migrate run --from /path/to/your/nexus.db
```

### "Namespace count mismatch"

This can happen if:
- The source database was modified during migration
- There were existing namespaces in the target

**Solution:** Run migration with `--reset` flag or manually reconcile.

### "Failed to connect to target database"

Check that:
- The target directory exists
- You have write permissions
- The path is valid

### "sqlite3 not found"

Install sqlite3 CLI tools:

```bash
# Ubuntu/Debian
sudo apt-get install sqlite3

# macOS
brew install sqlite3

# Arch Linux
sudo pacman -S sqlite
```

### "ripgrep not found"

ripgrep is optional but recommended for faster discovery:

```bash
# Ubuntu/Debian
sudo apt-get install ripgrep

# macOS
brew install ripgrep

# Arch Linux
sudo pacman -S ripgrep
```

## Post-Migration Checklist

After successful migration:

1. [ ] Validate migration with `nexus migrate validate`
2. [ ] Test memory search: `nexus search --query "test"`
3. [ ] Test memory store: `nexus store --content "test memory"`
4. [ ] Check statistics: `nexus stats`
5. [ ] Verify backup exists
6. [ ] Keep Python backup for 1-2 weeks before cleanup

## Performance Comparison

After migration, you can run benchmarks to verify performance improvements:

```bash
# Run benchmarks
cargo bench --workspace
```

Expected results:
- Embedding latency: <5ms (vs ~10ms Python)
- Vector search: <10ms for 1k docs (vs ~50ms Python)
- Memory store: <1ms (vs ~5ms Python)

## Support

If you encounter issues during migration:

1. Check the migration report at `~/.nexus/nexus.migration.json`
2. Check validation report at `~/.nexus/nexus.validation.json`
3. Review error messages and warnings
4. Use `--dry-run` to test without changes

## FAQ

**Q: Will I lose any data during migration?**

A: No. The migration tool creates a backup before making any changes. The original Python database remains untouched.

**Q: Can I run the migration multiple times?**

A: Yes. Running migration again will skip already-migrated namespaces and memories.

**Q: How long does migration take?**

A: For typical databases (<10k memories), migration takes less than a minute.

**Q: Can I migrate incrementally?**

A: Yes. The migration tool supports incremental migration by skipping existing records.

**Q: What about embeddings?**

A: Embeddings are migrated as-is. You don't need to regenerate them.
