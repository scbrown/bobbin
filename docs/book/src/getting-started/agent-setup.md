---
title: Agent Setup
description: Configuring bobbin with Claude Code, Cursor, and other AI coding tools
tags: [setup, mcp, claude-code, cursor]
status: draft
category: getting-started
related: [mcp/overview.md, mcp/client-config.md, cli/serve.md]
---

# Agent Setup

Bobbin integrates with AI coding assistants through the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/). This gives your AI agent semantic search, code coupling analysis, and context assembly capabilities over your codebase.

## Claude Code

### Option 1: MCP Server (Recommended)

Add bobbin as an MCP server in your Claude Code configuration:

**Project-level** (`.claude/settings.json`):

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

**Global** (`~/.claude/settings.json`): Same format, applies to all projects.

Once configured, Claude Code can use bobbin's tools (`search`, `grep`, `context`, `related`, `find_refs`, `list_symbols`, `read_chunk`, `hotspots`, `prime`) directly in conversation.

### Option 2: Hook Integration

For automatic context injection on every prompt (no manual tool calls needed):

```bash
bobbin hook install
```

This registers hooks in Claude Code's `settings.json` that:

1. **On every prompt** (`UserPromptSubmit`): Search your codebase for code relevant to the prompt and inject it as context.
2. **After compaction** (`SessionStart`): Restore codebase awareness when context is compressed.

You can also install the git hook for automatic re-indexing:

```bash
bobbin hook install-git-hook
```

See [hook CLI reference](../cli/hook.md) for configuration options (`--threshold`, `--budget`, `--global`).

### Both Together

MCP server and hooks complement each other:

- **Hooks** provide passive, automatic context on every prompt
- **MCP tools** let the agent actively search, explore, and analyze code

```bash
# Set up both
bobbin hook install
# Add MCP server to .claude/settings.json (see above)
```

## Cursor

Add bobbin as an MCP server in Cursor's settings:

**`.cursor/mcp.json`**:

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

## Other MCP Clients

Any MCP-compatible client can connect to bobbin. The server communicates via stdio by default:

```bash
bobbin serve            # MCP server on stdio
bobbin serve --server   # HTTP REST API instead
```

For remote or shared deployments, see [HTTP Mode](../mcp/http-mode.md).

## Verifying the Connection

Once configured, your AI agent should have access to bobbin's tools. Test by asking it to:

- "Search for error handling code" (uses `search` tool)
- "What files are related to `src/main.rs`?" (uses `related` tool)
- "Find the definition of `parse_config`" (uses `find_refs` tool)

## Prerequisites

Before connecting an agent, make sure your repository is initialized and indexed:

```bash
bobbin init
bobbin index
```

## Next Steps

- [MCP Overview](../mcp/overview.md) — how the MCP integration works
- [MCP Tools Reference](../mcp/tools.md) — all available tools and their parameters
- [Client Configuration](../mcp/client-config.md) — detailed configuration for each client
