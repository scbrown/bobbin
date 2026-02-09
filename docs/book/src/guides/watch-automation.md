---
title: "Watch & Automation"
description: "Continuous re-indexing with watch mode and CI integration patterns"
tags: [watch, automation, ci, guide]
commands: [watch]
status: draft
category: guide
---

# Watch & Automation

Bobbin's index is only useful if it's current. This guide covers three strategies for keeping your index up to date: filesystem watching for real-time updates, git hooks for commit-time indexing, and patterns for CI integration.

## Strategy 1: Watch mode

The `bobbin watch` command monitors your filesystem for changes and re-indexes automatically:

```bash
bobbin watch
```

This starts a long-running process that:

- Detects file creates, modifications, and deletions using OS-native notifications (inotify on Linux, FSEvents on macOS).
- `Debounces` rapid changes (default 500ms) to batch them efficiently.
- Re-parses, re-embeds, and upserts chunks for changed files.
- Removes chunks for deleted files.
- Skips files whose content hash hasn't changed.

Press `Ctrl+C` to stop cleanly.

### Tuning debounce

The debounce interval controls how long bobbin waits after a change before processing it. Lower values give faster feedback; higher values reduce CPU usage during bursts of changes:

```bash
# Fast feedback (200ms) — good for active development
bobbin watch --debounce-ms 200

# Slower batching (2000ms) — good for background daemon use
bobbin watch --debounce-ms 2000
```

The default of 500ms works well for most workflows.

### Multi-repo watching

Run separate watchers for each repository in a multi-repo setup:

```bash
# Watch the backend repo
bobbin watch --repo backend --source ~/projects/backend

# Watch the frontend repo (in another terminal)
bobbin watch --repo frontend --source ~/projects/frontend
```

See [Multi-Repo](multi-repo.md) for the full multi-repo indexing setup.

## Strategy 2: Git post-commit hook

If you don't need real-time updates, a post-commit hook re-indexes after each commit:

```bash
bobbin hook install-git-hook
```

This creates (or appends to) `.git/hooks/post-commit` with a call to `bobbin index`. Only files changed in the commit are re-indexed.

**Advantages over watch mode:**

- No background process to manage.
- Only runs when you commit, not on every file save.
- Works well in CI environments where filesystem watchers aren't practical.

**Disadvantages:**

- Index is stale between commits. Uncommitted changes aren't reflected.
- Adds a small delay to each commit.

Remove the hook when you no longer want it:

```bash
bobbin hook uninstall-git-hook
```

## Strategy 3: Explicit re-indexing

For maximum control, run `bobbin index` manually when you need it:

```bash
# Full re-index
bobbin index

# Incremental — only changed files
bobbin index --incremental

# Force re-index everything
bobbin index --force
```

Incremental mode checks file content hashes and skips unchanged files. It's fast for routine updates.

## Running watch as a background service

### Using systemd (Linux)

Bobbin can generate a systemd user service unit:

```bash
bobbin watch --generate-systemd > ~/.config/systemd/user/bobbin-watch.service
systemctl --user daemon-reload
systemctl --user enable --now bobbin-watch
```

The generated unit:

- Uses `Type=simple` with automatic restart on failure.
- Sets `RestartSec=5` and `RUST_LOG=info`.
- Includes the resolved working directory and any `--repo`/`--source` flags.

Check status:

```bash
systemctl --user status bobbin-watch
journalctl --user -u bobbin-watch -f
```

Stop or disable:

```bash
systemctl --user stop bobbin-watch
systemctl --user disable bobbin-watch
```

### Using a PID file

For non-systemd setups, use the `--pid-file` flag for daemon management:

```bash
bobbin watch --pid-file /tmp/bobbin-watch.pid &
```

Check if the watcher is running:

```bash
kill -0 $(cat /tmp/bobbin-watch.pid) 2>/dev/null && echo "running" || echo "stopped"
```

Stop it:

```bash
kill $(cat /tmp/bobbin-watch.pid)
```

### Using tmux or screen

A simple approach for development machines:

```bash
# Start in a detached tmux session
tmux new-session -d -s bobbin-watch 'bobbin watch'

# Attach to check on it
tmux attach -t bobbin-watch
```

## CI integration patterns

### Index as a CI step

Add bobbin indexing to your CI pipeline so the index is always fresh on the main branch:

```yaml
# GitHub Actions example
- name: Build bobbin index
  run: |
    bobbin init --if-needed
    bobbin index --incremental
```

### Hotspot checks in CI

Use hotspot analysis as a quality gate:

```bash
# Fail if any file exceeds a hotspot score of 0.8
HOTSPOTS=$(bobbin hotspots --json --threshold 0.8 | jq '.count')
if [ "$HOTSPOTS" -gt 0 ]; then
  echo "Warning: $HOTSPOTS files exceed hotspot threshold"
  bobbin hotspots --threshold 0.8
  exit 1
fi
```

### Pre-commit context check

Verify that changes to coupled files are coordinated:

```bash
# In a pre-commit or CI script
for file in $(git diff --name-only HEAD~1); do
  bobbin related "$file" --threshold 0.5
done
```

This surfaces strongly coupled files that the commit might have missed.

## Combining strategies

The strategies aren't mutually exclusive. A common setup:

1. **Watch mode** on your development machine for real-time updates during coding.
2. **Post-commit hook** as a safety net in case the watcher wasn't running.
3. **CI indexing** to maintain a canonical index on the main branch.

The incremental indexing in each strategy means redundant runs are cheap — bobbin skips files that haven't changed.

## Troubleshooting

### Watcher stops after a while

If the watcher exits silently, check the logs. Common causes:

- The `.bobbin/` directory was deleted or moved.
- Disk is full (LanceDB needs space for vector storage).
- inotify watch limit reached on Linux. Increase it:

```bash
echo 65536 | sudo tee /proc/sys/fs/inotify/max_user_watches
```

### Index seems stale

Verify the watcher is actually running and processing events:

```bash
bobbin status
```

The status output shows when the index was last updated. If it's older than expected, the watcher may have stopped or the debounce interval may be too high.

### High CPU during large changes

A bulk operation (branch switch, large merge) triggers many file events. The debounce interval batches these, but indexing the batch can still be CPU-intensive. This is temporary and normal. If it's disruptive, increase `--debounce-ms`.

## Next steps

- [Hooks](hooks.md) — automatic context injection into Claude Code sessions
- [Multi-Repo](multi-repo.md) — watching multiple repositories
- [`watch` CLI reference](../cli/watch.md) — full flag reference
- [`index` CLI reference](../cli/index.md) — manual indexing options
