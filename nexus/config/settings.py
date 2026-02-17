"""
Configuration management for Nexus Memory System
"""

import os
from pathlib import Path
from typing import Dict, Any, Optional, List
from pydantic import BaseModel, Field, validator
from pydantic_settings import BaseSettings

from .agent_namespaces import AGENT_NAMESPACES


# Lazy import for SyncPolicy to avoid circular dependency
def _get_sync_policy_enum(value: str) -> Any:
    """Lazy import SyncPolicy enum"""
    from ..orchestrator.sync import SyncPolicy
    policy_map = {
        "MANUAL": SyncPolicy.MANUAL,
        "AUTO": SyncPolicy.AUTO,
        "SELECTIVE": SyncPolicy.SELECTIVE,
        "BIDIRECTIONAL": SyncPolicy.BIDIRECTIONAL,
    }
    return policy_map.get(value.upper(), SyncPolicy.MANUAL)


class ServerConfig(BaseSettings):
    """Configuration for the Nexus Memory System server"""

    # Agent namespace mapping
    AGENT_NAMESPACES: Dict[str, str] = AGENT_NAMESPACES

    # Database configuration
    database_path: str = Field(
        default=str(Path.home() / ".nexus-memory-system" / "nexus.db"),
        description="Path to SQLite database file"
    )

    database_url: Optional[str] = Field(
        default=None,
        description="Database connection URL (overrides database_path if set)"
    )

    # Server configuration
    host: str = Field(
        default="127.0.0.1",
        description="Server host address"
    )

    port: int = Field(
        default=8767,
        description="Server port for MCP API"
    )

    web_port: int = Field(
        default=8768,
        description="Port for web dashboard"
    )

    # Agent configuration
    default_agent_type: str = Field(
        default="general",
        description="Default agent type for memory operations"
    )

    # Memory configuration
    conscious_ingest: bool = Field(
        default=True,
        description="Enable conscious ingest for working memory"
    )

    auto_ingest: bool = Field(
        default=True,
        description="Enable auto ingest for dynamic memory retrieval"
    )

    # Memory limits and performance
    conscious_memory_limit: int = Field(
        default=10,
        description="Limit for conscious memory processing"
    )

    memory_search_limit: int = Field(
        default=5,
        description="Limit for memory search results"
    )

    max_memory_size: int = Field(
        default=10000,
        description="Maximum memory content size in characters"
    )

    # Specification system configuration
    spec_similarity_threshold: float = Field(
        default=0.8,
        description="Similarity threshold for specification reuse"
    )

    # OpenAI API configuration (optional, for enhanced features)
    openai_api_key: Optional[str] = Field(
        default=None,
        description="OpenAI API key for enhanced memory features"
    )

    openai_model: str = Field(
        default="gpt-3.5-turbo",
        description="OpenAI model for enhanced features"
    )

    # Embeddings configuration
    embeddings_model: str = Field(
        default="all-MiniLM-L6-v2",
        description="Sentence transformer model for embeddings"
    )

    embeddings_enabled: bool = Field(
        default=False,
        description="Enable semantic search with embeddings"
    )

    # Debug configuration
    verbose: bool = Field(
        default=False,
        description="Enable verbose logging"
    )

    debug: bool = Field(
        default=False,
        description="Enable debug mode"
    )

    # Security configuration
    cors_origins: list[str] = Field(
        default=["http://localhost:8768", "http://127.0.0.1:8768"],
        description="CORS allowed origins for web UI"
    )

    api_key_required: bool = Field(
        default=False,
        description="Require API key for access"
    )

    api_key: Optional[str] = Field(
        default=None,
        description="API key for server access"
    )

    # Performance configuration
    max_concurrent_requests: int = Field(
        default=100,
        description="Maximum concurrent requests"
    )

    request_timeout: int = Field(
        default=30,
        description="Request timeout in seconds"
    )

    # Hooks/Automated Extraction configuration
    hooks_auto_extraction_enabled: bool = Field(
        default=True,
        description="Enable automated memory extraction via agent hooks"
    )

    hooks_inactivity_threshold_minutes: int = Field(
        default=5,
        description="Minutes of inactivity before triggering extraction"
    )

    hooks_buffer_dir: Optional[str] = Field(
        default=str(Path.home() / ".nexus-memory-system" / "buffers"),
        description="Directory for persistent crash recovery buffers"
    )

    hooks_monitoring_interval_seconds: int = Field(
        default=5,
        description="Interval between session monitoring checks"
    )

    hooks_enabled_agents: list[str] = Field(
        default_factory=list,
        description="List of agents with hooks enabled (empty = all supported)"
    )

    # Orchestrator configuration
    orchestrator_session_idle_threshold_seconds: int = Field(
        default=300,
        description="Seconds before session considered idle (5 minutes)"
    )

    orchestrator_session_timeout_seconds: int = Field(
        default=3600,
        description="Seconds before session auto-closes (1 hour)"
    )

    orchestrator_session_persistence_enabled: bool = Field(
        default=False,
        description="Enable session state persistence to disk"
    )

    orchestrator_event_queue_max_size: int = Field(
        default=10000,
        description="Maximum event queue size"
    )

    orchestrator_event_max_workers: int = Field(
        default=4,
        description="Maximum concurrent event handler workers"
    )

    orchestrator_event_persistence_enabled: bool = Field(
        default=False,
        description="Enable event persistence for crash recovery"
    )

    orchestrator_sync_policy_str: str = Field(
        default="MANUAL",
        description="Default sync policy: MANUAL, AUTO, SELECTIVE, or BIDIRECTIONAL"
    )

    orchestrator_auto_share_labels: list[str] = Field(
        default_factory=lambda: ["cross-agent", "shared"],
        description="Labels that trigger automatic memory sharing"
    )

    class Config:
        env_prefix = "NEXUS_"
        env_file = ".env"
        env_file_encoding = "utf-8"

    @validator('port', 'web_port')
    def validate_ports(cls, v):
        """Validate port numbers"""
        if not 1 <= v <= 65535:
            raise ValueError('Port must be between 1 and 65535')
        return v

    @validator('spec_similarity_threshold')
    def validate_similarity_threshold(cls, v):
        """Validate similarity threshold"""
        if not 0.0 <= v <= 1.0:
            raise ValueError('Similarity threshold must be between 0.0 and 1.0')
        return v

    @validator('conscious_memory_limit', 'memory_search_limit', 'max_concurrent_requests')
    def validate_positive_int(cls, v):
        """Validate positive integers"""
        if v < 1:
            raise ValueError('Value must be positive')
        return v

    @validator('hooks_inactivity_threshold_minutes', 'hooks_monitoring_interval_seconds')
    def validate_hooks_thresholds(cls, v):
        """Validate hooks thresholds"""
        if v < 1:
            raise ValueError('Threshold must be at least 1')
        return v

    @validator('orchestrator_sync_policy_str')
    def validate_sync_policy(cls, v):
        """Validate sync policy"""
        valid_policies = {"MANUAL", "AUTO", "SELECTIVE", "BIDIRECTIONAL"}
        if v.upper() not in valid_policies:
            raise ValueError(f'Sync policy must be one of {valid_policies}')
        return v.upper()

    @property
    def database_connection_url(self) -> str:
        """Get the database connection URL"""
        if self.database_url:
            return self.database_url
        return f"sqlite:///{self.database_path}"

    def get_agent_namespace(self, agent_type: str) -> str:
        """Get the memory namespace for an agent type"""
        return AGENT_NAMESPACES.get(agent_type, AGENT_NAMESPACES['general'])

    def is_web_enabled(self) -> bool:
        """Check if web UI is enabled"""
        return self.web_port > 0

    def get_cors_config(self) -> Dict[str, Any]:
        """Get CORS configuration for FastAPI"""
        return {
            "allow_origins": self.cors_origins,
            "allow_credentials": True,
            "allow_methods": ["*"],
            "allow_headers": ["*"],
        }

    def get_database_config(self) -> Dict[str, Any]:
        """Get database configuration"""
        return {
            "url": self.database_connection_url,
            "echo": self.debug,
            "pool_pre_ping": True,
            "pool_recycle": 3600,
        }

    def get_hooks_config(self) -> Dict[str, Any]:
        """Get hooks/automated extraction configuration"""
        return {
            "auto_extraction_enabled": self.hooks_auto_extraction_enabled,
            "inactivity_threshold_minutes": self.hooks_inactivity_threshold_minutes,
            "buffer_dir": Path(self.hooks_buffer_dir) if self.hooks_buffer_dir else None,
            "monitoring_interval_seconds": self.hooks_monitoring_interval_seconds,
            "enabled_agents": self.hooks_enabled_agents if self.hooks_enabled_agents else None,
        }

    @property
    def orchestrator_sync_policy(self):
        """Get orchestrator sync policy as enum"""
        return _get_sync_policy_enum(self.orchestrator_sync_policy_str)

    def get_orchestrator_auto_share_labels(self) -> List[str]:
        """Get auto-share labels list"""
        return self.__dict__.get('orchestrator_auto_share_labels') or []

    def get_orchestrator_config(self) -> Dict[str, Any]:
        """Get orchestrator configuration"""
        return {
            "session_idle_threshold_seconds": self.orchestrator_session_idle_threshold_seconds,
            "session_timeout_seconds": self.orchestrator_session_timeout_seconds,
            "session_persistence_enabled": self.orchestrator_session_persistence_enabled,
            "event_queue_max_size": self.orchestrator_event_queue_max_size,
            "event_max_workers": self.orchestrator_event_max_workers,
            "event_persistence_enabled": self.orchestrator_event_persistence_enabled,
            "sync_policy": self.orchestrator_sync_policy,
            "auto_share_labels": self.get_orchestrator_auto_share_labels(),
        }

    @classmethod
    def from_env(cls) -> "ServerConfig":
        """Create configuration from environment variables"""
        return cls()

    def to_dict(self) -> Dict[str, Any]:
        """Convert configuration to dictionary"""
        return self.dict()

    def save_to_file(self, path: str) -> None:
        """Save configuration to file"""
        import json
        with open(path, 'w') as f:
            json.dump(self.to_dict(), f, indent=2)

    @classmethod
    def load_from_file(cls, path: str) -> "ServerConfig":
        """Load configuration from file"""
        import json
        with open(path, 'r') as f:
            data = json.load(f)
        return cls(**data)


# Global configuration instance
config = ServerConfig.from_env()