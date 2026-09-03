#!/usr/bin/env python3
"""Static contract for release assets needed by the pull deploy path.

The x86_64 Linux archive and its checksum must be published by that matrix leg,
not by a job that waits for every target.  A full YAML parser is deliberately
unnecessary here: this checks the exact load-bearing workflow statements and
their ordering with only the Python standard library.
"""

import json
from pathlib import Path


workflow = Path(".github/workflows/release.yml").read_text()

build_start = workflow.index("  build:\n")
checksums_start = workflow.index("  checksums:\n")
release_start = workflow.index("  release:\n")
build = workflow[build_start:checksums_start]
checksums = workflow[checksums_start:release_start]
release = workflow[release_start:]
release_please = json.loads(Path("release-please-config.json").read_text())

required_build_fragments = (
    "    permissions:",
    "      contents: write",
    "- name: Generate early Linux deploy checksum",
    "if: matrix.target == 'x86_64-unknown-linux-gnu'",
    "- name: Publish Linux deploy archive immediately",
    "bobbin-${{ env.VERSION }}-${{ matrix.target }}.${{ matrix.archive }}",
    "- name: Build repository index pack",
    'PACK="bobbin-${{ env.VERSION }}-repository.bbpack"',
    '"$BOBBIN" pack verify "$PACK" --path "$PACK_HOME/home"',
    'sha256sum "$PACK" > "$PACK.sha256"',
    "bobbin-${{ env.VERSION }}-repository.bbpack.sha256",
    "SHA256SUMS.txt",
    "- name: Publish target archive immediately",
    "if: matrix.target != 'x86_64-unknown-linux-gnu'",
)
for fragment in required_build_fragments:
    assert fragment in build, f"release build contract missing: {fragment}"

assert release_please["draft"] is True, "Release Please must not expose an assetless release"
assert release_please["force-tag-creation"] is True, "draft release must still create the build trigger tag"
assert build.count("\n          draft: true\n") == 2, "both early matrix upload paths must preserve draft status"
assert release.count("\n          draft: true\n") == 1, "final asset upload must preserve draft status"
assert "- name: Publish complete GitHub Release" in release
assert 'gh release edit "${{ env.VERSION }}" --draft=false --latest' in release
assert release.index("Finalize GitHub Release assets and checksums") < release.index(
    "Publish complete GitHub Release"
), "the release must become public only after final asset upload"

assert build.index("Generate early Linux deploy checksum") < build.index(
    "Publish Linux deploy archive immediately"
), "checksum must exist before the early Linux upload"
assert build.index("Build repository index pack") < build.index(
    "Publish Linux deploy archive immediately"
), "verified repository pack must exist before the early Linux upload"
assert "needs: build" in checksums, "full checksums must still wait for every target"
assert "needs: [build, checksums]" in release, "finalizer must retain the full-build gate"
assert "Finalize GitHub Release assets and checksums" in release
assert "find . -maxdepth 1 -type f ! -name SHA256SUMS.txt" in release
assert 'name "*.bbpack"' in checksums
assert 'name "*.bbpack"' in release

print("release workflow contract: ok")
