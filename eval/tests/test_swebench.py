"""Synthetic contract tests: no benchmark gold is exposed to agents or executed."""

import copy
import hashlib
import json
import sys
from types import ModuleType, SimpleNamespace

import pytest

from runner import swebench


def row(repo="one/repo", number=1):
    return {
        "instance_id": f"{repo.replace('/', '__')}-{number}",
        "repo": repo,
        "base_commit": "a" * 40,
        "version": "1.0",
        "problem_statement": "Fix the described behavior.",
        "patch": "GOLD-SOLUTION-MARKER",
        "test_patch": "HIDDEN-TEST-MARKER",
        "hints_text": "HINT-MARKER",
        "eval_script": "EVALUATOR-COMMAND-MARKER",
        "created_at": "2020-01-01T00:00:00Z",
        "FAIL_TO_PASS": '["test_regression"]',
        "PASS_TO_PASS": "[]",
    }


def test_selection_is_order_independent_and_keeps_small_repositories():
    rows = [row(number=n) for n in range(12)] + [row("two/repo")]
    selected = swebench.select_rows(rows)
    assert selected == swebench.select_rows(list(reversed(rows)))
    assert len(selected) == 3
    assert sum(r["repo"] == "one/repo" for r in selected) == 2
    assert row("two/repo") in selected
    assert swebench.select_rows(rows, seed="different") != selected


@pytest.mark.parametrize(
    "field,value",
    [
        ("patch", ""),
        ("test_patch", ""),
        ("base_commit", "main"),
        ("FAIL_TO_PASS", "[]"),
        ("FAIL_TO_PASS", "not JSON"),
        ("PASS_TO_PASS", '{"not":"an array"}'),
        ("PASS_TO_PASS", "[4]"),
    ],
)
def test_malformed_records_refuse_instead_of_changing_selection(field, value):
    invalid = row()
    invalid[field] = value
    with pytest.raises(ValueError):
        swebench.select_rows([invalid])


def test_duplicate_identity_and_empty_dataset_refuse():
    for rows in ([], [row(), row()]):
        with pytest.raises(ValueError):
            swebench.select_rows(rows)


def test_prompt_allowlist_excludes_gold_tests_hints_and_future_fields():
    record = row()
    record["future_sensitive_field"] = "FUTURE-GOLD"
    assert swebench.agent_prompt(record) == {
        key: record[key] for key in ("instance_id", "repo", "base_commit", "problem_statement")
    }
    assert "MARKER" not in json.dumps(swebench.agent_prompt(record))


def fixture_source(tmp_path, monkeypatch):
    source = tmp_path / "source.parquet"
    source.write_bytes(b"synthetic parquet fixture")
    monkeypatch.setattr(swebench, "SOURCE_SHA256", hashlib.sha256(source.read_bytes()).hexdigest())
    rows = [row(), row(number=2)]
    parquet = ModuleType("pyarrow.parquet")
    parquet.read_table = lambda path: SimpleNamespace(to_pylist=lambda: copy.deepcopy(rows))
    arrow = ModuleType("pyarrow")
    arrow.parquet = parquet
    monkeypatch.setitem(sys.modules, "pyarrow", arrow)
    monkeypatch.setitem(sys.modules, "pyarrow.parquet", parquet)
    manifest_path = tmp_path / "frozen.json"
    manifest_path.write_text(json.dumps(swebench.build_manifest(rows, swebench.select_rows(rows))))
    return source, manifest_path


def test_prepare_separates_inputs_and_preserves_exact_evaluator_records(tmp_path, monkeypatch):
    source, manifest_path = fixture_source(tmp_path, monkeypatch)
    output = tmp_path / "prepared"
    manifest = swebench.prepare(source, output, manifest_path)
    evaluator = (output / "evaluator/instances.jsonl").read_bytes()
    assert hashlib.sha256(evaluator).hexdigest() == manifest["evaluator_sha256"]
    assert b"GOLD-SOLUTION-MARKER" in evaluator
    assert b"HIDDEN-TEST-MARKER" in evaluator
    assert "MARKER" not in (output / "prompts.jsonl").read_text()
    with pytest.raises(FileExistsError):
        swebench.prepare(source, output, manifest_path)


@pytest.mark.parametrize("tamper", ["source", "manifest"])
def test_changed_source_or_manifest_writes_nothing(tmp_path, monkeypatch, tamper):
    source, manifest_path = fixture_source(tmp_path, monkeypatch)
    if tamper == "source":
        source.write_bytes(b"changed snapshot")
    else:
        manifest = json.loads(manifest_path.read_text())
        manifest["instances"].pop()
        manifest_path.write_text(json.dumps(manifest))
    output = tmp_path / "refused"
    with pytest.raises(ValueError, match="checksum|frozen manifest"):
        swebench.prepare(source, output, manifest_path)
    assert not output.exists()
