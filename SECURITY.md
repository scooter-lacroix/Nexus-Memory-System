# Security Policy

## Supported Versions

Security fixes are applied on a best-effort basis to the active branch of this repository. In practice, contributors should assume:

- `master` or the default branch: supported
- historical snapshots and stale local forks: unsupported

## Reporting a Vulnerability

Please do not open a public GitHub issue for security-sensitive vulnerabilities.

Instead:

1. Send a private report to the project maintainer through the contact path documented in the repository settings or maintainer profile.
2. Include a clear description of the issue, affected files or commands, impact, and reproduction steps.
3. If possible, include a minimal proof of concept that avoids exposing secrets or harming systems.

Useful report contents:

- affected version or commit
- environment details
- exact command or API path
- expected behavior
- actual behavior
- impact assessment
- mitigation ideas, if known

## Response Expectations

Best-effort targets:

- Initial acknowledgement: within 5 business days
- Triage decision: within 10 business days
- Remediation timeline: depends on severity and maintainer availability

These targets are goals, not guarantees.

## Disclosure

Please allow reasonable time for triage and remediation before public disclosure.

Once a fix is available, maintainers may:

- merge a patch
- publish a changelog note
- add migration or upgrade guidance
- request coordinated disclosure timing

## Scope

Potentially sensitive areas in this repository include:

- CLI argument handling
- local file and shell integration
- hook execution paths
- agent integration scripts
- database access and migration logic
- web and MCP transport surfaces

## Non-Security Bugs

For ordinary defects, installation issues, documentation problems, and feature requests, use the standard support and issue-reporting paths described in [SUPPORT.md](SUPPORT.md).
