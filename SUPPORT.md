# Support

## Getting Help

If you need help with Nexus Memory System, start with the documentation:

- [README.md](README.md)
- [INSTALLATION.md](INSTALLATION.md)
- [DEVELOPMENT.md](DEVELOPMENT.md)
- [HOOKS.md](HOOKS.md)
- [MIGRATION.md](MIGRATION.md)
- [docs/](docs/)

## Where to Ask Questions

Use GitHub issues for:

- reproducible bugs
- documentation problems
- installation issues
- feature requests
- migration questions tied to the repo

When opening an issue, include:

- operating system
- Rust and Python versions if relevant
- exact command run
- expected result
- actual result
- logs or error output
- whether the problem is in the Rust path, Python path, or installation flow

## Before Opening an Issue

Please check:

- existing issues
- recent commits and changelog notes
- installation prerequisites
- whether the issue only affects local machine-specific configuration

## Best Practices for Faster Help

- provide a minimal reproduction
- paste exact commands, not paraphrases
- mention the commit or branch you are using
- say whether you are using the installed `nexus` binary or running from a build artifact
- note whether your issue involves shared CLI hooks or only the core storage layer

## Security Issues

Do not report vulnerabilities publicly in issues. Follow [SECURITY.md](SECURITY.md).

## Scope Notes

This repository currently contains both:

- a Rust-first implementation under `crates/`
- a legacy Python implementation under `nexus/`

Questions are much easier to answer when you specify which one you are working with.
