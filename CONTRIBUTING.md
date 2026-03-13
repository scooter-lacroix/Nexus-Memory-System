# Contributing to Nexus Memory System

Thanks for contributing. This document describes the expected workflow for changes to the Rust-first Nexus Memory System repository.

## Before You Start

- Read the [README.md](README.md) for project context
- Read the [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- Check open issues and existing pull requests before starting overlapping work
- Open or comment on an issue first for large features, behavioral changes, or architectural refactors

## Development Model

This repository currently contains two implementation paths:

- `crates/`: primary Rust implementation
- `nexus/`: legacy Python implementation kept for compatibility and migration support

New feature work should generally target the Rust workspace unless the task is specifically about backward compatibility, migration, or legacy support.

## Local Setup

### Rust

```bash
cargo build --workspace
cargo test --workspace
```

### Python legacy path

```bash
python -m venv .venv
source .venv/bin/activate
pip install -e .[dev,test]
pytest
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for a fuller workflow.

## Branching

Use focused branches. Suggested patterns:

- `fix/<short-description>`
- `feat/<short-description>`
- `docs/<short-description>`
- `chore/<short-description>`

Examples:

```bash
git checkout -b fix/rust-cli-stats
git checkout -b docs/community-health-files
```

## Commit Guidelines

Prefer small, reviewable commits with clear intent.

Good commit messages:

- `Fix Rust CLI stats to read live database counts`
- `Add community health files for public GitHub launch`
- `Document local development workflow`

## Pull Request Expectations

Each pull request should:

- explain the problem being solved
- describe the approach taken
- call out breaking changes or migration impact
- include validation steps
- update docs when behavior or setup changes

Use the repository pull request template if available.

## Coding Standards

### Rust

- Run `cargo fmt --all`
- Run `cargo clippy --workspace --all-targets`
- Add or update tests when behavior changes
- Prefer small, explicit APIs over broad implicit behavior

### Python

- Use clear type hints where practical
- Keep compatibility changes scoped and documented
- Run the relevant tests for touched modules

### Documentation

- Keep docs accurate to the current repo state
- Prefer direct setup commands over vague guidance
- Update top-level docs when installation or supported workflows change

## Testing

Run the narrowest meaningful test set while developing, and a broader set before opening a PR.

Typical Rust validation:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

If your change affects installation or the shared CLI flow, include a smoke test such as:

```bash
cargo build --release -p nexus-cli
./target/release/nexus init
./target/release/nexus stats
```

## Documentation Changes

Please update docs when you change:

- CLI flags or command behavior
- installation flow
- migration steps
- hook/integration setup
- environment variables
- repository structure or contribution workflow

## Reporting Security Issues

Do not open public issues for sensitive vulnerabilities. Follow [SECURITY.md](SECURITY.md).

## Questions and Support

See [SUPPORT.md](SUPPORT.md) for where to ask questions and how to provide reproducible reports.
