"""
API Routes for Nexus Memory System Web Dashboard

This package contains all FastAPI route handlers.
"""

from . import memories, stats, hooks

__all__ = ["memories", "stats", "hooks"]
