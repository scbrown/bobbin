#!/usr/bin/env python3
"""Static contract for release assets needed by the pull deploy path.

The x86_64 Linux archive and its checksum must be published by that matrix leg,
not by a job that waits for every target.  A full YAML parser is deliberately
unnecessary here: this checks the exact load-bearing workflow statements and
their ordering with only the Python standard library.
"""

import json
import re
import subprocess
import sys
from pathlib import Path


workflow = Path(".github/workflows/release.yml").read_text()

build_start = workflow.index("  build:\n")
checksums_start = workflow.index("  checksums:\n")
release_start = workflow.index("  release:\n")
build = workflow[build_start:checksums_start]
checksums = workflow[checksums_start:release_start]
release_please_start = workflow.index("  release-please:\n")
# Bounded: this used to run to EOF and swallow the release-please job, so every
# assertion "about the release job" was really about both.
release = workflow[release_start:release_please_start]
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
# The publish must clear the draft. It used to be pinned as one literal string
# that ALSO baked in an unconditional `--latest`, so this contract was holding
# the aegis-egqrv4 defect in place: any fix to the latest decision failed here.
# The draft-clearing and the latest decision are separate obligations and are now
# asserted separately (see the latest-marker block at the end of this file).
assert "--draft=false" in release, "the publish step must clear the draft"
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
# THE FULL-BUILD GATE IS A PROPERTY, NOT A SPELLING (aegis-g2gpw5).
#
# This used to assert the literal "needs: [build, checksums]". That pinned one
# way of writing the gate rather than the gate itself, so it failed a change that
# made the gate STRICTER -- adding release-please to `needs` and requiring both
# results to be 'success' -- while its own message says only that the full-build
# gate must be retained. A guard that fails a strengthening of the property it
# names will be silenced by whoever hits it, which costs more than it protects.
#
# What actually must hold: the finalizer cannot publish unless build AND
# checksums both ran and both succeeded.
release_needs = re.search(r"^    needs:\s*(.+)$", release, re.M)
assert release_needs, "finalizer must declare `needs`"
needed = set(re.findall(r"[A-Za-z][\w-]*", release_needs.group(1)))
assert {"build", "checksums"} <= needed, (
    f"finalizer must wait for build and checksums; needs = {sorted(needed)}"
)

# `always()` severs the implicit success requirement that a bare `needs` carries,
# so where it is present the success check must be explicit. Without this, a job
# could gain always() and publish a release whose assets never built -- the exact
# assetless release this whole contract exists to prevent.
release_if = re.search(r"^    if: (.+?)^    [a-z]", release, re.M | re.S)
release_if = release_if.group(1) if release_if else ""
if "always()" in release_if:
    for job in ("build", "checksums"):
        assert f"needs.{job}.result == 'success'" in release_if, (
            f"finalizer uses always(), so it must require needs.{job}.result == 'success' "
            "explicitly -- always() means a failed or skipped dependency no longer blocks it"
        )
assert "Finalize GitHub Release assets and checksums" in release
assert "find . -maxdepth 1 -type f ! -name SHA256SUMS.txt" in release
assert 'name "*.bbpack"' in checksums
assert 'name "*.bbpack"' in release

print("release workflow contract: ok")


# ── The `latest` marker must be DECIDED, not asserted (aegis-egqrv4) ──────────
#
# `gh release edit --latest` is unconditional wherever it appears bare, so the
# LAST release run to reach it wins `latest` regardless of version. That is
# reachable: the finalizer gates on every matrix leg, and the macOS legs dominate
# (79m41s measured for v0.15.0, ~100 min recorded in release.yml). Merge a second
# release PR inside that window and two matrices race with nothing ordering them.
#
# The damage is silent. Bobbin deploys by `github-release`: the deploy timer
# resolves the latest PUBLISHED release, so an inversion deploys the OLDER build
# over the newer one, with both releases complete and no run failing.
assert "--latest=false" in release, (
    "the publish step must be able to publish WITHOUT claiming latest; a bare "
    "--latest lets an older release demote a newer one (aegis-egqrv4)"
)
assert "sort -V" in release, (
    "the latest decision must compare versions with `sort -V`; a lexical sort "
    "ranks 0.9.0 above 0.10.0 and inverts the guard in both directions"
)
assert "--exclude-drafts" in release and "--exclude-pre-releases" in release, (
    "the highest published version must ignore drafts and prereleases — a draft "
    "is the NORMAL state for most of a release's ~80-minute build"
)
# `--latest` must never appear without a decision around it. Every occurrence
# lives in one of the two branches, so both spellings are present.
assert release.count("--latest") >= 2, (
    "expected both the --latest and --latest=false branches of the decision"
)

# The static assertions above check that the workflow SPELLS the decision
# correctly. This runs the decision and checks it is RIGHT — wired here rather
# than as its own CI step so it shares an execution path that already runs,
# instead of being a check nothing invokes (aegis-n5b1rd).
gate = Path("scripts/test-release-latest-gate.sh")
if gate.exists():
    result = subprocess.run(["bash", str(gate)], capture_output=True, text=True)
    if result.returncode != 0:
        sys.stdout.write(result.stdout)
        sys.stderr.write(result.stderr)
        raise SystemExit("release latest-gate logic test failed")
else:
    raise SystemExit(f"missing {gate}: the latest-gate logic is unverified")
