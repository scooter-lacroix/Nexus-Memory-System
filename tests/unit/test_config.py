"""
Unit tests for Nexus configuration
"""

import pytest
import os
from pathlib import Path
from nexus.config.settings import ServerConfig
from nexus.config.agent_namespaces import (
    AGENT_NAMESPACES,
    get_agent_namespace,
    is_supported_agent,
    list_supported_agents,
    get_agent_description
)


class TestServerConfig:
    """Test ServerConfig class"""

    def test_default_config(self):
        """Test default configuration values"""
        config = ServerConfig()

        assert config.host == "127.0.0.1"
        assert config.port == 8767
        assert config.web_port == 8768
        assert config.default_agent_type == "general"
        assert config.conscious_ingest is True
        assert config.auto_ingest is True
        assert config.verbose is False
        assert config.debug is False

    def test_config_validation(self):
        """Test configuration validation"""
        # Test valid configuration
        config = ServerConfig(
            host="0.0.0.0",
            port=8080,
            web_port=8081,
            spec_similarity_threshold=0.9
        )
        assert config.host == "0.0.0.0"
        assert config.port == 8080
        assert config.web_port == 8081
        assert config.spec_similarity_threshold == 0.9

        # Test invalid port
        with pytest.raises(ValueError):
            ServerConfig(port=70000)

        # Test invalid similarity threshold
        with pytest.raises(ValueError):
            ServerConfig(spec_similarity_threshold=1.5)

    def test_database_url(self):
        """Test database URL generation"""
        config = ServerConfig(database_path="/tmp/test.db")
        assert config.database_connection_url == "sqlite:///tmp/test.db"

        config = ServerConfig(database_url="postgresql://user:pass@host/db")
        assert config.database_connection_url == "postgresql://user:pass@host/db"

    def test_agent_namespace(self):
        """Test agent namespace mapping"""
        config = ServerConfig()
        assert config.get_agent_namespace("claude-code") == "claude_code_memory"
        assert config.get_agent_namespace("unknown") == "general_agent_memory"

    def test_cors_config(self):
        """Test CORS configuration"""
        config = ServerConfig()
        cors_config = config.get_cors_config()
        assert "allow_origins" in cors_config
        assert "localhost:8768" in str(cors_config["allow_origins"])

    def test_web_enabled(self):
        """Test web UI enabled check"""
        config = ServerConfig(web_port=8768)
        assert config.is_web_enabled() is True

        config = ServerConfig(web_port=0)
        assert config.is_web_enabled() is False

    def test_from_env(self, monkeypatch):
        """Test configuration from environment variables"""
        monkeypatch.setenv("NEXUS_HOST", "0.0.0.0")
        monkeypatch.setenv("NEXUS_PORT", "9000")
        monkeypatch.setenv("NEXUS_VERBOSE", "true")
        monkeypatch.setenv("NEXUS_SPEC_SIMILARITY_THRESHOLD", "0.9")

        config = ServerConfig.from_env()
        assert config.host == "0.0.0.0"
        assert config.port == 9000
        assert config.verbose is True
        assert config.spec_similarity_threshold == 0.9


class TestAgentNamespaces:
    """Test agent namespace functions"""

    def test_agent_namespaces_dict(self):
        """Test AGENT_NAMESPACES dictionary"""
        assert isinstance(AGENT_NAMESPACES, dict)
        assert "claude-code" in AGENT_NAMESPACES
        assert "gemini" in AGENT_NAMESPACES
        assert "general" in AGENT_NAMESPACES

    def test_get_agent_namespace(self):
        """Test get_agent_namespace function"""
        assert get_agent_namespace("claude-code") == "claude_code_memory"
        assert get_agent_namespace("general") == "general_agent_memory"
        assert get_agent_namespace("unknown") == "general_agent_memory"
        assert get_agent_namespace("UNKNOWN") == "general_agent_memory"  # Case insensitive

    def test_is_supported_agent(self):
        """Test is_supported_agent function"""
        assert is_supported_agent("claude-code") is True
        assert is_supported_agent("gemini") is True
        assert is_supported_agent("general") is True
        assert is_supported_agent("unknown") is False

    def test_list_supported_agents(self):
        """Test list_supported_agents function"""
        agents = list_supported_agents()
        assert isinstance(agents, list)
        assert len(agents) > 0
        assert "claude-code" in agents
        assert "general" in agents

    def test_get_agent_description(self):
        """Test get_agent_description function"""
        desc = get_agent_description("claude-code")
        assert isinstance(desc, str)
        assert "Claude Code" in desc

        desc = get_agent_description("unknown")
        assert isinstance(desc, str)
        assert "Unknown agent" in desc