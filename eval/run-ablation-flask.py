#!/usr/bin/env python3
"""Search-level ablation testing on flask tasks.

Disables one search method at a time and measures P/R/F1 against the
baseline config (sw=0.90, dd=0.30, k=60). No LLM calls — pure search
quality measurement using bobbin context --json.

Ablation variants:
  - baseline:           sw=0.90, dd=0.30, k=60, depth=1 (current defaults)
  - no_semantic:        sw=0.0 (pure keyword, no embeddings)
  - no_keyword:         sw=1.0 (pure semantic, no BM25)
  - no_coupling:        depth=0 (disable temporal coupling)
  - no_doc_demotion:    dd=1.0 (treat docs same as source)
  - no_recency:         recency_weight=0.0 via config.toml patch
  - no_blame_bridging:  blame_bridging=false — NOT testable at context level
                        (blame bridging is a hook-inject feature, not used by
                        bobbin context)

Usage:
    python3 eval/run-ablation-flask.py
    python3 eval/run-ablation-flask.py --task flask-004
"""

from __future__ import annotations

import json
import logging
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    datefmt="%H:%M:%S",
)
logger = logging.getLogger(__name__)

EVAL_ROOT = Path(__file__).resolve().parent
QUARANTINED = EVAL_ROOT / "tasks" / "_quarantined"
RESULTS_DIR = EVAL_ROOT / "results"

# Flask tasks to test (skip flask-001 — empty_index bug)
FLASK_TASKS = ["flask-002", "flask-003", "flask-004", "flask-005"]

# Baseline parameters (from calibration sweep — best F1=0.461)
BASELINE = {
    "semantic_weight": 0.90,
    "doc_demotion": 0.30,
    "rrf_k": 60.0,
    "depth": 1,
}


@dataclass
class AblationVariant:
    """One ablation: a name + the CLI overrides to apply on top of baseline."""
    name: str
    description: str
    cli_overrides: dict[str, str | float | int]
    config_patch: dict[str, str] | None = None  # section.key = value for config.toml


# Define all ablation variants
VARIANTS = [
    AblationVariant(
        name="baseline",
        description="Full pipeline: sw=0.90, dd=0.30, k=60, depth=1",
        cli_overrides={},
    ),
    AblationVariant(
        name="no_semantic",
        description="Disable semantic search (pure keyword/BM25)",
        cli_overrides={"semantic_weight": 0.0},
    ),
    AblationVariant(
        name="no_keyword",
        description="Disable keyword search (pure semantic/embedding)",
        cli_overrides={"semantic_weight": 1.0},
    ),
    AblationVariant(
        name="no_coupling",
        description="Disable temporal coupling expansion",
        cli_overrides={"depth": 0},
    ),
    AblationVariant(
        name="no_doc_demotion",
        description="Treat docs same as source (doc_demotion=1.0)",
        cli_overrides={"doc_demotion": 1.0},
    ),
    AblationVariant(
        name="no_recency",
        description="Disable recency/freshness signal",
        config_patch={"search.recency_weight": "0.0"},
        cli_overrides={},
    ),
]


def find_bobbin() -> str:
    found = shutil.which("bobbin")
    if found:
        return found
    cargo_bin = Path.home() / ".cargo" / "bin" / "bobbin"
    if cargo_bin.exists():
        return str(cargo_bin)
    local_bin = Path.home() / ".local" / "bin" / "bobbin"
    if local_bin.exists():
        return str(local_bin)
    raise RuntimeError("bobbin binary not found")


def load_task(task_id: str) -> dict:
    import yaml
    path = QUARANTINED / f"{task_id}.yaml"
    if not path.exists():
        raise FileNotFoundError(f"Task {task_id} not found at {path}")
    with open(path) as f:
        data = yaml.safe_load(f)
    data["id"] = data.get("id", task_id)
    return data


def get_ground_truth(workspace: Path, commit: str) -> list[str]:
    result = subprocess.run(
        ["git", "diff-tree", "--no-commit-id", "--name-only", "-r", commit],
        cwd=workspace, capture_output=True, text=True, check=True, timeout=30,
    )
    return [f.strip() for f in result.stdout.splitlines() if f.strip()]


def extract_query(task: dict) -> str:
    desc = task["description"].strip()
    if len(desc) > 200:
        cutoff = desc[:200].rfind(". ")
        if cutoff > 80:
            desc = desc[:cutoff + 1]
        else:
            desc = desc[:200]
    return desc


def patch_config_toml(workspace: Path, patches: dict[str, str]) -> None:
    """Append settings to .bobbin/config.toml.

    patches keys are "section.key" format, values are raw TOML values.
    """
    config_path = workspace / ".bobbin" / "config.toml"
    if not config_path.exists():
        config_path.parent.mkdir(parents=True, exist_ok=True)
        config_path.write_text("")

    content = config_path.read_text()

    # Group by section
    sections: dict[str, list[tuple[str, str]]] = {}
    for dotkey, value in patches.items():
        section, key = dotkey.rsplit(".", 1)
        sections.setdefault(section, []).append((key, value))

    for section, kvs in sections.items():
        header = f"[{section}]"
        if header not in content:
            content += f"\n{header}\n"
        for key, value in kvs:
            content += f"{key} = {value}\n"

    config_path.write_text(content)
    logger.info("Patched config.toml: %s", patches)


def restore_config_toml(workspace: Path, original: str) -> None:
    """Restore original config.toml content."""
    config_path = workspace / ".bobbin" / "config.toml"
    config_path.write_text(original)


def run_bobbin_context(
    workspace: Path,
    query: str,
    variant: AblationVariant,
    bobbin: str,
) -> dict:
    """Run bobbin context with variant overrides."""
    sw = variant.cli_overrides.get("semantic_weight", BASELINE["semantic_weight"])
    dd = variant.cli_overrides.get("doc_demotion", BASELINE["doc_demotion"])
    k = variant.cli_overrides.get("rrf_k", BASELINE["rrf_k"])
    depth = variant.cli_overrides.get("depth", BASELINE["depth"])

    cmd = [
        bobbin, "context", "--json",
        "--semantic-weight", str(sw),
        "--doc-demotion", str(dd),
        "--rrf-k", str(k),
        "--depth", str(depth),
        query,
    ]

    t0 = time.monotonic()
    try:
        result = subprocess.run(
            cmd, cwd=workspace, capture_output=True, text=True, timeout=60,
        )
    except subprocess.TimeoutExpired:
        return {"error": "timeout", "duration_ms": (time.monotonic() - t0) * 1000}

    duration_ms = (time.monotonic() - t0) * 1000

    if result.returncode != 0:
        return {
            "error": f"exit {result.returncode}: {result.stderr.strip()[:200]}",
            "duration_ms": duration_ms,
        }

    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"error": "invalid JSON", "duration_ms": duration_ms}

    data["duration_ms"] = duration_ms
    return data


def compute_metrics(returned: list[str], ground_truth: list[str]) -> dict:
    if not returned and not ground_truth:
        return {"precision": 1.0, "recall": 1.0, "f1": 1.0}
    if not returned or not ground_truth:
        return {"precision": 0.0, "recall": 0.0, "f1": 0.0}

    ret_set = set(returned)
    gt_set = set(ground_truth)
    overlap = ret_set & gt_set

    p = len(overlap) / len(ret_set) if ret_set else 0.0
    r = len(overlap) / len(gt_set) if gt_set else 0.0
    f1 = (2 * p * r / (p + r)) if (p + r) > 0 else 0.0
    return {"precision": round(p, 4), "recall": round(r, 4), "f1": round(f1, 4)}


def run_ablation(task_ids: list[str] | None = None) -> dict:
    """Run all ablation variants on flask tasks."""
    bobbin = find_bobbin()
    logger.info("Using bobbin: %s", bobbin)

    tasks_to_run = task_ids or FLASK_TASKS
    tasks = [load_task(tid) for tid in tasks_to_run]

    all_results = []
    per_variant: dict[str, list[dict]] = {v.name: [] for v in VARIANTS}

    with tempfile.TemporaryDirectory(prefix="bobbin-ablation-") as tmpdir:
        # All flask tasks use pallets/flask — clone once
        from runner.workspace import clone_repo, checkout_parent

        repo = tasks[0]["repo"]
        logger.info("Cloning %s...", repo)
        ws = clone_repo(repo, tmpdir)

        # Init and index once at first task's parent commit
        first_task = tasks[0]
        parent = checkout_parent(ws, first_task["commit"])
        logger.info("Checked out parent %s", parent[:12])

        logger.info("Running bobbin init...")
        subprocess.run(
            [bobbin, "init"], cwd=ws, check=True,
            capture_output=True, text=True, timeout=30,
        )

        # Save original config.toml for restoration
        config_path = ws / ".bobbin" / "config.toml"
        original_config = config_path.read_text() if config_path.exists() else ""

        logger.info("Running bobbin index...")
        subprocess.run(
            [bobbin, "index"], cwd=ws, check=True,
            capture_output=True, text=True, timeout=600,
        )

        for task in tasks:
            task_id = task["id"]
            commit = task["commit"]

            # Checkout parent of this task's commit
            try:
                checkout_parent(ws, commit)
            except Exception as exc:
                logger.warning("Skipping %s: %s", task_id, exc)
                continue

            # Re-index for this commit state
            logger.info("Re-indexing for %s...", task_id)
            try:
                subprocess.run(
                    [bobbin, "index"], cwd=ws, check=True,
                    capture_output=True, text=True, timeout=600,
                )
            except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
                logger.warning("Re-index failed for %s: %s", task_id, exc)
                continue

            gt_files = get_ground_truth(ws, commit)
            if not gt_files:
                logger.warning("No ground truth files for %s", task_id)
                continue

            query = extract_query(task)
            logger.info("Task %s: %d GT files, query=%.60s...", task_id, len(gt_files), query)

            for variant in VARIANTS:
                # Apply config.toml patches if needed
                if variant.config_patch:
                    patch_config_toml(ws, variant.config_patch)

                result_data = run_bobbin_context(ws, query, variant, bobbin)

                # Restore config.toml
                if variant.config_patch:
                    restore_config_toml(ws, original_config)

                if "error" in result_data:
                    logger.warning("  %s/%s: error: %s", task_id, variant.name, result_data["error"])
                    entry = {
                        "task_id": task_id,
                        "variant": variant.name,
                        "description": variant.description,
                        "error": result_data["error"],
                        "precision": 0.0, "recall": 0.0, "f1": 0.0,
                        "returned_files": [],
                        "ground_truth_files": gt_files,
                        "duration_ms": result_data.get("duration_ms", 0),
                    }
                    all_results.append(entry)
                    per_variant[variant.name].append(entry)
                    continue

                # Extract returned file paths
                ws_prefix = str(ws) + "/"
                returned_files = []
                for f in result_data.get("files", []):
                    p = f["path"]
                    if p.startswith(ws_prefix):
                        p = p[len(ws_prefix):]
                    returned_files.append(p)

                metrics = compute_metrics(returned_files, gt_files)

                entry = {
                    "task_id": task_id,
                    "variant": variant.name,
                    "description": variant.description,
                    "returned_files": returned_files,
                    "ground_truth_files": gt_files,
                    "duration_ms": result_data.get("duration_ms", 0),
                    **metrics,
                }
                all_results.append(entry)
                per_variant[variant.name].append(entry)

                logger.info(
                    "  %s/%s: P=%.3f R=%.3f F1=%.3f (%d files)",
                    task_id, variant.name,
                    metrics["precision"], metrics["recall"], metrics["f1"],
                    len(returned_files),
                )

    return {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "bobbin": bobbin,
        "baseline_config": BASELINE,
        "variants": [
            {"name": v.name, "description": v.description,
             "cli_overrides": {k: str(val) for k, val in v.cli_overrides.items()},
             "config_patch": v.config_patch}
            for v in VARIANTS
        ],
        "results": all_results,
        "summary": build_summary(per_variant),
    }


def build_summary(per_variant: dict[str, list[dict]]) -> list[dict]:
    """Build per-variant summary averages."""
    summary = []
    for name, results in per_variant.items():
        valid = [r for r in results if "error" not in r]
        if not valid:
            summary.append({"variant": name, "tasks": 0, "avg_f1": 0.0})
            continue
        n = len(valid)
        summary.append({
            "variant": name,
            "tasks": n,
            "avg_precision": round(sum(r["precision"] for r in valid) / n, 4),
            "avg_recall": round(sum(r["recall"] for r in valid) / n, 4),
            "avg_f1": round(sum(r["f1"] for r in valid) / n, 4),
            "avg_duration_ms": round(sum(r["duration_ms"] for r in valid) / n, 1),
        })
    return sorted(summary, key=lambda s: s.get("avg_f1", 0), reverse=True)


def generate_markdown(data: dict) -> str:
    """Generate markdown report from ablation results."""
    lines = ["# Flask Search-Level Ablation Report\n"]
    lines.append(f"Generated: {data['timestamp']}\n")
    lines.append(f"Baseline config: sw={BASELINE['semantic_weight']}, "
                 f"dd={BASELINE['doc_demotion']}, k={BASELINE['rrf_k']}, "
                 f"depth={BASELINE['depth']}\n")

    # Summary table
    lines.append("## Summary (sorted by F1)\n")
    lines.append("| Variant | Description | Tasks | Precision | Recall | F1 | Latency |")
    lines.append("|---------|-------------|-------|-----------|--------|----|---------|")

    for s in data["summary"]:
        lines.append(
            f"| {s['variant']} | "
            f"{next((v['description'] for v in data['variants'] if v['name'] == s['variant']), '')} | "
            f"{s['tasks']} | "
            f"{s.get('avg_precision', 0):.3f} | "
            f"{s.get('avg_recall', 0):.3f} | "
            f"**{s.get('avg_f1', 0):.3f}** | "
            f"{s.get('avg_duration_ms', 0):.0f}ms |"
        )

    # Delta from baseline
    baseline_f1 = next(
        (s.get("avg_f1", 0) for s in data["summary"] if s["variant"] == "baseline"), 0
    )
    lines.append("\n## Delta from Baseline\n")
    lines.append("| Variant | F1 | Delta | Impact |")
    lines.append("|---------|-----|-------|--------|")
    for s in data["summary"]:
        f1 = s.get("avg_f1", 0)
        delta = f1 - baseline_f1
        impact = "BASELINE" if s["variant"] == "baseline" else (
            f"+{delta:.3f}" if delta > 0 else f"{delta:.3f}"
        )
        marker = "" if s["variant"] == "baseline" else (
            " (helps)" if delta > 0 else " (hurts)" if delta < 0 else " (neutral)"
        )
        lines.append(
            f"| {s['variant']} | {f1:.3f} | {impact} | "
            f"{'—' if s['variant'] == 'baseline' else 'Removing this ' + ('hurts' if delta < 0 else 'helps' if delta > 0 else 'has no effect')}{marker} |"
        )

    # Per-task detail
    lines.append("\n## Per-Task Results\n")
    task_ids = sorted(set(r["task_id"] for r in data["results"]))
    for task_id in task_ids:
        task_results = [r for r in data["results"] if r["task_id"] == task_id]
        if not task_results:
            continue
        gt = task_results[0]["ground_truth_files"]
        lines.append(f"\n### {task_id}\n")
        lines.append(f"Ground truth: {', '.join(gt)}\n")
        lines.append("| Variant | Precision | Recall | F1 | Returned Files |")
        lines.append("|---------|-----------|--------|----|----------------|")
        for r in sorted(task_results, key=lambda x: x["f1"], reverse=True):
            ret = ", ".join(r["returned_files"][:5])
            if len(r["returned_files"]) > 5:
                ret += f" (+{len(r['returned_files']) - 5} more)"
            lines.append(
                f"| {r['variant']} | {r['precision']:.3f} | {r['recall']:.3f} | "
                f"{r['f1']:.3f} | {ret} |"
            )

    return "\n".join(lines)


def main():
    import sys
    task_ids = None
    if "--task" in sys.argv:
        idx = sys.argv.index("--task")
        if idx + 1 < len(sys.argv):
            task_ids = [sys.argv[idx + 1]]

    logger.info("Starting flask ablation tests...")
    data = run_ablation(task_ids)

    # Save JSON
    RESULTS_DIR.mkdir(parents=True, exist_ok=True)
    json_path = RESULTS_DIR / "ablation-flask-search.json"
    json_path.write_text(json.dumps(data, indent=2))
    logger.info("JSON results: %s", json_path)

    # Save markdown
    md_path = RESULTS_DIR / "ablation-flask-search.md"
    md_path.write_text(generate_markdown(data))
    logger.info("Markdown report: %s", md_path)

    # Print summary
    print("\n" + "=" * 70)
    print("ABLATION RESULTS SUMMARY")
    print("=" * 70)
    baseline_f1 = next(
        (s.get("avg_f1", 0) for s in data["summary"] if s["variant"] == "baseline"), 0
    )
    print(f"\nBaseline F1: {baseline_f1:.3f}\n")
    print(f"{'Variant':<20} {'F1':>8} {'Delta':>8} {'Effect'}")
    print("-" * 55)
    for s in data["summary"]:
        f1 = s.get("avg_f1", 0)
        delta = f1 - baseline_f1
        effect = "BASELINE" if s["variant"] == "baseline" else (
            "helps" if delta > 0 else "hurts" if delta < 0 else "neutral"
        )
        print(f"{s['variant']:<20} {f1:>8.3f} {delta:>+8.3f} {effect}")


if __name__ == "__main__":
    main()
