---
title: "Vision"
description: "The vision and philosophy behind bobbin as a local-first code context engine"
tags: [appendix, vision]
category: appendix
---

# Bobbin Vision & Mission

## Mission Statement

Bobbin is a local-first context engine that gives developers and AI agents deep, structured access to codebases without sending data to the cloud.

## Vision

In the era of AI-assisted development, context is everything. Current tools treat code as flat text, losing the rich structural and temporal information that makes codebases understandable. Bobbin changes this by treating code as a living, evolving artifact with history, structure, and relationships.

**Bobbin enables "Temporal RAG"** - retrieval that understands not just what code says, but how it evolved and what changes together.

## Core Principles

### 1. Local-First, Always

- All indexing and search happens on your machine
- No data leaves your environment
- Works offline, works air-gapped
- Your code stays yours

### 2. Structure-Aware

- Code is parsed, not chunked arbitrarily
- Functions, classes, and modules are first-class citizens
- Respects language semantics via Tree-sitter

### 3. Temporally-Aware

- Git history is a retrieval signal, not just version control
- Files that change together are semantically linked
- Understand "what usually changes when X changes"

### 4. Agent-Ready, Human-Friendly

- CLI interface works for both humans and AI agents
- No special agent protocols required (MCP optional)
- Simple, composable commands

## What Bobbin Is

- A **code indexer** that understands syntax structure
- A **documentation indexer** for markdown/text files
- A **git forensics engine** that tracks file relationships over time
- A **semantic search** engine using local embeddings
- A **keyword search** engine for precise lookups
- A **context aggregator** that pulls related information together

## What Bobbin Is Not

- Not a task manager or agent harness
- Not an IDE extension (headless by design)
- Not a cloud service
- Not an AI agent itself - it serves agents

## Key Differentiators

| Feature | Traditional RAG | Bobbin |
|---------|-----------------|--------|
| Chunking | Fixed token windows | AST-based structural units |
| History | HEAD only | Full git timeline |
| Relationships | Vector similarity only | Temporal coupling + similarity |
| Privacy | Often cloud-based | Strictly local |
| Runtime | Client-server | Embedded/serverless |

## Target Users

1. **AI Coding Agents** - Claude Code, Cursor, Aider, custom agents
2. **Developers** - Direct CLI usage for code exploration
3. **Tool Builders** - Foundation for context-aware dev tools
4. **Agent Harnesses** - Middleware integration for orchestration tools

## Success Metrics

- Sub-second query latency on repos up to 1M LOC
- Zero network calls during operation
- Retrieval precision that matches or exceeds cloud alternatives
- Seamless adoption in existing workflows

## Technology Choices

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Language | Rust | Performance, memory safety, ecosystem (Tree-sitter, LanceDB) |
| Vector Store | LanceDB | Embedded, serverless, git-friendly storage |
| Parser | Tree-sitter | Incremental, multi-language, battle-tested |
| Embeddings | Local ONNX (all-MiniLM-L6-v2) | Fast CPU inference, no API dependency |

---

*Bobbin: Local context for the age of AI-assisted development.*
