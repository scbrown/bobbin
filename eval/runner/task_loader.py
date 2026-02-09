"""Load and validate eval task definitions from YAML files."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

import yaml

logger = logging.getLogger(__name__)

REQUIRED_FIELDS = {"id", "repo", "commit", "description", "test_command"}
OPTIONAL_FIELDS = {"language", "difficulty", "tags"}
VALID_DIFFICULTIES = {"easy", "medium", "hard"}


class TaskLoadError(Exception):
    """Raised when a task YAML file is invalid or cannot be loaded."""


def _validate_task(data: dict[str, Any], path: Path) -> None:
    """Validate a parsed task dict, raising TaskLoadError on problems."""
    missing = REQUIRED_FIELDS - data.keys()
    if missing:
        raise TaskLoadError(f"{path.name}: missing required fields: {sorted(missing)}")

    if not isinstance(data["id"], str) or not data["id"].strip():
        raise TaskLoadError(f"{path.name}: 'id' must be a non-empty string")

    if not isinstance(data["repo"], str) or "/" not in data["repo"]:
        raise TaskLoadError(f"{path.name}: 'repo' must be a GitHub slug like 'owner/repo'")

    if not isinstance(data["commit"], str) or len(data["commit"]) < 7:
        raise TaskLoadError(f"{path.name}: 'commit' must be a hex hash (>=7 chars)")

    if not isinstance(data["description"], str) or not data["description"].strip():
        raise TaskLoadError(f"{path.name}: 'description' must be a non-empty string")

    if not isinstance(data["test_command"], str) or not data["test_command"].strip():
        raise TaskLoadError(f"{path.name}: 'test_command' must be a non-empty string")

    difficulty = data.get("difficulty")
    if difficulty is not None and difficulty not in VALID_DIFFICULTIES:
        raise TaskLoadError(
            f"{path.name}: 'difficulty' must be one of {sorted(VALID_DIFFICULTIES)}, "
            f"got '{difficulty}'"
        )

    tags = data.get("tags")
    if tags is not None and not isinstance(tags, list):
        raise TaskLoadError(f"{path.name}: 'tags' must be a list")


def load_task(path: str | Path) -> dict[str, Any]:
    """Load a single task definition from a YAML file.

    Parameters
    ----------
    path:
        Path to a ``.yaml`` file containing the task definition.

    Returns a dict with at least: id, repo, commit, description, test_command.
    Optional fields: language, difficulty, tags.

    Raises :class:`TaskLoadError` if the file is missing, unreadable, or invalid.
    """
    p = Path(path)
    if not p.exists():
        raise TaskLoadError(f"Task file not found: {p}")

    try:
        text = p.read_text(encoding="utf-8")
    except OSError as exc:
        raise TaskLoadError(f"Cannot read {p}: {exc}") from exc

    try:
        data = yaml.safe_load(text)
    except yaml.YAMLError as exc:
        raise TaskLoadError(f"Invalid YAML in {p}: {exc}") from exc

    if not isinstance(data, dict):
        raise TaskLoadError(f"{p.name}: expected a YAML mapping, got {type(data).__name__}")

    _validate_task(data, p)
    logger.debug("Loaded task %s from %s", data["id"], p)
    return data


def load_task_by_id(task_id: str, tasks_dir: str | Path = "tasks") -> dict[str, Any]:
    """Load a task by its ID, searching the tasks directory.

    Looks for ``<task_id>.yaml`` in *tasks_dir*.

    Raises :class:`TaskLoadError` if the task is not found.
    """
    tasks_path = Path(tasks_dir)
    candidate = tasks_path / f"{task_id}.yaml"
    if candidate.exists():
        return load_task(candidate)

    # Fall back to scanning all YAML files for matching id field.
    for yaml_file in sorted(tasks_path.glob("*.yaml")):
        task = load_task(yaml_file)
        if task["id"] == task_id:
            return task

    raise TaskLoadError(f"No task found with id '{task_id}' in {tasks_path}")


def load_all_tasks(tasks_dir: str | Path = "tasks") -> list[dict[str, Any]]:
    """Load all task YAML files from a directory.

    Parameters
    ----------
    tasks_dir:
        Directory containing ``.yaml`` task definition files.

    Returns a list of task dicts, sorted by task ID.

    Raises :class:`TaskLoadError` if the directory doesn't exist or any file is invalid.
    """
    tasks_path = Path(tasks_dir)
    if not tasks_path.is_dir():
        raise TaskLoadError(f"Tasks directory not found: {tasks_path}")

    yaml_files = sorted(tasks_path.glob("*.yaml"))
    if not yaml_files:
        raise TaskLoadError(f"No .yaml files found in {tasks_path}")

    tasks = []
    for yaml_file in yaml_files:
        tasks.append(load_task(yaml_file))

    # Check for duplicate IDs.
    ids = [t["id"] for t in tasks]
    dupes = {i for i in ids if ids.count(i) > 1}
    if dupes:
        raise TaskLoadError(f"Duplicate task IDs found: {sorted(dupes)}")

    logger.info("Loaded %d tasks from %s", len(tasks), tasks_path)
    return sorted(tasks, key=lambda t: t["id"])
