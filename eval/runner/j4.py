"""J4: per-chunk admission with a learned floor (eval v2, PREREGISTRATION amendment A1).

The shipped quality gate is query-level: one cosine threshold, then everything or nothing.
J4 judges each candidate chunk instead: one Jev ``noul`` per candidate ("would a developer
need this passage to complete the task?"). It keeps the candidates at or above a floor, and
returns a typed ``abstained`` outcome when none pass.

The design keeps the expensive step and the tunable step apart:

* ``judge_task`` asks Jev ONCE per candidate and records every probability;
* ``admit`` / ``choose_floor`` / the metrics are pure functions over those records.

So the floor grid is swept offline, with no further calls, and a reader can re-derive every
reported number from the recorded probabilities.

Outcomes follow l0's rule that they are never conflated. ``abstained`` is J4 choosing NO
CONTEXT. It is not ``skipped`` (the hook's own gate) and not ``error``. Its density and chunk
precision are undefined: never 0, never perfect.
"""

from __future__ import annotations

import json
import os
import random
import time
import urllib.error
import urllib.request
from collections import defaultdict
from collections.abc import Callable, Iterable, Sequence
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from runner.l0 import Chunk, _fill_budget, _merge, _overlap

# ---------------------------------------------------------------------------
# Preregistered constants (amendment A1). Changing any of these is a deviation.

MAX_CANDIDATES = 20
CANDIDATE_BUDGET = 600
CANDIDATE_ARM = "-gate"
PASSAGE_MAX_LINES = 120
QUESTION = (
    "Does this passage contain code or text that a developer would need to read or change "
    "to complete the task?"
)
FLOOR_GRID = tuple(round(0.05 * i, 2) for i in range(1, 20))
RECALL_MARGIN = 0.02
GATE_GRID = tuple(round(0.30 + 0.05 * i, 2) for i in range(9))
REPO_CYCLE = ("cargo", "django", "go", "nushell", "pandas", "polars", "ruff", "typst")
CALIBRATION_FILE = Path(__file__).resolve().parent.parent / "v2" / "j4-calibration-tasks.txt"

JEV_ENDPOINT = "https://api.typesafe.ai/v1/systemone"
#: Pinned by name (A1). The API accepts a version name and refuses an unknown one.
JEV_MODEL = "jev-1.13.0"
#: A failed judgement is retried this many more times, this far apart (A1).
JUDGE_RETRIES = 2
JUDGE_RETRY_DELAY_S = 5.0

#: A judge takes (state, question) and returns (probability, model version reported).
Judge = Callable[[str, str], tuple[float, str]]


class JevUnavailable(RuntimeError):
    """No usable key or no answer. Never silently degraded into "admit everything"."""


# ---------------------------------------------------------------------------
# Splits and the negative set


def calibration_ids(path: Path = CALIBRATION_FILE) -> list[str]:
    """The frozen calibration split, read from the committed file (never recomputed)."""
    return [
        ln.strip() for ln in path.read_text().splitlines() if ln.strip() and not ln.startswith("#")
    ]


def calibration_split(task_ids: Iterable[str], seed: int = 20260924) -> list[str]:
    """How the committed split was drawn: one RNG, repos in sorted order, 2 per repo.

    Kept only so a test can prove the committed file matches the registered procedure.
    """
    by_repo: dict[str, list[str]] = defaultdict(list)
    for t in task_ids:
        by_repo[t.rsplit("-", 1)[0]].append(t)
    rng = random.Random(seed)
    out: list[str] = []
    for repo in sorted(by_repo):
        out += sorted(rng.sample(sorted(by_repo[repo]), 2))
    return out


def negative_partner(task_id: str, task_ids: Iterable[str]) -> str | None:
    """The workspace a task's prompt is run against in the negative set.

    Task i of repo R -> task i of the next repo in REPO_CYCLE. ``None`` when that task does
    not exist (it is then reported as missing, never substituted).
    """
    repo, idx = task_id.rsplit("-", 1)
    if repo not in REPO_CYCLE:
        return None
    nxt = REPO_CYCLE[(REPO_CYCLE.index(repo) + 1) % len(REPO_CYCLE)]
    partner = f"{nxt}-{idx}"
    return partner if partner in set(task_ids) else None


# ---------------------------------------------------------------------------
# Judging


def build_state(prompt: str, workspace: Path, chunk: Chunk) -> str:
    path, start, end = chunk
    try:
        lines = (workspace / path).read_text(errors="replace").splitlines()
    except OSError as exc:
        raise JevUnavailable(f"cannot read candidate {path}: {exc}") from exc
    passage = lines[start - 1 : min(end, start - 1 + PASSAGE_MAX_LINES)]
    return f"Task: {prompt.strip()}\n\nPassage ({path}:{start}-{end}):\n" + "\n".join(passage)


def resolve_key() -> str:
    """The key from ``TYPESAFE_API_KEY``, else the file named by ``TYPESAFE_API_KEY_FILE``."""
    key = os.environ.get("TYPESAFE_API_KEY", "").strip()
    if not key and os.environ.get("TYPESAFE_API_KEY_FILE"):
        try:
            key = Path(os.environ["TYPESAFE_API_KEY_FILE"]).expanduser().read_text().strip()
        except OSError as exc:
            raise JevUnavailable(f"TYPESAFE_API_KEY_FILE unreadable: {exc}") from exc
    if not key or any(c.isspace() for c in key):
        raise JevUnavailable("no Jev key: set TYPESAFE_API_KEY or TYPESAFE_API_KEY_FILE")
    return key


def jev_judge(timeout: float = 30.0) -> Judge:
    """A live judge over Jev's HTTP API. Every failure raises; nothing defaults to admit."""
    key = resolve_key()

    def judge(state: str, question: str) -> tuple[float, str]:
        body = json.dumps(
            {
                "model": JEV_MODEL,
                "state": state,
                "questions": {"q": {"type": "noul", "instructions": question}},
            }
        ).encode()
        req = urllib.request.Request(
            JEV_ENDPOINT,
            data=body,
            headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                data = json.loads(resp.read())
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
            raise JevUnavailable(f"Jev request failed: {exc}") from exc
        try:
            return float(data["answers"]["q"]["noul"]), str(data.get("model", ""))
        except (KeyError, TypeError, ValueError) as exc:
            raise JevUnavailable(f"Jev answer has no noul: {str(data)[:200]}") from exc

    return judge


@dataclass
class Judged:
    """One task (or one negative pairing) with every candidate's recorded probability."""

    task_id: str
    set: str  # "positive" | "negative"
    workspace_task: str  # whose workspace the candidates came from
    candidate_outcome: str  # the candidate arm's own outcome (injected/skipped/error)
    candidates: list[Chunk] = field(default_factory=list)
    probs: list[float] = field(default_factory=list)
    model: str = ""
    detail: str = ""


def judge_task(
    task_id: str,
    set_name: str,
    workspace_task: str,
    prompt: str,
    workspace: Path,
    candidate_outcome: str,
    candidates: Sequence[Chunk],
    judge: Judge,
    *,
    workers: int = 8,
) -> Judged:
    cands = list(candidates)[:MAX_CANDIDATES]
    rec = Judged(task_id, set_name, workspace_task, candidate_outcome, cands)
    if not cands:
        return rec
    try:
        states = [build_state(prompt, workspace, c) for c in cands]
        with ThreadPoolExecutor(max_workers=workers) as pool:
            answers = list(pool.map(lambda s: judge_with_retries(judge, s), states))
    except JevUnavailable as exc:
        # A task with ANY unjudged candidate is an error as a whole (A1): partially judged
        # tasks would let failed calls quietly change what gets admitted.
        rec.candidate_outcome, rec.detail = "error", str(exc)[:300]
        rec.probs = []
        return rec
    rec.probs = [p for p, _ in answers]
    rec.model = ",".join(sorted({m for _, m in answers if m}))
    return rec


def judge_with_retries(
    judge: Judge, state: str, *, sleep: Callable[[float], None] = time.sleep
) -> tuple[float, str]:
    """One judgement, retried JUDGE_RETRIES more times on JevUnavailable, then raised."""
    for attempt in range(JUDGE_RETRIES + 1):
        try:
            return judge(state, QUESTION)
        except JevUnavailable:
            if attempt == JUDGE_RETRIES:
                raise
            sleep(JUDGE_RETRY_DELAY_S)
    raise AssertionError("unreachable")


def refuse_mixed_models(records: Iterable[Judged], pinned: str = JEV_MODEL) -> None:
    """Refuse to score a run whose responses report any model but the pinned one (A1)."""
    seen = {m for r in records for m in r.model.split(",") if m}
    foreign = seen - {pinned}
    if foreign:
        raise ValueError(
            f"judge responses report {sorted(foreign)}, not the pinned {pinned}: refusing to score"
        )


# ---------------------------------------------------------------------------
# Admission and metrics (pure: no calls)


def admit(rec: Judged, floor: float, budget: int) -> tuple[str, list[Chunk]]:
    """(outcome, chunks). ``abstained`` when candidates existed and none reached the floor."""
    if rec.candidate_outcome == "error":
        return "error", []
    if not rec.candidates:
        return "skipped", []
    kept = [c for c, p in zip(rec.candidates, rec.probs, strict=True) if p >= floor]
    if not kept:
        return "abstained", []
    return "injected", _fill_budget(kept, budget)


def chunk_precision_at_k(
    chunks: Sequence[Chunk], gold: dict[str, list[tuple[int, int]]], k: int
) -> float | None:
    """Of the first k injected chunks, the fraction overlapping a gold hunk. None if none."""
    head = list(chunks)[:k]
    if not head:
        return None
    hits = sum(1 for p, s, e in head if p in gold and _overlap([(s, e)], _merge(gold[p])) > 0)
    return hits / len(head)


def abstention_rate(outcomes: Iterable[str]) -> float | None:
    """Fraction of tasks that injected nothing on purpose (abstained or skipped).

    On the negative set this is abstention COVERAGE; on the positive set it is FALSE
    abstention. Errors are excluded from both numerator and denominator.
    """
    scored = [o for o in outcomes if o != "error"]
    if not scored:
        return None
    return sum(1 for o in scored if o in ("abstained", "skipped")) / len(scored)


def choose_floor(
    rows: Sequence[tuple[Judged, dict[str, list[tuple[int, int]]]]],
    full_mean_recall: float,
    score: Callable[[list[Chunk], dict[str, list[tuple[int, int]]]], dict[str, Any]],
    budget: int = 300,
) -> tuple[float | None, list[dict[str, Any]]]:
    """The registered rule, on calibration positives only.

    Maximise mean density subject to mean hunk recall >= full's mean recall - RECALL_MARGIN;
    ties go to the lower floor. An abstention contributes recall 0 (the gold was not shown)
    and no density. Returns (floor or None if no floor qualifies, the whole curve).
    """
    curve = []
    for floor in FLOOR_GRID:
        recalls, densities, abstained = [], [], 0
        for rec, gold in rows:
            outcome, chunks = admit(rec, floor, budget)
            if outcome == "error":
                continue
            if outcome in ("abstained", "skipped"):
                abstained += 1
                recalls.append(0.0)
                continue
            m = score(chunks, gold)
            recalls.append(m["hunk_recall"] or 0.0)
            if m["density"] is not None:
                densities.append(m["density"])
        curve.append(
            {
                "floor": floor,
                "mean_recall": sum(recalls) / len(recalls) if recalls else None,
                "mean_density": sum(densities) / len(densities) if densities else None,
                "abstained": abstained,
                "n": len(recalls),
            }
        )
    ok = [
        c
        for c in curve
        if c["mean_recall"] is not None
        and c["mean_density"] is not None
        and c["mean_recall"] >= full_mean_recall - RECALL_MARGIN
    ]
    if not ok:
        return None, curve
    best = max(ok, key=lambda c: (c["mean_density"], -c["floor"]))
    return best["floor"], curve
