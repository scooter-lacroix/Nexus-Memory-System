"""
SQLite-vec wrapper for high-performance vector similarity search.

This module provides a clean interface to sqlite-vec, enabling:
- Fast vector similarity search using vec0 virtual tables
- Efficient storage and indexing of embeddings
- Cosine similarity search with top-K results
- Async-safe operations with proper connection management

References:
    https://github.com/asg0r/sqlite-vec
"""

import sqlite3
import asyncio
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Tuple, Dict, Any, Generator
import numpy as np
from loguru import logger

try:
    import sqlite_vec
except ImportError:
    raise ImportError(
        "sqlite-vec is required. Install with: pip install sqlite-vec>=0.1.1"
    )


@dataclass
class VectorSearchResult:
    """
    Result from a vector similarity search.

    Attributes:
        memory_id: ID of the matching memory
        distance: Distance score (lower is more similar for cosine distance)
            For cosine similarity: similarity = 1 - distance
        similarity: Computed cosine similarity (1.0 = identical, 0.0 = orthogonal)
    """

    memory_id: int
    distance: float
    similarity: float

    def __post_init__(self):
        """Compute cosine similarity from distance."""
        # sqlite-vec returns cosine distance, convert to similarity
        self.similarity = max(0.0, 1.0 - self.distance)

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary."""
        return {
            "memory_id": self.memory_id,
            "distance": self.distance,
            "similarity": self.similarity,
        }


class SQLiteVecStore:
    """
    High-performance vector store using sqlite-vec.

    This class manages vector embeddings storage and similarity search
    using sqlite-vec's vec0 virtual table for efficient KNN search.

    Example:
        >>> store = SQLiteVecStore("/path/to/database.db")
        >>> await store.initialize()
        >>>
        >>> # Insert embedding
        >>> await store.insert_embedding(memory_id=1, embedding=np.array([0.1, 0.2, ...]))
        >>>
        >>> # Search similar vectors
        >>> results = await store.search(query_embedding, k=10)
    """

    # Table name for vector storage
    TABLE_NAME = "memory_embeddings"
    VECTOR_DIM = 384  # all-MiniLM-L6-v2 dimension

    def __init__(
        self,
        database_path: str,
        table_name: str = TABLE_NAME,
        vector_dim: int = VECTOR_DIM,
    ):
        """
        Initialize the SQLite-vec store.

        Args:
            database_path: Path to SQLite database file
            table_name: Name of the virtual table (default: memory_embeddings)
            vector_dim: Dimension of embedding vectors (default: 384)
        """
        self.database_path = Path(database_path)
        self.table_name = table_name
        self.vector_dim = vector_dim
        self._initialized = False

    def _get_connection(self) -> sqlite3.Connection:
        """
        Get a raw SQLite connection with sqlite-vec enabled.

        Returns:
            SQLite connection with vec0 extension loaded

        Note:
            This is a synchronous connection intended for use within
            thread pool executors. For async operations, use the
            async methods which handle this internally.
        """
        conn = sqlite3.connect(str(self.database_path))
        conn.enable_load_extension(True)
        sqlite_vec.load(conn)
        return conn

    @contextmanager
    def _connection_context(self) -> Generator[sqlite3.Connection, None, None]:
        """
        Context manager for SQLite connections.

        Yields:
            SQLite connection with sqlite-vec enabled
        """
        conn = self._get_connection()
        try:
            yield conn
        finally:
            conn.close()

    async def initialize(self) -> None:
        """
        Initialize the vector store.

        Creates the vec0 virtual table if it doesn't exist.
        Should be called before any other operations.
        """
        if self._initialized:
            return

        def _init():
            with self._connection_context() as conn:
                # Create virtual table for vector storage
                conn.execute(
                    f"""
                    CREATE VIRTUAL TABLE IF NOT EXISTS {self.table_name}
                    USING vec0(
                        embedding_float({self.vector_dim}),
                        memory_id INTEGER PRIMARY KEY
                    )
                    """
                )
                conn.commit()
                logger.info(
                    f"Vector table '{self.table_name}' initialized: {self.database_path}"
                )

        loop = asyncio.get_event_loop()
        await loop.run_in_executor(None, _init)
        self._initialized = True

    async def insert_embedding(
        self,
        memory_id: int,
        embedding: np.ndarray,
    ) -> bool:
        """
        Insert or update an embedding for a memory.

        Args:
            memory_id: ID of the memory
            embedding: Embedding vector of shape (vector_dim,)

        Returns:
            True if successful, False otherwise

        Raises:
            ValueError: If embedding dimension doesn't match
            RuntimeError: If operation fails
        """
        if not self._initialized:
            await self.initialize()

        if embedding.shape != (self.vector_dim,):
            raise ValueError(
                f"Embedding dimension mismatch: expected {self.vector_dim}, "
                f"got {embedding.shape[0]}"
            )

        def _insert():
            with self._connection_context() as conn:
                # Insert or replace (upsert)
                conn.execute(
                    f"""
                    INSERT OR REPLACE INTO {self.table_name}
                    (embedding_float, memory_id)
                    VALUES (?, ?)
                    """,
                    [embedding.tolist(), memory_id],
                )
                conn.commit()
                return True

        try:
            loop = asyncio.get_event_loop()
            return await loop.run_in_executor(None, _insert)
        except Exception as e:
            logger.error(f"Failed to insert embedding for memory {memory_id}: {e}")
            return False

    async def insert_batch(
        self,
        embeddings: List[Tuple[int, np.ndarray]],
    ) -> int:
        """
        Insert multiple embeddings in a single transaction.

        Args:
            embeddings: List of (memory_id, embedding) tuples

        Returns:
            Number of embeddings successfully inserted

        Example:
            >>> embeddings = [
            ...     (1, embedding1),
            ...     (2, embedding2),
            ... ]
            >>> count = await store.insert_batch(embeddings)
        """
        if not self._initialized:
            await self.initialize()

        if not embeddings:
            return 0

        def _insert_batch():
            with self._connection_context() as conn:
                # Use execmany for bulk insert
                data = [
                    (emb.tolist(), mem_id) for mem_id, emb in embeddings
                ]
                conn.executemany(
                    f"""
                    INSERT OR REPLACE INTO {self.table_name}
                    (embedding_float, memory_id)
                    VALUES (?, ?)
                    """,
                    data,
                )
                conn.commit()
                return len(data)

        try:
            loop = asyncio.get_event_loop()
            return await loop.run_in_executor(None, _insert_batch)
        except Exception as e:
            logger.error(f"Failed to insert batch embeddings: {e}")
            return 0

    async def search(
        self,
        query_embedding: np.ndarray,
        k: int = 10,
        threshold: Optional[float] = None,
    ) -> List[VectorSearchResult]:
        """
        Search for similar vectors using cosine similarity.

        Args:
            query_embedding: Query vector of shape (vector_dim,)
            k: Maximum number of results to return
            threshold: Minimum similarity threshold (0-1).
                If set, only results with similarity >= threshold are returned.

        Returns:
            List of VectorSearchResult objects, sorted by similarity (descending)

        Raises:
            ValueError: If query embedding dimension doesn't match
            RuntimeError: If search fails

        Example:
            >>> results = await store.search(query, k=5, threshold=0.7)
            >>> for r in results:
            ...     print(f"Memory {r.memory_id}: similarity={r.similarity:.3f}")
        """
        if not self._initialized:
            await self.initialize()

        if query_embedding.shape != (self.vector_dim,):
            raise ValueError(
                f"Query dimension mismatch: expected {self.vector_dim}, "
                f"got {query_embedding.shape[0]}"
            )

        def _search():
            with self._connection_context() as conn:
                # Perform KNN search using sqlite-vec MATCH
                cursor = conn.execute(
                    f"""
                    SELECT memory_id, distance
                    FROM {self.table_name}
                    WHERE embedding_float MATCH ?
                    AND k = ?
                    ORDER BY distance
                    """,
                    [query_embedding.tolist(), k],
                )

                results = []
                for memory_id, distance in cursor:
                    result = VectorSearchResult(
                        memory_id=memory_id,
                        distance=distance,
                        similarity=0.0,  # Computed in __post_init__
                    )
                    # Apply threshold if specified
                    if threshold is None or result.similarity >= threshold:
                        results.append(result)

                return results

        try:
            loop = asyncio.get_event_loop()
            return await loop.run_in_executor(None, _search)
        except Exception as e:
            logger.error(f"Vector search failed: {e}")
            raise RuntimeError(f"Vector search failed: {e}")

    async def delete_embedding(self, memory_id: int) -> bool:
        """
        Delete an embedding from the vector store.

        Args:
            memory_id: ID of the memory to delete

        Returns:
            True if embedding was deleted, False otherwise
        """
        if not self._initialized:
            await self.initialize()

        def _delete():
            with self._connection_context() as conn:
                cursor = conn.execute(
                    f"""
                    DELETE FROM {self.table_name}
                    WHERE memory_id = ?
                    """,
                    [memory_id],
                )
                conn.commit()
                return cursor.rowcount > 0

        try:
            loop = asyncio.get_event_loop()
            return await loop.run_in_executor(None, _delete)
        except Exception as e:
            logger.error(f"Failed to delete embedding for memory {memory_id}: {e}")
            return False

    async def get_stats(self) -> Dict[str, Any]:
        """
        Get statistics about the vector store.

        Returns:
            Dictionary with vector count and storage info
        """
        if not self._initialized:
            await self.initialize()

        def _get_stats():
            with self._connection_context() as conn:
                # Count vectors
                cursor = conn.execute(
                    f"""
                    SELECT COUNT(*) FROM {self.table_name}
                    """
                )
                count = cursor.fetchone()[0]

                # Get database file size
                db_size = self.database_path.stat().st_size if self.database_path.exists() else 0

                return {
                    "table_name": self.table_name,
                    "vector_count": count,
                    "vector_dim": self.vector_dim,
                    "database_path": str(self.database_path),
                    "database_size_bytes": db_size,
                }

        try:
            loop = asyncio.get_event_loop()
            return await loop.run_in_executor(None, _get_stats)
        except Exception as e:
            logger.error(f"Failed to get vector store stats: {e}")
            return {
                "error": str(e),
                "table_name": self.table_name,
            }

    async def drop_table(self) -> bool:
        """
        Drop the vector table.

        WARNING: This will permanently delete all stored embeddings.

        Returns:
            True if table was dropped successfully
        """
        def _drop():
            with self._connection_context() as conn:
                conn.execute(f"DROP TABLE IF EXISTS {self.table_name}")
                conn.commit()
                return True

        try:
            loop = asyncio.get_event_loop()
            result = await loop.run_in_executor(None, _drop)
            self._initialized = False
            logger.warning(f"Vector table '{self.table_name}' dropped")
            return result
        except Exception as e:
            logger.error(f"Failed to drop table: {e}")
            return False
