"""
Memory Routes - REST API endpoints for memory CRUD operations

Provides comprehensive memory management endpoints:
- List/search memories with filtering
- Create new memories
- Get specific memory by ID
- Update existing memories
- Delete memories
- Semantic search using embeddings
"""

import asyncio
from datetime import datetime, UTC
from typing import Dict, List, Any, Optional

from fastapi import APIRouter, HTTPException, Query, Path, status
from fastapi.responses import JSONResponse
from pydantic import BaseModel, Field
from loguru import logger

from ...server.nexus_manager import get_memory_manager
from ...database.managers import MemoryManager
from ..app import (
    MemoryCreateRequest,
    MemoryUpdateRequest,
    MemoryResponse,
    MemoryListResponse,
    MemoryCreateResponse,
    SemanticSearchRequest,
    SemanticSearchResponse,
)
from ..websocket.manager import broadcast_event


router = APIRouter()


# =============================================================================
# Memory CRUD Endpoints
# =============================================================================


@router.get(
    "/memories",
    response_model=MemoryListResponse,
    summary="List memories",
    description="Retrieve memories with optional filtering by query, category, and memory lane type"
)
async def list_memories(
    agent_type: str = Query(
        "general",
        description="Agent type to retrieve memories from",
        pattern="^(general|claude-code|gemini|qwen|amp|droid|opencode|codex)$"
    ),
    query: Optional[str] = Query(
        None,
        description="Search query for text filtering"
    ),
    category: Optional[str] = Query(
        None,
        description="Filter by category"
    ),
    memory_lane_type: Optional[str] = Query(
        None,
        description="Filter by Memory Lane type"
    ),
    limit: int = Query(
        20,
        ge=1,
        le=100,
        description="Maximum number of results"
    ),
    offset: int = Query(
        0,
        ge=0,
        description="Offset for pagination"
    )
):
    """
    List memories with optional filtering.

    Supports filtering by:
    - agent_type: The agent namespace
    - query: Text search in memory content
    - category: Memory category filter
    - memory_lane_type: Memory Lane cognitive type filter
    """
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        # Use text-based search if query provided
        if query:
            result = await manager.memory_manager.search_memories(
                query=query,
                agent_type=agent_type,
                limit=limit,
                category=category,
                memory_lane_type=memory_lane_type
            )
        else:
            # Direct listing without text search
            result = await manager.memory_manager.search_memories(
                query="",  # Empty query returns all
                agent_type=agent_type,
                limit=limit,
                category=category,
                memory_lane_type=memory_lane_type
            )

        if not result.get("success"):
            raise HTTPException(
                status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
                detail=result.get("error", "Unknown error")
            )

        return MemoryListResponse(
            success=True,
            total=result.get("total", 0),
            results=[
                MemoryResponse(
                    id=m["id"],
                    content=m["content"],
                    category=m["category"],
                    category_description=m.get("category_description"),
                    memory_lane_type=m.get("memory_lane_type"),
                    labels=m.get("labels", []),
                    metadata=m.get("metadata", {}),
                    similarity_score=m.get("similarity_score"),
                    relevance_score=m.get("relevance_score"),
                    created_at=m["created_at"],
                    last_accessed=m.get("last_accessed"),
                    access_count=m["access_count"]
                )
                for m in result.get("results", [])
            ],
            query=query,
            agent_type=agent_type,
            filters={
                "category": category,
                "memory_lane_type": memory_lane_type,
            }
        )

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error listing memories: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to list memories: {str(e)}"
        )


@router.post(
    "/memories",
    response_model=MemoryCreateResponse,
    summary="Create a memory",
    description="Store a new memory with automatic categorization and embedding generation",
    status_code=status.HTTP_201_CREATED
)
async def create_memory(request: MemoryCreateRequest):
    """
    Create a new memory.

    The memory will be:
    - Validated for content and category
    - Stored in the database
    - Optionally embedded for semantic search (if enabled)
    - Broadcast to WebSocket subscribers
    """
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        result = await manager.store_memory(
            content=request.content,
            agent_type=request.agent_type,
            category=request.category,
            labels=request.labels,
            metadata={
                **request.metadata,
                "memory_lane_type": request.memory_lane_type
            }
        )

        if not result.get("success"):
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail=result.get("error", "Failed to create memory")
            )

        # Broadcast memory created event
        await broadcast_event({
            "type": "memory_created",
            "data": {
                "memory_id": result.get("memory_id"),
                "agent_type": request.agent_type,
                "category": request.category,
                "content": request.content[:200] + "..." if len(request.content) > 200 else request.content,
                "created_at": datetime.now(UTC).isoformat()
            }
        })

        return MemoryCreateResponse(
            success=True,
            memory_id=result.get("memory_id"),
            agent_type=request.agent_type,
            category=request.category
        )

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error creating memory: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to create memory: {str(e)}"
        )


@router.get(
    "/memories/{memory_id}",
    response_model=MemoryResponse,
    summary="Get a specific memory",
    description="Retrieve detailed information about a specific memory by ID"
)
async def get_memory(
    memory_id: int = Path(
        ...,
        ge=1,
        description="Memory ID"
    ),
    agent_type: str = Query(
        "general",
        description="Agent type for the memory"
    )
):
    """Get a specific memory by ID"""
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        # Search specifically for this memory ID
        result = await manager.memory_manager.search_memories(
            query="",
            agent_type=agent_type,
            limit=1
        )

        if not result.get("success"):
            raise HTTPException(
                status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
                detail=result.get("error")
            )

        # Find the specific memory
        memory_data = None
        for m in result.get("results", []):
            if m["id"] == memory_id:
                memory_data = m
                break

        if not memory_data:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail=f"Memory {memory_id} not found"
            )

        return MemoryResponse(
            id=memory_data["id"],
            content=memory_data["content"],
            category=memory_data["category"],
            category_description=memory_data.get("category_description"),
            memory_lane_type=memory_data.get("memory_lane_type"),
            labels=memory_data.get("labels", []),
            metadata=memory_data.get("metadata", {}),
            similarity_score=memory_data.get("similarity_score"),
            relevance_score=memory_data.get("relevance_score"),
            created_at=memory_data["created_at"],
            last_accessed=memory_data.get("last_accessed"),
            access_count=memory_data["access_count"]
        )

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error getting memory: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to get memory: {str(e)}"
        )


@router.put(
    "/memories/{memory_id}",
    response_model=MemoryResponse,
    summary="Update a memory",
    description="Update an existing memory's content, labels, or metadata"
)
async def update_memory(
    memory_id: int = Path(..., ge=1),
    request: MemoryUpdateRequest = None
):
    """Update a memory"""
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        # Get the memory first
        db_manager = manager.db_manager
        async with db_manager.get_async_session() as session:
            from ...database.models import Memory
            from sqlalchemy import select

            stmt = select(Memory).where(Memory.id == memory_id)
            result = await session.execute(stmt)
            memory = result.scalar_one_or_none()

            if not memory:
                raise HTTPException(
                    status_code=status.HTTP_404_NOT_FOUND,
                    detail=f"Memory {memory_id} not found"
                )

            # Update fields
            if request.content is not None:
                memory.content = request.content
            if request.category is not None:
                memory.category = request.category
            if request.labels is not None:
                memory.labels = request.labels
            if request.metadata is not None:
                memory.extra_metadata = request.metadata
            if request.memory_lane_type is not None:
                memory.memory_lane_type = request.memory_lane_type
            if request.is_active is not None:
                memory.is_active = request.is_active
            if request.is_archived is not None:
                memory.is_archived = request.is_archived

            memory.updated_at = datetime.now(UTC)
            await session.commit()
            await session.refresh(memory)

        # Broadcast memory updated event
        await broadcast_event({
            "type": "memory_updated",
            "data": {
                "memory_id": memory_id,
                "updated_at": datetime.now(UTC).isoformat()
            }
        })

        return MemoryResponse(
            id=memory.id,
            content=memory.content,
            category=memory.category,
            category_description=None,
            memory_lane_type=memory.memory_lane_type,
            labels=memory.labels or [],
            metadata=memory.extra_metadata or {},
            similarity_score=memory.similarity_score,
            relevance_score=memory.relevance_score,
            created_at=memory.created_at.isoformat(),
            last_accessed=memory.last_accessed.isoformat() if memory.last_accessed else None,
            access_count=memory.access_count
        )

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error updating memory: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to update memory: {str(e)}"
        )


@router.delete(
    "/memories/{memory_id}",
    status_code=status.HTTP_204_NO_CONTENT,
    summary="Delete a memory",
    description="Soft-delete a memory (marks as inactive)"
)
async def delete_memory(
    memory_id: int = Path(..., ge=1)
):
    """Delete a memory (soft delete)"""
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        db_manager = manager.db_manager
        async with db_manager.get_async_session() as session:
            from ...database.models import Memory
            from sqlalchemy import select, update

            stmt = select(Memory).where(Memory.id == memory_id)
            result = await session.execute(stmt)
            memory = result.scalar_one_or_none()

            if not memory:
                raise HTTPException(
                    status_code=status.HTTP_404_NOT_FOUND,
                    detail=f"Memory {memory_id} not found"
                )

            # Soft delete
            memory.is_active = False
            memory.is_archived = True
            memory.updated_at = datetime.now(UTC)
            await session.commit()

        # Broadcast memory deleted event
        await broadcast_event({
            "type": "memory_deleted",
            "data": {
                "memory_id": memory_id,
                "deleted_at": datetime.now(UTC).isoformat()
            }
        })

        return JSONResponse(
            status_code=status.HTTP_204_NO_CONTENT,
            content=None
        )

    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Error deleting memory: {e}")
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail=f"Failed to delete memory: {str(e)}"
        )


# =============================================================================
# Semantic Search Endpoints
# =============================================================================


@router.post(
    "/search/semantic",
    response_model=SemanticSearchResponse,
    summary="Semantic search",
    description="Search memories using vector embeddings for semantic similarity"
)
async def semantic_search(request: SemanticSearchRequest):
    """
    Perform semantic search using vector embeddings.

    Requires sqlite-vec to be installed and embeddings enabled.
    Returns memories ranked by semantic similarity to the query.
    """
    try:
        manager = get_memory_manager()
        await manager.ensure_initialized()

        result = await manager.memory_manager.search_memories_by_embedding(
            query=request.query,
            agent_type=request.agent_type,
            k=request.k,
            threshold=request.threshold,
            category=request.category,
            memory_lane_type=request.memory_lane_type
        )

        if not result.get("success"):
            return SemanticSearchResponse(
                success=False,
                results=[],
                total=0,
                query=request.query,
                agent_type=request.agent_type,
                filters={
                    "category": request.category,
                    "memory_lane_type": request.memory_lane_type,
                    "threshold": request.threshold,
                },
                error=result.get("error", "Search failed")
            )

        return SemanticSearchResponse(
            success=True,
            results=result.get("results", []),
            total=result.get("total", 0),
            query=request.query,
            agent_type=request.agent_type,
            filters={
                "category": request.category,
                "memory_lane_type": request.memory_lane_type,
                "threshold": request.threshold,
            }
        )

    except Exception as e:
        logger.error(f"Error in semantic search: {e}")
        return SemanticSearchResponse(
            success=False,
            results=[],
            total=0,
            query=request.query,
            agent_type=request.agent_type,
            filters={
                "category": request.category,
                "memory_lane_type": request.memory_lane_type,
                "threshold": request.threshold,
            },
            error=str(e)
        )
