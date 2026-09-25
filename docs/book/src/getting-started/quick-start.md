---
title: Quick Start
description: A guided walkthrough of bobbin's core features using your own repository
tags: [tutorial, quickstart]
status: published
category: getting-started
related: [getting-started/concepts.md, cli/init.md, cli/search.md]
---

# Quick Start

First [install Bobbin](installation.md), then verify a disposable project.
You need Git and Python 3 for this example. Initial indexing downloads the model.

## A reproducible first result

```bash
mkdir -p bobbin-demo && cd bobbin-demo && git init -q && printf 'def greeting(name):\n    return "Hello, " + name\n' > greeting.py
bobbin init --quiet && BOBBIN_GPU=0 bobbin index --quiet --skip-calibrate
bobbin grep greeting --json | python3 -c 'import json,sys; r=json.load(sys.stdin)["results"][0]; print("{}:{}-{}".format(r["name"], r["start_line"], r["end_line"]))'
```

The final command prints:

```text
greeting:1-2
```

Bobbin found a parsed function, with its source range, in the index you just built.

## Use your own repository

### 1. Initialize

Navigate to your project and initialize bobbin:

```bash
cd your-project
bobbin init
```

This creates a `.bobbin/` directory containing configuration (`config.toml`), a SQLite database for coupling data, and a LanceDB vector store.

### 2. Index

Build the search index:

```bash
bobbin index
```

Bobbin walks your repository (respecting `.gitignore`), parses source files with tree-sitter into semantic chunks (functions, classes, structs, etc.), generates 384-dimensional embeddings using a local ONNX model, and stores everything in LanceDB.

Indexing time varies with your repository, model and hardware.

### 3. Search

Find code by meaning:

```bash
bobbin search "error handling"
```

This runs a hybrid search combining semantic similarity (vector search) with keyword matching (full-text search), fused via Reciprocal Rank Fusion. Results show the file, function name, line range, and a content preview.

### 4. Explore More

Try these commands to see what bobbin can do:

```bash
# Keyword/regex search
bobbin grep "TODO"

# Task-aware context assembly
bobbin context "fix the login bug"

# Find files that change together
bobbin related src/main.rs

# Find symbol definitions and usages
bobbin refs parse_config

# Identify high-churn, high-complexity files
bobbin hotspots

# Check index statistics
bobbin status
```

### 5. Interactive Tour

For a guided, interactive walkthrough of every feature, run:

```bash
bobbin tour
```

The tour runs each command against your actual repository, explaining what it does and how to use it. You can also tour a specific feature:

```bash
bobbin tour search
bobbin tour hooks
```

## Next Steps

- [Core Concepts](concepts.md) — understand chunks, embeddings, hybrid search, and coupling
- [Agent Setup](agent-setup.md) — connect bobbin to an AI coding assistant via MCP
- [Configuration](../config/reference.md) — tune index patterns, search weights, and embedding settings
