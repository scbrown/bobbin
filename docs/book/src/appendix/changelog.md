---
title: Changelog
description: Release history and notable changes
tags: [appendix, changelog]
status: draft
category: appendix
related: [appendix/roadmap.md]
---

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-02-07

### Added

- Code indexing with tree-sitter parsing for Rust, TypeScript, Python, Go, Java, and C++
- Semantic search using ONNX Runtime embeddings (all-MiniLM-L6-v2)
- Full-text keyword search via LanceDB/tantivy
- Hybrid search combining semantic and keyword results with Reciprocal Rank Fusion
- Git history analysis for temporal context
- Coupling detection between files based on co-change patterns
- MCP server for AI agent integration
- CLI with `index`, `search`, `grep`, `mcp-server`, and `completions` subcommands
- LanceDB as primary vector storage with SQLite for coupling metadata
- Support for `.bobbinignore` exclude patterns
