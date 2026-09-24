"""Tests for the eval v2 L0 harness (runner.l0).

Pure logic plus a synthetic git repository for the gold and baseline paths.
Nothing here runs an eval over the task set.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pytest

from runner import l0

FIXTURES = Path(__file__).resolve().parent.parent / "v2" / "fixtures"


# --- gold ------------------------------------------------------------------

DIFF = """\
diff --git a/src/app.py b/src/app.py
index 1..2 100644
--- a/src/app.py
+++ b/src/app.py
@@ -10,3 +10,4 @@ def f():
@@ -40,0 +42,2 @@ def g():
diff --git a/tests/test_app.py b/tests/test_app.py
--- a/tests/test_app.py
+++ b/tests/test_app.py
@@ -1,2 +1,3 @@
diff --git a/src/new.py b/src/new.py
new file mode 100644
--- /dev/null
+++ b/src/new.py
@@ -0,0 +1,5 @@
diff --git a/Cargo.lock b/Cargo.lock
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -5 +5 @@
diff --git a/src/top.py b/src/top.py
--- a/src/top.py
+++ b/src/top.py
@@ -0,0 +1 @@
@@ -7 +8 @@
"""


def test_gold_uses_preimage_ranges_and_excludes_non_context():
    gold = l0.gold_from_diff(DIFF)
    assert gold == {
        "src/app.py": [(10, 12), (40, 40)],  # pure insertion -> the line it follows
        "src/top.py": [(1, 1), (7, 7)],  # insertion at the top -> line 1; bare "-7" -> 1 line
    }


@pytest.mark.parametrize(
    "path,excluded",
    [
        ("tests/test_x.py", True),
        ("pkg/test_utils.py", True),
        ("src/foo_test.go", True),
        ("web/a.spec.ts", True),
        ("crates/x/tests/it.rs", True),
        ("CHANGELOG.md", True),
        ("uv.lock", True),
        ("src/contest.py", False),
        ("src/latest.rs", False),
        ("src/testing_utils_impl.rs", False),
    ],
)
def test_gold_exclusions(path, excluded):
    assert l0.is_excluded_from_gold(path) is excluded


# --- hook output -------------------------------------------------------------

INJECTED = """\
Bobbin found 2 relevant files (2 direct, 0 coupled, 0 bridged, 12/300 budget lines, 0 held back) [injection_id: x]:
--- src/app.py:8-14 f (function, score 0.91) ---
def f():
    pass
--- src/other.py:1-5 other (function, score 0.40) ---
x = 1
"""


def test_parse_injected_chunks():
    assert l0.parse_injected_chunks(INJECTED) == [("src/app.py", 8, 14), ("src/other.py", 1, 5)]


def test_classify_error_is_never_read_as_nothing_to_inject():
    # The hook exits 0 on error by design; stderr is the only discriminator.
    outcome, detail = l0.classify_hook_result(0, "", "bobbin inject-context: Failed to open index")
    assert outcome == "error" and "Failed to open index" in detail


def test_classify_skip_carries_the_reason():
    assert l0.classify_hook_result(0, "", "bobbin: skipped (semantic=0.31 < gate=0.45)\n") == (
        "skipped",
        "bobbin: skipped (semantic=0.31 < gate=0.45)",
    )


def test_classify_injected_and_unparseable():
    assert l0.classify_hook_result(0, INJECTED, "bobbin: injecting 2 files")[0] == "injected"
    assert l0.classify_hook_result(0, "<xml>no headers</xml>", "")[0] == "error"


# --- metrics -----------------------------------------------------------------


def test_score_chunks_recall_precision_density():
    gold = {"src/app.py": [(10, 12), (40, 40)], "src/b.py": [(3, 3)]}
    chunks = [("src/app.py", 8, 14), ("src/other.py", 1, 5)]
    m = l0.score_chunks(chunks, gold, injected_chars=400)
    assert m["gold_files"] == 2 and m["gold_hunks"] == 3 and m["gold_lines"] == 5
    assert m["file_recall"] == pytest.approx(1 / 2)
    assert m["hunk_recall"] == pytest.approx(1 / 3)
    assert m["file_precision"] == pytest.approx(1 / 2)
    assert m["injected_lines"] == 12
    assert m["density"] == pytest.approx(3 / 12)
    assert m["injected_tokens"] == 100


def test_density_is_undefined_not_zero_when_nothing_injected():
    m = l0.score_chunks([], {"a.py": [(1, 2)]})
    assert m["density"] is None
    assert m["hunk_recall"] == 0.0


def test_overlapping_chunks_are_not_double_counted():
    m = l0.score_chunks([("a.py", 1, 10), ("a.py", 5, 12)], {"a.py": [(4, 6)]})
    assert m["injected_lines"] == 12
    assert m["density"] == pytest.approx(3 / 12)


def test_fill_budget_truncates_the_last_chunk():
    got = l0._fill_budget([("a", 1, 10), ("b", 1, 10), ("c", 1, 10)], 15)
    assert got == [("a", 1, 10), ("b", 1, 5)]


# --- statistics --------------------------------------------------------------


def test_holm_is_monotone_and_capped():
    adj = l0.holm({"a": 0.01, "b": 0.04, "c": 0.03})
    assert adj["a"] == pytest.approx(0.03)
    assert adj["c"] == pytest.approx(0.06)
    assert adj["b"] == pytest.approx(0.06)  # monotone: never below the previous step
    assert max(adj.values()) <= 1.0


def test_wilcoxon_separates_a_consistent_shift_from_noise():
    assert l0.wilcoxon_signed_rank([0.1 + i * 0.01 for i in range(30)]) < 0.001
    assert l0.wilcoxon_signed_rank([0.1, -0.1] * 15) > 0.5
    assert l0.wilcoxon_signed_rank([0.0] * 10) == 1.0


def test_bootstrap_is_seeded():
    diffs = [0.1, -0.2, 0.3, 0.05, 0.0, 0.15]
    assert l0.paired_bootstrap_ci(diffs, resamples=500) == l0.paired_bootstrap_ci(
        diffs, resamples=500
    )


def test_compare_pairs_by_task_and_excludes_errors():
    S = l0.ArmScore
    scores = [
        S("t1", "full", 300, "injected", hunk_recall=0.5),
        S("t1", "-semantic", 300, "injected", hunk_recall=0.25),
        S("t2", "full", 300, "injected", hunk_recall=1.0),
        S("t2", "-semantic", 300, "skipped", hunk_recall=0.0),
        S("t3", "full", 300, "injected", hunk_recall=1.0),
        S("t3", "-semantic", 300, "error", detail="boom"),
    ]
    rows = l0.compare_to_reference(scores, "hunk_recall")
    assert rows["-semantic"]["n"] == 2
    assert rows["-semantic"]["mean_diff"] == pytest.approx((-0.25 - 1.0) / 2)
    assert "p_holm" in rows["-semantic"]


# --- the calibration contract with bobbin -----------------------------------


@pytest.mark.parametrize(
    "arm,fixture",
    [
        ("full", "calibration-full.json"),
        ("-blame", "calibration-noblame.json"),
        ("-semantic", "calibration-nosemantic.json"),
    ],
)
def test_calibration_output_matches_the_fixture_bobbin_tests(tmp_path, arm, fixture):
    # src/cli/calibrate.rs deserializes these same fixtures. If the harness
    # output drifts from them, this fails before an arm silently runs as `full`.
    l0.write_calibration(tmp_path, l0.ABLATIONS[arm].get("calibration", {}))
    written = json.loads((tmp_path / ".bobbin" / "calibration.json").read_text())
    assert written == json.loads((FIXTURES / fixture).read_text())


def test_every_ablation_changes_exactly_one_thing():
    for arm, spec in l0.ABLATIONS.items():
        knobs = (
            len(spec.get("calibration", {})) + len(spec.get("hook_args", [])) + ("index" in spec)
        )
        assert knobs == (0 if arm == "full" else 1), arm


# --- synthetic repository ------------------------------------------------------


def _git(cwd, *args):
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True)


@pytest.fixture
def repo(tmp_path):
    _git(tmp_path, "init", "-q")
    _git(tmp_path, "config", "user.email", "eval@example.test")
    _git(tmp_path, "config", "user.name", "eval")
    (tmp_path / "session.py").write_text(
        "def save_session(resp):\n    resp.set_cookie('s')\n\n\ndef load():\n    return 1\n"
    )
    (tmp_path / "unrelated.py").write_text("def other():\n    return 2\n")
    _git(tmp_path, "add", ".")
    _git(tmp_path, "commit", "-q", "-m", "base")
    (tmp_path / "session.py").write_text(
        "def save_session(resp):\n    resp.headers.add('Vary', 'Cookie')\n    resp.set_cookie('s')\n\n\n"
        "def load():\n    return 1\n"
    )
    _git(tmp_path, "commit", "-q", "-am", "fix vary cookie")
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=tmp_path, capture_output=True, text=True, check=True
    ).stdout.strip()
    _git(tmp_path, "checkout", "-q", f"{commit}^")
    return tmp_path, commit


def test_gold_for_commit_on_a_real_repo(repo):
    ws, commit = repo
    assert l0.gold_for_commit(ws, commit) == {"session.py": [(1, 1)]}


def test_bm25_ranks_the_relevant_window_first(repo):
    ws, _ = repo
    chunks = l0.baseline_bm25(ws, "Set the Vary Cookie header in save_session", 300)
    assert chunks and chunks[0][0] == "session.py"


def test_rg_baseline_finds_prompt_identifiers(repo):
    ws, _ = repo
    if not l0.shutil.which("rg"):
        pytest.skip("ripgrep not installed")
    chunks = l0.baseline_rg(ws, "save_session should set the cookie", 300)
    assert chunks and chunks[0][0] == "session.py"


def test_isolated_env_drops_bobbin_vars(tmp_path, monkeypatch):
    monkeypatch.setenv("BOBBIN_SERVER", "http://shared.example")
    env = l0.isolated_env(tmp_path)
    assert "BOBBIN_SERVER" not in env
    assert env["XDG_CONFIG_HOME"].startswith(str(tmp_path))


def test_l0_cli_uses_the_path_clone_repo_returns(tmp_path, monkeypatch):
    # clone_repo creates <dest>/<owner>--<name>; the first L0 run used <dest>
    # itself and died with "not a git repository" after a full clone.
    from click.testing import CliRunner

    from runner import cli as cli_mod
    from runner import workspace as ws_mod
    from runner import bobbin_setup as bs_mod

    tasks = tmp_path / "tasks"
    tasks.mkdir()
    (tasks / "demo-001.yaml").write_text(
        "id: demo-001\nrepo: owner/demo\ncommit: abc1234def\n"
        "description: fix the thing in the demo\nsetup_command: 'true'\n"
        "test_command: 'true'\nlanguage: python\ndifficulty: easy\ntags: [x]\n"
    )
    seen = {}

    def fake_clone(repo, dest, **kw):
        path = Path(dest) / repo.replace("/", "--")
        path.mkdir(parents=True)
        return path

    monkeypatch.setattr(ws_mod, "clone_repo", fake_clone)
    monkeypatch.setattr(ws_mod, "checkout_parent", lambda ws, c: seen.setdefault("checkout", ws))
    monkeypatch.setattr(bs_mod, "setup_bobbin", lambda ws, **kw: seen.setdefault("index", Path(ws)))
    monkeypatch.setattr(bs_mod, "_find_bobbin", lambda: "true")
    monkeypatch.setattr(l0, "run_task", lambda task, ws, *a, **k: seen.setdefault("score", ws) and [])

    out = tmp_path / "scores.jsonl"
    result = CliRunner().invoke(
        cli_mod.cli,
        ["l0", "--tasks-dir", str(tasks), "--arms", "full", "--budgets", "300",
         "--out", str(out), "--workdir", str(tmp_path / "work")],
    )
    assert result.exit_code == 0, result.output
    expected = tmp_path / "work" / "demo-001" / "owner--demo"
    assert seen["checkout"] == expected
    assert seen["index"] == expected
    assert seen["score"] == expected
