# Docker Deployment Guide

> **Deploy Nexus Memory System with Docker**

**Version:** 1.1.0

---

## Table of Contents

- [Overview](#overview)
- [Dockerfile](#dockerfile)
- [Docker Compose](#docker-compose)
- [Environment Variables](#environment-variables)
- [Volume Mounts](#volume-mounts)
- [Networking](#networking)
- [Deployment](#deployment)

---

## Overview

This guide covers deploying Nexus Memory System using Docker and Docker Compose.

### Prerequisites

- Docker 20.10+
- Docker Compose 2.0+

---

## Dockerfile

### Multi-Stage Dockerfile

Create `Dockerfile` in project root:

```dockerfile
# Build stage
FROM python:3.11-slim as builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc \
    g++ \
    && rm -rf /var/lib/apt/lists/*

# Install Python dependencies
COPY pyproject.toml ./
RUN pip install --no-cache-dir --user -e .[embeddings]

# Runtime stage
FROM python:3.11-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 nexus && \
    mkdir -p /app /data && \
    chown -R nexus:nexus /app /data

WORKDIR /app

# Copy Python packages from builder
COPY --from=builder /root/.local /root/.local

# Copy application
COPY nexus/ ./nexus/
COPY pyproject.toml ./

# Make sure scripts in .local are usable
ENV PATH=/root/.local/bin:$PATH

# Switch to non-root user
USER nexus

# Expose ports
EXPOSE 8000 8767

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/health || exit 1

# Run application
CMD ["nexus", "serve", "--transport", "web", "--host", "0.0.0.0", "--web-port", "8000"]
```

### Build Image

```bash
# Build image
docker build -t nexus-memory-system:1.1.0 .

# Build with buildx for multi-platform
docker buildx build --platform linux/amd64,linux/arm64 -t nexus-memory-system:1.1.0 .
```

---

## Docker Compose

### docker-compose.yml

```yaml
version: '3.8'

services:
  # PostgreSQL database
  db:
    image: postgres:15-alpine
    container_name: nexus-db
    restart: unless-stopped
    environment:
      POSTGRES_DB: nexus_memory
      POSTGRES_USER: nexus_user
      POSTGRES_PASSWORD: ${DB_PASSWORD:-changeme}
    volumes:
      - postgres_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U nexus_user -d nexus_memory"]
      interval: 10s
      timeout: 5s
      retries: 5

  # Redis cache (optional)
  redis:
    image: redis:7-alpine
    container_name: nexus-redis
    restart: unless-stopped
    command: redis-server --appendonly yes
    volumes:
      - redis_data:/data
    ports:
      - "6379:6379"

  # Nexus application
  nexus:
    image: nexus-memory-system:1.1.0
    container_name: nexus-app
    restart: unless-stopped
    depends_on:
      db:
        condition: service_healthy
    environment:
      # Database
      NEXUS_DATABASE_URL: postgresql://nexus_user:${DB_PASSWORD:-changeme}@db:5432/nexus_memory

      # Server
      NEXUS_HOST: 0.0.0.0
      NEXUS_WEB_PORT: 8000

      # Security
      NEXUS_API_KEY: ${NEXUS_API_KEY:-your-api-key}

      # Embeddings
      NEXUS_EMBEDDINGS_ENABLED: "true"
      NEXUS_EMBEDDING_DEVICE: cpu

      # Cache (optional)
      NEXUS_CACHE_ENABLED: "true"
      NEXUS_CACHE_URL: redis://redis:6379/0

      # Hooks
      NEXUS_NATIVE_HOOKS: "true"
      NEXUS_BUFFER_ENABLED: "true"
    volumes:
      - nexus_data:/data
      - nexus_logs:/var/log/nexus
    ports:
      - "8000:8000"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8000/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s

  # Nginx reverse proxy
  nginx:
    image: nginx:alpine
    container_name: nexus-nginx
    restart: unless-stopped
    depends_on:
      - nexus
    volumes:
      - ./nginx.conf:/etc/nginx/nginx.conf:ro
      - ./ssl:/etc/nginx/ssl:ro
    ports:
      - "80:80"
      - "443:443"

volumes:
  postgres_data:
    driver: local
  redis_data:
    driver: local
  nexus_data:
    driver: local
  nexus_logs:
    driver: local

networks:
  default:
    name: nexus-network
```

---

## Environment Variables

### .env File

Create `.env` in the same directory:

```bash
# Database
DB_PASSWORD=your_secure_password_here

# Security
NEXUS_API_KEY=your_api_key_here

# Server
NEXUS_WEB_PORT=8000

# Embeddings
NEXUS_EMBEDDINGS_ENABLED=true
NEXUS_EMBEDDING_DEVICE=cpu

# Cache
NEXUS_CACHE_ENABLED=true

# Hooks
NEXUS_NATIVE_HOOKS=true
NEXUS_BUFFER_ENABLED=true
```

**Security Note:** Never commit `.env` to version control. Add to `.gitignore`:

```bash
echo ".env" >> .gitignore
```

---

## Volume Mounts

### Directory Structure

```
nexus-docker/
├── docker-compose.yml
├── .env
├── nginx.conf
├── ssl/
│   ├── nexus.crt
│   └── nexus.key
└── data/
    ├── postgres/      # PostgreSQL data
    ├── redis/         # Redis data
    ├── nexus/         # Nexus application data
    └── logs/          # Application logs
```

### Bind Mounts (Development)

For development, use bind mounts:

```yaml
services:
  nexus:
    volumes:
      - ./nexus:/app/nexus:ro
      - ./data/nexus:/data
      - ./data/logs:/var/log/nexus
```

### Named Volumes (Production)

For production, use named volumes:

```yaml
services:
  nexus:
    volumes:
      - nexus_data:/data
      - nexus_logs:/var/log/nexus

volumes:
  nexus_data:
    driver: local
    driver_opts:
      type: none
      o: bind
      device: /var/lib/nexus/data
  nexus_logs:
    driver: local
    driver_opts:
      type: none
      o: bind
      device: /var/log/nexus
```

---

## Networking

### nginx.conf

```nginx
events {
    worker_connections 1024;
}

http {
    upstream nexus {
        server nexus:8000;
    }

    # HTTP redirect to HTTPS
    server {
        listen 80;
        server_name nexus.example.com;
        return 301 https://$server_name$request_uri;
    }

    # HTTPS
    server {
        listen 443 ssl http2;
        server_name nexus.example.com;

        ssl_certificate /etc/nginx/ssl/nexus.crt;
        ssl_certificate_key /etc/nginx/ssl/nexus.key;
        ssl_protocols TLSv1.2 TLSv1.3;
        ssl_ciphers HIGH:!aNULL:!MD5;

        # Rate limiting
        limit_req_zone $binary_remote_addr zone=api:10m rate=10r/s;
        limit_req zone=api burst=20 nodelay;

        location / {
            proxy_pass http://nexus;
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto $scheme;
        }

        location /ws/ {
            proxy_pass http://nexus;
            proxy_http_version 1.1;
            proxy_set_header Upgrade $http_upgrade;
            proxy_set_header Connection "upgrade";
        }
    }
}
```

---

## Deployment

### Start Services

```bash
# Start all services
docker-compose up -d

# View logs
docker-compose logs -f nexus

# Check status
docker-compose ps
```

### Stop Services

```bash
# Stop all services
docker-compose down

# Stop and remove volumes
docker-compose down -v
```

### Update Deployment

```bash
# Pull new image
docker-compose pull nexus

# Restart with new image
docker-compose up -d nexus

# Or rebuild from source
docker-compose build nexus
docker-compose up -d nexus
```

### Database Migration

```bash
# Run migrations in container
docker-compose exec nexus nexus init

# Or connect to database directly
docker-compose exec db psql -U nexus_user -d nexus_memory
```

### Backup and Restore

```bash
# Backup database
docker-compose exec db pg_dump -U nexus_user nexus_memory > backup.sql

# Restore database
docker-compose exec -T db psql -U nexus_user nexus_memory < backup.sql

# Backup volumes
docker run --rm -v nexus-docker_postgres_data:/data -v $(pwd):/backup alpine tar czf /backup/postgres_backup.tar.gz /data
```

---

## Production Considerations

### Resource Limits

```yaml
services:
  nexus:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 4G
        reservations:
          cpus: '1'
          memory: 2G
```

### Auto-Restart

```yaml
services:
  nexus:
    restart: unless-stopped
```

### Logging

```yaml
services:
  nexus:
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"
```

### Health Checks

```yaml
services:
  nexus:
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8000/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s
```

---

## Related Documentation

- [Production Deployment](production.md) - Production configuration
- [INSTALLATION.md](../../INSTALLATION.md) - Installation guide
- [ARCHITECTURE.md](../../ARCHITECTURE.md) - System architecture

---

**Last Updated:** 2025-12-23
