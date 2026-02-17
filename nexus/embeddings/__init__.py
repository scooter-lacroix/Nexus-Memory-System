"""
Embeddings module for Nexus Memory System

This module provides vector embedding storage and similarity search
using sqlite-vec for high-performance vector operations.
"""

from .service import EmbeddingService, get_embedding_service

# Optional sqlite-vec import - may not be available in all environments
try:
    from .sqlite_vec import SQLiteVecStore, VectorSearchResult
    _sqlite_vec_available = True
except ImportError:
    _sqlite_vec_available = False
    SQLiteVecStore = None
    VectorSearchResult = None

__all__ = [
    "EmbeddingService",
    "get_embedding_service",
]


def get_sqlite_vec_classes():
    """
    Get sqlite-vec classes if available.

    Returns:
        Tuple of (SQLiteVecStore, VectorSearchResult) or raises ImportError

    Raises:
        ImportError: If sqlite-vec is not installed
    """
    if not _sqlite_vec_available:
        raise ImportError(
            "sqlite-vec is required for vector storage. "
            "Install with: pip install sqlite-vec>=0.1.1"
        )
    return SQLiteVecStore, VectorSearchResult
