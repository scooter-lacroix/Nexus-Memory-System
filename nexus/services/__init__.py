"""
Services package for Nexus Memory System

Provides high-level service orchestrators:
- HooksManager: Agent hooks coordination and automated memory extraction
- Orchestrator: Main coordination layer for session tracking, events, and cross-agent sync
"""

from .hooks_manager import HooksManager, HookInstallationResult

__all__ = [
    "HooksManager",
    "HookInstallationResult",
]

# Note: Orchestrator is imported from its own package to avoid circular imports
# The orchestrator is available at nexus.orchestrator.Orchestrator
