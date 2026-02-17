# Plan: Rust Hooks System

**Track ID:** rust-hooks-system_20250216
**Status:** New

---

## Phase 1: Crate Setup and Core Traits

### Task 1.1: Create Hooks Crate
- [ ] Sub-task: Add `nexus-hooks` to workspace
- [ ] Sub-task: Configure dependencies (sysinfo, tokio signals)
- [ ] Sub-task: Create module structure
- [ ] Sub-task: Add tests to Cargo.toml

### Task 1.2: Define AgentHook Trait
- [ ] Sub-task: Define AgentHook trait with all methods
- [ ] Sub-task: Define SessionContext struct
- [ ] Sub-task: Define HookError types
- [ ] Sub-task: Add tests for trait definitions

### Task 1.3: Task: Maestro - Phase Verification 'Crate Setup and Core Traits'
- [ ] Sub-task: Verify crate builds
- [ ] Sub-task: Verify trait compiles
- [ ] Sub-task: Run `codex-reviewer` for Tzar review

---

## Phase 2: Hook Factory and Session Detection

### Task 2.1: Hook Factory Implementation
- [ ] Sub-task: Implement HookFactory
- [ ] Sub-task: Add agent type detection
- [ ] Sub-task: Implement hook creation for each agent
- [ ] Sub-task: Add tests for factory

### Task 2.2: Session Detection
- [ ] Sub-task: Implement process monitoring
- [ ] Sub-task: Implement session file detection
- [ ] Sub-task: Add support for detecting agent processes
- [ ] Sub-task: Add tests for detection

### Task 2.3: Task: Maestro - Phase Verification 'Hook Factory and Session Detection'
- [ ] Sub-task: Verify all agent types detectable
- [ ] Sub-task: Verify factory creates correct hooks
- [ ] Sub-task: Run `codex-reviewer` for Tzar review

---

## Phase 3: Four-Layer Extraction System

### Task 3.1: Native Hooks Layer
- [ ] Sub-task: Implement Claude Code hook
- [ ] Sub-task: Implement Gemini hook
- [ ] Sub-task: Implement Qwen hook
- [ ] Sub-task: Add tests for each native hook

### Task 3.2: Session Monitor Layer
- [ ] Sub-task: Implement process polling
- [ ] Sub-task: Detect session start/end
- [ ] Sub-task: Track session activity
- [ ] Sub-task: Add tests for monitoring

### Task 3.3: Inactivity Detector Layer
- [ ] Sub-task: Implement timeout detection
- [ ] Sub-task: Configurable timeout duration
- [ ] Sub-task: Add tests for detection

### Task 3.4: Persistent Buffer Layer
- [ ] Sub-task: Implement buffer storage
- [ ] Sub-task: Implement crash recovery
- [ ] Sub-task: Implement buffer flush
- [ ] Sub-task: Add tests for buffer

### Task 3.5: Task: Maestro - Phase Verification 'Four-Layer Extraction System'
- [ ] Sub-task: Verify 95-100% extraction reliability
- [ ] Sub-task: Verify all layers functional
- [ ] Sub-task: Run `codex-reviewer` for Tzar review

---

## Phase 4: MANDATORY Pi-Agent Hooks

### Task 4.1: pi-mono Hook Implementation
- [ ] Sub-task: Implement PiMonoHook struct
- [ ] Sub-task: Detect pi-mono process
- [ ] Sub-task: Read ~/.pi/agent/skills/
- [ ] Sub-task: Parse SKILL.md format
- [ ] Sub-task: Add tests for pi-mono hook

### Task 4.2: oh-my-pi Hook Implementation
- [ ] Sub-task: Implement OhMyPiHook struct
- [ ] Sub-task: Detect oh-my-pi process
- [ ] Sub-task: Read ~/.omp/agent/skills/
- [ ] Sub-task: Support TTSR triggers
- [ ] Sub-task: Add tests for oh-my-pi hook

### Task 4.3: pi-skills Cross-Compatible Hook
- [ ] Sub-task: Implement PiSkillsHook struct
- [ ] Sub-task: Support cross-platform skills
- [ ] Sub-task: Load skills from pi-skills repo
- [ ] Sub-task: Add tests for pi-skills hook

### Task 4.4: Task: Maestro - Phase Verification 'MANDATORY Pi-Agent Hooks'
- [ ] Sub-task: Verify all pi-agent hooks functional
- [ ] Sub-task: Verify skill loading works
- [ ] Sub-task: Run `codex-reviewer` for Tzar review

---

## Phase 5: Signal Handling and Integration

### Task 5.1: Signal Handling
- [ ] Sub-task: Implement SIGTERM handler
- [ ] Sub-task: Implement SIGINT handler
- [ ] Sub-task: Flush buffer on signal
- [ ] Sub-task: Add tests for signal handling

### Task 5.2: Storage Integration
- [ ] Sub-task: Integrate with nexus-storage
- [ ] Sub-task: Store extracted memories
- [ ] Sub-task: Implement session context storage
- [ ] Sub-task: Add integration tests

### Task 5.3: Task: Maestro - Final Phase Verification and User Approval
- [ ] Sub-task: Verify all acceptance criteria met
- [ ] Sub-task: Verify MANDATORY pi-agent support complete
- [ ] Sub-task: Verify 95%+ test coverage
- [ ] Sub-task: Run `codex-reviewer` for Tzar review
- [ ] Sub-task: Await user final approval

---

**Created:** 2025-02-16
