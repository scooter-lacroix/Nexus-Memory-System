# Auto-Star Implementation

This document describes the automatic GitHub repository starring feature implemented across all Nexus Memory System installation methods.

## Overview

All installation methods now automatically star the Nexus Memory System repository (https://github.com/scooter-lacroix/Nexus-Memory-System.git) when users install the software. This feature is non-intrusive and includes a simple opt-out mechanism.

## Implementation Details

### 1. Bash Script (`scripts/star-repo.sh`)

A standalone bash script that:
- Checks if `NEXUS_NO_STAR=1` environment variable is set
- Uses a marker file (`~/.config/nexus-memory-system/.star-attempted`) to avoid repeated attempts
- Requires `gh` CLI to be installed and authenticated
- Runs silently in the background
- Fails gracefully if `gh` is not available or not authenticated

### 2. Rust CLI Integration (`crates/nexus-cli/src/star.rs`)

A Rust module that:
- Spawns a background thread on CLI startup
- Checks the same marker file and environment variable
- Uses `gh` CLI via `std::process::Command`
- Runs asynchronously without blocking CLI operations
- Integrated into `main.rs` to run on every CLI invocation

### 3. Python CLI Integration (`nexus/star.py`)

A Python module that:
- Uses threading to run in the background
- Checks the same marker file and environment variable
- Uses `subprocess` to call `gh` CLI
- Integrated into `nexus/cli.py` main entry point
- Handles timeouts and errors gracefully

### 4. Install Script Integration (`scripts/install.sh`)

The main installation script now:
- Calls `star-repo.sh` in the background during installation
- Displays the opt-out message at the end of installation
- Does not interrupt the installation workflow

## Opt-Out Mechanism

Users can disable auto-starring by setting an environment variable before installation or CLI usage:

```bash
export NEXUS_NO_STAR=1
```

This variable is checked by all three implementations (bash, Rust, Python).

## User Experience

### Non-Intrusive Design

- Runs in background thread/process
- Does not block installation or CLI operations
- Fails silently if `gh` is not available
- Only attempts once (marker file prevents retries)
- No output or notifications to the user

### Visibility

The opt-out option is mentioned in:
- README.md (Quick Start section)
- INSTALLATION.md (both Rust and Python sections)
- Install script output (final message)

## Technical Details

### Marker File Location

`~/.config/nexus-memory-system/.star-attempted` (or `$XDG_CONFIG_HOME/nexus-memory-system/.star-attempted`)

This file is created after the first star attempt (successful or not) to prevent repeated API calls.

### GitHub API Call

```bash
gh api --silent -X PUT /user/starred/scooter-lacroix/Nexus-Memory-System
```

This requires:
- `gh` CLI installed
- User authenticated with `gh auth login`
- Appropriate GitHub permissions

### Error Handling

All implementations handle these scenarios gracefully:
- `gh` CLI not installed → skip silently
- `gh` not authenticated → skip silently
- API call fails → skip silently
- Network unavailable → skip silently

## Testing

To test the implementation:

1. **Test normal flow:**
   ```bash
   gh auth login
   rm -f ~/.config/nexus-memory-system/.star-attempted
   ./scripts/install.sh --binary ./target/release/nexus
   # Check if repo is starred on GitHub
   ```

2. **Test opt-out:**
   ```bash
   export NEXUS_NO_STAR=1
   rm -f ~/.config/nexus-memory-system/.star-attempted
   ./scripts/install.sh --binary ./target/release/nexus
   # Verify repo is not starred
   ```

3. **Test without gh CLI:**
   ```bash
   unset NEXUS_NO_STAR
   rm -f ~/.config/nexus-memory-system/.star-attempted
   PATH=/usr/bin:/bin ./scripts/install.sh --binary ./target/release/nexus
   # Should complete without errors
   ```

## Files Modified

- `scripts/star-repo.sh` (new)
- `scripts/install.sh` (modified)
- `crates/nexus-cli/src/star.rs` (new)
- `crates/nexus-cli/src/main.rs` (modified)
- `nexus/star.py` (new)
- `nexus/cli.py` (modified)
- `README.md` (modified)
- `INSTALLATION.md` (modified)
