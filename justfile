# Bobbin: Semantic code indexing
# Run `just --list` to see available recipes

mod tambour 'tambour.just'

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
