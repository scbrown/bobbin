#!/usr/bin/env python3
"""Exercise release asset staging with synthetic archives, without publishing.

# arming: ci; invoked by test-release-workflow-contract.py
"""

import hashlib
import io
import os
import re
import subprocess
import tarfile
import tempfile
import textwrap
from pathlib import Path

WORKFLOW = Path(".github/workflows/release.yml").read_text()
TARGETS = (
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
)
ARCHIVE = "bobbin-${{ env.VERSION }}-${{ matrix.target }}.${{ matrix.archive }}"
SBOM = "bobbin-${{ env.VERSION }}-${{ matrix.target }}.cdx.json"


def step(name):
    return WORKFLOW.split(f"      - name: {name}\n", 1)[1].split("\n      - name:", 1)[
        0
    ]


def run_body(name):
    lines = step(name).split("        run: |\n", 1)[1].splitlines()
    body = []
    for line in lines:
        if line and not line.startswith("          "):
            break
        body.append(line)
    return textwrap.dedent("\n".join(body))


def fixture(root):
    """Match download-artifact's per-target directories and real tar layout."""
    expected = {}
    for target in TARGETS:
        prefix = f"bobbin-v0.0.0-{target}"
        folder = root / "artifacts" / f"bobbin-{target}"
        folder.mkdir(parents=True)
        archive = folder / f"{prefix}.tar.gz"
        with tarfile.open(archive, "w:gz") as out:
            for path, data in [
                ("bobbin", target.encode()),
                ("lib/libonnxruntime.so.1", b"ort"),
            ]:
                info = tarfile.TarInfo(f"{prefix}/{path}")
                info.size = len(data)
                out.addfile(info, io.BytesIO(data))
        for suffix in (
            ".cdx.json",
            ".tar.gz.sigstore.json",
            ".tar.gz.sbom.sigstore.json",
        ):
            (folder / f"{prefix}{suffix}").write_text('{"fixture":true}')
        expected.update({p.name: p.read_bytes() for p in folder.iterdir()})
    return expected


def verify_staging(body):
    with tempfile.TemporaryDirectory(prefix="release-provenance-") as tmp:
        root = Path(tmp)
        expected = fixture(root)
        result = subprocess.run(
            ["bash", "-euo", "pipefail", "-c", body],
            cwd=root,
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, result.stderr
        assets = root / "artifacts"
        manifest = {}
        for row in (assets / "SHA256SUMS.txt").read_text().splitlines():
            digest, name = row.split(maxsplit=1)
            manifest[name] = digest
        for name, data in expected.items():
            assert (assets / name).read_bytes() == data, f"lost or changed {name}"
            assert manifest[name] == hashlib.sha256(data).hexdigest(), name
        for name, digest in manifest.items():
            assert hashlib.sha256((assets / name).read_bytes()).hexdigest() == digest
        if "bobbin-linux-amd64" in manifest:
            assert (assets / "bobbin-linux-amd64").read_bytes() == TARGETS[0].encode()
            assert (assets / "bobbin-linux-arm64").read_bytes() == TARGETS[1].encode()


def main():
    build = WORKFLOW.split("  build:\n", 1)[1].split("  checksums:\n", 1)[0]
    assert (
        WORKFLOW.count("      id-token: write") == 2
    )  # build + existing crates trusted publishing
    assert "      id-token: write" in build and "      attestations: write" in build
    assert WORKFLOW.count("      attestations: write") == 1
    assert (
        "cargo auditable build --release --locked --features knowledge --target"
        in step("Build")
    )
    for tool in ("cargo-auditable", "cargo-cyclonedx"):
        assert re.search(
            rf"cargo install {tool} --version \d+\.\d+\.\d+ --locked",
            step("Install release metadata tools"),
        )
    provenance = step("Attest build provenance")
    sbom = step("Attest Rust dependency SBOM")
    for chunk in (provenance, sbom):
        assert re.search(r"uses: actions/attest[\w-]*@[a-f0-9]{40}\b", chunk)
        assert ARCHIVE in chunk and "continue-on-error" not in chunk
    assert "target/${{ matrix.target }}/release/bobbin" in provenance
    assert "sbom-path: " + SBOM in sbom
    assert "--features knowledge --target" in step("Generate Rust dependency SBOM")
    assert "git diff --exit-code -- Cargo.lock" in step("Generate Rust dependency SBOM")
    metadata = (SBOM, ARCHIVE + ".sigstore.json", ARCHIVE + ".sbom.sigstore.json")
    for name in (
        "Upload artifact",
        "Publish Linux deploy archive immediately",
        "Publish target archive immediately",
    ):
        for asset in metadata:
            assert asset in step(name), (name, asset)
    assert build.index("Attest build provenance") < build.index("Upload artifact")
    assert build.index("Attest Rust dependency SBOM") < build.index("Upload artifact")
    with tempfile.TemporaryDirectory(prefix="release-bundles-") as tmp:
        root = Path(tmp)
        provenance_path, sbom_path = root / "provenance", root / "sbom"
        provenance_path.write_bytes(b"provenance fixture")
        sbom_path.write_bytes(b"sbom fixture")
        body = run_body("Preserve offline attestation bundles")
        for key, value in (
            ("env.VERSION", "v0.0.0"),
            ("matrix.target", TARGETS[0]),
            ("matrix.archive", "tar.gz"),
        ):
            body = body.replace("${{ " + key + " }}", value)
        subprocess.run(
            ["bash", "-euo", "pipefail", "-c", body],
            cwd=root,
            check=True,
            env={
                **os.environ,
                "PROVENANCE_BUNDLE": str(provenance_path),
                "SBOM_BUNDLE": str(sbom_path),
            },
        )
        archive = f"bobbin-v0.0.0-{TARGETS[0]}.tar.gz"
        assert (
            root / (archive + ".sigstore.json")
        ).read_bytes() == b"provenance fixture"
        assert (
            root / (archive + ".sbom.sigstore.json")
        ).read_bytes() == b"sbom fixture"
    for name in ("Generate SHA256 checksums", "Prepare release assets"):
        body = run_body(name)
        verify_staging(body)
        # Prove the check catches the exact silent-loss regression this guards:
        # either extension removed from the flatten command loses those assets.
        for pattern in (' -o -name "*.cdx.json"', ' -o -name "*.sigstore.json"'):
            assert pattern in body
            try:
                verify_staging(body.replace(pattern, ""))
            except (AssertionError, FileNotFoundError, KeyError):
                pass
            else:
                raise AssertionError(f"{name}: missing-extension control passed")
    print("release provenance: two real staging paths + four loss controls passed")


if __name__ == "__main__":
    main()
