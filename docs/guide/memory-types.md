# Memory Types Guide

> **Understanding the Hybrid Memory Type System**

**Version:** 1.1.0

---

## Table of Contents

- [Overview](#overview)
- [Nexus Categories](#nexus-categories)
- [Memory Lane Cognitive Types](#memory-lane-cognitive-types)
- [Memory Lane Priority Types](#memory-lane-priority-types)
- [Agent-Specific Categories](#agent-specific-categories)
- [When to Use Each Type](#when-to-use-each-type)
- [Best Practices](#best-practices)

---

## Overview

Nexus uses a **hybrid memory type system** that combines:

1. **Nexus Categories** - Core, flexible categorization
2. **Memory Lane Cognitive Types** - Science-based cognitive taxonomy
3. **Memory Lane Priority Types** - Priority-based categorization
4. **Agent-Specific Categories** - Per-agent specialized categories

This hybrid approach provides:
- Flexibility for diverse use cases
- Cognitive science grounding
- Priority-based retrieval
- Agent namespace isolation

---

## Nexus Categories

The original Nexus categories are preserved for backward compatibility.

### Core Categories

| Category | Description | Use When |
|----------|-------------|----------|
| `general` | General purpose memories | Default category, miscellaneous |
| `facts` | Factual information | Concrete facts, data points |
| `preferences` | User preferences and settings | User choices, configurations |
| `context` | Situational context | Session context, environment info |
| `specifications` | Task specifications | Reusable task specifications |
| `session` | Session-based memories | Extracted session context |

### Examples

```python
# General memory
await manager.store_memory(
    content="Project uses Python 3.11",
    agent_type="claude-code",
    category="general"
)

# Fact
await manager.store_memory(
    content="The speed of light is 299,792,458 m/s",
    agent_type="general",
    category="facts"
)

# Preference
await manager.store_memory(
    content="User prefers dark mode in all applications",
    agent_type="claude-code",
    category="preferences",
    labels=["ui", "theme"]
)

# Context
await manager.store_memory(
    content="Working on e-commerce checkout optimization",
    agent_type="claude-code",
    category="context"
)

# Session
await manager.store_memory(
    content="Session focused on refactoring payment module",
    agent_type="claude-code",
    category="session"
)
```

---

## Memory Lane Cognitive Types

Based on cognitive science research, these types represent how human memory works.

### Cognitive Types

| Type | Description | Use When |
|------|-------------|----------|
| `semantic` | General knowledge and facts | Storing domain knowledge |
| `episodic` | Event-based experiences | Remembering specific events |
| `procedural` | How-to knowledge and processes | Storing workflows |
| `working` | Temporary active processing | Short-term context |
| `explicit` | Conscious declarative facts | Deliberate memories |
| `implicit` | Unconscious patterns | Learned patterns |
| `flashbulb` | High-importance events | Significant events |
| `metamemory` | Knowledge about memory | Memory about memory |
| `collective` | Cross-agent shared knowledge | Shared across agents |

### Examples

```python
# Semantic memory (general knowledge)
await manager.store_memory(
    content="FastAPI is a modern Python web framework",
    agent_type="claude-code",
    category="semantic",
    memory_lane_type="semantic"
)

# Episodic memory (event-based)
await manager.store_memory(
    content="During the sprint planning, we decided to postpone the payment gateway refactor",
    agent_type="claude-code",
    category="episodic",
    memory_lane_type="episodic",
    metadata={"event_date": "2025-12-23", "event_type": "sprint_planning"}
)

# Procedural memory (how-to)
await manager.store_memory(
    content="To deploy: run tests, build docker image, push to registry, update k8s deployment",
    agent_type="claude-code",
    category="procedural",
    memory_lane_type="procedural",
    labels=["deployment", "workflow"]
)

# Working memory (temporary)
await manager.store_memory(
    content="Current task: fix checkout button styling",
    agent_type="claude-code",
    category="working",
    memory_lane_type="working"
)

# Flashbulb memory (high importance)
await manager.store_memory(
    content="Critical bug discovered in payment processing - affects all transactions",
    agent_type="claude-code",
    category="flashbulb",
    memory_lane_type="flashbulb",
    metadata={"priority": "critical", "discovered": "2025-12-23"}
)

# Collective memory (shared)
await manager.store_memory(
    content="All agents should prioritize user privacy over optimization",
    agent_type="general",
    category="collective",
    memory_lane_type="collective"
)
```

---

## Memory Lane Priority Types

Priority-based types for retrieval and importance scoring.

### Priority Levels

#### High Priority (1)

| Type | Description | Use When |
|------|-------------|----------|
| `correction` | User corrected agent behavior | User explicitly corrected something |
| `decision` | Explicit choice with reasoning | A decision was made with rationale |
| `commitment` | User preference/commitment | User committed to a preference |

#### Medium Priority (2)

| Type | Description | Use When |
|------|-------------|----------|
| `insight` | Non-obvious discovery or connection | New insight discovered |
| `learning` | New knowledge gained | Agent learned something new |
| `confidence` | Strong confidence in approach | High confidence expressed |

#### Lower Priority (3)

| Type | Description | Use When |
|------|-------------|----------|
| `pattern_seed` | Repeated behavior worth formalizing | Potential pattern detected |
| `cross_agent` | Info relevant to other agents | Shareable information |
| `workflow_note` | Process observation | Workflow observation |
| `gap` | Missing capability or limitation | Limitation identified |

### Examples

```python
# Correction (high priority)
await manager.store_memory(
    content="User corrected: don't use asyncio.sleep(), use asyncio.create_task() instead",
    agent_type="claude-code",
    category="correction",
    memory_lane_type="correction",
    metadata={"priority": "high", "user_corrected": True}
)

# Decision (high priority)
await manager.store_memory(
    content="Decision: Use PostgreSQL for production, SQLite for development",
    agent_type="claude-code",
    category="decision",
    memory_lane_type="decision",
    metadata={
        "priority": "high",
        "reasoning": "PostgreSQL scales better, SQLite simpler for local dev",
        "alternatives_considered": ["MySQL", "MongoDB"]
    }
)

# Commitment (high priority)
await manager.store_memory(
    content="User committed: always use type hints in Python code",
    agent_type="claude-code",
    category="commitment",
    memory_lane_type="commitment"
)

# Insight (medium priority)
await manager.store_memory(
    content="Insight: The performance issue is caused by N+1 queries, not database size",
    agent_type="claude-code",
    category="insight",
    memory_lane_type="insight"
)

# Learning (medium priority)
await manager.store_memory(
    content="Learned: FastAPI's dependency injection is more powerful than Flask's",
    agent_type="claude-code",
    category="learning",
    memory_lane_type="learning"
)

# Gap (lower priority)
await manager.store_memory(
    content="Gap identified: No good Python library for real-time chart updates",
    agent_type="claude-code",
    category="gap",
    memory_lane_type="gap"
)
```

---

## Agent-Specific Categories

Each agent can have its own specialized categories.

### Supported Agents

- `claude-code` - Claude Code specific
- `gemini` - Gemini specific
- `qwen` - Qwen specific
- `amp` - AMP pipeline specific
- `droid` - Droid automation specific
- `opencode` - OpenCode API specific
- `codex` - Codex review specific

### Examples

```python
# Agent-specific memory
await manager.store_memory(
    content="Claude Code skill lifecycle: on_session_end triggers last",
    agent_type="claude-code",
    category="claude-code",
    labels=["skills", "lifecycle"]
)
```

---

## When to Use Each Type

### Decision Tree

```
┌─────────────────────────────────────────────────────────────┐
│                    What are you storing?                    │
└────────────────────────┬────────────────────────────────────┘
                         │
         ┌───────────────┼───────────────┐
         │               │               │
         ▼               ▼               ▼
    ┌─────────┐    ┌─────────┐    ┌─────────┐
    │ General │    │ User    │    │ Event/  │
    │ Purpose │    │ Choice  │    │Session  │
    └────┬────┘    └────┬────┘    └────┬────┘
         │              │              │
         ▼              ▼              ▼
    Use Nexus      Was it a     Is it a specific
    Categories     correction?   event/session?
                         │              │
         ┌───────────────┼───────────────┐
         │               │               │
         ▼               ▼               ▼
    Use Memory    Use Memory     Use Memory
    Lane Priority   Lane Priority  Lane Cognitive
    Types           Correction      Types
```

### Quick Reference

| Situation | Recommended Type |
|-----------|------------------|
| Default/general memory | `general` |
| User preference | `preferences` or `commitment` |
| User correction | `correction` |
| Decision made | `decision` |
| Domain fact | `semantic` |
| Specific event | `episodic` |
| How-to process | `procedural` |
| Temporary context | `working` |
| Important event | `flashbulb` |
| New insight | `insight` |
| Something learned | `learning` |
| Shareable info | `cross_agent` |
| Limitation found | `gap` |

---

## Best Practices

### 1. Be Specific with Categories

```python
# Good
await manager.store_memory(
    content="User prefers dark mode",
    category="preferences",
    labels=["ui", "theme"]
)

# Less specific
await manager.store_memory(
    content="User prefers dark mode",
    category="general"
)
```

### 2. Use Labels for Additional Context

```python
# Good - with labels
await manager.store_memory(
    content="Payment API endpoint needs authentication",
    category="context",
    labels=["api", "security", "payment"]
)

# Less useful - without labels
await manager.store_memory(
    content="Payment API endpoint needs authentication",
    category="context"
)
```

### 3. Leverage Metadata for Cognitive Attributes

```python
# Good - metadata for cognitive science attributes
await manager.store_memory(
    content="User corrected the approach to database migrations",
    category="correction",
    memory_lane_type="correction",
    metadata={
        "priority": "high",
        "emotional_weight": 0.8,
        "rehearsal_count": 3,
        "source_confidence": "high",
        "user_corrected": True
    }
)
```

### 4. Combine Types When Appropriate

```python
# Nexus category + Memory Lane type
await manager.store_memory(
    content="During code review, we decided to use async for all I/O operations",
    category="decision",           # Nexus category
    memory_lane_type="episodic",   # Memory Lane cognitive type
    metadata={
        "event_type": "code_review",
        "priority": "high"
    }
)
```

### 5. Use Priority Types for Important Events

```python
# High priority events
await manager.store_memory(
    content="Critical: Production database credentials changed",
    category="flashbulb",
    memory_lane_type="correction",
    metadata={"priority": "critical", "urgency": "immediate"}
)
```

---

## Related Documentation

- [ARCHITECTURE.md](../../ARCHITECTURE.md) - Hybrid type system architecture
- [Embeddings Guide](embeddings.md) - Semantic search with embeddings
- [API Reference](../api/rest-api.md) - Memory storage API

---

**Last Updated:** 2025-12-23
