---
title: "hook"
description: "Manage Claude Code hooks for automatic context injection"
category: cli-reference
tags: [cli, hook]
commands: [hook]
feature: hook
source_files: [src/cli/hook.rs]
---

# hook

Manage Claude Code hooks for automatic context injection.

## Synopsis

```bash
bobbin hook <SUBCOMMAND> [OPTIONS]
```

## Description

The `hook` command manages the integration between bobbin and [Claude Code](https://claude.com/claude-code). When installed, bobbin hooks fire on every user prompt to inject semantically relevant code context, giving Claude automatic awareness of your codebase.

The hook system has two layers:

- **Claude Code hooks** — entries in `settings.json` that call `bobbin hook inject-context` and `bobbin hook session-context` automatically.
- **Git hooks** — an optional post-commit hook that re-indexes changed files after each commit.

## Subcommands

### install

Install bobbin hooks into Claude Code's `settings.json`.

```bash
bobbin hook install [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--global` | Install to `~/.claude/settings.json` instead of project-local `<git-root>/.claude/settings.json` |
| `--threshold <SCORE>` | Minimum relevance score to include in injected context |
| `--budget <LINES>` | Maximum lines of injected context |

This registers two hook entries:

1. **UserPromptSubmit** — calls `bobbin hook inject-context` on every prompt, adding relevant code snippets.
2. **SessionStart** (compact matcher) — calls `bobbin hook session-context` after context compaction to restore codebase awareness.

### uninstall

Remove bobbin hooks from Claude Code settings.

```bash
bobbin hook uninstall [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--global` | Remove from global settings instead of project-local |

### status

Show installed hooks and current configuration values.

```bash
bobbin hook status [PATH]
```

| Argument | Default | Description |
|----------|---------|-------------|
| `[PATH]` | `.` | Directory to check |

Displays whether Claude Code hooks and the git hook are installed, along with the active configuration (threshold, budget, content mode, gate threshold, dedup settings).

### inject-context

Handle `UserPromptSubmit` events. This is called internally by Claude Code — you typically do not run it manually.

```bash
bobbin hook inject-context [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--threshold <SCORE>` | Minimum relevance score (overrides config) |
| `--budget <LINES>` | Maximum lines of context (overrides config) |
| `--content-mode <MODE>` | Display mode: `full`, `preview`, or `none` (overrides config) |
| `--min-prompt-length <N>` | Minimum prompt length to trigger injection (overrides config) |
| `--gate-threshold <SCORE>` | Minimum raw semantic similarity to inject at all (overrides config) |
| `--no-dedup` | Force injection even if results match the previous session |

### session-context

Handle `SessionStart` compact events. Called internally by Claude Code after context compaction.

```bash
bobbin hook session-context [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--budget <LINES>` | Maximum lines of context (overrides config) |

### install-git-hook

Install a post-commit git hook that runs `bobbin index` after each commit.

```bash
bobbin hook install-git-hook
```

Creates or appends to `.git/hooks/post-commit`. The hook re-indexes only files changed in the commit.

### uninstall-git-hook

Remove the bobbin post-commit git hook.

```bash
bobbin hook uninstall-git-hook
```

### hot-topics

Generate `hot-topics.md` from injection frequency data. Analyzes which code areas are most frequently injected as context and writes a summary.

```bash
bobbin hook hot-topics [OPTIONS] [PATH]
```

| Argument/Option | Default | Description |
|-----------------|---------|-------------|
| `[PATH]` | `.` | Directory to operate on |
| `--force` | | Regenerate even if the injection count hasn't reached the threshold |

## Examples

Set up hooks for a project:

```bash
bobbin hook install
```

Set up globally with custom thresholds:

```bash
bobbin hook install --global --threshold 0.3 --budget 200
```

Check current hook status:

```bash
bobbin hook status
```

Also install the git hook for automatic post-commit indexing:

```bash
bobbin hook install-git-hook
```

Remove all hooks:

```bash
bobbin hook uninstall
bobbin hook uninstall-git-hook
```

JSON output for status:

```bash
bobbin hook status --json
```

## JSON Output (status)

```json
{
  "hooks_installed": true,
  "git_hook_installed": true,
  "config": {
    "threshold": 0.2,
    "budget": 150,
    "content_mode": "preview",
    "min_prompt_length": 20,
    "gate_threshold": 0.15,
    "dedup_enabled": true
  }
}
```

## Prerequisites

Requires a bobbin index. Run `bobbin init` and `bobbin index` first.

## See Also

- [Hooks Guide](../guides/hooks.md) — detailed setup and tuning guide
- [Hooks Configuration](../config/hooks.md) — all configurable hook parameters
- [watch](watch.md) — alternative continuous indexing via filesystem watcher
