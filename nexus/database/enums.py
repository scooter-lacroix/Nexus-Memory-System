"""
Memory type enums and constants for Nexus Memory System Hybrid Type System

This module implements the hybrid memory type system that combines:
1. Nexus's existing flexible category system (preserved)
2. Memory Lane cognitive types (additive, optional)
3. Memory Lane 10 types for enhanced categorization

Key Design Principles:
- DO NOT remove existing category field - it's working
- Add memory_lane_type as optional field for Memory Lane categorization
- Preserve backward compatibility - existing code must still work
"""

from enum import Enum
from typing import Dict, Set, List, FrozenSet


class MemoryCategory(str, Enum):
    """
    Core Nexus memory categories (existing, working)

    These are the original Nexus categories that are preserved
    for backward compatibility. They represent general purpose
    categorization for agent memories.
    """

    GENERAL = "general"
    FACTS = "facts"
    PREFERENCES = "preferences"
    CONTEXT = "context"
    SPECIFICATIONS = "specifications"
    SESSION = "session"  # Session-based memories and context


class MemoryLaneCognitiveType(str, Enum):
    """
    Memory Lane cognitive science-based memory types (optional, additive)

    These types are based on cognitive science research and provide
    a structured taxonomy for memory categorization. They are OPTIONAL
    and additive to the existing Nexus category system.

    Source: Memory Lane cognitive taxonomy
    """

    # Core cognitive types
    SEMANTIC = "semantic"          # General knowledge and facts
    EPISODIC = "episodic"          # Event-based experiences
    PROCEDURAL = "procedural"      # How-to knowledge and processes
    WORKING = "working"            # Temporary active processing
    EXPLICIT = "explicit"          # Conscious declarative facts
    IMPLICIT = "implicit"          # Unconscious patterns
    FLASHBULB = "flashbulb"        # High-importance events
    METAMEMORY = "metamemory"      # Knowledge about memory
    COLLECTIVE = "collective"      # Cross-agent shared knowledge


class MemoryLanePriorityType(str, Enum):
    """
    Memory Lane 10 priority-based memory types

    These types represent the Memory Lane priority system for
    categorizing memories by their importance and relevance.
    """

    # High Priority
    CORRECTION = "correction"      # User corrected agent behavior
    DECISION = "decision"          # Explicit choice with reasoning
    COMMITMENT = "commitment"      # User preference/commitment

    # Medium Priority
    INSIGHT = "insight"            # Non-obvious discovery or connection
    LEARNING = "learning"          # New knowledge gained
    CONFIDENCE = "confidence"      # Strong confidence in approach

    # Lower Priority
    PATTERN_SEED = "pattern_seed"  # Repeated behavior worth formalizing
    CROSS_AGENT = "cross_agent"    # Info relevant to other agents
    WORKFLOW_NOTE = "workflow_note"  # Process observation
    GAP = "gap"                    # Missing capability or limitation


class AgentCategory(str, Enum):
    """
    Agent-specific memory categories (existing pattern)

    These categories are used for agent-specific memories that
    don't fit into the general cognitive taxonomy.
    """

    CLAUDE_CODE = "claude-code"
    GEMINI = "gemini"
    QWEN = "qwen"
    AMP = "amp"
    DROID = "droid"
    OPENCODE = "opencode"
    CODEX = "codex"


# Combined valid category set (all categories that can be used)
VALID_CATEGORIES: FrozenSet[str] = frozenset([
    # Core Nexus categories
    cat.value for cat in MemoryCategory
] + [
    # Memory Lane cognitive types (as categories)
    mlt.value for mlt in MemoryLaneCognitiveType
] + [
    # Memory Lane priority types (as categories)
    mlp.value for mlp in MemoryLanePriorityType
] + [
    # Agent-specific categories
    agent.value for agent in AgentCategory
])


# Valid memory_lane_type values (only Memory Lane types)
VALID_MEMORY_LANE_TYPES: FrozenSet[str] = frozenset([
    mlt.value for mlt in MemoryLaneCognitiveType
] + [
    mlp.value for mlp in MemoryLanePriorityType
])


# Hybrid category descriptions
HYBRID_CATEGORY_DESCRIPTIONS: Dict[str, str] = {
    # Core Nexus categories
    **{cat.value: desc for cat, desc in [
        (MemoryCategory.GENERAL, "General purpose memories"),
        (MemoryCategory.FACTS, "Factual information"),
        (MemoryCategory.PREFERENCES, "User preferences and settings"),
        (MemoryCategory.CONTEXT, "Situational context"),
        (MemoryCategory.SPECIFICATIONS, "Task specifications (via TaskSpecification model)"),
        (MemoryCategory.SESSION, "Session-based memories and context"),
    ]},

    # Memory Lane cognitive types
    **{mlt.value: desc for mlt, desc in [
        (MemoryLaneCognitiveType.SEMANTIC, "General knowledge (Memory Lane type)"),
        (MemoryLaneCognitiveType.EPISODIC, "Event-based experiences (Memory Lane type)"),
        (MemoryLaneCognitiveType.PROCEDURAL, "How-to processes (Memory Lane type)"),
        (MemoryLaneCognitiveType.WORKING, "Temporary active memory (Memory Lane type)"),
        (MemoryLaneCognitiveType.EXPLICIT, "Conscious declarative facts (Memory Lane type)"),
        (MemoryLaneCognitiveType.IMPLICIT, "Unconscious patterns (Memory Lane type)"),
        (MemoryLaneCognitiveType.FLASHBULB, "High-importance events (Memory Lane type)"),
        (MemoryLaneCognitiveType.METAMEMORY, "Knowledge about memory (Memory Lane type)"),
        (MemoryLaneCognitiveType.COLLECTIVE, "Cross-agent shared knowledge (hybrid)"),
    ]},

    # Memory Lane priority types
    **{mlp.value: desc for mlp, desc in [
        (MemoryLanePriorityType.CORRECTION, "User corrected agent behavior"),
        (MemoryLanePriorityType.DECISION, "Explicit choice with reasoning"),
        (MemoryLanePriorityType.COMMITMENT, "User preference/commitment"),
        (MemoryLanePriorityType.INSIGHT, "Non-obvious discovery or connection"),
        (MemoryLanePriorityType.LEARNING, "New knowledge gained"),
        (MemoryLanePriorityType.CONFIDENCE, "Strong confidence in approach"),
        (MemoryLanePriorityType.PATTERN_SEED, "Repeated behavior worth formalizing"),
        (MemoryLanePriorityType.CROSS_AGENT, "Info relevant to other agents"),
        (MemoryLanePriorityType.WORKFLOW_NOTE, "Process observation"),
        (MemoryLanePriorityType.GAP, "Missing capability or limitation"),
    ]},

    # Agent-specific categories
    **{agent.value: f"{agent.value.replace('-', ' ').title()} specific" for agent in AgentCategory},
}


# Memory type priority levels (for Memory Lane types)
MEMORY_LANE_PRIORITY_LEVELS: Dict[str, int] = {
    # High Priority
    MemoryLanePriorityType.CORRECTION.value: 1,
    MemoryLanePriorityType.DECISION.value: 1,
    MemoryLanePriorityType.COMMITMENT.value: 1,

    # Medium Priority
    MemoryLanePriorityType.INSIGHT.value: 2,
    MemoryLanePriorityType.LEARNING.value: 2,
    MemoryLanePriorityType.CONFIDENCE.value: 2,

    # Lower Priority
    MemoryLanePriorityType.PATTERN_SEED.value: 3,
    MemoryLanePriorityType.CROSS_AGENT.value: 3,
    MemoryLanePriorityType.WORKFLOW_NOTE.value: 3,
    MemoryLanePriorityType.GAP.value: 3,
}


# Helper functions

def is_valid_category(category: str) -> bool:
    """
    Check if a category string is valid.

    Args:
        category: Category string to validate

    Returns:
        True if category is in valid categories set
    """
    return category in VALID_CATEGORIES


def is_valid_memory_lane_type(memory_lane_type: str) -> bool:
    """
    Check if a memory_lane_type string is valid.

    Args:
        memory_lane_type: Memory Lane type string to validate

    Returns:
        True if memory_lane_type is in valid Memory Lane types
    """
    return memory_lane_type in VALID_MEMORY_LANE_TYPES


def get_category_description(category: str) -> str:
    """
    Get description for a category.

    Args:
        category: Category string

    Returns:
        Description of the category, or "Unknown category" if not found
    """
    return HYBRID_CATEGORY_DESCRIPTIONS.get(category, "Unknown category")


def get_memory_lane_priority(memory_lane_type: str) -> int:
    """
    Get priority level for a Memory Lane type.

    Args:
        memory_lane_type: Memory Lane type string

    Returns:
        Priority level (1=high, 2=medium, 3=low), or 3 if not found
    """
    return MEMORY_LANE_PRIORITY_LEVELS.get(memory_lane_type, 3)


def get_categories_by_type(category_type: str) -> List[str]:
    """
    Get all categories of a specific type.

    Args:
        category_type: Type of categories to return
            - "nexus": Core Nexus categories
            - "cognitive": Memory Lane cognitive types
            - "priority": Memory Lane priority types
            - "agent": Agent-specific categories
            - "all": All categories

    Returns:
        List of category strings
    """
    if category_type == "nexus":
        return [cat.value for cat in MemoryCategory]
    elif category_type == "cognitive":
        return [mlt.value for mlt in MemoryLaneCognitiveType]
    elif category_type == "priority":
        return [mlp.value for mlp in MemoryLanePriorityType]
    elif category_type == "agent":
        return [agent.value for agent in AgentCategory]
    elif category_type == "all":
        return list(VALID_CATEGORIES)
    else:
        return []


def get_memory_lane_types() -> List[str]:
    """
    Get all valid Memory Lane types.

    Returns:
        List of all valid memory_lane_type values
    """
    return list(VALID_MEMORY_LANE_TYPES)


# Validation exceptions

class InvalidCategoryError(ValueError):
    """Raised when an invalid category is provided"""

    def __init__(self, category: str):
        self.category = category
        super().__init__(f"Invalid category: '{category}'. Must be one of {list(VALID_CATEGORIES)[:5]}...")


class InvalidMemoryLaneTypeError(ValueError):
    """Raised when an invalid memory_lane_type is provided"""

    def __init__(self, memory_lane_type: str):
        self.memory_lane_type = memory_lane_type
        super().__init__(f"Invalid memory_lane_type: '{memory_lane_type}'. Must be one of {list(VALID_MEMORY_LANE_TYPES)[:5]}...")


def validate_category(category: str) -> None:
    """
    Validate a category string. Raises InvalidCategoryError if invalid.

    Args:
        category: Category string to validate

    Raises:
        InvalidCategoryError: If category is not valid
    """
    if not is_valid_category(category):
        raise InvalidCategoryError(category)


def validate_memory_lane_type(memory_lane_type: str) -> None:
    """
    Validate a memory_lane_type string. Raises InvalidMemoryLaneTypeError if invalid.

    Args:
        memory_lane_type: Memory Lane type string to validate

    Raises:
        InvalidMemoryLaneTypeError: If memory_lane_type is not valid
    """
    if memory_lane_type is not None and not is_valid_memory_lane_type(memory_lane_type):
        raise InvalidMemoryLaneTypeError(memory_lane_type)
