"""Controls for the paid pilot's assignment and grading boundaries."""
import json
import os
import subprocess

import pytest

from runner import pilot


def test_schedule_has_all_four_cells_once_and_reproduces():
    tasks = [str(i) for i in range(30)]
    rows = pilot.schedule(tasks)
    assert len(rows) == len(set(rows)) == 120
    assert rows == pilot.schedule(tasks)
    assert len({tuple(c for t, c in rows if t == task) for task in tasks}) > 1
    for task in tasks:
        assert {c for t, c in rows if t == task} == set(pilot.CELLS)


@pytest.mark.parametrize("cell,tool,inject", [
    ("none", False, False), ("tool", True, False),
    ("inject", False, True), ("both", True, True),
])
def test_factors_are_independent(cell, tool, inject):
    settings, mcp, denied = pilot.cell_config(cell, "/test/bin/bobbin")
    assert bool(mcp["mcpServers"]) == tool
    assert bool(settings["hooks"]) == inject
    assert "Bash(bobbin *)" in denied


@pytest.mark.parametrize("output,rc,passed,valid", [
    ("test result: ok. 0 passed; 0 failed; 0 ignored;", 0, False, False),
    ("test result: ok. 1 passed; 0 failed; 0 ignored;", 0, True, True),
    ("test result: FAILED. 0 passed; 1 failed; 0 ignored;", 1, False, True),
    ("2 skipped in 0.01s", 0, False, False),
    ("all green", 0, False, False),
    # go test -v: top-level result lines are the only execution evidence.
    ("=== RUN   TestX\n--- PASS: TestX (0.00s)\nPASS\nok  \tdebug/elf\t0.004s\n", 0, True, True),
    ("--- FAIL: TestX (0.00s)\nFAIL\nFAIL\tdebug/elf\t0.002s\nFAIL\n", 1, False, True),
    ("--- FAIL: TestX (0.00s)\n    --- PASS: TestX/sub (0.00s)\nFAIL\n", 1, False, True),
    ("--- SKIP: TestX (0.00s)\nok  \tdebug/elf\t0.002s\n", 0, False, False),
    ("ok  \tdebug/elf\t0.002s\n", 0, False, False),
    ("ok  \tdebug/elf\t0.002s [no tests to run]\n", 0, False, False),
])
def test_grader_requires_observed_execution(tmp_path, monkeypatch, output, rc, passed, valid):
    monkeypatch.setattr(pilot.subprocess, "run", lambda *a, **k:
                        subprocess.CompletedProcess([], rc, output, ""))
    result = pilot.grade(tmp_path, "tests", {}, 1)
    assert result["passed"] is passed
    assert result["valid"] is valid


def test_cell_checkout_cannot_read_fix_history(tmp_path):
    env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
    source, dest = tmp_path / "source", tmp_path / "cell"
    source.mkdir()
    pilot.run(["git", "init", "-q"], source, env)
    pilot.run(["git", "config", "core.hooksPath", "/dev/null"], source, env)
    (source / "code.py").write_text("bug\n")
    pilot.run(["git", "add", "code.py"], source, env)
    commit = ["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm"]
    pilot.run([*commit, "parent"], source, env)
    (source / "code.py").write_text("fixed\n")
    pilot.run(["git", "add", "code.py"], source, env)
    pilot.run([*commit, "answer"], source, env)
    fix = pilot.run(["git", "rev-parse", "HEAD"], source, env).stdout.strip()
    pilot.run(["git", "checkout", "--detach", "HEAD^"], source, env)
    pilot.cell_checkout(source, dest, env)
    assert (dest / "code.py").read_text() == "bug\n"
    assert pilot.run(["git", "rev-list", "--count", "HEAD"], dest, env).stdout.strip() == "1"
    result = subprocess.run(["git", "cat-file", "-e", fix], cwd=dest, capture_output=True, check=False)
    assert result.returncode != 0


def test_isolation_copies_credentials_without_real_home_writes(tmp_path, monkeypatch):
    real, sandbox = tmp_path / "real", tmp_path / "sandbox"
    (real / ".claude").mkdir(parents=True)
    cred = real / ".claude/.credentials.json"
    cred.write_text(json.dumps({"fake": "credential"}))
    before = cred.stat().st_mtime_ns
    monkeypatch.setattr(pilot.Path, "home", lambda: real)
    env = pilot.isolated_env(sandbox)
    copy = sandbox / ".claude/.credentials.json"
    assert not copy.is_symlink()
    assert copy.stat().st_ino != cred.stat().st_ino
    copy.write_text("changed")
    assert cred.stat().st_mtime_ns == before
    assert env["HOME"] == str(sandbox)


def test_cell_checkout_preserves_ignored_paths_after_guidance_removal(tmp_path):
    import shutil

    env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
    source, dest = tmp_path / "source", tmp_path / "cell"
    source.mkdir()
    pilot.run(["git", "init", "-q"], source, env)
    pilot.run(["git", "config", "core.hooksPath", "/dev/null"], source, env)
    (source / ".claude").mkdir()
    (source / ".claude/settings.json").write_text('{}')
    (source / ".gitignore").write_text('ignored.py\ncache/\n')
    (source / "ignored.py").write_text('tracked despite ignore\n')
    (source / "literal[1].py").write_text('literal path\n')
    pilot.run(["git", "add", "--force", "."], source, env)
    pilot.run(["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
               "commit", "-qm", "parent"], source, env)
    shutil.rmtree(source / ".claude")
    (source / "cache").mkdir()
    (source / "cache/index").write_text('prepared untracked index')
    pilot.cell_checkout(source, dest, env)
    assert set(pilot.run(["git", "ls-files"], dest, env).stdout.splitlines()) == {
        '.gitignore', 'ignored.py', 'literal[1].py'}
    assert (dest / "cache/index").read_text() == 'prepared untracked index'
    assert not (dest / '.claude').exists()


@pytest.mark.parametrize('model,subtype,terminal,rc,limited', [
    (pilot.MODEL, 'error_max_turns', 'max_turns', 1, True),
    ('wrong-model', 'error_max_turns', 'max_turns', 1, False),
    (pilot.MODEL, 'error_during_execution', 'max_turns', 1, False),
    (pilot.MODEL, 'error_max_turns', None, 1, False),
    (pilot.MODEL, 'error_max_turns', 'max_turns', -1, False),
])
def test_turn_limit_is_failed_cell_without_false_unavailability(
        tmp_path, monkeypatch, model, subtype, terminal, rc, limited):
    receipt = {'type': 'result', 'is_error': True, 'subtype': subtype,
               'terminal_reason': terminal, 'modelUsage': {model: {}}}
    monkeypatch.setattr(pilot, 'parse_stream_json', lambda _: {
        'result_line': receipt, 'tool_use_summary': {}})
    monkeypatch.setattr(pilot.subprocess, 'run', lambda *a, **kw:
                        subprocess.CompletedProcess([], rc))
    result = pilot.invoke({'repo': 'fixture', 'description': 'Fix bug'}, 'none',
                          tmp_path, tmp_path, {}, '/bin/bobbin', '/bin/claude', 2, 900, 40)
    assert result['valid'] is False
    assert result['turn_limit_reached'] is limited


@pytest.mark.parametrize("path,hidden", [
    ("crates/ruff_linter/src/rules/flake8_pyi/snapshots/"
     "ruff_linter__rules__flake8_pyi__tests__PYI034_PYI034.py.snap", True),
    ("crates/ruff_python_formatter/tests/snapshots/format@statement__try.py.snap", True),
    ("crates/ruff_linter/resources/test/fixtures/flake8_pyi/PYI034.py", True),
    ("src/debug/elf/file_test.go", True),
    ("crates/ruff_linter/src/rules/flake8_pyi/rules/non_self_return_type.rs", False),
    ("docs/snapshots.md", False),
])
def test_hidden_tests_include_snapshot_expectations(path, hidden):
    assert pilot.is_hidden_test_path(path) is hidden


def test_index_setup_failure_is_recorded_stops_and_never_invokes(tmp_path, monkeypatch):
    # aegis-bgk9ho: bobbin init timed out in setup; no cell may exist afterwards.
    monkeypatch.setattr(pilot, "load_task_by_id", lambda task, root: {"id": task, "commit": "x"})
    monkeypatch.setattr(pilot, "isolated_env", lambda home: {"PATH": ""})

    def prepare(task, scratch, env, timeout):
        (scratch / "controls.json").write_text("{}")
        return scratch, "parent", []
    monkeypatch.setattr(pilot, "prepare", prepare)
    seen = {}

    def setup(ws, *, timeout, init_timeout):
        seen["init_timeout"] = init_timeout
        raise pilot.BobbinSetupError("bobbin init timed out after 900s")
    monkeypatch.setattr(pilot, "setup_bobbin", setup)
    monkeypatch.setattr(pilot, "invoke", lambda *a: pytest.fail("paid invocation"))
    with pytest.raises(pilot.BobbinSetupError):
        pilot.run_task("t", tmp_path, tmp_path, "bobbin", "claude",
                       [("t", c) for c in pilot.CELLS], None, 2.0, 900, 40)
    assert seen["init_timeout"] == 900
    assert json.loads((tmp_path / "t/infrastructure-error.json").read_text())["error"]
    assert sorted(p.name for p in (tmp_path / "t").iterdir()) == [
        "controls.json", "infrastructure-error.json"]
