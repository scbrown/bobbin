---
title: "HTTP Mode"
description: "Running bobbin as an HTTP REST API server for centralized deployments"
tags: [mcp, http, server, thin-client]
category: mcp
---

# HTTP Mode

Bobbin can run as an HTTP REST API server for centralized deployments, shared team use, or webhook-driven indexing.

## Starting the HTTP Server

```bash
bobbin serve --server                  # HTTP on port 3030 (default)
bobbin serve --server --port 8080      # Custom port
bobbin serve --server --mcp            # HTTP + MCP stdio simultaneously
```

The server binds to `0.0.0.0` on the specified port and includes CORS headers for browser-based clients.

## REST API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/search` | Search the index |
| `GET` | `/chunk/{id}` | Read a specific chunk |
| `GET` | `/status` | Index statistics |
| `POST` | `/webhook/push` | Trigger re-indexing (for CI/CD) |

### GET /search

Query parameters:

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `q` | string | required | Search query |
| `mode` | string | `hybrid` | Search mode: `hybrid`, `semantic`, `keyword` |
| `type` | string | all | Filter by chunk type |
| `limit` | integer | 10 | Maximum results |
| `repo` | string | all | Filter by repository |

```bash
curl "http://localhost:3030/search?q=error+handling&limit=5"
```

### GET /status

Returns index statistics in JSON format.

```bash
curl http://localhost:3030/status
```

### POST /webhook/push

Triggers re-indexing. Useful for CI/CD pipelines or git webhook integrations.

```bash
curl -X POST http://localhost:3030/webhook/push
```

## Thin-Client Mode

When a remote bobbin server is running, CLI commands can delegate to it instead of accessing local storage:

```bash
bobbin search "auth" --server http://localhost:3030
bobbin status --server http://localhost:3030
```

The `--server` flag is a global option available on all commands. When set, the CLI acts as a thin client that forwards requests to the HTTP server.

## Use Cases

### Team Server

Run bobbin on a shared machine with a large codebase indexed once:

```bash
# On the server
bobbin init
bobbin index
bobbin serve --server --port 3030
```

Team members connect via `--server` flag or configure their AI client to point at the server.

### CI/CD Integration

Add a webhook step to your CI pipeline to keep the index fresh:

```bash
# After deploy or merge
curl -X POST http://bobbin-server:3030/webhook/push
```

### Combined Mode

Run both HTTP and MCP simultaneously for maximum flexibility:

```bash
bobbin serve --server --mcp
```

This starts the HTTP server on the configured port and the MCP stdio server concurrently. The MCP server handles AI client connections while the HTTP server handles REST API requests and webhooks.
