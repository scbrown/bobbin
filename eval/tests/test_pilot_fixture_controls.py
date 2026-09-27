"""Free fixture preflight cannot reach the paid invocation path."""
import json
import subprocess

import pytest

from runner import pilot
from runner import pilot_fixture_controls as fixtures


def test_all_tasks_checked_despite_fixture_and_infrastructure_failures(tmp_path, monkeypatch):
    source, output = tmp_path / "source", tmp_path / "output"
    source.mkdir()
    (source / "manifest.json").write_text(json.dumps({"timeout": 30}))
    monkeypatch.setattr(fixtures, "plan", lambda *args: {
        "remaining_schedule": [[task, cell] for task in ("good", "bad", "network")
                               for cell in ("none", "both")]})
    monkeypatch.setattr(fixtures, "load_task_by_id", lambda task, root: {"id": task})
    monkeypatch.setattr(pilot, "invoke", lambda *a: pytest.fail("paid invocation"))
    monkeypatch.setattr(pilot, "_find_claude", lambda: pytest.fail("client discovery"))
    monkeypatch.setattr(pilot, "_find_bobbin", lambda: pytest.fail("binary discovery"))
    def environment(home, *, copy_credentials):
        assert copy_credentials is False
        return {"ANTHROPIC_API_KEY": "test", "OPENAI_API_KEY": "test"}
    monkeypatch.setattr(pilot, "isolated_env", environment)
    seen = []
    def prepare(task, scratch, env, timeout):
        assert not env
        seen.append(task["id"])
        if task["id"] == "network":
            raise subprocess.CalledProcessError(128, ["git", "clone"])
        (scratch / "controls.json").write_text('{}')
        if task["id"] == "bad":
            raise ValueError("does not discriminate")
    monkeypatch.setattr(pilot, "prepare", prepare)
    result = fixtures.controls(source, "pin", output, tmp_path)
    assert seen == ["good", "bad", "network"]
    assert result["all_tasks_checked"] is True
    assert result["model_calls"] == 0
    assert result["eligible_tasks"] == ["good"]
    assert [r["status"] for r in result["tasks"]] == [
        "fixture_pass", "fixture_exclusion", "infrastructure_exclusion"]
    assert (output / "bad/controls.json").exists()
    with pytest.raises(FileExistsError):
        fixtures.controls(source, "pin", output, tmp_path)


def test_output_cannot_mutate_original_campaign(tmp_path):
    with pytest.raises(ValueError, match="outside"):
        fixtures.controls(tmp_path, "pin", tmp_path / "new", tmp_path)


@pytest.mark.parametrize("summary,rc,executed,passed", [
    ("1 passed, 0 failed, 2764 skipped\n", 0, 1, True),
    ("0 passed, 1 failed, 2764 skipped\n", 1, 1, False),
    ("0 passed, 0 failed, 2764 skipped\n", 0, 0, False),
    ("ok  go/build  0.002s\n", 0, 0, False),
])
def test_custom_summary_still_requires_executed_tests(monkeypatch, summary, rc, executed, passed):
    monkeypatch.setattr(pilot.subprocess, "run", lambda *a, **k:
                        subprocess.CompletedProcess([], rc, summary, ""))
    result = pilot.grade("unused", "unused", {}, 1)
    assert result["executed"] == executed
    assert result["passed"] is passed
