---
title: completions
description: Generate shell completions for bash, zsh, fish, and PowerShell
tags: [cli, completions]
status: draft
category: cli-reference
related: [cli/overview.md]
commands: [completions]
feature: completions
source_files: [src/cli/completions.rs]
---

# completions

Generate shell completion scripts for bobbin.

## Synopsis

```bash
bobbin completions <SHELL>
```

## Description

The `completions` command outputs a shell completion script to stdout. Pipe the output to the appropriate file for your shell to enable tab-completion for all bobbin commands, subcommands, and options.

Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

## Arguments

| Argument | Description |
|----------|-------------|
| `<SHELL>` | Shell to generate completions for (required) |

## Setup

### Bash

```bash
bobbin completions bash > ~/.local/share/bash-completion/completions/bobbin
```

Or for the current session only:

```bash
source <(bobbin completions bash)
```

### Zsh

```bash
bobbin completions zsh > ~/.zfunc/_bobbin
```

Make sure `~/.zfunc` is in your `$fpath` (add `fpath=(~/.zfunc $fpath)` to `~/.zshrc` before `compinit`).

### Fish

```bash
bobbin completions fish > ~/.config/fish/completions/bobbin.fish
```

### Elvish

```bash
bobbin completions elvish > ~/.config/elvish/lib/bobbin.elv
```

### PowerShell

```powershell
bobbin completions powershell > $HOME\.config\powershell\bobbin.ps1
# Add to your $PROFILE: . $HOME\.config\powershell\bobbin.ps1
```

## See Also

- [Installation](../getting-started/installation.md) — installing bobbin
- [CLI Overview](overview.md) — all available commands
