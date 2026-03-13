# Production Deployment

For production-style deployments, treat Nexus as a Rust service with a persistent database path and explicit process supervision.

## Checklist

- build `nexus-cli` in release mode
- set a stable `NEXUS_DATABASE_PATH`
- run `nexus init` during provisioning
- expose `nexus serve --transport http`
- monitor process health and database disk usage

## Example Startup

```bash
export NEXUS_DATABASE_PATH=/var/lib/nexus/nexus.db
nexus init
nexus serve --transport http --port 8768
```
