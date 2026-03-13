# Contributing to Nexus Memory System

Thanks for contributing.

## Scope

This repository is maintained as a Rust-first workspace. Public contributions should target the current crates under `crates/` and the public documentation set.

## Before Opening a Pull Request

- read [README.md](README.md)
- read [ARCHITECTURE.md](ARCHITECTURE.md)
- read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- check whether an issue or pull request already covers the same change

## Development Workflow

Create a focused branch:

```bash
git checkout -b fix/short-description
```

Run the standard validation set:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

If you change install behavior, hooks, or the web/API surface, also include a smoke test that exercises the changed path.

## Coding Expectations

### Rust

- keep changes small and reviewable
- prefer explicit APIs and predictable behavior
- add or update tests when behavior changes
- keep command help, README examples, and docs aligned

### Documentation

Update the docs in the same branch when you change:

- CLI commands or flags
- hook installation behavior
- API routes or server behavior
- repository layout
- release or installation steps

## Pull Request Checklist

- explain the problem
- describe the fix
- list validation commands
- note any migration or compatibility impact
- update docs if user-facing behavior changed

## Security Reports

Do not open public issues for sensitive vulnerabilities. Follow [SECURITY.md](SECURITY.md).
