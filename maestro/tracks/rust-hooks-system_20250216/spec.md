# Spec: Rust Hooks System

**Track ID:** rust-hooks-system_20250216
**Type:** Feature
**Status:** New

---

## Overview

Implement the hooks system in Rust for automated memory extraction. Includes AgentHook trait, factory, session detection, four-layer extraction system, and MANDATORY support for pi-mono, oh-my-pi, and pi-skills agent families.

**Python Mapping:** `nexus/hooks/`

---

## Functional Requirements

### FR1: AgentHook Trait

```rust
#[async_trait]
pub trait AgentHook: Send + Sync {
    fn agent_type(&self) -> &str;
    async fn install_session_end_hook(&mut self, callback: Arc<dyn Fn(String) + Send + Sync>) -> Result<(), Error>;
    async fn detect_session_activity(&self) -> bool;
    async fn extract_session_context(&self) -> Result<SessionContext, Error>;
}
```

### FR2: Hook Factory

- Create agent-specific hooks from configuration
- Support all supported agent types
- Lazy initialization

### FR3: Four-Layer Extraction System

1. **Native Hooks** (100%): Claude Skills, Gemini Functions, Qwen Hooks, pi-mono, oh-my-pi
2. **Session Monitor** (95%): Process monitoring via sysinfo/heim
3. **Inactivity Detector** (90%): 5-min timeout detection
4. **Persistent Buffer** (99%): Crash recovery from buffer

### FR4: MANDATORY Pi-Agent Support

#### pi-mono (badlogic/pi-mono)
- Repository: https://github.com/badlogic/pi-mono
- Config paths: `~/.pi/agent/skills/`, `.pi/skills/`
- Process detection: `pi` or `pi-coding-agent`
- Skills format: SKILL.md compatible

#### oh-my-pi (can1357/oh-my-pi)
- Repository: https://github.com/can1357/oh-my-pi
- Config paths: `~/.omp/agent/skills/`, `.omp/skills/`
- Process detection: `omp` or `oh-my-pi`
- Features: Rust N-API, TTSR, MCP plugin system

#### pi-skills (badlogic/pi-skills)
- Cross-compatible skills repository
- Compatible with: pi-mono, oh-my-pi, Claude Code, Codex CLI, Amp, Droid
- Skills: brave-search, browser-tools, gccli, gdcli, gmcli, transcribe, vscode, youtube-transcript

### FR5: Session Detection

- Process monitoring (sysinfo or heim crates)
- Session file detection
- Signal handling (SIGTERM, SIGINT)

---

## Non-Functional Requirements

### NFR1: Reliability

| Layer | Success Rate Target |
|-------|---------------------|
| Native Hooks | 100% |
| Session Monitor | 95% |
| Inactivity Detector | 90% |
| Persistent Buffer | 99% |

**Overall:** 95-100% memory capture reliability

### NFR2: Platform Support

- Linux (primary)
- macOS
- Windows (best-effort)

### NFR3: Code Quality

- 95%+ test coverage
- Signal-safe code
- Cross-platform where possible

---

## Acceptance Criteria

### AC1: Hook Creation and Detection

```rust
let factory = HookFactory::new();
let mut hook = factory.create_hook("claude-code")?;
assert!(hook.detect_session_activity().await);
```

### AC2: Four-Layer System Functional

```rust
let extractor = MultiLayerExtractor::new(hooks);
let context = extractor.extract_context().await?;
assert!(context.memories.len() > 0 || context.buffer.len() > 0);
```

### AC3: Pi-Agent Hooks Functional

```rust
let pi_mono_hook = factory.create_hook("pi-mono")?;
let oh_my_pi_hook = factory.create_hook("oh-my-pi")?;
let pi_skills_hook = factory.create_hook("pi-skills")?;
```

### AC4: Signal Handling

- Graceful shutdown on SIGTERM/SIGINT
- Buffer flush before exit
- No memory loss on crash

---

## Dependencies

### External Crates

```toml
[dependencies]
sysinfo = "0.30"      # Process monitoring
tokio = { version = "1.40", features = ["signal"] }
async-trait = "0.1"
serde_json = "1.0"
```

### Local Dependencies

- `nexus-core` - Core types, SessionContext
- `nexus-storage` - Buffer persistence

---

## Out of Scope

- Custom hook implementations for external agents (future extension)
- Real-time stream processing (deferred)
- Hook marketplace (future feature)

---

## Pi-Agent Implementation Details

### pi-mono Hook

```rust
pub struct PiMonoHook {
    agent_type: &'static str,
    config_dir: PathBuf,
    skills_dir: PathBuf,
}

impl PiMonoHook {
    pub const AGENT_TYPE: &'static str = "pi-mono";
    pub const CONFIG_DIR: &'static str = ".pi";
    pub const SKILLS_DIR: &'static str = "agent/skills";
}
```

### oh-my-pi Hook

```rust
pub struct OhMyPiHook {
    agent_type: &'static str,
    config_dir: PathBuf,
    skills_dir: PathBuf,
    // Rust N-API features
    has_native_engine: bool,
}

impl OhMyPiHook {
    pub const AGENT_TYPE: &'static str = "oh-my-pi";
    pub const CONFIG_DIR: &'static str = ".omp";
    pub const SKILLS_DIR: &'static str = "agent/skills";

    // Native Rust N-API features
    pub fn has_native_grep(&self) -> bool { true }
    pub fn has_native_shell(&self) -> bool { true }
}
```

### Skill Format (SKILL.md)

```markdown
---
name: skill-name
description: Short description
triggers:
  - on_session_end
  - on_checkpoint
---

# Instructions

Helper files at: {baseDir}/
```

---

## References

- Python implementation: `nexus/hooks/`
- CLAUDE.md: Pi Agent Family section
- pi-mono: https://github.com/badlogic/pi-mono
- oh-my-pi: https://github.com/can1357/oh-my-pi
- pi-skills: https://github.com/badlogic/pi-skills

---

**Version:** 1.0
**Created:** 2025-02-16
