"""
Database managers for Nexus Memory System
"""

import asyncio
from datetime import datetime, UTC
from typing import Dict, List, Any, Optional, Tuple
from sqlalchemy.ext.asyncio import AsyncSession, create_async_engine, async_sessionmaker
from sqlalchemy.orm import sessionmaker, Session
from sqlalchemy import select, update, delete, func, and_, or_
from sqlalchemy.sql import text
from loguru import logger
import numpy as np

from ..config import config
from .models import (
    Base, Memory, TaskSpecification, AgentNamespace,
    MemoryRelation, SystemMetrics
)
from .enums import (
    is_valid_category,
    is_valid_memory_lane_type,
    validate_category,
    validate_memory_lane_type,
    InvalidCategoryError,
    InvalidMemoryLaneTypeError,
    get_memory_lane_priority,
)

# Embedding imports
from ..embeddings.service import get_embedding_service

# Optional sqlite-vec import
try:
    from ..embeddings.sqlite_vec import SQLiteVecStore, VectorSearchResult
    _sqlite_vec_available = True
except ImportError:
    _sqlite_vec_available = False
    SQLiteVecStore = None
    VectorSearchResult = None


class DatabaseManager:
    """Main database manager for Nexus Memory System"""

    def __init__(self, database_url: Optional[str] = None):
        self.database_url = database_url or config.database_connection_url
        self.engine = None
        self.async_engine = None
        self.session_factory = None
        self.async_session_factory = None
        self._initialized = False

    async def initialize(self):
        """Initialize database connections"""
        if self._initialized:
            return

        try:
            # Create async engine for async operations
            if self.database_url.startswith("sqlite"):
                async_url = self.database_url.replace("sqlite:///", "sqlite+aiosqlite:///")
            else:
                async_url = self.database_url

            self.async_engine = create_async_engine(
                async_url,
                echo=config.debug,
                pool_pre_ping=True,
                pool_recycle=3600,
            )

            # Create sync engine for migrations
            self.engine = create_async_engine(
                async_url,
                echo=config.debug,
            )

            # Create session factories
            self.async_session_factory = async_sessionmaker(
                self.async_engine,
                class_=AsyncSession,
                expire_on_commit=False,
            )

            self._initialized = True
            logger.info(f"Database initialized: {self.database_url}")

        except Exception as e:
            logger.error(f"Failed to initialize database: {e}")
            raise

    async def close(self):
        """Close database connections"""
        if self.async_engine:
            await self.async_engine.dispose()
        if self.engine:
            await self.engine.dispose()
        self._initialized = False

    def get_async_session(self) -> AsyncSession:
        """Get async database session"""
        if not self._initialized:
            raise RuntimeError("Database not initialized")
        return self.async_session_factory()

    async def execute_raw_sql(self, sql: str, params: Dict[str, Any] = None) -> Any:
        """Execute raw SQL query"""
        async with self.get_async_session() as session:
            result = await session.execute(text(sql), params or {})
            await session.commit()
            return result


class MemoryManager:
    """Manager for memory operations with vector embedding support"""

    def __init__(self, db_manager: DatabaseManager):
        self.db = db_manager
        self._embedding_service = None
        self._vec_store = None

    @property
    def embedding_service(self):
        """Lazy-load embedding service."""
        if self._embedding_service is None:
            self._embedding_service = get_embedding_service()
        return self._embedding_service

    @property
    def vec_store(self) -> Optional[SQLiteVecStore]:
        """Lazy-load vector store if sqlite-vec is available."""
        if not _sqlite_vec_available:
            return None
        if self._vec_store is None:
            self._vec_store = SQLiteVecStore(config.database_path)
        return self._vec_store

    async def initialize_embeddings(self) -> bool:
        """
        Initialize vector store for embeddings.

        Returns:
            True if embeddings are available, False otherwise
        """
        if not _sqlite_vec_available:
            logger.warning("sqlite-vec not available, embeddings disabled")
            return False
        await self.vec_store.initialize()
        logger.info("Embedding vector store initialized")
        return True

    async def store_memory(
        self,
        content: str,
        agent_type: str,
        category: str = "general",
        labels: List[str] = None,
        metadata: Dict[str, Any] = None,
        memory_lane_type: Optional[str] = None
    ) -> Dict[str, Any]:
        """
        Store a new memory with hybrid type system support.

        Args:
            content: Memory content
            agent_type: Agent type (e.g., "claude-code", "gemini")
            category: Nexus category (preserved, required)
            labels: Optional list of labels
            metadata: Optional metadata dictionary
            memory_lane_type: Optional Memory Lane cognitive/priority type

        Returns:
            Dictionary with success status and memory details

        Raises:
            InvalidCategoryError: If category is not valid
            InvalidMemoryLaneTypeError: If memory_lane_type is not valid
        """
        try:
            # Validate category (required)
            validate_category(category)

            # Validate memory_lane_type (optional)
            if memory_lane_type is not None:
                validate_memory_lane_type(memory_lane_type)

            async with self.db.get_async_session() as session:
                # Get or create namespace
                namespace = await self._get_or_create_namespace(session, agent_type)

                # Create memory record with hybrid type support
                memory = Memory(
                    namespace_id=namespace.id,
                    content=content,
                    category=category,
                    memory_lane_type=memory_lane_type,
                    labels=labels or [],
                    extra_metadata=metadata or {},
                    created_at=datetime.now(UTC),
                )

                # Validate types before storing
                memory.validate_types()

                session.add(memory)
                await session.commit()
                await session.refresh(memory)

                # Generate and store embedding (optional)
                if self.vec_store is not None:
                    try:
                        await self.initialize_embeddings()
                        embedding = await self.embedding_service.encode(content)
                        await self.vec_store.insert_embedding(memory.id, embedding)

                        # Update embedding model reference in memory
                        memory.embedding_model = self.embedding_service.model_name
                        memory.content_embedding = embedding.tolist()
                        await session.commit()

                        logger.debug(f"Generated embedding for memory {memory.id}")
                    except Exception as e:
                        logger.warning(f"Failed to generate embedding for memory {memory.id}: {e}")
                        # Continue without embedding - non-critical
                else:
                    logger.debug("Embedding generation skipped (sqlite-vec not available)")

                logger.info(
                    f"Stored memory {memory.id} for agent {agent_type} "
                    f"(category={category}, memory_lane_type={memory_lane_type})"
                )

                return {
                    "success": True,
                    "memory_id": memory.id,
                    "agent_type": agent_type,
                    "category": category,
                    "memory_lane_type": memory_lane_type,
                    "created_at": memory.created_at.isoformat(),
                }

        except (InvalidCategoryError, InvalidMemoryLaneTypeError) as e:
            logger.error(f"Validation error storing memory: {e}")
            return {
                "success": False,
                "error": str(e),
                "error_type": "validation_error",
            }
        except Exception as e:
            logger.error(f"Failed to store memory: {e}")
            return {
                "success": False,
                "error": str(e),
            }

    async def search_memories(
        self,
        query: str,
        agent_type: str = "general",
        limit: int = 5,
        category: Optional[str] = None,
        memory_lane_type: Optional[str] = None
    ) -> Dict[str, Any]:
        """
        Search memories with intelligent retrieval and hybrid type filtering.

        Args:
            query: Search query string
            agent_type: Agent type to search within
            limit: Maximum number of results
            category: Optional Nexus category filter
            memory_lane_type: Optional Memory Lane type filter

        Returns:
            Dictionary with search results
        """
        try:
            async with self.db.get_async_session() as session:
                # Get namespace
                namespace = await self._get_namespace(session, agent_type)
                if not namespace:
                    return {
                        "success": False,
                        "error": f"Agent namespace '{agent_type}' not found",
                    }

                # Build query
                stmt = select(Memory).where(
                    and_(
                        Memory.namespace_id == namespace.id,
                        Memory.is_active == True,
                        Memory.is_archived == False,
                    )
                )

                # Add category filter if specified
                if category:
                    stmt = stmt.where(Memory.category == category)

                # Add memory_lane_type filter if specified
                if memory_lane_type:
                    stmt = stmt.where(Memory.memory_lane_type == memory_lane_type)

                # Add text search
                if query:
                    search_terms = f"%{query}%"
                    stmt = stmt.where(
                        or_(
                            Memory.content.ilike(search_terms),
                            # Note: For SQLite, JSON labels are stored as JSON text
                            # Simplified search for labels
                        )
                    )

                # Add ordering and limit
                # If memory_lane_type is specified, prioritize by Memory Lane priority
                if memory_lane_type:
                    priority = get_memory_lane_priority(memory_lane_type)
                    stmt = stmt.order_by(
                        Memory.relevance_score.desc().nullslast(),
                        Memory.created_at.desc()
                    )
                else:
                    stmt = stmt.order_by(
                        Memory.relevance_score.desc().nullslast(),
                        Memory.created_at.desc()
                    )

                stmt = stmt.limit(limit)

                result = await session.execute(stmt)
                memories = result.scalars().all()

                # Update access timestamps
                for memory in memories:
                    memory.last_accessed = datetime.now(UTC)
                    memory.access_count += 1

                await session.commit()

                # Format results with hybrid type info
                results = []
                for memory in memories:
                    from .enums import get_category_description
                    results.append({
                        "id": memory.id,
                        "content": memory.content,
                        "category": memory.category,
                        "category_description": get_category_description(memory.category),
                        "memory_lane_type": memory.memory_lane_type,
                        "labels": memory.labels or [],
                        "metadata": memory.extra_metadata or {},
                        "similarity_score": memory.similarity_score,
                        "relevance_score": memory.relevance_score,
                        "created_at": memory.created_at.isoformat(),
                        "access_count": memory.access_count,
                    })

                return {
                    "success": True,
                    "results": results,
                    "total": len(results),
                    "query": query,
                    "agent_type": agent_type,
                    "filters": {
                        "category": category,
                        "memory_lane_type": memory_lane_type,
                    },
                }

        except Exception as e:
            logger.error(f"Failed to search memories: {e}")
            return {
                "success": False,
                "error": str(e),
                "query": query,
                "agent_type": agent_type,
            }

    async def search_memories_by_embedding(
        self,
        query: str,
        agent_type: str = "general",
        k: int = 10,
        threshold: Optional[float] = None,
        category: Optional[str] = None,
        memory_lane_type: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Search memories using vector semantic similarity.

        Uses sqlite-vec for fast KNN search with cosine similarity.

        Args:
            query: Search query text
            agent_type: Agent type to search within
            k: Maximum number of results to return
            threshold: Minimum similarity threshold (0-1)
            category: Optional category filter
            memory_lane_type: Optional memory lane type filter

        Returns:
            Dictionary with search results and similarity scores

        Example:
            >>> result = await manager.search_memories_by_embedding(
            ...     "machine learning algorithms",
            ...     agent_type="claude-code",
            ...     k=5,
            ...     threshold=0.7
            ... )
            >>> for r in result["results"]:
            ...     print(f"{r['similarity']:.2f}: {r['content'][:50]}")
        """
        try:
            # Check if sqlite-vec is available
            if self.vec_store is None:
                return {
                    "success": False,
                    "error": "sqlite-vec not available. Install with: pip install sqlite-vec>=0.1.1",
                    "query": query,
                    "agent_type": agent_type,
                }

            # Ensure vector store is initialized
            await self.initialize_embeddings()

            # Generate query embedding
            query_embedding = await self.embedding_service.encode(query)

            # Perform vector search
            vector_results = await self.vec_store.search(
                query_embedding,
                k=k,
                threshold=threshold,
            )

            if not vector_results:
                return {
                    "success": True,
                    "results": [],
                    "total": 0,
                    "query": query,
                    "agent_type": agent_type,
                    "message": "No similar memories found",
                }

            # Get memory IDs from results
            memory_ids = [r.memory_id for r in vector_results]

            # Fetch full memory details
            async with self.db.get_async_session() as session:
                # Get namespace
                namespace = await self._get_namespace(session, agent_type)
                if not namespace:
                    return {
                        "success": False,
                        "error": f"Agent namespace '{agent_type}' not found",
                    }

                # Build query for memory details
                stmt = select(Memory).where(
                    and_(
                        Memory.id.in_(memory_ids),
                        Memory.namespace_id == namespace.id,
                        Memory.is_active == True,
                        Memory.is_archived == False,
                    )
                )

                # Apply optional filters
                if category:
                    stmt = stmt.where(Memory.category == category)
                if memory_lane_type:
                    stmt = stmt.where(Memory.memory_lane_type == memory_lane_type)

                result = await session.execute(stmt)
                memories = result.scalars().all()

                # Update access timestamps
                for memory in memories:
                    memory.last_accessed = datetime.now(UTC)
                    memory.access_count += 1

                await session.commit()

                # Build results with similarity scores
                memory_similarity_map = {r.memory_id: r for r in vector_results}

                from .enums import get_category_description
                results = []
                for memory in memories:
                    vec_result = memory_similarity_map.get(memory.id)
                    if vec_result:
                        results.append({
                            "id": memory.id,
                            "content": memory.content,
                            "category": memory.category,
                            "category_description": get_category_description(memory.category),
                            "memory_lane_type": memory.memory_lane_type,
                            "labels": memory.labels or [],
                            "metadata": memory.extra_metadata or {},
                            "similarity": vec_result.similarity,
                            "distance": vec_result.distance,
                            "created_at": memory.created_at.isoformat(),
                            "access_count": memory.access_count,
                        })

                # Sort by similarity descending
                results.sort(key=lambda x: x["similarity"], reverse=True)

                return {
                    "success": True,
                    "results": results,
                    "total": len(results),
                    "query": query,
                    "agent_type": agent_type,
                    "filters": {
                        "category": category,
                        "memory_lane_type": memory_lane_type,
                        "threshold": threshold,
                    },
                }

        except Exception as e:
            logger.error(f"Failed to search memories by embedding: {e}")
            return {
                "success": False,
                "error": str(e),
                "query": query,
                "agent_type": agent_type,
            }

    async def get_memory_stats(self, agent_type: Optional[str] = None) -> Dict[str, Any]:
        """Get memory statistics"""
        try:
            async with self.db.get_async_session() as session:
                if agent_type:
                    # Get stats for specific agent
                    namespace = await self._get_namespace(session, agent_type)
                    if not namespace:
                        return {"success": False, "error": f"Agent '{agent_type}' not found"}

                    memories_query = select(func.count(Memory.id)).where(
                        Memory.namespace_id == namespace.id
                    )
                    total_memories = (await session.execute(memories_query)).scalar()

                    category_query = select(
                        Memory.category,
                        func.count(Memory.id).label('count')
                    ).where(
                        Memory.namespace_id == namespace.id
                    ).group_by(Memory.category)

                    category_result = await session.execute(category_query)
                    categories = dict(category_result.all())

                else:
                    # Get stats for all agents
                    memories_query = select(func.count(Memory.id))
                    total_memories = (await session.execute(memories_query)).scalar()

                    category_query = select(
                        Memory.category,
                        func.count(Memory.id).label('count')
                    ).group_by(Memory.category)

                    category_result = await session.execute(category_query)
                    categories = dict(category_result.all())

                return {
                    "success": True,
                    "total_memories": total_memories,
                    "categories": categories,
                    "agent_type": agent_type,
                }

        except Exception as e:
            logger.error(f"Failed to get memory stats: {e}")
            return {
                "success": False,
                "error": str(e),
                "agent_type": agent_type,
            }

    async def _get_or_create_namespace(self, session: AsyncSession, agent_type: str) -> AgentNamespace:
        """Get or create agent namespace"""
        namespace_query = select(AgentNamespace).where(
            AgentNamespace.agent_type == agent_type
        )
        result = await session.execute(namespace_query)
        namespace = result.scalar_one_or_none()

        if not namespace:
            from ..config.agent_namespaces import get_agent_namespace, get_agent_description
            namespace = AgentNamespace(
                name=get_agent_namespace(agent_type),
                agent_type=agent_type,
                description=get_agent_description(agent_type),
            )
            session.add(namespace)
            await session.flush()

        return namespace

    async def _get_namespace(self, session: AsyncSession, agent_type: str) -> Optional[AgentNamespace]:
        """Get agent namespace"""
        namespace_query = select(AgentNamespace).where(
            AgentNamespace.agent_type == agent_type
        )
        result = await session.execute(namespace_query)
        return result.scalar_one_or_none()


class SpecificationManager:
    """Manager for task specifications"""

    def __init__(self, db_manager: DatabaseManager):
        self.db = db_manager

    async def store_specification(
        self,
        task_description: str,
        spec_content: Dict[str, Any],
        agent_type: str = "droid",
        complexity_score: float = 0.5,
        spec_id: Optional[str] = None
    ) -> Dict[str, Any]:
        """Store a task specification"""
        try:
            async with self.db.get_async_session() as session:
                # Get or create namespace
                namespace = await self._get_or_create_namespace(session, agent_type)

                # Generate spec_id if not provided
                if not spec_id:
                    spec_id = f"spec_{datetime.now(UTC).strftime('%Y%m%d_%H%M%S')}_{namespace.id}"

                # Create specification
                specification = TaskSpecification(
                    namespace_id=namespace.id,
                    spec_id=spec_id,
                    task_description=task_description,
                    spec_content=spec_content,
                    complexity_score=complexity_score,
                    created_at=datetime.now(UTC),
                )

                session.add(specification)
                await session.commit()
                await session.refresh(specification)

                logger.info(f"Stored specification {spec_id} for agent {agent_type}")

                return {
                    "success": True,
                    "spec_id": spec_id,
                    "agent_type": agent_type,
                    "complexity_score": complexity_score,
                    "created_at": specification.created_at.isoformat(),
                }

        except Exception as e:
            logger.error(f"Failed to store specification: {e}")
            return {
                "success": False,
                "error": str(e),
            }

    async def find_reusable_specification(
        self,
        task_description: str,
        agent_type: str = "droid",
        threshold: float = 0.8
    ) -> Tuple[Optional[Dict[str, Any]], List[Dict[str, Any]]]:
        """Find reusable specification based on task description"""
        try:
            async with self.db.get_async_session() as session:
                # Get namespace
                namespace = await self._get_namespace(session, agent_type)
                if not namespace:
                    return None, []

                # Search for specifications (simplified similarity search)
                # In a real implementation, this would use vector embeddings
                stmt = select(TaskSpecification).where(
                    and_(
                        TaskSpecification.namespace_id == namespace.id,
                        TaskSpecification.is_active == True,
                        TaskSpecification.is_public == True,
                    )
                ).order_by(
                    TaskSpecification.usage_count.desc(),
                    TaskSpecification.success_rate.desc()
                ).limit(10)

                result = await session.execute(stmt)
                specifications = result.scalars().all()

                # Simple text similarity matching (placeholder for proper embedding search)
                best_match = None
                alternatives = []

                for spec in specifications:
                    # Simple keyword matching (replace with proper semantic search)
                    description_words = set(spec.task_description.lower().split())
                    query_words = set(task_description.lower().split())
                    overlap = len(description_words.intersection(query_words))
                    similarity = overlap / max(len(description_words), len(query_words), 1)

                    spec_data = {
                        "spec_id": spec.spec_id,
                        "task_description": spec.task_description,
                        "spec_content": spec.spec_content,
                        "complexity_score": spec.complexity_score,
                        "usage_count": spec.usage_count,
                        "success_rate": spec.success_rate,
                        "similarity": similarity,
                    }

                    if similarity >= threshold and best_match is None:
                        best_match = spec_data
                    elif similarity > 0.1:  # Some minimum similarity
                        alternatives.append(spec_data)

                return best_match, alternatives[:5]  # Limit alternatives

        except Exception as e:
            logger.error(f"Failed to find reusable specification: {e}")
            return None, []

    async def update_spec_usage(self, spec_id: str) -> bool:
        """Update specification usage count"""
        try:
            async with self.db.get_async_session() as session:
                stmt = update(TaskSpecification).where(
                    TaskSpecification.spec_id == spec_id
                ).values(
                    usage_count=TaskSpecification.usage_count + 1,
                    last_used=datetime.now(UTC)
                )

                result = await session.execute(stmt)
                await session.commit()

                return result.rowcount > 0

        except Exception as e:
            logger.error(f"Failed to update spec usage: {e}")
            return False

    async def _get_or_create_namespace(self, session: AsyncSession, agent_type: str) -> AgentNamespace:
        """Get or create agent namespace (reuse from MemoryManager logic)"""
        # This could be moved to a shared utility
        from ..config.agent_namespaces import get_agent_namespace, get_agent_description

        namespace_query = select(AgentNamespace).where(
            AgentNamespace.agent_type == agent_type
        )
        result = await session.execute(namespace_query)
        namespace = result.scalar_one_or_none()

        if not namespace:
            namespace = AgentNamespace(
                name=get_agent_namespace(agent_type),
                agent_type=agent_type,
                description=get_agent_description(agent_type),
            )
            session.add(namespace)
            await session.flush()

        return namespace

    async def _get_namespace(self, session: AsyncSession, agent_type: str) -> Optional[AgentNamespace]:
        """Get agent namespace"""
        namespace_query = select(AgentNamespace).where(
            AgentNamespace.agent_type == agent_type
        )
        result = await session.execute(namespace_query)
        return result.scalar_one_or_none()


# Global database manager instance
_db_manager: Optional[DatabaseManager] = None


async def get_database_manager() -> DatabaseManager:
    """Get global database manager instance"""
    global _db_manager
    if _db_manager is None:
        _db_manager = DatabaseManager()
        await _db_manager.initialize()
    return _db_manager


def get_memory_manager() -> MemoryManager:
    """Get memory manager instance"""
    # Note: This should be async in a real implementation
    return MemoryManager(_db_manager or DatabaseManager())


def get_specification_manager() -> SpecificationManager:
    """Get specification manager instance"""
    # Note: This should be async in a real implementation
    return SpecificationManager(_db_manager or DatabaseManager())