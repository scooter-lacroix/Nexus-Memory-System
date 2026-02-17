"""
Database layer for Nexus Memory System
"""

from .models import (
    Base,
    Memory,
    TaskSpecification,
    AgentNamespace,
    MemoryRelation,
)
from .managers import (
    DatabaseManager,
    MemoryManager,
    SpecificationManager,
    get_database_manager,
)
from .migrations import (
    setup_database,
    create_tables,
    get_database_info,
    run_migrations,
)

__all__ = [
    # Models
    "Base",
    "Memory",
    "TaskSpecification",
    "AgentNamespace",
    "MemoryRelation",

    # Managers
    "DatabaseManager",
    "MemoryManager",
    "SpecificationManager",
    "get_database_manager",

    # Database operations
    "setup_database",
    "create_tables",
    "get_database_info",
    "run_migrations",
]