# REST API

The web/API surface is provided by the `nexus-web` crate and mounted under `/api`, with a WebSocket endpoint at `/ws`.

## Routes

### Health

- `GET /api/health`

### Memories

- `GET /api/memories`
- `POST /api/memories`
- `GET /api/memories/:id`
- `DELETE /api/memories/:id`
- `POST /api/memories/search`

### Namespaces

- `GET /api/namespaces`
- `POST /api/namespaces`
- `GET /api/namespaces/:id`

### Statistics

- `GET /api/stats`
- `GET /api/stats/:agent`

### WebSocket

- `GET /ws`

## Starting the Server

```bash
nexus serve --transport web --port 8768
```

## Example Requests

### Health check

```bash
curl http://127.0.0.1:8768/api/health
```

### List memories

```bash
curl http://127.0.0.1:8768/api/memories
```

### Search memories

```bash
curl \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{"query":"release completed"}' \
  http://127.0.0.1:8768/api/memories/search
```
