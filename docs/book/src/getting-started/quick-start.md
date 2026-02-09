---
title: "Quick Start"
description: "A guided walkthrough of bobbin's core features using your own repository"
tags: [tutorial, quickstart]
category: getting-started
---

# Quick Start

Get bobbin running on your codebase in under two minutes.

## 1. Initialize

Navigate to your project and initialize bobbin:

```bash
cd your-project
bobbin init
```

This creates a `.bobbin/` directory containing configuration (`config.toml`), a SQLite database for coupling data, and a LanceDB vector store.

## 2. Index

Build the search index:

```bash
bobbin index
```

Bobbin walks your repository (respecting `.gitignore`), parses source files with tree-sitter into semantic chunks (functions, classes, structs, etc.), generates 384-dimensional embeddings using a local ONNX model, and stores everything in LanceDB.

Indexing a typical project (10k–50k lines) takes 10–30 seconds.

## 3. Search

Find code by meaning:

```bash
bobbin search "error handling"
```

This runs a hybrid search combining semantic similarity (vector search) with keyword matching (full-text search), fused via Reciprocal Rank Fusion. Results show the file, function name, line range, and a content preview.

## 4. Explore More

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

## 5. Interactive Tour

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
