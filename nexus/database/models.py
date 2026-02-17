"""
Database models for Nexus Memory System
"""

from datetime import datetime
from typing import Optional, Dict, Any, List
from sqlalchemy import (
    Column, Integer, String, Text, DateTime, Float, Boolean,
    ForeignKey, JSON, Index, UniqueConstraint, CheckConstraint
)
from sqlalchemy.orm import declarative_base, relationship
from sqlalchemy.sql import func

Base = declarative_base()


# Helper functions that use enums - defined as lazy imports to avoid
# circular import and metadata attribute conflict
def _get_category_description(category: str) -> str:
    """Lazy import helper for category description"""
    from .enums import get_category_description
    return get_category_description(category)


def _is_valid_category(category: str) -> bool:
    """Lazy import helper for category validation"""
    from .enums import is_valid_category
    return is_valid_category(category)


def _is_valid_memory_lane_type(memory_lane_type: str) -> bool:
    """Lazy import helper for memory lane type validation"""
    from .enums import is_valid_memory_lane_type
    return is_valid_memory_lane_type(memory_lane_type)


class AgentNamespace(Base):
    """Agent namespace model for organizing memories by agent type"""
    __tablename__ = "agent_namespaces"

    id = Column(Integer, primary_key=True, index=True)
    name = Column(String(100), unique=True, nullable=False, index=True)
    description = Column(Text, nullable=True)
    agent_type = Column(String(50), unique=True, nullable=False, index=True)
    created_at = Column(DateTime(timezone=True), server_default=func.now())
    updated_at = Column(DateTime(timezone=True), onupdate=func.now())

    # Relationships
    memories = relationship("Memory", back_populates="namespace", cascade="all, delete-orphan")
    specifications = relationship("TaskSpecification", back_populates="namespace", cascade="all, delete-orphan")

    def __repr__(self):
        return f"<AgentNamespace(name='{self.name}', agent_type='{self.agent_type}')>"


class Memory(Base):
    """
    Memory model for storing agent memories with hybrid type system

    Hybrid Type System:
    - category: Nexus flexible category (preserved, required)
    - memory_lane_type: Memory Lane cognitive/priority type (optional, additive)

    The category field is preserved for backward compatibility and represents
    the general Nexus categorization system.

    The memory_lane_type field is optional and allows for Memory Lane's
    cognitive science-based categorization.
    """

    __tablename__ = "memories"

    id = Column(Integer, primary_key=True, index=True)
    namespace_id = Column(Integer, ForeignKey("agent_namespaces.id"), nullable=False)
    content = Column(Text, nullable=False)

    # Nexus category (existing, preserved)
    category = Column(String(50), nullable=False, index=True, default="general")

    # Memory Lane type (optional, additive)
    # Uses CHECK constraint to validate against valid Memory Lane types
    memory_lane_type = Column(
        String(50),
        nullable=True,
        index=True,
        comment="Optional Memory Lane cognitive or priority type"
    )

    labels = Column(JSON, nullable=True)  # List of labels
    # Note: Renamed from 'metadata' to avoid conflict with SQLAlchemy's Base.metadata
    # Column name in database remains 'metadata' for backward compatibility
    extra_metadata = Column("metadata", JSON, nullable=True)  # Additional metadata

    # Search and scoring
    similarity_score = Column(Float, nullable=True, index=True)
    relevance_score = Column(Float, nullable=True, index=True)

    # Embeddings for semantic search
    content_embedding = Column(JSON, nullable=True)  # Vector embedding
    embedding_model = Column(String(100), nullable=True)

    # Timestamps
    created_at = Column(DateTime(timezone=True), server_default=func.now(), index=True)
    updated_at = Column(DateTime(timezone=True), onupdate=func.now())
    last_accessed = Column(DateTime(timezone=True), nullable=True)

    # Status and flags
    is_active = Column(Boolean, default=True, index=True)
    is_archived = Column(Boolean, default=False, index=True)
    access_count = Column(Integer, default=0)

    # Relationships
    namespace = relationship("AgentNamespace", back_populates="memories")
    relations_as_source = relationship("MemoryRelation", foreign_keys="MemoryRelation.source_memory_id", back_populates="source_memory")
    relations_as_target = relationship("MemoryRelation", foreign_keys="MemoryRelation.target_memory_id", back_populates="target_memory")

    # Indexes for performance
    __table_args__ = (
        Index('idx_memory_namespace_category', 'namespace_id', 'category'),
        Index('idx_memory_created_active', 'created_at', 'is_active'),
        Index('idx_memory_relevance', 'relevance_score', 'created_at'),
        Index('idx_memory_labels', 'labels'),
        # Hybrid type system indexes
        Index('idx_memory_namespace_lane_type', 'namespace_id', 'memory_lane_type'),
        Index('idx_memory_category_lane_type', 'category', 'memory_lane_type'),
        # CHECK constraint for memory_lane_type validation
        # Note: This constraint will be added via migration for existing databases
        CheckConstraint(
            "memory_lane_type IS NULL OR memory_lane_type IN ('correction', 'decision', 'commitment', 'insight', 'learning', 'confidence', 'pattern_seed', 'cross_agent', 'workflow_note', 'gap', 'semantic', 'episodic', 'procedural', 'working', 'explicit', 'implicit', 'flashbulb', 'metamemory', 'collective')",
            name='ck_memory_lane_type_valid'
        ),
    )

    def __repr__(self):
        return f"<Memory(id={self.id}, category='{self.category}', created='{self.created_at}')>"

    def to_dict(self) -> Dict[str, Any]:
        """Convert memory to dictionary"""
        return {
            "id": self.id,
            "namespace_id": self.namespace_id,
            "content": self.content,
            "category": self.category,
            "category_description": _get_category_description(self.category),
            "memory_lane_type": self.memory_lane_type,
            "labels": self.labels or [],
            "metadata": self.extra_metadata or {},  # Exposed as 'metadata' for API compatibility
            "similarity_score": self.similarity_score,
            "relevance_score": self.relevance_score,
            "embedding_model": self.embedding_model,
            "created_at": self.created_at.isoformat() if self.created_at else None,
            "updated_at": self.updated_at.isoformat() if self.updated_at else None,
            "last_accessed": self.last_accessed.isoformat() if self.last_accessed else None,
            "is_active": self.is_active,
            "is_archived": self.is_archived,
            "access_count": self.access_count,
        }

    def validate_types(self) -> None:
        """
        Validate category and memory_lane_type fields.

        Raises:
            InvalidCategoryError: If category is not valid
            InvalidMemoryLaneTypeError: If memory_lane_type is not valid
        """
        from .enums import InvalidCategoryError, InvalidMemoryLaneTypeError

        if not _is_valid_category(self.category):
            raise InvalidCategoryError(self.category)

        if self.memory_lane_type is not None and not _is_valid_memory_lane_type(self.memory_lane_type):
            raise InvalidMemoryLaneTypeError(self.memory_lane_type)


class TaskSpecification(Base):
    """Task specification model for reusable task specifications"""
    __tablename__ = "task_specifications"

    id = Column(Integer, primary_key=True, index=True)
    namespace_id = Column(Integer, ForeignKey("agent_namespaces.id"), nullable=False)
    spec_id = Column(String(100), unique=True, nullable=False, index=True)

    # Specification content
    task_description = Column(Text, nullable=False)
    spec_content = Column(JSON, nullable=False)  # The actual specification
    complexity_score = Column(Float, default=0.5, index=True)

    # Usage tracking
    usage_count = Column(Integer, default=0, index=True)
    success_rate = Column(Float, default=0.0)
    last_used = Column(DateTime(timezone=True), nullable=True)

    # Quality metrics
    avg_execution_time = Column(Float, nullable=True)
    user_rating = Column(Float, nullable=True)

    # Embeddings for similarity matching
    description_embedding = Column(JSON, nullable=True)
    spec_embedding = Column(JSON, nullable=True)

    # Timestamps
    created_at = Column(DateTime(timezone=True), server_default=func.now(), index=True)
    updated_at = Column(DateTime(timezone=True), onupdate=func.now())

    # Status
    is_active = Column(Boolean, default=True, index=True)
    is_public = Column(Boolean, default=True, index=True)  # Shareable across agents

    # Relationships
    namespace = relationship("AgentNamespace", back_populates="specifications")

    # Indexes
    __table_args__ = (
        Index('idx_spec_namespace_active', 'namespace_id', 'is_active'),
        Index('idx_spec_complexity_usage', 'complexity_score', 'usage_count'),
        Index('idx_spec_success_rate', 'success_rate', 'usage_count'),
    )

    def __repr__(self):
        return f"<TaskSpecification(id={self.id}, spec_id='{self.spec_id}', usage_count={self.usage_count})>"

    def to_dict(self) -> Dict[str, Any]:
        """Convert specification to dictionary"""
        return {
            "id": self.id,
            "namespace_id": self.namespace_id,
            "spec_id": self.spec_id,
            "task_description": self.task_description,
            "spec_content": self.spec_content,
            "complexity_score": self.complexity_score,
            "usage_count": self.usage_count,
            "success_rate": self.success_rate,
            "last_used": self.last_used.isoformat() if self.last_used else None,
            "avg_execution_time": self.avg_execution_time,
            "user_rating": self.user_rating,
            "created_at": self.created_at.isoformat() if self.created_at else None,
            "updated_at": self.updated_at.isoformat() if self.updated_at else None,
            "is_active": self.is_active,
            "is_public": self.is_public,
        }


class MemoryRelation(Base):
    """Memory relationship model for connecting related memories"""
    __tablename__ = "memory_relations"

    id = Column(Integer, primary_key=True, index=True)
    source_memory_id = Column(Integer, ForeignKey("memories.id"), nullable=False)
    target_memory_id = Column(Integer, ForeignKey("memories.id"), nullable=False)
    relation_type = Column(String(50), nullable=False, index=True)  # e.g., "similar", "related", "parent", "child"
    strength = Column(Float, default=1.0, index=True)  # Relationship strength (0.0-1.0)
    # Note: Renamed from 'metadata' to avoid conflict with SQLAlchemy's Base.metadata
    extra_metadata = Column("metadata", JSON, nullable=True)  # Additional relation data

    # Timestamps
    created_at = Column(DateTime(timezone=True), server_default=func.now())
    updated_at = Column(DateTime(timezone=True), onupdate=func.now())

    # Relationships
    source_memory = relationship("Memory", foreign_keys=[source_memory_id], back_populates="relations_as_source")
    target_memory = relationship("Memory", foreign_keys=[target_memory_id], back_populates="relations_as_target")

    # Constraints
    __table_args__ = (
        UniqueConstraint('source_memory_id', 'target_memory_id', 'relation_type', name='unique_memory_relation'),
        Index('idx_relation_source_type', 'source_memory_id', 'relation_type'),
        Index('idx_relation_target_type', 'target_memory_id', 'relation_type'),
        Index('idx_relation_strength', 'strength', 'created_at'),
    )

    def __repr__(self):
        return f"<MemoryRelation(source={self.source_memory_id}, target={self.target_memory_id}, type='{self.relation_type}')>"


class SystemMetrics(Base):
    """System metrics model for monitoring and analytics"""
    __tablename__ = "system_metrics"

    id = Column(Integer, primary_key=True, index=True)
    metric_name = Column(String(100), nullable=False, index=True)
    metric_value = Column(Float, nullable=False)
    metric_unit = Column(String(20), nullable=True)
    # Note: Renamed from 'metadata' to avoid conflict with SQLAlchemy's Base.metadata
    extra_metadata = Column("metadata", JSON, nullable=True)

    # Timestamps
    recorded_at = Column(DateTime(timezone=True), server_default=func.now(), index=True)

    # Indexes
    __table_args__ = (
        Index('idx_metrics_name_time', 'metric_name', 'recorded_at'),
    )

    def __repr__(self):
        return f"<SystemMetrics(name='{self.metric_name}', value={self.metric_value}, time='{self.recorded_at}')>"