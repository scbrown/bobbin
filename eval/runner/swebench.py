"""Prepare the frozen external L2 task set; never execute dataset commands."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from collections import defaultdict
from datetime import datetime
from pathlib import Path
from typing import Any

DATASET = "SWE-bench/SWE-bench_Verified"
REVISION = "78f471bf655a3137b2e8a75af1501690ec009ec3"
SOURCE_SHA256 = "030cfd7f2a704c4c0226e7f104c725a3b41230b1d3517f9c915ad7ea5be3fa25"
SOURCE_FILE = "data/test-00000-of-00001.parquet"
SEED = "20260925"
PER_REPO = 2
MANIFEST = Path(__file__).resolve().parents[1] / "v2/swebench-verified-manifest.json"


def canonical(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":")) + "\n"
    ).encode()


def validate_rows(rows: list[dict]) -> None:
    """Refuse malformed gold/tests instead of silently changing eligibility."""
    seen: set[str] = set()
    if not rows:
        raise ValueError("empty dataset")
    for row in rows:
        for field in (
            "instance_id",
            "repo",
            "base_commit",
            "version",
            "problem_statement",
            "patch",
            "test_patch",
            "created_at",
        ):
            if not isinstance(row.get(field), str) or not row[field].strip():
                raise ValueError(f"missing or empty {field}")
        instance_id = row["instance_id"]
        if instance_id in seen:
            raise ValueError(f"duplicate instance_id: {instance_id}")
        seen.add(instance_id)
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", row["repo"]):
            raise ValueError(f"invalid repo: {instance_id}")
        if not re.fullmatch(r"[0-9a-f]{40}", row["base_commit"]):
            raise ValueError(f"invalid base_commit: {instance_id}")
        datetime.fromisoformat(row["created_at"])
        for field in ("FAIL_TO_PASS", "PASS_TO_PASS"):
            tests = json.loads(row[field]) if isinstance(row.get(field), str) else row.get(field)
            if not isinstance(tests, list) or any(not isinstance(t, str) or not t for t in tests):
                raise ValueError(f"invalid {field}: {instance_id}")
            if field == "FAIL_TO_PASS" and not tests:
                raise ValueError(f"empty FAIL_TO_PASS: {instance_id}")


def select_rows(rows: list[dict], *, seed: str = SEED, per_repo: int = PER_REPO) -> list[dict]:
    validate_rows(rows)
    if per_repo < 1:
        raise ValueError("per_repo must be positive")
    groups: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        groups[row["repo"]].append(row)
    selected = []
    for repo in sorted(groups):
        ranked = sorted(
            groups[repo],
            key=lambda row: (
                hashlib.sha256(f"{seed}\n{row['instance_id']}".encode()).hexdigest(),
                row["instance_id"],
            ),
        )
        selected.extend(ranked[:per_repo])
    return sorted(selected, key=lambda row: row["instance_id"])


def agent_prompt(row: dict) -> dict:
    """Allowlist the input: gold, tests, hints and environment scripts stay out."""
    return {key: row[key] for key in ("instance_id", "repo", "base_commit", "problem_statement")}


def build_manifest(rows: list[dict], selected: list[dict]) -> dict:
    return {
        "schema_version": 1,
        "dataset": DATASET,
        "revision": REVISION,
        "source_file": SOURCE_FILE,
        "source_sha256": SOURCE_SHA256,
        "source_rows": len(rows),
        "selection": {
            "algorithm": "per-repo sha256(seed + newline + instance_id), ascending",
            "seed": SEED,
            "per_repo": PER_REPO,
        },
        "selected_rows": len(selected),
        "evaluator_sha256": hashlib.sha256(b"".join(canonical(r) for r in selected)).hexdigest(),
        "instances": [
            {
                **{key: row[key] for key in ("instance_id", "repo", "base_commit", "created_at")},
                "record_sha256": hashlib.sha256(canonical(row)).hexdigest(),
            }
            for row in selected
        ],
    }


def prepare(source: Path, output: Path, manifest_path: Path = MANIFEST) -> dict:
    """Check source and frozen selection before writing any agent/evaluator data."""
    if hashlib.sha256(source.read_bytes()).hexdigest() != SOURCE_SHA256:
        raise ValueError("source checksum mismatch")
    # Optional preparation dependency, not needed by normal eval commands/tests.
    from pyarrow import parquet

    rows = parquet.read_table(source).to_pylist()
    selected = select_rows(rows)
    manifest = build_manifest(rows, selected)
    if manifest != json.loads(manifest_path.read_text()):
        raise ValueError("prepared subset differs from frozen manifest")
    output.mkdir(parents=True, exist_ok=False)
    (output / "prompts.jsonl").write_bytes(b"".join(canonical(agent_prompt(r)) for r in selected))
    evaluator = output / "evaluator"
    evaluator.mkdir(mode=0o700)
    (evaluator / "instances.jsonl").write_bytes(b"".join(canonical(r) for r in selected))
    (output / "manifest.json").write_bytes(canonical(manifest))
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True, help="pinned downloaded Parquet file")
    parser.add_argument(
        "--output", type=Path, required=True, help="new directory outside agent repos"
    )
    args = parser.parse_args()
    manifest = prepare(args.source, args.output)
    print(f"Prepared {manifest['selected_rows']} tasks; no benchmark or test commands executed.")


if __name__ == "__main__":
    main()
