"""Tests for serving-model attribution (bobbin-daa).

Covers the three enforcement points:

* extraction — ``scorer.attribution`` reads the agent's own usage record;
* write time — ``runner.cli._run_single`` persists ``serving_model`` into the
  artifact from that record, never from the configured model;
* scoring — mixed-model aggregation is refused, unattributed runs are
  excluded from cross-arm comparison and counted.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from click.testing import CliRunner

from runner.cli import _run_single, cli
from scorer.aggregator import aggregate_across_runs
from scorer.attribution import (
    ServingModelMismatch,
    check_comparable,
    partition_by_attribution,
    record_serving_model,
    serving_model_counts,
    serving_model_from_usage,
)
from tests.test_cli import _make_result


def _all_output(result) -> str:
    """stdout + stderr of a CliRunner result, across click versions."""
    out = result.output
    try:
        out += result.stderr
    except (ValueError, AttributeError):
        pass  # older click mixes stderr into output already
    return out


class TestServingModelFromUsage:
    def test_primary_model_extracted(self):
        usage = {"model-under-test": {"inputTokens": 10}}
        assert serving_model_from_usage(usage) == "model-under-test"

    def test_haiku_skipped_as_secondary(self):
        usage = {
            "model-under-test": {"inputTokens": 10},
            "some-haiku-helper": {"inputTokens": 2},
        }
        assert serving_model_from_usage(usage) == "model-under-test"

    def test_haiku_only_is_unattributed(self):
        assert serving_model_from_usage({"some-haiku-helper": {}}) is None

    def test_absent_usage_is_unattributed(self):
        assert serving_model_from_usage(None) is None
        assert serving_model_from_usage({}) is None

    def test_multiple_primaries_keep_mixed_identity(self):
        usage = {"model-b": {}, "model-a": {}}
        assert serving_model_from_usage(usage) == "model-a+model-b"


class TestRecordServingModel:
    def test_write_time_field_preferred(self):
        record = {
            "serving_model": "model-recorded",
            "agent_result": {"model_usage": {"model-other": {}}},
        }
        assert record_serving_model(record) == "model-recorded"

    def test_explicit_null_stays_unattributed(self):
        # A write-time null means "no usage record"; it is never overridden.
        assert record_serving_model({"serving_model": None}) is None

    def test_legacy_artifact_falls_back_to_usage(self):
        record = {"agent_result": {"model_usage": {"model-legacy": {}}}}
        assert record_serving_model(record) == "model-legacy"

    def test_no_information_is_unattributed(self):
        # Notably: agent_config.model (the requested model) is never used.
        record = {"agent_config": {"model": "model-requested"}}
        assert record_serving_model(record) is None


class TestPartitionAndCheck:
    def test_partition(self):
        runs = [
            {"serving_model": "model-a"},
            {"serving_model": None},
            {"agent_result": {"model_usage": {"model-a": {}}}},
            {},
        ]
        attributed, unattributed = partition_by_attribution(runs)
        assert len(attributed) == 2
        assert len(unattributed) == 2

    def test_check_comparable_uniform_ok(self):
        arms = {
            "no-bobbin": [{"serving_model": "model-a"}] * 2,
            "with-bobbin": [{"serving_model": "model-a"}, {"serving_model": None}],
        }
        counts = check_comparable(arms)
        assert counts["no-bobbin"]["model-a"] == 2
        assert counts["with-bobbin"]["model-a"] == 1

    def test_check_comparable_refuses_cross_arm_mixture(self):
        arms = {
            "no-bobbin": [{"serving_model": "model-a"}] * 3,
            "with-bobbin": [{"serving_model": "model-b"}] * 2,
        }
        with pytest.raises(ServingModelMismatch) as exc:
            check_comparable(arms)
        msg = str(exc.value)
        assert "model-a" in msg and "model-b" in msg
        assert "model-a: 3" in msg and "model-b: 2" in msg

    def test_check_comparable_refuses_within_arm_mixture(self):
        arms = {
            "with-bobbin": [{"serving_model": "model-a"}, {"serving_model": "model-b"}],
        }
        with pytest.raises(ServingModelMismatch):
            check_comparable(arms)


class TestAggregatorIdentity:
    def test_refuses_mixed_models(self):
        runs = [
            _make_result(serving_model="model-a"),
            _make_result(serving_model="model-b"),
        ]
        with pytest.raises(ServingModelMismatch) as exc:
            aggregate_across_runs(runs)
        assert "model-a" in str(exc.value) and "model-b" in str(exc.value)

    def test_reports_serving_model_and_unattributed_count(self):
        runs = [
            _make_result(serving_model="model-a"),
            _make_result(serving_model="model-a"),
            _make_result(serving_model=None),
        ]
        stats = aggregate_across_runs(runs)
        assert stats["serving_model"] == "model-a"
        assert stats["unattributed_count"] == 1
        assert stats["count"] == 3

    def test_empty_results_schema(self):
        stats = aggregate_across_runs([])
        assert stats["serving_model"] is None
        assert stats["unattributed_count"] == 0


class TestWriteTimeAttribution:
    """The artifact written by _run_single carries the serving model."""

    def _fake_collaborators(self, monkeypatch, tmp_path, model_usage):
        ws = tmp_path / "ws"
        ws.mkdir(exist_ok=True)

        def fake_setup_workspace(repo, commit, test_command, tmpdir, **kwargs):
            return ws, "parentsha"

        def fake_run_agent(workspace, prompt, **kwargs):
            result_line = {
                "type": "result",
                "total_cost_usd": 0.5,
                "usage": {"input_tokens": 100, "output_tokens": 50},
                "num_turns": 3,
                "session_id": "sess",
                "stop_reason": "end_turn",
                "result": "done",
            }
            if model_usage is not None:
                result_line["modelUsage"] = model_usage
            return {
                "result": result_line,
                "output_raw": "",
                "stderr": "",
                "exit_code": 0,
                "duration_seconds": 12.0,
                "timed_out": False,
                "tool_use_summary": {},
            }

        monkeypatch.setattr("runner.workspace.setup_workspace", fake_setup_workspace)
        monkeypatch.setattr("runner.workspace.collect_loc_stats", lambda w: {"total_lines": 1})
        monkeypatch.setattr("runner.workspace.snapshot", lambda w: "snap")
        monkeypatch.setattr("runner.workspace.diff_snapshot", lambda w, a, b: "diff")
        monkeypatch.setattr("runner.agent_runner.run_agent", fake_run_agent)
        monkeypatch.setattr(
            "scorer.test_scorer.run_tests",
            lambda w, c: {
                "passed": True, "total": 1, "failures": 0, "parsed": True,
                "output": "", "exit_code": 0, "timed_out": False,
            },
        )
        monkeypatch.setattr(
            "scorer.diff_scorer.score_diff",
            lambda w, c, snapshot=None, baseline=None: {
                "file_precision": 1.0, "file_recall": 1.0, "f1": 1.0,
                "files_touched": [], "ground_truth_files": [],
                "exact_file_match": True,
            },
        )

    _TASK = {
        "id": "ruff-001",
        "repo": "https://example.invalid/repo.git",
        "commit": "abc123",
        "test_command": "true",
        "description": "Fix the bug.",
    }

    def _artifact(self, results_dir: Path) -> dict:
        path = results_dir / "runs" / "20260101-000000-aaaa" / "ruff-001_no-bobbin_0.json"
        return json.loads(path.read_text(encoding="utf-8"))

    def test_artifact_gains_serving_model_from_usage(self, monkeypatch, tmp_path):
        usage = {"model-under-test": {"inputTokens": 100}, "tiny-haiku": {"inputTokens": 1}}
        self._fake_collaborators(monkeypatch, tmp_path, usage)
        rdir = tmp_path / "results"
        _run_single(
            self._TASK, "no-bobbin", 0, rdir,
            run_id="20260101-000000-aaaa",
            model="model-requested-by-config",
        )
        artifact = self._artifact(rdir)
        assert artifact["serving_model"] == "model-under-test"
        # The configured model is recorded as intent only, never as attribution.
        assert artifact["agent_config"]["model"] == "model-requested-by-config"

    def test_absent_usage_writes_explicit_null_not_config(self, monkeypatch, tmp_path):
        self._fake_collaborators(monkeypatch, tmp_path, None)
        rdir = tmp_path / "results"
        _run_single(
            self._TASK, "no-bobbin", 0, rdir,
            run_id="20260101-000000-aaaa",
            model="model-requested-by-config",
        )
        artifact = self._artifact(rdir)
        assert "serving_model" in artifact
        assert artifact["serving_model"] is None


class TestScoreCommandIdentity:
    def _write(self, rdir: Path, results: list[dict]) -> None:
        rdir.mkdir(parents=True, exist_ok=True)
        for r in results:
            name = f"{r['task_id']}_{r['approach']}_{r['attempt']}.json"
            (rdir / name).write_text(json.dumps(r), encoding="utf-8")

    def test_refuses_mixed_arms_naming_models_and_counts(self, tmp_path):
        rdir = tmp_path / "results"
        self._write(rdir, [
            _make_result(approach="no-bobbin", attempt=0, serving_model="model-a"),
            _make_result(approach="no-bobbin", attempt=1, serving_model="model-a"),
            _make_result(approach="with-bobbin", attempt=0, serving_model="model-b"),
        ])
        result = CliRunner().invoke(cli, ["score", str(rdir)])
        assert result.exit_code != 0
        out = _all_output(result)
        assert "model-a: 2" in out
        assert "model-b: 1" in out

    def test_unattributed_runs_excluded_and_counted(self, tmp_path):
        rdir = tmp_path / "results"
        self._write(rdir, [
            _make_result(approach="no-bobbin", attempt=0, serving_model="model-a"),
            _make_result(approach="with-bobbin", attempt=0, serving_model="model-a"),
            _make_result(approach="with-bobbin", attempt=1, serving_model=None),
            _make_result(approach="no-bobbin", attempt=1, serving_model=None),
        ])
        result = CliRunner().invoke(cli, ["score", str(rdir)])
        assert result.exit_code == 0
        out = _all_output(result)
        assert "excluded 2" in out
        assert "model-a" in out  # uniform serving model is reported

    def test_all_unattributed_refuses_comparison(self, tmp_path):
        rdir = tmp_path / "results"
        self._write(rdir, [
            _make_result(approach="no-bobbin", serving_model=None),
            _make_result(approach="with-bobbin", serving_model=None),
        ])
        result = CliRunner().invoke(cli, ["score", str(rdir)])
        assert result.exit_code != 0
        assert "No attributed results" in _all_output(result)

    def test_mixed_models_flag_labels_output(self, tmp_path):
        rdir = tmp_path / "results"
        self._write(rdir, [
            _make_result(approach="no-bobbin", serving_model="model-a"),
            _make_result(approach="with-bobbin", serving_model="model-b"),
        ])
        result = CliRunner().invoke(cli, ["score", str(rdir), "--mixed-models"])
        assert result.exit_code == 0
        out = _all_output(result)
        assert "MODEL-MIXED" in out
        assert "[models: model-a: 1]" in out
        assert "[models: model-b: 1]" in out
