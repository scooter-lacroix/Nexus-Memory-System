# Nexus Memory System

[![Rust](https://img.shields.io/badge/Rust-Workspace-black?logo=rust)](https://www.rust-lang.org/)
[![SQLite](https://img.shields.io/badge/Storage-SQLite-07405E?logo=sqlite)](https://sqlite.org/)
[![MCP](https://img.shields.io/badge/Protocol-MCP-4B5563)](https://modelcontextprotocol.io/)
[![GitHub stars](https://img.shields.io/github/stars/scooter-lacroix/Nexus-Memory-System?style=social)](https://github.com/scooter-lacroix/Nexus-Memory-System)
[![MIT License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

![Nexus Memory System banner](docs/images/banner.png)

Nexus Memory System gives AI coding agents a shared memory, a bounded subconscious, and a practical way to stay context-aware across sessions without turning your machine into a distributed-systems hobby project.

It is a Rust-first, SQLite-backed memory runtime for tools like Claude Code, Codex, Gemini CLI, Qwen, Amp, OpenCode, Droid, and Hermes. Nexus captures useful activity, distills noise into signal, builds semantic recall, and runs bounded background dreaming so your agents can remember what matters and stop forgetting what they just learned.

If you want the short version: Nexus makes your agents feel sharper, steadier, and dramatically less forgetful without asking you to ship your workflow into an overbuilt memory platform.

## Why People Notice Nexus

- Always-on memory without requiring a heavyweight external stack
- Automatic lifecycle capture through hooks, wrappers, and monitor-aware integrations
- Representation-first recall built from explicit observations, session digests, semantic matches, derived insights, and contradictions
- Bounded dreaming that reinforces patterns, detects conflicts, and compresses noisy activity into usable memory
- Provider-flexible generation and embeddings with remote and local runtime options
- One shared memory layer across CLI, web, MCP, and agent integrations

This is the part that matters: Nexus does not just log events. It turns activity into something an agent can actually use.

## What Nexus Feels Like

Imagine a coding agent that can:

- remember that you switched providers, fixed the installer, and validated the release
- recall the right session digest instead of replaying a hundred raw tool events
- notice that a newer memory contradicts an older assumption
- answer with the memories it used and explain where they came from
- keep doing all of that locally, with bounded runtime costs, while you keep working

That is the system this repo ships.

## The Big Ideas

### 1. Capture what matters, not just the noise

Nexus hooks and wrappers capture lifecycle events, tool activity, and session context from supported agent environments. Low-signal operational events are not simply dumped into the main memory table forever. They can be buffered, distilled, summarized, and folded into higher-value session memory.

### 2. Turn raw activity into explicit memory

The cognition runtime derives explicit observations from raw activity, attaches evidence lineage, and stores cognitive metadata that higher-level recall can reason over.

### 3. Dream in bounded cycles

Nexus enables consolidation and reflection through dreaming. Dream cycles are bounded, replay-safe, and practical. They reinforce recurring truths, detect contradictions, refresh digests, and produce more retrieval-friendly memory than a plain event log ever could.

### 4. Recall with a working representation

Instead of searching raw text and hoping for the best, Nexus builds a working representation from:

- recent explicit memories
- vector-ranked semantic matches
- session digests
- derived insights
- contradictions and conflict markers

That gives your agent a usable context window instead of a bag of unranked lines.

### 5. Stay flexible about models

Nexus supports remote and local generation and embeddings. You can:

- use the same provider and same model for both
- use the same provider with a different embedding model
- use different providers for generation and embeddings
- run local OpenAI-compatible backends through `vLLM`, `LM Studio`, or `llama.cpp`
- keep local ONNX embeddings if you prefer

## Quick Start

### 1. Clone, build, and install

```bash
git clone https://github.com/scooter-lacroix/Nexus-Memory-System.git
cd Nexus-Memory-System
cargo build --release -p nexus-memory
./scripts/install.sh --binary ./target/release/nexus
```

### 2. Initialize storage

```bash
nexus init
```

### 3. Store and recall a first memory

```bash
nexus store \
  --content "Release validation passed after fixing provider-backed embeddings" \
  --agent codex \
  --category session \
  --labels release,validation,embeddings

nexus recall --agent codex --query "What changed in the release validation work?"
```

### 4. Install hooks and wrappers

```bash
nexus hooks install --agent all
nexus hooks status --verbose
```

### 5. Inspect the subconscious

```bash
nexus represent --agent claude-code --query "provider rollout timeline" --introspect
nexus digest --agent claude-code --session-key <session-key>
nexus dream --agent claude-code
```

### 6. Configure providers and embeddings

```bash
nexus config
nexus config show
```

### 7. Start the API and dashboard

```bash
NEXUS_AGENT_ENABLED=true nexus serve --transport web --port 8768 --agent
```

## Fast Examples

### Ask Nexus what actually happened

```bash
nexus recall --agent claude-code --query "What changed in the installer and why?"
```

### See the exact memory mix used for recall

```bash
nexus represent --agent claude-code --query "What changed in the installer and why?" --introspect
```

### Pull the latest digest for a session

```bash
nexus digest latest --agent claude-code --session-key <session-key>
```

### Run a manual dream cycle

```bash
nexus dream run --agent claude-code
```

### Explain where a memory came from

```bash
nexus lineage show --memory-id <id>
```

## What Ships

### Cognition runtime

- explicit derivation from raw activity
- short and long session digests
- bounded dreaming with contradiction handling and reinforcement
- identity-aware, representation-first recall
- introspection and lineage explanation

### Multi-agent integration

- native lifecycle integrations for Claude Code and the pi-family tools
- wrapper-based lifecycle support for Codex, Amp, OpenCode, Droid, and Hermes
- monitor-aware support for Gemini and Qwen
- honest support-tier reporting so the system tells you what is truly installed and active

### Retrieval and embeddings

- vector-first semantic retrieval with bounded text fallback
- provider-backed embeddings
- local ONNX embeddings
- local OpenAI-compatible runtimes
- configurable provider/model inheritance between generation and embeddings

### Operator tooling

- CLI commands for `list`, `recall`, `represent`, `digest`, `dream`, `lineage`, `session`, and migration flows
- MCP server access to the same cognition layer
- web agent and observability routes
- runtime health, digests, recall composition, and job visibility

## Architecture At A Glance

```text
Agent tools and clients
    |
    +-- nexus-cli
    +-- nexus-hooks
    +-- nexus-mcp
    +-- nexus-web
             |
             v
         nexus-core
             |
             +-- nexus-storage
             +-- nexus-vectors
             +-- nexus-embeddings
             +-- nexus-llm
             +-- nexus-orchestrator
             +-- nexus-agent
```

The shared domain model lives in `nexus-core`. The canonical store lives in `nexus-storage`. The cognition engine lives primarily in `nexus-agent`, powered by `nexus-llm`, `nexus-embeddings`, and `nexus-vectors`. CLI, hooks, MCP, and web all sit on top of the same memory runtime rather than building parallel silos.

For the full architectural walkthrough, see [ARCHITECTURE.md](ARCHITECTURE.md).

## Why Nexus Works

Many agent memory tools either:

- store too little to be useful
- store too much noise to be usable
- depend on a heavy remote stack
- or pretend retrieval is solved once a vector search returns ten strings

Nexus takes a different route:

- keep the source of truth local and understandable
- enrich memory gradually instead of pretending raw logs are knowledge
- dream in bounded cycles instead of open-ended background churn
- build a working representation for recall instead of a loose search result list
- give operators visibility into what the system is doing

That combination is why the system feels more like a subconscious and less like a database wrapper.

## Provider and Embedding Flexibility

Nexus supports generation through providers such as OpenAI, Anthropic, Gemini, OpenRouter, Groq, Z.ai, Minimax, and Mistral. Embeddings are independently configurable.

You can run:

- remote generation + remote embeddings
- remote generation + local embeddings
- local generation + local embeddings
- local generation + remote embeddings

That includes OpenAI-compatible local runtimes such as:

- `vLLM`
- `LM Studio`
- `llama.cpp`

## Documentation

### Start here

- [Installation Guide](INSTALLATION.md)
- [Architecture](ARCHITECTURE.md)
- [Hooks](HOOKS.md)
- [Documentation Index](docs/index.md)

### Guides

- [Getting Started](docs/guide/getting-started.md)
- [Cognition Rollout Guide](docs/guide/cognition-rollout.md)
- [Embeddings Guide](docs/guide/embeddings.md)
- [Cognition Excellence Release Note](docs/guide/cognition-excellence-release-note.md)

### Reference

- [CLI Reference](docs/api/cli-reference.md)
- [REST API Reference](docs/api/rest-api.md)

## Repository Layout

```text
.
├── crates/
│   ├── nexus-agent/
│   ├── nexus-cli/
│   ├── nexus-core/
│   ├── nexus-embeddings/
│   ├── nexus-hooks/
│   ├── nexus-lephase/
│   ├── nexus-llm/
│   ├── nexus-mcp/
│   ├── nexus-orchestrator/
│   ├── nexus-storage/
│   ├── nexus-vectors/
│   └── nexus-web/
├── docs/
├── scripts/
├── Cargo.toml
└── Cargo.lock
```

## LLM Provider Evaluation

The `nexus eval` command tests an LLM provider against the memory system's core operations: structured extraction, consolidation, and query synthesis. Each aspect is scored out of 200 for a total of 600.

| Provider / Model | Extraction | Consolidation | Query | Total | Rating |
|---|---|---|---|---|---|
| OpenRouter / `arcee-ai/trinity-large-preview:free` | 185 | 185 | 185 | **555 / 600** | GOOD |
| Z.ai / `glm-4.5` | 180 | 170 | 170 | **520 / 600** | GOOD |
| Gemini / `gemini-3.1-flash-lite-preview` | 100 | 80 | 55 | **235 / 600** | EXPERIMENTAL |
| Groq / `moonshotai/kimi-k2-instruct-0905` | 100 | 80 | 55 | **235 / 600** | EXPERIMENTAL |
| Groq / `llama-3.3-70b-versatile` | 160 | 155 | 150 | **465 / 600** | ACCEPTABLE |

### Practical picks

- Best overall value: OpenRouter / `arcee-ai/trinity-large-preview:free`
- Strong structured extraction: Z.ai / `glm-4.5`
- Best used selectively or after environment-specific validation: Gemini and Groq budget models

## Validation

Recommended validation before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p nexus-memory-agent --bench cognition
```

## License

This project is licensed under the [MIT License](LICENSE).
