# Publishing Bobbin

## CI Pipeline

Every push to `main` and every pull request triggers the CI workflow (`.github/workflows/ci.yml`), which runs:

1. `cargo check` - type checking
2. `cargo test` - unit and integration tests
3. `cargo clippy` - lint warnings

## Publishing to crates.io

Bobbin uses **OIDC trusted publishing** for crates.io — no API tokens stored in GitHub Secrets.

### How it works

1. Create a GitHub Release (or push a `v*` tag)
2. The `crates.yml` workflow triggers automatically
3. GitHub Actions authenticates with crates.io via OIDC
4. `cargo publish` runs with the OIDC-issued token

### crates.io setup (one-time)

1. Go to <https://crates.io/settings/tokens>
2. Under "Trusted Publishers", add:
   - **Repository owner**: `scbrown`
   - **Repository name**: `bobbin`
   - **Workflow filename**: `crates.yml`
3. No secrets need to be added to the GitHub repository

### Manual / dry-run publish

Use the workflow dispatch trigger:

1. Go to Actions > "Publish to crates.io"
2. Click "Run workflow"
3. Check "Dry run" to test without publishing

## Release Binaries

The `release.yml` workflow builds pre-compiled binaries when a `v*` tag is pushed. It produces:

| Target | OS | Archive |
|--------|-----|---------|
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 | tar.gz |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 (cross) | tar.gz |
| `aarch64-apple-darwin` | macOS 14 | tar.gz |
| `x86_64-apple-darwin` | macOS 15 Intel | tar.gz |

Each archive includes the `bobbin` binary and bundled ONNX Runtime shared library.

A `SHA256SUMS.txt` file and standalone `bobbin-linux-amd64` / `bobbin-linux-arm64` binaries are also attached to the GitHub Release.

### Creating a release

```bash
# Tag and push
git tag v0.1.0
git push origin v0.1.0

# Then create a GitHub Release from the tag
gh release create v0.1.0 --generate-notes
```

The release workflow will build all targets and attach them to the release automatically.

## Documentation

The `docs.yml` workflow runs on doc changes (PRs and pushes to main):

- Markdown linting via markdownlint-cli2
- Style checking via Vale
- mdbook build and deploy to GitHub Pages
