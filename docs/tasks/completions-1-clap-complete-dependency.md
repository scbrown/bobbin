# Task: Add clap_complete dependency and completions subcommand

## Summary

Add the `clap_complete` crate and create a `bobbin completions <SHELL>` subcommand that generates shell completion scripts to stdout.

## Files

- `Cargo.toml` (modify)
- `src/cli/completions.rs` (new)
- `src/cli/mod.rs` (modify)

## Implementation

### `Cargo.toml`

Add `clap_complete` as a dependency:
```toml
clap_complete = "4"
```

No additional clap features needed — the existing `derive` feature is sufficient.

### `src/cli/completions.rs`

```rust
use clap::Args;
use clap_complete::{generate, Shell};

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    shell: Shell,
}

pub fn run(args: CompletionsArgs) {
    let mut cmd = crate::cli::Cli::command();
    generate(args.shell, &mut cmd, "bobbin", &mut std::io::stdout());
}
```

`Shell` is an enum provided by `clap_complete` that accepts: `bash`, `zsh`, `fish`, `powershell`, `elvish`. Clap parses it automatically from the CLI argument.

Note: This is a synchronous function — no async needed. `generate()` writes directly to stdout.

### `src/cli/mod.rs`

Add:
- `mod completions;`
- `Completions(completions::CompletionsArgs)` to `Commands` enum with doc comment `/// Generate shell completion scripts`
- Match arm: `Commands::Completions(args) => { completions::run(args); Ok(()) }`

The `Cli::command()` call requires `use clap::CommandFactory;` — add this import if not already present.

## Tests

- Verify `bobbin completions bash` produces output containing `bobbin`
- Verify `bobbin completions zsh` produces output containing `#compdef`
- Verify `bobbin completions fish` produces output containing `complete`
- Verify invalid shell name produces a helpful error

## Acceptance Criteria

- [ ] `bobbin completions bash` outputs bash completion script
- [ ] `bobbin completions zsh` outputs zsh completion script
- [ ] `bobbin completions fish` outputs fish completion script
- [ ] `bobbin completions --help` shows available shells
- [ ] `bobbin help` lists the completions command
- [ ] Generated scripts include all subcommands and flags
