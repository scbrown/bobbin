# Bobbin Hooks Integration Plan

**Bead**: bobbin-62g
**Status**: Planning
**Priority**: P1

## Context

Bobbin has a working MCP server and JSON CLI output, but no automatic
integration with Claude Code or other AI tools. Users must manually configure
MCP servers and there's no automatic context injection. This plan adds a
`bobbin hook` subcommand family that makes bobbin a drop-in context provider
for Claude Code — automatic context injection on every prompt, working context
recovery after compaction, and index freshness via git commit hooks.

## Design Principles

- **Config pattern**: TOML defaults in `.bobbin/config.toml`, CLI flags override.
  Every tunable follows this pattern. No magic numbers in code.
- **Conservative injection**: Only inject context above a confidence threshold.
  Irrelevant context wastes tokens and confuses the model.
- **Built-in subcommands**: Hook handlers are `bobbin hook <cmd>`, not external
  shell scripts. One binary, no permissions issues, fast startup.
- **No PostToolUse**: Index updates happen on git commit, not on every file write.
  This avoids constant reindexing churn during active editing sessions.

## Configuration

### `.bobbin/config.toml`

```toml
[hooks]
threshold = 0.5           # Min relevance score to include in injected context
budget = 150              # Max lines of injected context
content_mode = "preview"  # full | preview | none
min_prompt_length = 10    # Skip injection for very short prompts
```

All values have CLI flag equivalents (`--threshold`, `--budget`,
`--content-mode`, `--min-prompt-length`). CLI flags override TOML values.

### Implementation in `src/config.rs`

Add `HooksConfig` struct with serde defaults:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct HooksConfig {
    #[serde(default = "default_threshold")]
    pub threshold: f32,           // 0.5
    #[serde(default = "default_budget")]
    pub budget: usize,            // 150
    #[serde(default = "default_content_mode")]
    pub content_mode: String,     // "preview"
    #[serde(default = "default_min_prompt_length")]
    pub min_prompt_length: usize, // 10
}
```

Wire into the existing `Config` struct alongside `SearchConfig`, `GitConfig`, etc.

---

## CLI Surface

### User-facing commands

```
bobbin hook install [--global] [--threshold <f>] [--budget <n>]
    Install Claude Code hooks into .claude/settings.json (project) or
    ~/.claude/settings.json (--global). Merges with existing hooks config.
    Idempotent — safe to re-run.

bobbin hook uninstall [--global]
    Remove bobbin hooks from Claude Code settings. Leaves other hooks intact.

bobbin hook status
    Show what's installed: which hooks are configured, where, and current
    config values (merged TOML + flag overrides).

bobbin hook install-git-hook
    Install a post-commit git hook that runs `bobbin index` in the background.
    Creates .git/hooks/post-commit (or appends to existing).

bobbin hook uninstall-git-hook
    Remove the bobbin post-commit hook.
```

### Internal commands (called by Claude Code, not users)

```
bobbin hook inject-context [--threshold <f>] [--budget <n>] [--content-mode <m>]
    Handles UserPromptSubmit events. Reads hook JSON from stdin, queries
    bobbin for relevant context, outputs Claude Code hook response JSON.

bobbin hook session-context [--budget <n>]
    Handles SessionStart (compact) events. Reads hook JSON from stdin,
    reconstructs working context from git state + bobbin, outputs response.
```

---

## Hook Implementations

### `inject-context` (UserPromptSubmit handler)

**Input**: Claude Code hook JSON on stdin:
```json
{
  "prompt": "fix the auth bug in the login flow",
  "cwd": "/home/user/project",
  "session_id": "abc123"
}
```

**Algorithm**:
1. Parse stdin JSON, extract `prompt` and `cwd`
2. If `prompt.len() < config.min_prompt_length`, exit 0 (no injection)
3. Run bobbin context query: same logic as `bobbin context` CLI but using
   the hooks config (threshold, budget, content_mode)
4. Filter results: drop any chunks with score below `config.threshold`
5. If no results survive filtering, exit 0 (no injection)
6. Format results as compact markdown:
   ```
   ## Relevant Code Context (via bobbin)

   **src/auth.rs** (score: 0.87)
   - `validate_token` (fn, lines 42-68)
   - `refresh_session` (fn, lines 70-95)

   **src/middleware.rs** (score: 0.72, coupled via src/auth.rs)
   - `auth_middleware` (fn, lines 15-40)
   ```
7. Output hook response JSON:
   ```json
   {
     "hookSpecificOutput": {
       "hookEventName": "UserPromptSubmit",
       "additionalContext": "<formatted markdown>"
     }
   }
   ```

**Error handling**: Any failure → exit 0 (never block the user's prompt).
Log errors to stderr for `--verbose` diagnostics.

### `session-context` (SessionStart compact handler)

**Input**: Claude Code hook JSON on stdin:
```json
{
  "source": "compact",
  "cwd": "/home/user/project",
  "session_id": "abc123"
}
```

**Algorithm**:
1. Parse stdin JSON, check `source == "compact"`. If not compact, exit 0.
2. Gather durable signals from the working directory:
   a. `git status --porcelain` → list of modified/staged/untracked files
   b. `git log --oneline -5` → recent commit summaries
   c. `git diff --name-only HEAD~3..HEAD` → recently changed files (broader net)
3. For each modified/recent file, query bobbin for:
   - File's symbols (via existing `list_symbols` logic)
   - Coupled files (via `get_coupling`)
4. Assemble compact context block:
   ```
   ## Working Context (recovered after compaction)

   ### Modified files
   - src/auth.rs (3 functions: validate_token, refresh_session, logout)
   - src/middleware.rs (1 function: auth_middleware)

   ### Recent commits
   - a1b2c3d fix: token refresh race condition
   - d4e5f6g feat: add logout endpoint

   ### Related files (via coupling)
   - tests/auth_test.rs (coupled with src/auth.rs, score: 0.91)
   - src/config.rs (coupled with src/auth.rs, score: 0.65)
   ```
5. Respect budget config — truncate if assembled context exceeds budget lines
6. Output as `additionalContext` hook response

**Error handling**: Same as inject-context — never block, fail silently.

### `install-git-hook` (post-commit index update)

Creates or appends to `.git/hooks/post-commit`:

```bash
#!/bin/sh
# bobbin: update search index after commit
if command -v bobbin >/dev/null 2>&1; then
  bobbin index --quiet &
fi
```

- Checks for existing post-commit hook, appends rather than overwrites
- Marks the bobbin section with comments for clean uninstall
- Makes the hook executable

### `install` (Claude Code hooks setup)

Reads or creates `.claude/settings.json` (or global equivalent), merges in:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "bobbin hook inject-context",
            "timeout": 10,
            "statusMessage": "Loading code context..."
          }
        ]
      }
    ],
    "SessionStart": [
      {
        "matcher": "compact",
        "hooks": [
          {
            "type": "command",
            "command": "bobbin hook session-context",
            "timeout": 10,
            "statusMessage": "Recovering project context..."
          }
        ]
      }
    ]
  }
}
```

Uses JSON merge (not overwrite) to preserve existing hooks.

---

## File Structure

### New files

```
src/cli/hook.rs              # Hook subcommand dispatcher + install/uninstall/status
src/hook/mod.rs              # Hook module root
src/hook/inject_context.rs   # UserPromptSubmit handler
src/hook/session_context.rs  # SessionStart compact handler
src/hook/installer.rs        # Claude Code settings.json + git hook management
src/hook/format.rs           # Compact markdown formatter for injected context
```

### Modified files

```
src/cli/mod.rs               # Add Hook subcommand to CLI enum
src/config.rs                # Add HooksConfig struct
Cargo.toml                   # No new deps expected (serde_json already available)
```

---

## Bead Breakdown

### bobbin-62g-1: HooksConfig + CLI scaffolding
- Add `[hooks]` section to config.toml parsing
- Add `bobbin hook` subcommand with install/uninstall/status/inject-context/session-context stubs
- Wire config defaults + CLI flag overrides
- **Acceptance**: `bobbin hook status` runs and shows config values

### bobbin-62g-2: inject-context implementation
- Read stdin JSON (UserPromptSubmit format)
- Query bobbin context with hooks config
- Filter by threshold
- Format as compact markdown
- Output hook response JSON
- **Acceptance**: `echo '{"prompt":"fix auth bug","cwd":"."}' | bobbin hook inject-context` returns valid hook JSON with relevant context

### bobbin-62g-3: session-context implementation
- Read stdin JSON (SessionStart format)
- Gather git status + recent commits
- Query bobbin for symbols and coupling on modified files
- Format recovery context block
- **Acceptance**: `echo '{"source":"compact","cwd":"."}' | bobbin hook session-context` returns working context

### bobbin-62g-4: hook installer (Claude Code + git)
- `bobbin hook install` — merge hooks into .claude/settings.json
- `bobbin hook uninstall` — remove bobbin hooks cleanly
- `bobbin hook install-git-hook` — post-commit hook for index updates
- `bobbin hook uninstall-git-hook` — clean removal
- **Acceptance**: `bobbin hook install && bobbin hook status` shows installed hooks. `bobbin hook uninstall` removes them. Git hook triggers `bobbin index` on commit.

### bobbin-62g-5: compact formatter + polish
- Refine the markdown output format for injected context
- Ensure budget enforcement (truncation when over limit)
- Add `--quiet` mode for zero output on no results
- Error handling: never block user prompts
- **Acceptance**: Integration test — full round-trip from hook JSON input to context output with budget enforcement

---

## Dependency Order

```
bobbin-62g-1 (config + scaffolding)
    ↓
bobbin-62g-2 (inject-context)  ←→  bobbin-62g-3 (session-context)
    ↓                                    ↓
              bobbin-62g-4 (installer)
                    ↓
              bobbin-62g-5 (formatter + polish)
```

1 must come first. 2 and 3 can be parallel. 4 needs 2+3. 5 is polish.

---

## Verification

End-to-end test:
1. `bobbin hook install` → .claude/settings.json has hooks
2. `echo '{"prompt":"search auth","cwd":"."}' | bobbin hook inject-context` → returns context JSON
3. `echo '{"source":"compact","cwd":"."}' | bobbin hook session-context` → returns recovery JSON
4. `bobbin hook install-git-hook` → .git/hooks/post-commit exists
5. Make a commit → bobbin index runs in background
6. `bobbin hook uninstall` → hooks removed from settings
7. `bobbin hook uninstall-git-hook` → git hook cleaned up
8. `bobbin hook status` → shows "no hooks installed"
