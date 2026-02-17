# Product Guidelines: Nexus Memory System

This document defines the documentation and communication standards for the Nexus Memory System project. All contributors MUST follow these guidelines.

---

## Core Principles

1. **Comprehensive but Concise** - Provide complete information without verbosity
2. **Example-Driven** - Show, don't just tell
3. **Structured Clarity** - Use headers, lists, and diagrams for scannability
4. **Technical Accuracy** - Verify all code, commands, and claims
5. **Security-First** - Document security implications and best practices

---

## Writing Style: Technical & Direct

### Voice Guidelines

- **No fluff** - Get straight to the point
- **Active voice** - "Store the memory" not "The memory should be stored"
- **Concrete examples** - Every concept should have an example
- **Precise terminology** - Use established technical terms correctly
- **Avoid marketing language** - No "revolutionary," "cutting-edge," etc.

### Anti-Patterns to Avoid

| Don't | Do |
|-------|-----|
| "In order to" | "To" |
| "Basically," "Essentially" | Delete entirely |
| "very," "really," "quite" | Delete entirely |
| "We believe," "We think" | State facts directly |
| Passive voice | Active voice |

---

## Formatting Conventions

### Document Structure

Every documentation file should follow this hierarchy:

```markdown
# Title

> Brief one-line summary if needed

---

## Section
### Subsection
#### Detail

---

## Code Examples

```python
# Always show working code
result = manager.store_memory("content")
```

---

## References
```

### ASCII Diagrams

Use ASCII diagrams for architecture and flow visualization:

```
┌─────────────────────────────────────────────────────────────┐
│                    NEXUS MEMORY SYSTEM                      │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   Storage    │  │  Processing  │  │  Agent Hooks │      │
│  │   Manager    │  │    Engine    │  │   Manager    │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### Tables for Comparison

Use tables for side-by-side comparisons:

| Agent | Hook Type | Status |
|-------|-----------|--------|
| Claude Code | Skills (Oct 2025) | Fully Supported |
| Gemini | Function Calling | Fully Supported |

### Code Blocks

- **Syntax highlighting** - Always specify language: \`\`\`python
- **Working examples** - All code must be runnable
- **Inline comments** - Explain non-obvious logic
- **Error handling** - Show error cases where relevant

```python
# GOOD - Complete with context
async def store_memory(self, content: str, category: str = "general") -> Memory:
    """Store a memory with embedding generation.

    Args:
        content: Memory content to store
        category: Nexus category (default: general)

    Returns:
        The created Memory object

    Raises:
        InvalidCategoryError: If category is not valid
    """
    if not self._is_valid_category(category):
        raise InvalidCategoryError(category)

    # Generate embedding for semantic search
    embedding = await self._embedding_service.embed(content)
    return await self._db.create_memory(content, category, embedding)
```

---

## Code Documentation

### Python Docstrings

Use Google-style docstrings:

```python
def search_memories(
    query: str,
    agent_type: str,
    limit: int = 10,
    threshold: float = 0.7
) -> list[Memory]:
    """Search memories by semantic similarity.

    Args:
        query: Search query string
        agent_type: Filter by agent namespace
        limit: Maximum results to return (default: 10)
        threshold: Minimum similarity score 0-1 (default: 0.7)

    Returns:
        List of Memory objects ordered by relevance

    Raises:
        NamespaceNotFoundError: If agent_type doesn't exist
    """
```

### Inline Comments

- **WHY, not WHAT** - Code shows what, comments explain why
- **Non-obvious logic** - Comment algorithms and heuristics
- **TODO markers** - Use TODO(filename): for tracked items

```python
# GOOD - Explains non-obvious behavior
# Use cosine similarity for normalized vectors (more stable than dot product)
similarity = np.dot(a, b) / (np.linalg.norm(a) * np.linalg.norm(b))

# BAD - Restates the obvious
# Add one to count
count += 1
```

---

## Documentation Types

### README.md

- **Purpose** - Project overview and quick start
- **Sections** - About, Features, Quick Start, Architecture, Links
- **Length** - Keep under 300 lines
- **Examples** - One complete usage example

### API Documentation

- **Endpoints** - Method, path, parameters, response
- **Examples** - cURL or Python for each endpoint
- **Errors** - List all possible error codes

### ARCHITECTURE.md

- **Components** - 5-layer architecture
- **Data flow** - ASCII diagrams
- **Decisions** - Why X was chosen over Y

---

## Quality Checklist

Before committing documentation, verify:

- [ ] All code examples are runnable
- [ ] All links resolve correctly
- [ ] Technical terms are used correctly
- [ ] No marketing language or fluff
- [ ] Active voice throughout
- [ ] ASCII diagrams render correctly
- [ ] Tables are properly formatted
- [ ] Error scenarios are documented
- [ ] Security considerations included

---

## Example: Good vs. Bad

### BAD (Verbose, passive, no examples)

```markdown
## Memory Storage

In order to store a memory, the MemoryManager class should be utilized.
The system is designed to provide a very robust mechanism for storing
memories that is essentially quite powerful. Users are able to store
memories by calling the appropriate method.
```

### GOOD (Direct, active, example-driven)

```markdown
## Memory Storage

Store memories through the `MemoryManager` with automatic embedding generation.

### Usage

```python
manager = NexusManager()
await manager.initialize()

result = await manager.store_memory(
    content="User prefers dark mode",
    category="preferences",
    labels=["ui", "theme"]
)
```

### Categories

- `general` - Default category
- `facts` - Factual information
- `preferences` - User preferences
- `context` - Situational context
- `specifications` - Task specifications
- `session` - Session-based memories
```

---

**Version:** 1.0
**Last Updated:** 2025-02-16
