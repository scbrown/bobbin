# Task: Add MCP `context` Tool

## Summary

Expose `bobbin context` as an MCP tool for AI agent integration.

## Files

- `src/mcp/tools.rs` (modify)
- `src/mcp/server.rs` (modify)

## Implementation

### `src/mcp/tools.rs`

Add request/response types following the existing `search` tool pattern:

```rust
pub struct ContextRequest {
    pub query: String,
    pub budget: Option<usize>,        // default 500
    pub depth: Option<u32>,            // default 1
    pub max_coupled: Option<usize>,    // default 3
    pub limit: Option<usize>,          // default 20
    pub coupling_threshold: Option<f32>, // default 0.1
    pub repo: Option<String>,
}

pub struct ContextResponse {
    pub query: String,
    pub budget: BudgetInfo,
    pub files: Vec<ContextFileOutput>,
    pub summary: ContextSummaryOutput,
}

// Include full content by default for MCP (agents need the code)
```

### `src/mcp/server.rs`

Add `context` tool to the server following the `search` tool pattern:

1. Register tool in `list_tools()` with schema describing all parameters
2. Add match arm in `call_tool()` for "context"
3. Parse `ContextRequest` from arguments
4. Create `ContextAssembler` with config (always `ContentMode::Full` for MCP)
5. Call `assemble()` and convert to `ContextResponse`
6. Return as `CallToolResult` with JSON content

Tool description for MCP:
```
"context" - Assemble a comprehensive context bundle for a task. Given a natural language
task description, combines semantic search results with temporally coupled files from git
history. Returns a deduplicated, budget-aware set of relevant code chunks grouped by file.
Ideal for understanding everything relevant to a task before making changes.
```

## Dependencies

- Requires Task 2 (context assembler) and Task 3 (CLI - for shared types)

## Tests

- Verify tool appears in `list_tools()` response
- Verify tool schema matches expected parameters

## Acceptance Criteria

- [ ] `context` tool appears in MCP tool list
- [ ] Tool returns structured JSON with file groups and chunks
- [ ] Content is always full (not truncated) in MCP mode
- [ ] All optional parameters work with sensible defaults
- [ ] Error handling follows existing MCP patterns (McpError)
