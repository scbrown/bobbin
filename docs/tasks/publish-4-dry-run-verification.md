# Task: Verify cargo publish dry run

## Summary

Final verification that everything is ready for crates.io. Run dry-run publish and fix any remaining issues.

## Implementation

### Steps

1. Ensure all previous publish tasks are done (LICENSE, metadata, excludes, workflows)

2. Run verification commands:
```bash
# Check package contents
cargo package --list

# Verify package builds
cargo package

# Dry run publish
cargo publish --dry-run
```

3. Verify output:
- No warnings about missing fields
- No internal files in package
- Package size is reasonable (should be well under 1MB)
- All metadata looks correct on the preview

4. Check the generated .crate file:
```bash
ls -la target/package/bobbin-*.crate
```

### Common issues to fix

- **"1 files in the working directory contain changes"**: state.json must be gitignored
- **License warning**: LICENSE file must exist
- **Missing readme**: readme field must point to README.md
- **Dependency issues**: All deps must be published on crates.io

## Dependencies

- All other publish tasks must be complete

## Acceptance Criteria

- [ ] `cargo package` succeeds with no warnings
- [ ] `cargo publish --dry-run` succeeds
- [ ] Package contents are clean (only source + docs)
- [ ] Package size under 1MB
