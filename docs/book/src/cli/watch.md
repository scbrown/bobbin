---
title: watch
description: Watch for file changes and re-index continuously
tags: [cli, watch]
status: draft
category: cli-reference
related: [guides/watch-automation.md, cli/index.md]
commands: [watch]
feature: watch
source_files: [src/cli/watch.rs]
---

# watch

Watch for file changes and re-index continuously in the background.

## Synopsis

```bash
bobbin watch [OPTIONS] [PATH]
```

## Description

The `watch` command starts a long-running process that monitors the filesystem for changes and incrementally re-indexes modified files. It uses OS-native file notification (inotify on Linux, FSEvents on macOS) with configurable debouncing to batch rapid changes.

The watcher:

- **Creates/modifies** — re-parses, re-embeds, and upserts chunks for changed files.
- **Deletes** — removes chunks for deleted files from the vector store.
- **Skips** — `.git/` and `.bobbin/` directories are always excluded. Additional include/exclude patterns come from `bobbin.toml`.
- **Deduplicates** — files whose content hash hasn't changed are skipped.

The process responds to `Ctrl+C` and `SIGTERM` for clean shutdown.

## Arguments

| Argument | Default | Description |
|----------|---------|-------------|
| `[PATH]` | `.` | Directory containing `.bobbin/` config |

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `--repo <NAME>` | | Repository name for multi-repo indexing |
| `--source <DIR>` | same as PATH | Source directory to watch (if different from config dir) |
| `--debounce-ms <MS>` | `500` | Debounce interval in milliseconds |
| `--pid-file <FILE>` | | Write PID to this file for daemon management |
| `--generate-systemd` | | Print a systemd service unit to stdout and exit |

## Examples

Watch the current directory:

```bash
bobbin watch
```

Watch with a shorter debounce for fast feedback:

```bash
bobbin watch --debounce-ms 200
```

Multi-repo setup — watch a separate source directory:

```bash
bobbin watch --repo frontend --source ../frontend-app
```

Write a PID file for daemon management:

```bash
bobbin watch --pid-file /tmp/bobbin-watch.pid &
```

Generate a systemd user service:

```bash
bobbin watch --generate-systemd > ~/.config/systemd/user/bobbin-watch.service
systemctl --user enable --now bobbin-watch
```

## Systemd Integration

The `--generate-systemd` flag prints a ready-to-use systemd service unit. The generated unit:

- Uses `Type=simple` with automatic restart on failure.
- Sets `RestartSec=5` and `RUST_LOG=info`.
- Includes the resolved working directory and any `--repo`/`--source` flags.

## Prerequisites

Requires a bobbin index. Run `bobbin init` and `bobbin index` first to create the initial index, then use `watch` to keep it up to date.

## See Also

- [Watch & Automation Guide](../guides/watch-automation.md) — patterns for continuous indexing
- [index](index.md) — one-shot full index build
- [hook install-git-hook](hook.md#install-git-hook) — alternative: re-index on git commit
