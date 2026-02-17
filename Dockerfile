# Nexus Memory System Dockerfile
FROM python:3.11-slim

# Set metadata
LABEL maintainer="scooter-lacroix"
LABEL description="Nexus Memory System - Cross-agent memory management platform"
LABEL version="1.0.0"

# Set environment variables
ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

# Install system dependencies
RUN apt-get update && apt-get install -y \
    gcc \
    g++ \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd --create-home --shell /bin/bash nexus

# Set work directory
WORKDIR /app

# Copy requirements first for better layer caching
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application code
COPY . .

# Install the application
RUN pip install -e .

# Create data directory
RUN mkdir -p /app/data && chown -R nexus:nexus /app

# Switch to non-root user
USER nexus

# Expose ports
EXPOSE 8767 8768

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8767/health || exit 1

# Default command
CMD ["nexus", "serve", "--transport", "http"]