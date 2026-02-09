# Bobbin: Semantic code indexing
# Run `just --list` to see available recipes

# Quiet by default to save context; use verbose=true for full output
verbose := "false"
cargo_flags := if verbose == "true" { "" } else { "-q --message-format=short" }

# Default recipe - show available commands
default:
    @just --list

# === Development ===

# Build the project (quiet by default, use verbose=true for full output)
build:
    cargo build {{cargo_flags}}

# Run tests (quiet by default, use verbose=true for full output)
test:
    cargo test {{cargo_flags}}

# Type check without building (quiet by default, use verbose=true for full output)
check:
    cargo check {{cargo_flags}}

# Lint with clippy (quiet by default, use verbose=true for full output)
lint:
    cargo clippy {{cargo_flags}}

# Build and run
run *args:
    cargo run {{cargo_flags}} -- {{args}}

# === Setup ===

# Install system dependencies (idempotent, Linux apt / macOS brew)
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Checking system dependencies..."
    # Rust toolchain
    if command -v rustc &>/dev/null; then
        echo "  rustc $(rustc --version | awk '{print $2}') ✓"
    else
        echo "  rustc not found — install via https://rustup.rs"
        exit 1
    fi
    # C++ compiler (needed by cc-rs crate)
    if command -v c++ &>/dev/null; then
        echo "  c++ ✓"
    else
        echo "  c++ not found — installing..."
        if command -v apt-get &>/dev/null; then
            sudo apt-get install -y -qq g++
        elif command -v brew &>/dev/null; then
            echo "  Xcode CLT provides c++ on macOS. Run: xcode-select --install"
            exit 1
        else
            echo "  Please install a C++ compiler manually."
            exit 1
        fi
    fi
    # protoc (needed by lancedb transitive dep lance-encoding)
    if command -v protoc &>/dev/null; then
        echo "  protoc $(protoc --version | awk '{print $2}') ✓"
    else
        echo "  protoc not found — installing..."
        if command -v apt-get &>/dev/null; then
            sudo apt-get install -y -qq protobuf-compiler
        elif command -v brew &>/dev/null; then
            brew install protobuf
        else
            echo "  Please install protobuf-compiler manually."
            exit 1
        fi
    fi
    echo "All dependencies satisfied."

# === Documentation ===

# Documentation management: just docs <cmd>
# Commands: build, serve, lint, fix, fmt, vale, validate, coverage, check
docs cmd="build":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{cmd}}" in
        build)    mdbook build docs/book ;;
        serve)    mdbook serve docs/book --open ;;
        lint)     npx markdownlint-cli2 "docs/book/src/**/*.md" "README.md" "CONTRIBUTING.md" ;;
        fix)      npx markdownlint-cli2 --fix "docs/book/src/**/*.md" "README.md" "CONTRIBUTING.md" ;;
        fmt)      npx prettier --write "docs/book/src/**/*.md" --prose-wrap preserve ;;
        vale)     vale docs/book/src/ ;;
        validate) bash scripts/validate-frontmatter.sh ;;
        coverage) bash scripts/doc-coverage.sh ;;
        check)    just docs lint && just docs vale && just docs validate && just docs build ;;
        *)        echo "Unknown: {{cmd}}. Try: build serve lint fix fmt vale validate coverage check" ;;
    esac
