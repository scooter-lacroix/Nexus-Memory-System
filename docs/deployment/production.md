# Production Deployment

For production-style deployments, treat Nexus as a Rust service with a persistent database path, explicit supervision, and a clear boundary between user-facing transports and the optional cognition runtime.

## Checklist

- build `nexus-memory` in release mode
- install or deploy the exact release binary you built
- set a stable `NEXUS_DATABASE_PATH`
- run `nexus init` during provisioning
- expose `nexus serve --transport web`
- monitor process health and database disk usage

## Example Startup

```bash
export NEXUS_DATABASE_PATH=/var/lib/nexus/nexus.db
nexus init
nexus serve --transport web --port 8768
```
