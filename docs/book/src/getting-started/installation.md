---
title: "Installation"
description: "Installing bobbin from source or pre-built binaries"
tags: [installation, setup]
category: getting-started
---

# Installation

Bobbin is a Rust application distributed via Cargo. It runs entirely locally — no API keys, no cloud services, no data leaves your machine.

## Requirements

- **Rust toolchain** (1.75+): Install via [rustup](https://rustup.rs/)
- **Git**: Required for temporal coupling analysis
- **C compiler**: Required by tree-sitter build (usually pre-installed on Linux/macOS)

## Install from Source

```bash
cargo install bobbin
```

This builds an optimized release binary with LTO enabled and installs it to `~/.cargo/bin/bobbin`.

## Build from Repository

```bash
git clone https://github.com/scbrown/bobbin.git
cd bobbin
cargo build --release
```

The binary is at `target/release/bobbin`.

## First-Run Behavior

On first use, bobbin automatically downloads the embedding model (`all-MiniLM-L6-v2`, ~23 MB) to a local cache directory. This is a one-time download — subsequent runs use the cached model.

The model cache location follows platform conventions:

- **Linux**: `~/.cache/bobbin/models/`
- **macOS**: `~/Library/Caches/bobbin/models/`

## Verify Installation

```bash
bobbin --version
```

## Shell Completions

Generate completions for your shell:

```bash
bobbin completions bash > ~/.local/share/bash-completion/completions/bobbin
bobbin completions zsh > ~/.zfunc/_bobbin
bobbin completions fish > ~/.config/fish/completions/bobbin.fish
```

## Next Steps

- [Quick Start](quick-start.md) — initialize and search your first repository
- [Agent Setup](agent-setup.md) — connect bobbin to Claude Code, Cursor, or other AI tools
