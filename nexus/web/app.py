"""
FastAPI Web Application for Nexus Memory System

Production-quality FastAPI application with:
- REST API for memory CRUD operations
- WebSocket support for real-time updates
- CORS handling
- Request validation with Pydantic
- Comprehensive API documentation
"""

import asyncio
import json
from datetime import datetime, UTC
from typing import Dict, List, Any, Optional, Set
from pathlib import Path

from fastapi import FastAPI, WebSocket, WebSocketDisconnect, HTTPException, Query, status
from fastapi.middleware.cors import CORSMiddleware
from fastapi.middleware.gzip import GZipMiddleware
from fastapi.responses import HTMLResponse, JSONResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, Field, validator, constr
from loguru import logger

from ..config import config
from ..server.nexus_manager import get_memory_manager
from .websocket.manager import WebSocketManager, broadcast_event


# =============================================================================
# Pydantic Models for Request/Response Validation
# =============================================================================


class MemoryCreateRequest(BaseModel):
    """Request model for creating a memory"""
    content: constr(min_length=1, max_length=10000) = Field(
        ...,
        description="Memory content",
        json_schema_extra={"example": "The user prefers dark mode in the UI"}
    )
    agent_type: str = Field(
        default="general",
        description="Agent type storing the memory",
        pattern="^(general|claude-code|gemini|qwen|amp|droid|opencode|codex)$"
    )
    category: str = Field(
        default="general",
        description="Memory category (facts, preferences, context, specifications, etc.)"
    )
    labels: List[str] = Field(
        default_factory=list,
        description="Optional labels for categorization"
    )
    metadata: Dict[str, Any] = Field(
        default_factory=dict,
        description="Additional metadata"
    )
    memory_lane_type: Optional[str] = Field(
        default=None,
        description="Memory Lane cognitive type (working, short_term, long_term, archival)"
    )

    @validator("content")
    def validate_content(cls, v):
        """Validate content is not empty after stripping"""
        if not v or not v.strip():
            raise ValueError("Content cannot be empty")
        return v.strip()


class MemoryUpdateRequest(BaseModel):
    """Request model for updating a memory"""
    content: Optional[constr(min_length=1, max_length=10000)] = None
    category: Optional[str] = None
    labels: Optional[List[str]] = None
    metadata: Optional[Dict[str, Any]] = None
    memory_lane_type: Optional[str] = None
    is_active: Optional[bool] = None
    is_archived: Optional[bool] = None


class MemoryResponse(BaseModel):
    """Response model for memory data"""
    id: int
    content: str
    category: str
    category_description: Optional[str] = None
    memory_lane_type: Optional[str] = None
    labels: List[str]
    metadata: Dict[str, Any]
    similarity_score: Optional[float] = None
    relevance_score: Optional[float] = None
    created_at: str
    last_accessed: Optional[str] = None
    access_count: int

    class Config:
        orm_mode = True


class MemoryListResponse(BaseModel):
    """Response model for memory list"""
    success: bool
    total: int
    results: List[MemoryResponse]
    query: Optional[str] = None
    agent_type: str
    filters: Dict[str, Any]


class MemoryCreateResponse(BaseModel):
    """Response model for memory creation"""
    success: bool
    memory_id: Optional[int] = None
    agent_type: str
    category: str
    error: Optional[str] = None


class SemanticSearchRequest(BaseModel):
    """Request model for semantic search"""
    query: constr(min_length=1, max_length=500) = Field(
        ...,
        description="Search query text",
        example="machine learning algorithms"
    )
    agent_type: str = Field(
        default="general",
        description="Agent type to search within"
    )
    k: int = Field(
        default=10,
        ge=1,
        le=100,
        description="Maximum number of results"
    )
    threshold: Optional[float] = Field(
        default=None,
        ge=0.0,
        le=1.0,
        description="Minimum similarity threshold"
    )
    category: Optional[str] = None
    memory_lane_type: Optional[str] = None


class SemanticSearchResponse(BaseModel):
    """Response model for semantic search"""
    success: bool
    results: List[Dict[str, Any]]
    total: int
    query: str
    agent_type: str
    filters: Dict[str, Any]
    error: Optional[str] = None


class StatsResponse(BaseModel):
    """Response model for statistics"""
    success: bool
    total_memories: int
    categories: Dict[str, int]
    system_info: Optional[Dict[str, Any]] = None
    performance_metrics: Optional[Dict[str, Any]] = None
    error: Optional[str] = None


class HooksStatusResponse(BaseModel):
    """Response model for hooks status"""
    agent_type: str
    installed: bool
    monitoring: bool
    hook_type: str
    last_extraction: Optional[str]
    extraction_count: int
    error_count: int
    last_error: Optional[str]


class OrchestratorStatusResponse(BaseModel):
    """Response model for orchestrator status"""
    success: bool
    status: Optional[Dict[str, Any]] = None
    active_sessions: int
    total_events_processed: int
    error: Optional[str] = None


class ErrorResponse(BaseModel):
    """Standard error response"""
    success: bool = False
    error: str
    detail: Optional[str] = None


# =============================================================================
# FastAPI Application Factory
# =============================================================================

def create_app() -> FastAPI:
    """
    Create and configure the FastAPI application

    Returns:
        Configured FastAPI application instance
    """
    app = FastAPI(
        title="Nexus Memory System API",
        description="Cross-agent memory management platform with semantic search and real-time updates",
        version="1.0.0",
        docs_url="/api/docs",
        redoc_url="/api/redoc",
        openapi_url="/api/openapi.json"
    )

    # CORS middleware
    app.add_middleware(
        CORSMiddleware,
        **config.get_cors_config()
    )

    # GZip compression
    app.add_middleware(GZipMiddleware, minimum_size=1000)

    # Global exception handlers
    @app.exception_handler(HTTPException)
    async def http_exception_handler(request, exc):
        """Handle HTTP exceptions with consistent error response"""
        return JSONResponse(
            status_code=exc.status_code,
            content={
                "success": False,
                "error": exc.detail,
                "status_code": exc.status_code
            }
        )

    @app.exception_handler(Exception)
    async def general_exception_handler(request, exc):
        """Handle uncaught exceptions"""
        logger.error(f"Unhandled exception: {exc}")
        return JSONResponse(
            status_code=500,
            content={
                "success": False,
                "error": "Internal server error",
                "detail": str(exc) if config.debug else None
            }
        )

    # Startup and shutdown events
    @app.on_event("startup")
    async def startup_event():
        """Initialize services on startup"""
        logger.info("Starting Nexus Web Dashboard")
        manager = get_memory_manager()
        await manager.initialize()
        logger.info("Nexus Manager initialized")

    @app.on_event("shutdown")
    async def shutdown_event():
        """Cleanup on shutdown"""
        logger.info("Shutting down Nexus Web Dashboard")
        manager = get_memory_manager()
        await manager.close()

    # Mount static files
    static_dir = Path(__file__).parent / "static"
    if static_dir.exists():
        app.mount("/static", StaticFiles(directory=str(static_dir)), name="static")

    # Import and include routes
    from .routes import memories, stats, hooks

    app.include_router(memories.router, prefix="/api/v1", tags=["memories"])
    app.include_router(stats.router, prefix="/api/v1", tags=["stats"])
    app.include_router(hooks.router, prefix="/api/v1", tags=["hooks"])

    # WebSocket endpoint
    @app.websocket("/ws/events")
    async def websocket_events_endpoint(websocket: WebSocket):
        """WebSocket endpoint for real-time event streaming"""
        await websocket_manager.connect(websocket)
        try:
            while True:
                # Keep connection alive and handle incoming messages
                data = await websocket.receive_text()
                try:
                    message = json.loads(data)
                    # Handle client messages if needed (e.g., subscription updates)
                    logger.debug(f"WebSocket received: {message}")
                except json.JSONDecodeError:
                    logger.warning(f"Invalid JSON received: {data}")
        except WebSocketDisconnect:
            logger.info("WebSocket client disconnected")
        except Exception as e:
            logger.error(f"WebSocket error: {e}")
        finally:
            await websocket_manager.disconnect(websocket)

    # Root endpoint - serve dashboard
    @app.get("/", response_class=HTMLResponse)
    async def root():
        """Serve the dashboard HTML"""
        index_path = static_dir / "index.html"
        if index_path.exists():
            with open(index_path, "r") as f:
                return HTMLResponse(content=f.read())
        return HTMLResponse(
            content="""
            <html>
                <head><title>Nexus Memory System</title></head>
                <body>
                    <h1>Nexus Memory System</h1>
                    <p>Web Dashboard not available. Static files missing.</p>
                    <p>Visit <a href="/api/docs">API Documentation</a> for available endpoints.</p>
                </body>
            </html>
            """
        )

    # Health check endpoint
    @app.get("/health", tags=["health"])
    async def health_check():
        """Health check endpoint"""
        return {
            "status": "healthy",
            "timestamp": datetime.now(UTC).isoformat(),
            "version": "1.0.0"
        }

    return app


# Global WebSocket manager instance
websocket_manager: Optional[WebSocketManager] = None


def get_web_app() -> FastAPI:
    """Get or create the FastAPI application instance"""
    global websocket_manager
    if websocket_manager is None:
        websocket_manager = WebSocketManager()
    return create_app()


# Export for convenience
__all__ = [
    "create_app",
    "get_web_app",
    "websocket_manager",
]
