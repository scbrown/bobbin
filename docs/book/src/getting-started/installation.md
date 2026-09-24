---
title: Installation
description: Verify a release checksum and install Bobbin, or build from source
tags: [installation, setup]
status: published
category: getting-started
related: [getting-started/quick-start.md, cli/init.md]
---

# Installation

## Release binaries

Releases with checksums support Linux and macOS on x86-64 and ARM64. For Linux
x86-64, with `curl`, `tar` and `sha256sum` installed:

```bash
mkdir -p bobbin-download && cd bobbin-download
base=https://github.com/scbrown/bobbin/releases/download/v0.18.1
asset=bobbin-v0.18.1-x86_64-unknown-linux-gnu.tar.gz
curl -fLO "$base/$asset" && curl -fLO "$base/SHA256SUMS.txt"
sha256sum --check --ignore-missing SHA256SUMS.txt && tar xzf "$asset"
export PATH="$PWD/bobbin-v0.18.1-x86_64-unknown-linux-gnu:$PATH"
bobbin --version
```

Keep the extracted directory intact: its `lib/` contains ONNX Runtime.
The pinned release reports:

```text
bobbin 0.18.1 (bc246f84003f89eec07618e8c341bb6835500f58)
```

For [other platforms and runtime prerequisites](#choose-your-platform),
use the matching release asset. With the Rust build prerequisites installed,
`cargo install bobbin-ai --locked` builds from source and installs `bobbin`.
Run `bobbin --version`; if it reports an older version, check `command -v bobbin`
for another installation earlier in `PATH`.

## Choose your platform

Download the archive and `SHA256SUMS.txt` from the
[v0.18.1 release](https://github.com/scbrown/bobbin/releases/tag/v0.18.1).

| System | Archive suffix |
|---|---|
| Linux x86-64 | `x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 | `aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |

Every filename starts with `bobbin-v0.18.1-`. On macOS, use `shasum -a 256`
and compare the archive's digest with its entry in `SHA256SUMS.txt` before
extracting. Add the extracted directory to your shell's `PATH`.

Linux releases target GNU C library, not musl/Alpine. The archive includes ONNX
Runtime in `lib/`; keep it beside the executable. If loading fails, confirm
that you extracted the entire archive and selected your CPU architecture.

## Build from source

Install stable Rust, a C++ compiler, `cmake` and `protoc` (Protocol Buffers).
The crate is named `bobbin-ai`; the executable it installs is `bobbin`.

```bash
cargo install bobbin-ai --locked
bobbin --version
```

For repository development, install `just` and follow
[Contributing](../../../../CONTRIBUTING.md). `just build` includes the
`knowledge` feature for Quipu integration; a default Cargo install does not.

## First-run behavior

Local indexing downloads its embedding model on first use. Allow network
access for this step; subsequent searches use the cached model and local index.
Indexing time depends on repository size, model, hardware and enabled analysis.
Git history supplies temporal coupling; a new repository has no history yet.
A GPU is optional. Set `BOBBIN_GPU=0` to explicitly use the CPU.

## Next steps

- [Quick start](quick-start.md) — build a tiny index and verify the result.
- [Agent setup](agent-setup.md) — connect your coding assistant.
