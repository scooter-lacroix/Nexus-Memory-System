# Troubleshooting Guide

> **Common Issues and Solutions**

**Version:** 1.1.0

---

## Table of Contents

- [Installation Issues](#installation-issues)
- [Database Issues](#database-issues)
- [Hooks Issues](#hooks-issues)
- [Embeddings Issues](#embeddings-issues)
- [Performance Issues](#performance-issues)
- [Getting Help](#getting-help)

---

## Installation Issues

### Issue: Module not found

**Symptoms:**
```
ModuleNotFoundError: No module named 'nexus'
```

**Solutions:**

```bash
# Verify installation
pip show nexus-memory-system

# Reinstall
pip install --force-reinstall nexus-memory-system[embeddings]

# Check Python path
python -c "import sys; print(sys.path)"
```

### Issue: Permission denied

**Symptoms:**
```
Permission denied: '/usr/local/lib/python3.x/site-packages'
```

**Solutions:**

```bash
# Use user install
pip install --user nexus-memory-system[embeddings]

# Or use virtual environment
python -m venv venv
source venv/bin/activate
pip install nexus-memory-system[embeddings]
```

### Issue: Dependencies conflict

**Symptoms:**
```
ERROR: pip's dependency resolver does not currently take into account...
```

**Solutions:**

```bash
# Use uv for better dependency resolution
pip install uv
uv pip install nexus-memory-system[embeddings]

# Or create clean virtual environment
python -m venv nexus-env --clear
source nexus-env/bin/activate
pip install nexus-memory-system[embeddings]
```

---

## Database Issues

### Issue: Database initialization fails

**Symptoms:**
```
Error: Database initialization failed
```

**Solutions:**

```bash
# Reset database
nexus init --reset

# Check permissions
ls -la ~/.nexus-memory-system/

# Create directory manually
mkdir -p ~/.nexus-memory-system
chmod 755 ~/.nexus-memory-system

# Check disk space
df -h
```

### Issue: Database locked

**Symptoms:**
```
sqlite3.OperationalError: database is locked
```

**Solutions:**

```bash
# Check for running processes
ps aux | grep nexus

# Kill any hanging processes
killall -9 nexus

# Remove lock file
rm ~/.nexus-memory-system/nexus.db-wal
rm ~/.nexus-memory-system/nexus.db-shm

# Restart application
nexus serve --transport web
```

### Issue: PostgreSQL connection fails

**Symptoms:**
```
psycopg2.OperationalError: could not connect to server
```

**Solutions:**

```bash
# Check PostgreSQL is running
sudo systemctl status postgresql

# Check connection
psql -U nexus_user -d nexus_memory -h localhost

# Verify credentials
cat ~/.pgpass

# Check firewall
sudo ufw status

# Check PostgreSQL config
sudo cat /etc/postgresql/*/main/postgresql.conf | grep listen_addresses
```

---

## Hooks Issues

### Issue: Hooks not triggering

**Symptoms:**
Session ends but no memories extracted.

**Solutions:**

```bash
# Check hooks status
nexus hooks status --verbose

# Verify hook files exist
ls -la ~/.claude/skills/nexus-memory/
ls -la ~/.gemini/extensions/nexus-memory.json

# Reinstall hooks
nexus hooks uninstall claude-code
nexus hooks install claude-code

# Check monitoring
nexus hooks start
```

### Issue: Hook installation fails

**Symptoms:**
```
Error: Failed to install hooks for claude-code
```

**Solutions:**

```bash
# Check agent directories exist
mkdir -p ~/.claude/skills/
mkdir -p ~/.gemini/extensions/

# Check permissions
ls -la ~/.claude/
ls -la ~/.gemini/

# Manual installation
# See HOOKS.md for manual installation steps
```

### Issue: Buffer not recovering

**Symptoms:**
Crash but buffer doesn't restore.

**Solutions:**

```bash
# Check buffer directory
ls -la ~/.nexus/buffer/

# Check buffer files
cat ~/.nexus/buffer/*.json

# Reset buffer
rm -rf ~/.nexus/buffer/*
nexus hooks install --all

# Manual extraction
nexus hooks extract --all
```

---

## Embeddings Issues

### Issue: Model download fails

**Symptoms:**
```
OSError: Can't load tokenizer for 'all-MiniLM-L6-v2'
```

**Solutions:**

```bash
# Check internet connection
ping huggingface.co

# Set cache directory
export TRANSFORMERS_CACHE=~/.cache/huggingface
export HF_HOME=~/.cache/huggingface

# Create cache directory
mkdir -p ~/.cache/huggingface

# Pre-download model
python -c "from sentence_transformers import SentenceTransformer; SentenceTransformer('all-MiniLM-L6-v2')"
```

### Issue: Out of memory during embedding

**Symptoms:**
```
RuntimeError: CUDA out of memory
```

**Solutions:**

```bash
# Use CPU instead
export NEXUS_EMBEDDING_DEVICE=cpu

# Reduce batch size
export NEXUS_EMBEDDING_BATCH_SIZE=16

# Clear cache
pip cache purge

# Use smaller model
export NEXUS_EMBEDDING_MODEL=paraphrase-MiniLM-L3-v2
```

### Issue: Slow embedding generation

**Symptoms:**
Embedding takes >1 second per document.

**Solutions:**

```bash
# Use GPU if available
export NEXUS_EMBEDDING_DEVICE=cuda

# Increase batch size
export NEXUS_EMBEDDING_BATCH_SIZE=64

# Use faster model
export NEXUS_EMBEDDING_MODEL=all-MiniLM-L6-v2
```

---

## Performance Issues

### Issue: Slow search queries

**Symptoms:**
Search takes >1 second.

**Solutions:**

```bash
# Check database size
du -sh ~/.nexus-memory-system/nexus.db

# Vacuum database
sqlite3 ~/.nexus-memory-system/nexus.db "VACUUM;"

# Analyze tables
sqlite3 ~/.nexus-memory-system/nexus.db "ANALYZE;"

# Check indexes
sqlite3 ~/.nexus-memory-system/nexus.db ".indexes"

# Reduce search limit
nexus search "query" --limit 5
```

### Issue: High memory usage

**Symptoms:**
Process using >2GB RAM.

**Solutions:**

```bash
# Check memory usage
ps aux | grep nexus

# Reduce batch size
export NEXUS_EMBEDDING_BATCH_SIZE=16

# Disable caching
export NEXUS_CACHE_ENABLED=false

# Archive old memories
nexus archive --before 2024-01-01
```

### Issue: Web dashboard slow

**Symptoms:**
Dashboard takes >5 seconds to load.

**Solutions:**

```bash
# Check server status
curl http://localhost:8000/health

# Check logs
tail -f /var/log/nexus/nexus.log

# Restart server
systemctl restart nexus

# Check network
ping localhost
traceroute localhost
```

---

## Getting Help

### Diagnostic Information

Collect before asking for help:

```bash
# System info
nexus status

# Version info
nexus --version

# Python version
python --version

# Installed packages
pip list | grep nexus

# Database info
sqlite3 ~/.nexus-memory-system/nexus.db ".databases"
sqlite3 ~/.nexus-memory-system/nexus.db ".tables"

# Hooks status
nexus hooks status --verbose

# Logs
journalctl -u nexus -n 100
```

### Log Files

Check log files:

```bash
# Application logs
tail -f ~/.nexus-memory-system/logs/nexus.log

# System logs (if using systemd)
journalctl -u nexus -f

# Docker logs
docker-compose logs -f nexus
```

### Debug Mode

Enable debug logging:

```bash
# CLI debug
nexus --verbose status

# Server debug
nexus serve --transport web --debug

# Environment variable
export NEXUS_LOG_LEVEL=DEBUG
```

### Where to Get Help

- **Documentation:** Check [docs/](docs/)
- **Issues:** https://github.com/scooter-lacroix/nexus-memory-system/issues
- **Architecture:** [ARCHITECTURE.md](ARCHITECTURE.md)
- **Installation:** [INSTALLATION.md](INSTALLATION.md)

### Creating an Issue

When creating an issue, include:

1. **Nexus version:** `nexus --version`
2. **Python version:** `python --version`
3. **OS:** `uname -a`
4. **Error message:** Full error traceback
5. **Steps to reproduce:** What you did before the error
6. **Expected behavior:** What you expected to happen
7. **Actual behavior:** What actually happened
8. **Diagnostic info:** Output of diagnostic commands above

---

**Last Updated:** 2025-12-23
