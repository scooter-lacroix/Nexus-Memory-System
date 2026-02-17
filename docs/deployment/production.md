# Production Deployment Guide

> **Deploying Nexus Memory System in Production**

**Version:** 1.1.0

---

## Table of Contents

- [Overview](#overview)
- [Configuration Best Practices](#configuration-best-practices)
- [Security Considerations](#security-considerations)
- [Performance Tuning](#performance-tuning)
- [Monitoring Setup](#monitoring-setup)
- [High Availability](#high-availability)
- [Backup and Recovery](#backup-and-recovery)

---

## Overview

This guide covers deploying Nexus Memory System in a production environment.

### Pre-Deployment Checklist

- [ ] Database selected and configured (PostgreSQL recommended)
- [ ] SSL/TLS certificates obtained
- [ ] Authentication configured
- [ ] Monitoring and logging setup
- [ ] Backup strategy defined
- [ ] Resource limits configured
- [ ] Health checks configured

---

## Configuration Best Practices

### Environment Variables

Create a production `.env` file:

```bash
# Database (PostgreSQL recommended)
NEXUS_DATABASE_URL=postgresql://nexus:secure_password@db-server:5432/nexus_prod

# Server
NEXUS_HOST=0.0.0.0
NEXUS_PORT=8767
NEXUS_WEB_PORT=8000

# Security
NEXUS_API_KEY=your-secure-api-key-here
NEXUS_API_KEY_HEADER=X-API-Key

# Performance
NEXUS_EMBEDDINGS_ENABLED=true
NEXUS_EMBEDDING_DEVICE=cuda
NEXUS_WORKER_COUNT=4
NEXUS_MEMORY_SEARCH_LIMIT=50

# Logging
NEXUS_LOG_LEVEL=INFO
NEXUS_LOG_FILE=/var/log/nexus/nexus.log

# Cross-Agent Sync
NEXUS_SYNC_POLICY=manual
NEXUS_AUTO_SHARE_LABELS=cross-agent,shared
```

### Configuration File

Create `/etc/nexus/config.yml`:

```yaml
database:
  url: postgresql://nexus:secure_password@db-server:5432/nexus_prod
  pool_size: 20
  max_overflow: 10
  pool_timeout: 30

server:
  host: 0.0.0.0
  port: 8000
  workers: 4
  reload: false

embeddings:
  enabled: true
  model: all-MiniLM-L6-v2
  device: cuda
  batch_size: 32

security:
  api_key_enabled: true
  api_key: ${NEXUS_API_KEY}
  cors_origins:
    - https://nexus.example.com
    - https://app.example.com

logging:
  level: INFO
  file: /var/log/nexus/nexus.log
  rotation: 100 MB
  retention: 30 days

monitoring:
  enabled: true
  metrics_port: 9090
  health_check_interval: 30
```

---

## Security Considerations

### API Authentication

Implement API key authentication:

```python
# In nexus/web/app.py
from fastapi import Security, HTTPException
from fastapi.security import APIKeyHeader

api_key_header = APIKeyHeader(name="X-API-Key")

async def verify_api_key(api_key: str = Security(api_key_header)):
    correct_key = os.getenv("NEXUS_API_KEY")
    if not correct_key or api_key != correct_key:
        raise HTTPException(status_code=403, detail="Invalid API key")
    return api_key

# Apply to endpoints
@app.post("/api/v1/memories", dependencies=[Depends(verify_api_key)])
async def create_memory(...):
    ...
```

### SSL/TLS Configuration

Use reverse proxy (nginx) for SSL:

```nginx
# /etc/nginx/sites-available/nexus
server {
    listen 443 ssl http2;
    server_name nexus.example.com;

    ssl_certificate /etc/ssl/certs/nexus.crt;
    ssl_certificate_key /etc/ssl/private/nexus.key;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    location / {
        proxy_pass http://127.0.0.1:8000;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    location /ws/ {
        proxy_pass http://127.0.0.1:8000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}

# Redirect HTTP to HTTPS
server {
    listen 80;
    server_name nexus.example.com;
    return 301 https://$server_name$request_uri;
}
```

### Firewall Rules

```bash
# Allow only necessary ports
ufw allow 22/tcp    # SSH
ufw allow 80/tcp    # HTTP
ufw allow 443/tcp   # HTTPS
ufw deny 8000/tcp   # Block direct access to app
ufw enable
```

### Database Security

```sql
-- Create dedicated database user
CREATE USER nexus_prod WITH PASSWORD 'secure_password';
CREATE DATABASE nexus_prod OWNER nexus_prod;

-- Grant minimal permissions
GRANT CONNECT ON DATABASE nexus_prod TO nexus_prod;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO nexus_prod;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO nexus_prod;

-- Revoke dangerous permissions
REVOKE CREATE ON SCHEMA public FROM nexus_prod;
REVOKE DROP ON SCHEMA public FROM nexus_prod;
```

---

## Performance Tuning

### Database Optimization

#### PostgreSQL Configuration

```ini
# postgresql.conf
shared_buffers = 2GB
effective_cache_size = 6GB
maintenance_work_mem = 512MB
checkpoint_completion_target = 0.9
wal_buffers = 16MB
default_statistics_target = 100
random_page_cost = 1.1
effective_io_concurrency = 200
work_mem = 2621kB
min_wal_size = 1GB
max_wal_size = 4GB
max_worker_processes = 8
max_parallel_workers_per_gather = 4
max_parallel_workers = 8
max_parallel_maintenance_workers = 4
```

#### Connection Pooling

Use PgBouncer for connection pooling:

```ini
# pgbouncer.ini
[databases]
nexus_prod = host=db-server port=5432 dbname=nexus_prod

[pgbouncer]
listen_addr = 127.0.0.1
listen_port = 6432
auth_type = md5
auth_file = /etc/pgbouncer/userlist.txt
pool_mode = transaction
max_client_conn = 1000
default_pool_size = 25
reserve_pool_size = 5
reserve_pool_timeout = 3
max_db_connections = 50
max_user_connections = 50
server_idle_timeout = 600
```

### Application Tuning

#### Gunicorn Workers

```bash
# Calculate workers: (2 x CPU cores) + 1
# For 4 cores: 9 workers

gunicorn nexus.web.app:get_web_app \
  --workers 9 \
  --worker-class uvicorn.workers.UvicornWorker \
  --bind 0.0.0.0:8000 \
  --worker-connections 1000 \
  --max-requests 1000 \
  --max-requests-jitter 50 \
  --timeout 30 \
  --keep-alive 5 \
  --access-logfile /var/log/nexus/access.log \
  --error-logfile /var/log/nexus/error.log \
  --log-level info
```

#### Embedding Optimization

```python
# Use GPU for embeddings
export NEXUS_EMBEDDING_DEVICE=cuda

# Increase batch size
NEXUS_EMBEDDING_BATCH_SIZE=64

# Pre-load model at startup
NEXUS_PRELOAD_MODEL=true
```

### Caching Layer (Optional)

Add Redis for caching:

```bash
# Install Redis
apt-get install redis-server

# Enable caching
export NEXUS_CACHE_ENABLED=true
export NEXUS_CACHE_URL=redis://localhost:6379/0
export NEXUS_CACHE_TTL=3600
```

---

## Monitoring Setup

### Health Check Endpoint

Configure health checks:

```bash
# Add to load balancer
curl http://localhost:8000/health

# Expected response
{
  "status": "healthy",
  "timestamp": "2025-12-23T10:30:00Z",
  "version": "1.1.0"
}
```

### Metrics Collection

Use Prometheus metrics:

```python
# Add to application
from prometheus_client import Counter, Histogram, start_http_server

memory_creates = Counter('nexus_memory_creates_total', 'Total memories created')
search_duration = Histogram('nexus_search_duration_seconds', 'Search duration')

# Expose metrics on port 9090
start_http_server(9090)
```

Prometheus configuration:

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'nexus'
    static_configs:
      - targets: ['localhost:9090']
    scrape_interval: 15s
```

### Logging

Configure structured logging:

```python
# In nexus/config.py
LOGGING = {
    'version': 1,
    'disable_existing_loggers': False,
    'formatters': {
        'json': {
            'format': '{"time": "%(asctime)s", "level": "%(levelname)s", "message": "%(message)s"}'
        }
    },
    'handlers': {
        'file': {
            'class': 'logging.handlers.RotatingFileHandler',
            'filename': '/var/log/nexus/nexus.log',
            'maxBytes': 104857600,  # 100 MB
            'backupCount': 10,
            'formatter': 'json'
        }
    },
    'root': {
        'level': 'INFO',
        'handlers': ['file']
    }
}
```

### Alerts

Configure alerts for:

- Database connection failures
- High memory usage (>80%)
- Slow queries (>1s)
- API error rate (>5%)
- Extraction failures

---

## High Availability

### Database Replication

Set up PostgreSQL streaming replication:

```bash
# Primary (master)
# postgresql.conf
wal_level = replica
max_wal_senders = 3
wal_keep_size = 1GB

# Replica (slave)
# recovery.conf
standby_mode = on
primary_conninfo = 'host=primary port=5432 user=replicator'
restore_command = 'cp /var/lib/postgresql/archive/%f %p'
```

### Application Redundancy

Deploy multiple app instances behind load balancer:

```
┌──────────────┐
│  Load Balancer │
│  (nginx/HAProxy)│
└──────┬────────┘
       │
       ├──────────┬──────────┬──────────┐
       │          │          │          │
   ┌───▼───┐ ┌──▼───┐ ┌──▼───┐ ┌──▼───┐
   │ App 1 │ │ App 2 │ │ App 3 │ │ App 4 │
   └───────┘ └──────┘ └──────┘ └──────┘
       │          │          │          │
       └──────────┴──────────┴──────────┘
                    │
              ┌─────▼─────┐
              │  Primary  │
              │ Database  │
              └───────────┘
```

### Session Management

Use sticky sessions or session storage:

```nginx
upstream nexus {
    ip_hash;  # Sticky sessions
    server 10.0.0.1:8000;
    server 10.0.0.2:8000;
    server 10.0.0.3:8000;
}
```

---

## Backup and Recovery

### Database Backup

Automated backups with cron:

```bash
# /etc/cron.daily/nexus-backup
#!/bin/bash
BACKUP_DIR="/backups/nexus"
DATE=$(date +%Y%m%d_%H%M%S)

pg_dump -U nexus_prod nexus_prod | gzip > "$BACKUP_DIR/nexus_$DATE.sql.gz"

# Keep last 30 days
find $BACKUP_DIR -name "nexus_*.sql.gz" -mtime +30 -delete
```

### Point-in-Time Recovery

Enable WAL archiving:

```ini
# postgresql.conf
archive_mode = on
archive_command = 'cp %p /wal_archive/%f'
```

### Recovery Procedure

```bash
# Stop application
systemctl stop nexus

# Restore from backup
gunzip -c /backups/nexus/nexus_20251223.sql.gz | psql -U nexus_prod nexus_prod

# Or use PITR
cp /wal_archive/* /var/lib/postgresql/wal/
```

---

## Related Documentation

- [Docker Deployment](docker.md) - Container deployment
- [ARCHITECTURE.md](../../ARCHITECTURE.md) - System architecture
- [INSTALLATION.md](../../INSTALLATION.md) - Installation guide

---

**Last Updated:** 2025-12-23
