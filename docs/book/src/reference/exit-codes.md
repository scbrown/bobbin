---
title: Exit Codes
description: Bobbin CLI exit codes and their meanings
tags: [reference, exit-codes]
status: draft
category: reference
related: [cli/overview.md]
---

# Exit Codes

Bobbin uses standard Unix exit codes. All commands follow the same convention.

## Exit Code Table

| Code | Meaning | Common Causes |
|------|---------|---------------|
| `0` | Success | Command completed normally |
| `1` | General error | Invalid arguments, missing configuration, runtime errors |
| `2` | Usage error | Invalid command syntax (from clap argument parser) |

## Common Error Scenarios

### Not Initialized (exit 1)

```text
Error: Bobbin not initialized in /path/to/project. Run `bobbin init` first.
```

Occurs when running any command that requires an index (`search`, `grep`, `context`, `status`, `serve`, etc.) before running `bobbin init`.

### No Indexed Content (exit 0, empty results)

Commands like `search` and `grep` return exit code 0 with zero results if the index exists but is empty. Run `bobbin index` to populate it.

### Invalid Arguments (exit 2)

```text
error: unexpected argument '--foo' found
```

The clap argument parser returns exit code 2 for unrecognized flags, missing required arguments, or invalid argument values.

### File Not Found (exit 1)

```text
Error: File not found in index: src/nonexistent.rs
```

Occurs when `related` or `read_chunk` references a file that isn't in the index.

### Invalid Search Mode (exit 1)

```text
Error: Invalid search mode: 'fuzzy'. Use 'hybrid', 'semantic', or 'keyword'
```

## Using Exit Codes in Scripts

```bash
# Check if bobbin is initialized
if bobbin status --quiet 2>/dev/null; then
    echo "Index ready"
else
    bobbin init && bobbin index
fi

# Search with error handling
if ! bobbin search "auth" --json > results.json; then
    echo "Search failed" >&2
    exit 1
fi
```

## JSON Error Output

When using `--json` mode, errors are still printed to stderr as plain text. Only successful results are written to stdout as JSON.
