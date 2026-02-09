"""Workspace manager: clone, checkout parent commit, snapshot."""


def clone_repo(repo: str, dest: str) -> str:
    """Clone a repository to the given destination. Returns the path."""
    raise NotImplementedError("Workspace manager not yet implemented — see bo-lr80")


def checkout_parent(workspace: str, commit: str) -> None:
    """Checkout the parent of the given commit in the workspace."""
    raise NotImplementedError("Workspace manager not yet implemented — see bo-lr80")


def snapshot(workspace: str) -> str:
    """Create a snapshot of the workspace state. Returns snapshot ID."""
    raise NotImplementedError("Workspace manager not yet implemented — see bo-lr80")
