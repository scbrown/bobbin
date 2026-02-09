---
title: tour
description: Interactive guided walkthrough of bobbin features
tags: [cli, tour, getting-started]
status: draft
category: cli-reference
related: [getting-started/quick-start.md]
commands: [tour]
feature: tour
source_files: [src/cli/tour.rs]
---

# tour

Interactive guided walkthrough of bobbin features.

## Synopsis

```bash
bobbin tour [OPTIONS] [FEATURE]
```

## Description

The `tour` command runs an interactive walkthrough that demonstrates bobbin's features using your actual codebase. Each step shows a real command and its output, pausing for you to read before continuing.

Provide a feature name to run only that section of the tour.

## Arguments

| Argument | Description |
|----------|-------------|
| `FEATURE` | Run tour for a specific feature only (e.g., `search`, `hooks`) |

## Options

| Option | Default | Description |
|--------|---------|-------------|
| `--path <DIR>` | `.` | Directory to tour |
| `--non-interactive` | | Skip interactive pauses (run all steps continuously) |
| `--list` | | List available tour steps without running them |

## Examples

Run the full interactive tour:

```bash
bobbin tour
```

Tour a specific feature:

```bash
bobbin tour search
```

List available tour steps:

```bash
bobbin tour --list
```

Non-interactive mode (useful for CI or demos):

```bash
bobbin tour --non-interactive
```

## Prerequisites

Requires a bobbin index. Run `bobbin init` and `bobbin index` first.

## See Also

- [Quick Start](../getting-started/quick-start.md) — getting started guide
