"""Tests for agent spawner."""

import json
import subprocess
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from tambour.agent import AgentSpawner, BeadsClient
from tambour.config import Config


class TestBeadsClient:
    """Tests for BeadsClient."""

    def test_get_ready_issues_returns_tasks_only(self):
        """Test that get_ready_issues filters to tasks only."""
        mock_output = json.dumps([
            {"id": "proj-001", "title": "Task 1", "issue_type": "task"},
            {"id": "proj-002", "title": "Epic 1", "issue_type": "epic"},
            {"id": "proj-003", "title": "Task 2", "issue_type": "task"},
        ])

        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(
                stdout=mock_output,
                returncode=0,
            )
            issues = BeadsClient.get_ready_issues()

        assert len(issues) == 2
        assert all(i["issue_type"] == "task" for i in issues)

    def test_get_ready_issues_filters_by_label(self):
        """Test that get_ready_issues can filter by label."""
        mock_output = json.dumps([
            {"id": "proj-001", "title": "Bug 1", "issue_type": "task", "labels": ["bug"]},
            {"id": "proj-002", "title": "Feature 1", "issue_type": "task", "labels": ["feature"]},
            {"id": "proj-003", "title": "Bug 2", "issue_type": "task", "labels": ["bug", "critical"]},
        ])

        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(
                stdout=mock_output,
                returncode=0,
            )
            issues = BeadsClient.get_ready_issues(label="bug")

        assert len(issues) == 2
        assert all("bug" in i.get("labels", []) for i in issues)

    def test_get_issue_returns_first_result(self):
        """Test that get_issue returns the issue dict."""
        mock_output = json.dumps([
            {"id": "proj-001", "title": "Test Issue", "status": "open"},
        ])

        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(
                stdout=mock_output,
                returncode=0,
            )
            issue = BeadsClient.get_issue("proj-001")

        assert issue["id"] == "proj-001"
        assert issue["title"] == "Test Issue"

    def test_get_issue_raises_on_empty_result(self):
        """Test that get_issue raises ValueError if not found."""
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(
                stdout="[]",
                returncode=0,
            )

            with pytest.raises(ValueError) as exc_info:
                BeadsClient.get_issue("nonexistent")

        assert "Issue not found" in str(exc_info.value)

    def test_claim_issue_returns_success(self):
        """Test claim_issue returns True on success."""
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(returncode=0)
            result = BeadsClient.claim_issue("proj-001")

        assert result is True
        mock_run.assert_called_once()
        assert "update" in mock_run.call_args[0][0]
        assert "--claim" in mock_run.call_args[0][0]

    def test_claim_issue_returns_failure(self):
        """Test claim_issue returns False on failure."""
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(returncode=1)
            result = BeadsClient.claim_issue("proj-001")

        assert result is False

    def test_unclaim_issue_sets_status_and_clears_assignee(self):
        """Test unclaim_issue sets status to open and clears assignee."""
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(returncode=0)
            result = BeadsClient.unclaim_issue("proj-001")

        assert result is True
        call_args = mock_run.call_args[0][0]
        assert "--status" in call_args
        assert "open" in call_args
        assert "--assignee" in call_args


class TestAgentSpawner:
    """Tests for AgentSpawner."""

    def test_get_worktree_base_resolves_template(self):
        """Test that worktree base path template is resolved."""
        config = Config()
        config.worktree.base_path = "../{repo}-worktrees"

        with tempfile.TemporaryDirectory() as tmpdir:
            main_repo = Path(tmpdir) / "myproject"
            main_repo.mkdir()

            spawner = AgentSpawner(config, main_repo=main_repo)
            base = spawner._get_worktree_base()

            assert "myproject-worktrees" in str(base)

    def test_get_worktree_path_includes_issue_id(self):
        """Test that worktree path includes issue ID."""
        config = Config()

        with tempfile.TemporaryDirectory() as tmpdir:
            main_repo = Path(tmpdir) / "myproject"
            main_repo.mkdir()

            spawner = AgentSpawner(config, main_repo=main_repo)
            path = spawner._get_worktree_path("proj-001")

            assert path.name == "proj-001"

    def test_select_issue_with_specific_id(self):
        """Test selecting a specific issue by ID."""
        config = Config()
        spawner = AgentSpawner(config)

        mock_issue = {"id": "proj-001", "title": "Test Issue"}
        with patch.object(BeadsClient, "get_issue", return_value=mock_issue):
            issue_id, title = spawner.select_issue(issue_id="proj-001")

        assert issue_id == "proj-001"
        assert title == "Test Issue"

    def test_select_issue_picks_next_ready(self):
        """Test selecting next ready issue."""
        config = Config()
        spawner = AgentSpawner(config)

        mock_issues = [
            {"id": "proj-001", "title": "First Task", "issue_type": "task"},
            {"id": "proj-002", "title": "Second Task", "issue_type": "task"},
        ]
        with patch.object(BeadsClient, "get_ready_issues", return_value=mock_issues):
            issue_id, title = spawner.select_issue()

        assert issue_id == "proj-001"
        assert title == "First Task"

    def test_select_issue_raises_when_none_available(self):
        """Test that select_issue raises when no tasks available."""
        config = Config()
        spawner = AgentSpawner(config)

        with patch.object(BeadsClient, "get_ready_issues", return_value=[]):
            with pytest.raises(ValueError) as exc_info:
                spawner.select_issue()

        assert "No ready tasks available" in str(exc_info.value)

    def test_select_issue_with_label_filter(self):
        """Test selecting with label filter."""
        config = Config()
        spawner = AgentSpawner(config)

        mock_issues = [
            {"id": "proj-001", "title": "Bug Fix", "issue_type": "task", "labels": ["bug"]},
        ]
        with patch.object(
            BeadsClient, "get_ready_issues", return_value=mock_issues
        ) as mock_get:
            issue_id, title = spawner.select_issue(label="bug")

        mock_get.assert_called_once_with(label="bug")
        assert issue_id == "proj-001"

    def test_build_prompt_includes_issue_details(self):
        """Test that build_prompt includes issue information."""
        config = Config()
        spawner = AgentSpawner(config)

        mock_show_output = "proj-001: Test Issue\nStatus: open\n"
        with patch.object(BeadsClient, "show_issue", return_value=mock_show_output):
            with patch.object(
                spawner.context_collector, "collect", return_value=("", [])
            ):
                prompt = spawner._build_prompt(
                    "proj-001",
                    Path("/tmp/worktrees/proj-001"),
                )

        assert "bd show proj-001" in prompt
        assert "Test Issue" in prompt
        assert "/tmp/worktrees/proj-001" in prompt
        assert "Branch: proj-001" in prompt

    def test_build_prompt_includes_completion_context(self):
        """Test that completion context is prepended."""
        config = Config()
        spawner = AgentSpawner(config)

        with patch.object(BeadsClient, "show_issue", return_value="issue details"):
            with patch.object(
                spawner.context_collector, "collect", return_value=("", [])
            ):
                prompt = spawner._build_prompt(
                    "proj-001",
                    Path("/tmp/worktrees/proj-001"),
                    completion_context="Previous session context",
                )

        assert "Previous session context" in prompt
        # Context should come before the main prompt
        assert prompt.index("Previous session context") < prompt.index("bd show")

    def test_build_prompt_appends_injected_context(self):
        """Test that injected context is appended."""
        config = Config()
        spawner = AgentSpawner(config)

        with patch.object(BeadsClient, "show_issue", return_value="issue details"):
            with patch.object(
                spawner.context_collector,
                "collect",
                return_value=("# Project Structure\nfiles here", []),
            ):
                prompt = spawner._build_prompt(
                    "proj-001",
                    Path("/tmp/worktrees/proj-001"),
                )

        assert "# Project Structure" in prompt
        # Injected context should come after the main prompt
        assert prompt.index("# Project Structure") > prompt.index("bd show")


class TestAgentSpawnerIntegration:
    """Integration tests for AgentSpawner.spawn()."""

    def test_spawn_returns_agent_exit_code(self):
        """Test that spawn returns the agent's exit code."""
        config = Config()
        config.agent.default_cli = "echo"

        with tempfile.TemporaryDirectory() as tmpdir:
            main_repo = Path(tmpdir) / "myproject"
            main_repo.mkdir()

            spawner = AgentSpawner(config, main_repo=main_repo)

            with patch.object(
                BeadsClient,
                "get_ready_issues",
                return_value=[{"id": "proj-001", "title": "Test", "issue_type": "task"}],
            ):
                with patch.object(BeadsClient, "get_issue", return_value={"id": "proj-001", "title": "Test"}):
                    with patch.object(BeadsClient, "show_issue", return_value="details"):
                        with patch.object(BeadsClient, "claim_issue", return_value=True):
                            with patch.object(BeadsClient, "create_worktree", return_value=True):
                                with patch.object(spawner.context_collector, "collect", return_value=("", [])):
                                    with patch.object(spawner.event_dispatcher, "dispatch"):
                                        with patch("subprocess.run") as mock_run:
                                            # First call is health check (if exists)
                                            # Then agent call
                                            mock_run.return_value = MagicMock(returncode=0)

                                            # The worktree doesn't exist so it will try to create it
                                            # and the agent will "run" (mocked)
                                            exit_code = spawner.spawn()

                                            # Should have tried to run the agent
                                            assert mock_run.called

    def test_spawn_unclaims_on_failure(self):
        """Test that spawn unclaims issue when agent fails."""
        config = Config()
        config.agent.default_cli = "false"  # Command that exits with 1

        with tempfile.TemporaryDirectory() as tmpdir:
            main_repo = Path(tmpdir) / "myproject"
            main_repo.mkdir()

            # Create worktree directory to skip creation
            worktree = main_repo.parent / "myproject-worktrees" / "proj-001"
            worktree.mkdir(parents=True)

            spawner = AgentSpawner(config, main_repo=main_repo)

            with patch.object(
                BeadsClient,
                "get_ready_issues",
                return_value=[{"id": "proj-001", "title": "Test", "issue_type": "task"}],
            ):
                with patch.object(BeadsClient, "get_issue", return_value={"id": "proj-001", "title": "Test"}):
                    with patch.object(BeadsClient, "show_issue", return_value="details"):
                        with patch.object(BeadsClient, "claim_issue", return_value=True):
                            with patch.object(BeadsClient, "unclaim_issue") as mock_unclaim:
                                with patch.object(spawner.context_collector, "collect", return_value=("", [])):
                                    with patch.object(spawner.event_dispatcher, "dispatch"):
                                        with patch("subprocess.run") as mock_run:
                                            # Agent returns non-zero
                                            mock_run.return_value = MagicMock(returncode=1)

                                            exit_code = spawner.spawn()

                                            assert exit_code == 1
                                            mock_unclaim.assert_called_with("proj-001")
