---
title: "Client Configuration"
description: "Configuring bobbin MCP with Claude Code, Cursor, and other clients"
tags: [mcp, claude-code, cursor, configuration]
category: mcp
---

# Client Configuration

Step-by-step MCP configuration for each supported AI coding client.

## Claude Code

### MCP Server

Add to `.claude/settings.json` (project-level) or `~/.claude/settings.json` (global):

```json
{
  "mcpServers": {
    "bobbin": {
      "command": "bobbin",
      "args": ["serve"],
      "env": {}
    }
  }
}
```

Claude Code will launch the bobbin MCP server automatically when you start a session. The agent can then call tools like `search`, `context`, and `find_refs` directly.

### Hook Integration

For automatic context injection without manual tool calls:

```bash
# Project-local hooks
bobbin hook install

# Global hooks (all projects)
bobbin hook install --global

# With custom settings
bobbin hook install --threshold 0.3 --budget 200
```

The hook system and MCP server work independently. You can use either or both.

### Verifying

Ask Claude Code: "What MCP tools do you have available?" It should list the bobbin tools (search, grep, context, etc.).

## Cursor

Add to `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "bobbin": {
      "command": "bobbin",
      "args": ["serve"]
    }
  }
}
```

Restart Cursor after adding the configuration. The MCP server will start automatically.

## Windsurf

Add to your Windsurf MCP configuration:

```json
{
  "mcpServers": {
    "bobbin": {
      "command": "bobbin",
      "args": ["serve"]
    }
  }
}
```

## Generic MCP Client

Any MCP-compatible client can connect to bobbin. The server uses stdio transport by default:

```bash
# The client launches this command and communicates via stdin/stdout
bobbin serve
```

The server advertises:

- **Protocol version**: `2024-11-05`
- **Server name**: `bobbin`
- **Capabilities**: tools, resources, prompts

## Remote Server

For shared or centralized deployments, use HTTP mode instead of stdio:

```bash
# Start HTTP server on port 3030
bobbin serve --server --port 3030
```

Then configure your client to use the `--server` flag for thin-client mode:

```bash
# CLI queries hit the remote server
bobbin search "auth" --server http://localhost:3030
```

See [HTTP Mode](http-mode.md) for details.

## Multi-Repository Setup

Bobbin indexes one repository at a time. For multi-repo setups, run a separate bobbin instance per repository. Each MCP server is scoped to its repository root.

## Troubleshooting

**"Bobbin not initialized"**: Run `bobbin init && bobbin index` in your project first.

**"No indexed content"**: Run `bobbin index` to build the search index.

**Tools not appearing**: Check that `bobbin` is on your PATH. Try running `bobbin serve` manually to verify it starts without errors.

**Slow first query**: The first query after startup loads the ONNX model (~1–2 seconds). Subsequent queries are fast.
