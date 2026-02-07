# Task: Add shell completion installation documentation

## Summary

Add a "Shell Completions" section to the README with installation instructions for bash, zsh, and fish.

## Files

- `README.md` (modify)

## Implementation

Add a section after the installation/usage section:

```markdown
### Shell Completions

Generate completion scripts with:

```sh
bobbin completions <SHELL>
```

**Bash** — add to `~/.bashrc`:
```sh
eval "$(bobbin completions bash)"
```

**Zsh** — add to `~/.zshrc` (before `compinit`):
```sh
eval "$(bobbin completions zsh)"
```

Or save to your fpath:
```sh
bobbin completions zsh > "${fpath[1]}/_bobbin"
```

**Fish** — add to fish config:
```sh
bobbin completions fish | source
```

Or save permanently:
```sh
bobbin completions fish > ~/.config/fish/completions/bobbin.fish
```
```

Keep the section concise — just enough to get users going for each major shell.

## Dependencies

- Requires Task 1 (clap_complete dependency and completions subcommand)

## Acceptance Criteria

- [ ] README has a "Shell Completions" section
- [ ] Instructions cover bash, zsh, and fish
- [ ] Commands are copy-pasteable and correct
