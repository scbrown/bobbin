# Task: Create LICENSE file and fix Cargo.toml metadata

## Summary

Create the MIT LICENSE file and fix/add missing metadata in Cargo.toml for crates.io publishing.

## Files

- `LICENSE` (new)
- `Cargo.toml` (modify)

## Implementation

### LICENSE file

Create `/LICENSE` with standard MIT license text:
- Copyright holder: "Steve Brown"
- Year: 2026

### Cargo.toml fixes

**Fix repository URL** (critical - currently wrong):
```toml
# WRONG: repository = "https://github.com/bobbin-dev/bobbin"
# RIGHT:
repository = "https://github.com/scbrown/bobbin"
```

Verify with: `git remote -v` (shows `git@github.com:scbrown/bobbin.git`)

**Add missing fields:**
```toml
readme = "README.md"
homepage = "https://github.com/scbrown/bobbin"
```

## Acceptance Criteria

- [ ] LICENSE file exists with MIT text
- [ ] Repository URL matches actual git remote
- [ ] readme and homepage fields present
- [ ] `cargo package` doesn't warn about missing license file
