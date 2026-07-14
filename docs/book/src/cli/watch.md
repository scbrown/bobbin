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
| `--reindex-interval-secs <SECS>` | `900` | Periodic full-tree reindex backstop (see below). `0` disables |
| `--pid-file <FILE>` | | Write PID to this file for daemon management |
| `--generate-systemd` | | Print a systemd service unit to stdout and exit |

## Reindex backstop

File watchers are the fast path, but they are not a complete freshness
guarantee: a process restart, a high-churn burst, or a dropped inotify event
can leave content silently stale until the next manual reindex.

To close that gap, `watch` runs a periodic **reindex backstop** (on by default,
every 15 minutes). Each sweep reconciles the whole source tree against the
index — re-embedding any file whose content hash drifted and pruning rows for
files that have disappeared from disk. Sweeps are incremental: unchanged files
are skipped by hash, so a sweep where the watcher kept up does almost no work.

Tune the cadence with `--reindex-interval-secs`, or set it to `0` to disable the
backstop and rely on watcher events alone:

```bash
# Reconcile every 5 minutes
bobbin watch --reindex-interval-secs 300

# Disable the backstop
bobbin watch --reindex-interval-secs 0
```

`bobbin status` reports a **Freshness** signal — `stale` when the current git
HEAD is newer than the last index run — so drift is observable without waiting
for a search to miss.

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
