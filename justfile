# Bobbin: Semantic code indexing
# Run `just --list` to see available recipes

mod tambour 'tambour.just'

# Default recipe - show available commands
default:
    @just --list

# === Development ===

# Build the project
build:
    cargo build

# Run tests
test:
    cargo test

# Type check without building
check:
    cargo check

# Lint with clippy
lint:
    cargo clippy

# Build and run
run *args:
    cargo run -- {{args}}
