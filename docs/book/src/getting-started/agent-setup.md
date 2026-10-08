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

Register bobbin by **absolute path**. MCP clients launch servers without your
shell's `PATH`, so a bare `bobbin` fails with `ENOENT`. From the indexed repository:

```bash
claude mcp add bobbin -- "$(command -v bobbin)" serve "$PWD"
```

That registers it for this project only (Claude Code's default *local* scope).
Add `--scope project` to write a shared `.mcp.json` at the repository root, or
`--scope user` to make it available in every project. The equivalent
`.mcp.json` is:

```json
{
  "mcpServers": {
    "bobbin": {
      "command": "/absolute/path/to/bobbin",
      "args": ["serve", "/path/to/repo"]
    }
  }
}
```

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
claude mcp add bobbin -- "$(command -v bobbin)" serve "$PWD"
```

## Cursor

Add bobbin as an MCP server in Cursor's settings:

**`.cursor/mcp.json`**:

```json
{
  "mcpServers": {
    "bobbin": {
      "command": "/absolute/path/to/bobbin",
      "args": ["serve", "/path/to/repo"]
    }
  }
}
```

## Other MCP Clients

Any MCP-compatible client can connect to bobbin. The server communicates via stdio by default:

```bash
bobbin serve            # MCP server on stdio
bobbin serve --http   # HTTP REST API instead
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
