.PHONY: help install install-dev install-all test lint format type-check clean build docker docker-build docker-run serve serve-http serve-stdio docs serve-docs

# Default target
help:
	@echo "Nexus Memory System - Build and Development Commands"
	@echo ""
	@echo "Installation:"
	@echo "  install       Install package in development mode"
	@echo "  install-dev   Install package with development dependencies"
	@echo "  install-all   Install package with all optional dependencies"
	@echo ""
	@echo "Development:"
	@echo "  test          Run tests"
	@echo "  test-cov      Run tests with coverage"
	@echo "  lint          Run code linting"
	@echo "  format        Format code with black"
	@echo "  type-check    Run type checking with mypy"
	@echo "  clean         Clean build artifacts"
	@echo ""
	@echo "Build and Deploy:"
	@echo "  build         Build distribution packages"
	@echo "  docker-build  Build Docker image"
	@echo "  docker-run    Run Docker container"
	@echo "  docker-push   Push Docker image to registry"
	@echo ""
	@echo "Server:"
	@echo "  serve         Start HTTP server (default)"
	@echo "  serve-http    Start HTTP server with verbose logging"
	@echo "  serve-stdio   Start STDIO server for MCP"
	@echo "  serve-ui      Start Web UI server"
	@echo ""
	@echo "Documentation:"
	@echo "  docs          Generate documentation"
	@echo "  serve-docs    Serve documentation locally"

# Installation targets
install:
	pip install -e .

install-dev:
	pip install -e ".[dev]"

install-all:
	pip install -e ".[dev,postgres,embeddings]"

# Development targets
test:
	pytest

test-cov:
	pytest --cov=nexus --cov-report=html --cov-report=term

lint:
	ruff check nexus/ tests/

format:
	black nexus/ tests/
	ruff format nexus/ tests/

type-check:
	mypy nexus/

clean:
	rm -rf build/
	rm -rf dist/
	rm -rf *.egg-info/
	rm -rf .pytest_cache/
	rm -rf .coverage
	rm -rf htmlcov/
	find . -type d -name __pycache__ -delete
	find . -type f -name "*.pyc" -delete

# Build targets
build: clean
	python -m build

# Docker targets
docker-build:
	docker build -t nexus-memory-system:latest .

docker-run:
	docker run -d \
		--name nexus \
		-p 8767:8767 \
		-p 8768:8768 \
		-v nexus-data:/data \
		nexus-memory-system:latest

docker-push:
	docker tag nexus-memory-system:latest scooter-lacroix/nexus-memory-system:latest
	docker push scooter-lacroix/nexus-memory-system:latest

# Server targets
serve:
	nexus-serve --transport http

serve-http:
	nexus-serve --transport http --verbose

serve-stdio:
	nexus-serve --transport stdio

serve-ui:
	nexus-ui

# Documentation targets
docs:
	@echo "Documentation is available at docs/"
	@echo "To serve documentation locally, run: make serve-docs"

serve-docs:
	cd docs && python -m http.server 8000

# Development shortcuts
dev-setup: install-dev
	@echo "Setting up development environment..."
	@echo "Installing pre-commit hooks..."
	pre-commit install
	@echo "Creating example .env file..."
	@if [ ! -f .env ]; then cp .env.example .env; echo "Created .env file"; fi
	@echo "Development setup complete!"

# Database targets
db-init:
	nexus db init

db-migrate:
	nexus db migrate

db-reset:
	nexus db reset

# Testing shortcuts
test-fast:
	pytest -x -v tests/unit/

test-integration:
	pytest -x -v tests/integration/

test-all:
	pytest -v --cov=nexus --cov-report=html --cov-report=term

# Production targets
prod-build:
	@echo "Building for production..."
	python -m build --sdist --wheel

prod-test:
	@echo "Testing production build..."
	python -m twine check dist/*

# Quality targets
quality: lint type-check test
	@echo "All quality checks passed!"

# Quick development cycle
dev: format lint test
	@echo "Development cycle complete!"