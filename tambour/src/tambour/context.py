"""Context provider system for tambour.

Context providers generate content that gets injected into the agent prompt
at session start. They receive the base prompt and can use it to generate
task-aware context (e.g., searching for relevant code based on the task).

Environment variables passed to providers:
    TAMBOUR_PROMPT: The base prompt (task description, issue details)
    TAMBOUR_ISSUE_ID: The issue ID being worked on
    TAMBOUR_WORKTREE: Path to the worktree
    TAMBOUR_MAIN_REPO: Path to the main repository
"""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tambour.config import Config, ContextProviderConfig


@dataclass
class ProviderResult:
    """Result of executing a context provider."""

    provider_name: str
    success: bool
    content: str | None = None
    error: str | None = None
    duration_ms: int | None = None


@dataclass
class ContextRequest:
    """Request for context collection.

    Attributes:
        prompt: The base prompt that will be sent to the agent.
        issue_id: The issue ID being worked on.
        worktree: Path to the worktree directory.
        main_repo: Path to the main repository.
    """

    prompt: str
    issue_id: str | None = None
    worktree: Path | None = None
    main_repo: Path | None = None

    def to_env(self) -> dict[str, str]:
        """Convert request to environment variables for provider execution."""
        env: dict[str, str] = {
            "TAMBOUR_PROMPT": self.prompt,
        }

        if self.issue_id:
            env["TAMBOUR_ISSUE_ID"] = self.issue_id
        if self.worktree:
            env["TAMBOUR_WORKTREE"] = str(self.worktree.absolute())
        if self.main_repo:
            env["TAMBOUR_MAIN_REPO"] = str(self.main_repo.absolute())

        return env


class ContextCollector:
    """Collects context from configured providers."""

    def __init__(self, config: Config):
        """Initialize the collector with configuration.

        Args:
            config: The tambour configuration.
        """
        self.config = config

    def collect(self, request: ContextRequest) -> tuple[str, list[ProviderResult]]:
        """Collect context from all enabled providers.

        Args:
            request: The context request with prompt and metadata.

        Returns:
            Tuple of (combined context string, list of individual results).
            The combined context is ready to be injected into the agent prompt.
        """
        providers = self.config.get_enabled_context_providers()
        results: list[ProviderResult] = []
        context_parts: list[str] = []

        for provider in providers:
            result = self._execute_provider(provider, request)
            results.append(result)

            if result.success and result.content:
                context_parts.append(result.content)

        # Join all context parts with blank lines
        combined = "\n\n".join(context_parts) if context_parts else ""
        return combined, results

    def _execute_provider(
        self, provider: ContextProviderConfig, request: ContextRequest
    ) -> ProviderResult:
        """Execute a single context provider.

        Args:
            provider: The provider configuration.
            request: The context request.

        Returns:
            Result of the provider execution.
        """
        # Build environment with request data
        env = os.environ.copy()
        env.update(request.to_env())

        # Inject provider options as environment variables
        # Format: PROVIDERNAME_KEY (e.g. TREE_EXCLUDE for "tree" provider and "exclude" option)
        for key, value in provider.options.items():
            if isinstance(value, list):
                env_value = ",".join(str(v) for v in value)
            else:
                env_value = str(value)
            
            env_var_name = f"{provider.name.upper()}_{key.upper()}"
            env[env_var_name] = env_value

        # Set working directory
        cwd = request.worktree or request.main_repo or Path.cwd()

        start_time = datetime.now()

        try:
            result = subprocess.run(
                provider.run,
                shell=True,
                env=env,
                capture_output=True,
                text=True,
                timeout=provider.timeout,
                cwd=cwd,
            )

            duration_ms = int((datetime.now() - start_time).total_seconds() * 1000)

            if result.returncode == 0:
                # Strip trailing whitespace but preserve content structure
                content = result.stdout.rstrip() if result.stdout else None
                return ProviderResult(
                    provider_name=provider.name,
                    success=True,
                    content=content,
                    duration_ms=duration_ms,
                )
            else:
                return ProviderResult(
                    provider_name=provider.name,
                    success=False,
                    error=result.stderr or f"Exit code {result.returncode}",
                    duration_ms=duration_ms,
                )

        except subprocess.TimeoutExpired:
            duration_ms = int((datetime.now() - start_time).total_seconds() * 1000)
            return ProviderResult(
                provider_name=provider.name,
                success=False,
                error=f"Provider timed out after {provider.timeout}s",
                duration_ms=duration_ms,
            )

        except Exception as e:
            duration_ms = int((datetime.now() - start_time).total_seconds() * 1000)
            return ProviderResult(
                provider_name=provider.name,
                success=False,
                error=str(e),
                duration_ms=duration_ms,
            )
