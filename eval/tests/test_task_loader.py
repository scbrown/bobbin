"""Tests for runner.task_loader."""

from __future__ import annotations

import textwrap

import pytest

from runner.task_loader import (
    TaskLoadError,
    load_all_tasks,
    load_task,
    load_task_by_id,
)


def _write_yaml(tmp_path, filename, content):
    """Write a YAML file to tmp_path and return the path."""
    p = tmp_path / filename
    p.write_text(textwrap.dedent(content), encoding="utf-8")
    return p


VALID_YAML = """\
    id: ruff-001
    repo: astral-sh/ruff
    commit: f14fd5d88507553830d78cf3cfae625a17297ebd
    description: |
      Fix the formatter to preserve parentheses.
    test_command: "cargo test -p ruff_python_formatter"
    language: rust
    difficulty: medium
    tags: [formatter, bug-fix]
"""


class TestLoadTask:
    def test_load_valid_task(self, tmp_path):
        path = _write_yaml(tmp_path, "ruff-001.yaml", VALID_YAML)
        task = load_task(path)
        assert task["id"] == "ruff-001"
        assert task["repo"] == "astral-sh/ruff"
        assert task["commit"] == "f14fd5d88507553830d78cf3cfae625a17297ebd"
        assert "Fix the formatter" in task["description"]
        assert task["test_command"] == "cargo test -p ruff_python_formatter"
        assert task["language"] == "rust"
        assert task["difficulty"] == "medium"
        assert task["tags"] == ["formatter", "bug-fix"]

    def test_load_minimal_task(self, tmp_path):
        yaml_content = """\
            id: flask-001
            repo: pallets/flask
            commit: abc1234567
            description: Fix session handling
            test_command: pytest tests/
        """
        path = _write_yaml(tmp_path, "flask-001.yaml", yaml_content)
        task = load_task(path)
        assert task["id"] == "flask-001"
        assert task.get("language") is None
        assert task.get("difficulty") is None
        assert task.get("tags") is None

    def test_missing_required_field(self, tmp_path):
        yaml_content = """\
            id: bad-001
            repo: owner/repo
            description: Missing commit and test_command
        """
        path = _write_yaml(tmp_path, "bad.yaml", yaml_content)
        with pytest.raises(TaskLoadError, match="missing required fields"):
            load_task(path)

    def test_empty_id(self, tmp_path):
        yaml_content = """\
            id: ""
            repo: owner/repo
            commit: abc1234567
            description: Test
            test_command: pytest
        """
        path = _write_yaml(tmp_path, "empty-id.yaml", yaml_content)
        with pytest.raises(TaskLoadError, match="non-empty string"):
            load_task(path)

    def test_bad_repo_format(self, tmp_path):
        yaml_content = """\
            id: test-001
            repo: not-a-slug
            commit: abc1234567
            description: Test
            test_command: pytest
        """
        path = _write_yaml(tmp_path, "bad-repo.yaml", yaml_content)
        with pytest.raises(TaskLoadError, match="GitHub slug"):
            load_task(path)

    def test_short_commit(self, tmp_path):
        yaml_content = """\
            id: test-001
            repo: owner/repo
            commit: abc
            description: Test
            test_command: pytest
        """
        path = _write_yaml(tmp_path, "short-commit.yaml", yaml_content)
        with pytest.raises(TaskLoadError, match="hex hash"):
            load_task(path)

    def test_invalid_difficulty(self, tmp_path):
        yaml_content = """\
            id: test-001
            repo: owner/repo
            commit: abc1234567
            description: Test
            test_command: pytest
            difficulty: impossible
        """
        path = _write_yaml(tmp_path, "bad-diff.yaml", yaml_content)
        with pytest.raises(TaskLoadError, match="difficulty"):
            load_task(path)

    def test_tags_not_list(self, tmp_path):
        yaml_content = """\
            id: test-001
            repo: owner/repo
            commit: abc1234567
            description: Test
            test_command: pytest
            tags: "not-a-list"
        """
        path = _write_yaml(tmp_path, "bad-tags.yaml", yaml_content)
        with pytest.raises(TaskLoadError, match="tags.*list"):
            load_task(path)

    def test_file_not_found(self, tmp_path):
        with pytest.raises(TaskLoadError, match="not found"):
            load_task(tmp_path / "nonexistent.yaml")

    def test_invalid_yaml_syntax(self, tmp_path):
        p = tmp_path / "bad.yaml"
        p.write_text("{{invalid: yaml: [", encoding="utf-8")
        with pytest.raises(TaskLoadError, match="Invalid YAML"):
            load_task(p)

    def test_yaml_not_mapping(self, tmp_path):
        p = tmp_path / "list.yaml"
        p.write_text("- item1\n- item2\n", encoding="utf-8")
        with pytest.raises(TaskLoadError, match="expected a YAML mapping"):
            load_task(p)


class TestLoadTaskById:
    def test_find_by_filename(self, tmp_path):
        _write_yaml(tmp_path, "ruff-001.yaml", VALID_YAML)
        task = load_task_by_id("ruff-001", tmp_path)
        assert task["id"] == "ruff-001"

    def test_find_by_scanning(self, tmp_path):
        # File named differently than the id.
        yaml_content = """\
            id: custom-id
            repo: owner/repo
            commit: abc1234567
            description: Test task
            test_command: pytest
        """
        _write_yaml(tmp_path, "renamed-file.yaml", yaml_content)
        task = load_task_by_id("custom-id", tmp_path)
        assert task["id"] == "custom-id"

    def test_not_found(self, tmp_path):
        _write_yaml(tmp_path, "ruff-001.yaml", VALID_YAML)
        with pytest.raises(TaskLoadError, match="No task found"):
            load_task_by_id("nonexistent", tmp_path)


class TestLoadAllTasks:
    def test_load_multiple(self, tmp_path):
        _write_yaml(tmp_path, "ruff-001.yaml", VALID_YAML)
        yaml2 = """\
            id: flask-001
            repo: pallets/flask
            commit: def4567890
            description: Fix flask
            test_command: pytest tests/
        """
        _write_yaml(tmp_path, "flask-001.yaml", yaml2)
        tasks = load_all_tasks(tmp_path)
        assert len(tasks) == 2
        assert tasks[0]["id"] == "flask-001"  # Sorted by ID.
        assert tasks[1]["id"] == "ruff-001"

    def test_empty_directory(self, tmp_path):
        with pytest.raises(TaskLoadError, match="No .yaml files"):
            load_all_tasks(tmp_path)

    def test_directory_not_found(self, tmp_path):
        with pytest.raises(TaskLoadError, match="not found"):
            load_all_tasks(tmp_path / "nope")

    def test_duplicate_ids(self, tmp_path):
        _write_yaml(tmp_path, "a.yaml", VALID_YAML)
        _write_yaml(tmp_path, "b.yaml", VALID_YAML)  # Same id: ruff-001.
        with pytest.raises(TaskLoadError, match="Duplicate"):
            load_all_tasks(tmp_path)

    def test_loads_real_tasks(self):
        """Smoke test: load the actual eval tasks from the repo."""
        tasks_dir = Path(__file__).parent.parent / "tasks"
        if not tasks_dir.is_dir():
            pytest.skip("tasks/ directory not available")
        tasks = load_all_tasks(tasks_dir)
        assert len(tasks) >= 1
        for t in tasks:
            assert "id" in t
            assert "repo" in t


# Need Path import for TestLoadAllTasks.test_loads_real_tasks
from pathlib import Path  # noqa: E402
