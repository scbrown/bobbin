# Task: Add GitHub Actions CI and cargo publish workflow

## Summary

Add CI workflow for tests/lint on every push, and a crates.io publish workflow using OIDC trusted publishing (no API keys needed).

## Files

- `.github/workflows/ci.yml` (new)
- `.github/workflows/crates.yml` (new)
- `.github/workflows/release.yml` (new)

## Implementation

### CI Workflow (`.github/workflows/ci.yml`)

Runs on every push and PR:
```yaml
name: CI
on: [push, pull_request]
env:
  CARGO_TERM_COLOR: always
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo check
      - run: cargo test
      - run: cargo clippy -- -D warnings
```

### Crates.io Publish (`.github/workflows/crates.yml`)

Uses OIDC trusted publishing - no secrets needed.
Adapted from pixelsrc's workflow:

```yaml
name: Publish to crates.io
on:
  release:
    types: [published]
  workflow_dispatch:
    inputs:
      dry_run:
        description: 'Dry run (do not publish)'
        required: false
        default: false
        type: boolean
env:
  CARGO_TERM_COLOR: always
jobs:
  publish:
    name: Publish to crates.io
    runs-on: ubuntu-latest
    permissions:
      contents: read
      id-token: write  # Required for OIDC trusted publishing
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Verify package
        run: cargo package --no-verify
      - name: Publish (dry run)
        if: ${{ inputs.dry_run == true }}
        run: cargo publish --dry-run
      - name: Authenticate with crates.io
        if: ${{ inputs.dry_run != true }}
        id: crates-auth
        uses: rust-lang/crates-io-auth-action@v1
      - name: Publish to crates.io
        if: ${{ inputs.dry_run != true }}
        run: cargo publish
        env:
          CARGO_REGISTRY_TOKEN: ${{ steps.crates-auth.outputs.token }}
```

**crates.io setup required (human action):**
1. Go to https://crates.io/settings/tokens
2. Under "Trusted Publishers", add:
   - Repository: `scbrown/bobbin`
   - Workflow: `crates.yml`

### Release Workflow (`.github/workflows/release.yml`)

Build pre-compiled binaries for all platforms on tag push.
Adapted from pixelsrc. Targets:
- Linux: x86_64, aarch64 (cross-compiled)
- macOS: x86_64, aarch64
- Windows: x86_64

Produces tar.gz/zip archives + SHA256SUMS.txt and creates GitHub Release.

## Dependencies

- Task 1 must be done first (LICENSE, correct repo URL)

## Documentation

Create `docs/PUBLISHING.md` explaining the OIDC trusted publishing setup, adapted from pixelsrc's docs.

## Acceptance Criteria

- [ ] CI runs on push and catches errors
- [ ] `crates.yml` workflow uses OIDC (no API keys in secrets)
- [ ] `release.yml` builds binaries for 5+ targets
- [ ] `docs/PUBLISHING.md` documents the setup
