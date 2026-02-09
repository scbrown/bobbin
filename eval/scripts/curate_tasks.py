#!/usr/bin/env python3
"""Semi-automated task curation script for bobbin eval framework.

Filters git log from target repos for commits matching eval task criteria:
- 2-5 files changed (cross-file = bobbin's sweet spot)
- 20-200 lines of real logic (not bulk renames)
- Clear commit message usable as a prompt
- Excludes noise patterns (chore:, ci:, docs:, deps, etc.)

Usage:
    # Clone target repo first, then point the script at it:
    python eval/scripts/curate_tasks.py /path/to/ruff --language rust
    python eval/scripts/curate_tasks.py /path/to/flask --language python

    # Or use --repo to auto-clone from GitHub:
    python eval/scripts/curate_tasks.py --repo astral-sh/ruff --language rust
    python eval/scripts/curate_tasks.py --repo pallets/flask --language python

    # Limit results:
    python eval/scripts/curate_tasks.py --repo astral-sh/ruff --language rust --limit 20

Output: Candidate commits printed as YAML task stubs for manual review.
"""

import argparse
import re
import subprocess
import sys
import tempfile


# Commit message prefixes that indicate non-code changes
NOISE_PREFIXES = re.compile(
    r"^\s*("
    r"chore|ci|docs|style|build|release|bump|merge|revert"
    r"|Merge pull request|Merge branch|Auto-merge"
    r"|Update dependency|Bump |chore\(deps\)"
    r")\b",
    re.IGNORECASE,
)

# File patterns that indicate generated or non-logic changes
NOISE_FILE_PATTERNS = re.compile(
    r"(\.lock$|\.snap$|\.generated\.|package-lock\.json|Cargo\.lock"
    r"|\.min\.js|\.min\.css|vendor/|__pycache__)"
)


def run_git(repo_path: str, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", repo_path] + list(args),
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout


def clone_repo(repo_slug: str) -> str:
    """Clone a GitHub repo to a temp directory, return path."""
    tmpdir = tempfile.mkdtemp(prefix="bobbin-eval-")
    url = f"https://github.com/{repo_slug}.git"
    print(f"Cloning {url} into {tmpdir}...", file=sys.stderr)
    subprocess.run(
        ["git", "clone", "--no-checkout", "--filter=blob:none", url, tmpdir],
        check=True,
    )
    return tmpdir


def get_commit_stats(repo_path: str, sha: str) -> dict:
    """Get file count and line stats for a commit."""
    # Get numstat for precise line counts
    numstat = run_git(repo_path, "diff", "--numstat", f"{sha}^..{sha}")
    files = []
    total_added = 0
    total_deleted = 0
    for line in numstat.strip().split("\n"):
        if not line:
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            continue
        added, deleted, filename = parts
        # Skip binary files (shown as -)
        if added == "-" or deleted == "-":
            continue
        # Skip noise files
        if NOISE_FILE_PATTERNS.search(filename):
            continue
        a, d = int(added), int(deleted)
        total_added += a
        total_deleted += d
        files.append({"name": filename, "added": a, "deleted": d})

    return {
        "files": files,
        "file_count": len(files),
        "lines_added": total_added,
        "lines_deleted": total_deleted,
        "lines_changed": total_added + total_deleted,
    }


def get_commits(repo_path: str, limit: int = 500) -> list[dict]:
    """Get recent commits with metadata."""
    # Format: sha|subject|body (using separator unlikely in messages)
    fmt = "%H|%s|%b|---END---"
    log = run_git(
        repo_path, "log", f"--format={fmt}", f"-{limit}", "--no-merges"
    )

    commits = []
    for entry in log.split("|---END---\n"):
        entry = entry.strip()
        if not entry:
            continue
        parts = entry.split("|", 2)
        if len(parts) < 2:
            continue
        sha = parts[0].strip()
        subject = parts[1].strip()
        body = parts[2].strip() if len(parts) > 2 else ""
        commits.append({"sha": sha, "subject": subject, "body": body})

    return commits


def is_noise_commit(subject: str) -> bool:
    """Check if commit message indicates a non-code change."""
    return bool(NOISE_PREFIXES.match(subject))


def filter_candidates(
    repo_path: str,
    commits: list[dict],
    min_files: int = 2,
    max_files: int = 5,
    min_lines: int = 20,
    max_lines: int = 200,
) -> list[dict]:
    """Filter commits to those matching eval task criteria."""
    candidates = []
    for commit in commits:
        if is_noise_commit(commit["subject"]):
            continue

        try:
            stats = get_commit_stats(repo_path, commit["sha"])
        except subprocess.CalledProcessError:
            continue

        fc = stats["file_count"]
        lc = stats["lines_changed"]

        if fc < min_files or fc > max_files:
            continue
        if lc < min_lines or lc > max_lines:
            continue

        commit["stats"] = stats
        candidates.append(commit)

    return candidates


def format_yaml_stub(
    commit: dict, repo_slug: str, language: str, index: int
) -> str:
    """Format a candidate commit as a YAML task stub."""
    prefix = repo_slug.split("/")[-1]
    task_id = f"{prefix}-{index:03d}"
    sha = commit["sha"]
    subject = commit["subject"]
    body = commit.get("body", "").strip()
    stats = commit["stats"]

    description = subject
    if body:
        description += "\n  " + body.replace("\n", "\n  ")

    files_list = "\n".join(
        f"#   {f['name']} (+{f['added']}/-{f['deleted']})"
        for f in stats["files"]
    )

    return f"""---
id: {task_id}
repo: {repo_slug}
commit: {sha}
description: |
  {description}
test_command: "TODO: fill in specific test command"
language: {language}
difficulty: medium  # TODO: assess manually
tags: [cross-file]  # TODO: refine tags
# --- Curation stats (remove before finalizing) ---
# files_changed: {stats['file_count']}
# lines_added: {stats['lines_added']}
# lines_deleted: {stats['lines_deleted']}
# lines_total: {stats['lines_changed']}
# files:
{files_list}
"""


def main():
    parser = argparse.ArgumentParser(
        description="Find candidate commits for bobbin eval tasks"
    )
    parser.add_argument(
        "repo_path",
        nargs="?",
        help="Path to local git repo clone",
    )
    parser.add_argument(
        "--repo",
        help="GitHub repo slug (e.g., astral-sh/ruff) — will auto-clone",
    )
    parser.add_argument(
        "--language",
        required=True,
        help="Programming language (rust, python, etc.)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=20,
        help="Max candidates to show (default: 20)",
    )
    parser.add_argument(
        "--scan-depth",
        type=int,
        default=500,
        help="Number of recent commits to scan (default: 500)",
    )
    parser.add_argument(
        "--min-files", type=int, default=2, help="Min files changed"
    )
    parser.add_argument(
        "--max-files", type=int, default=5, help="Max files changed"
    )
    parser.add_argument(
        "--min-lines", type=int, default=20, help="Min lines changed"
    )
    parser.add_argument(
        "--max-lines", type=int, default=200, help="Max lines changed"
    )

    args = parser.parse_args()

    if not args.repo_path and not args.repo:
        parser.error("Provide either repo_path or --repo")

    repo_path = args.repo_path
    repo_slug = args.repo or ""

    if not repo_path and args.repo:
        repo_path = clone_repo(args.repo)
        repo_slug = args.repo

    if not repo_slug:
        # Try to infer from remote
        try:
            remote = run_git(repo_path, "remote", "get-url", "origin").strip()
            if "github.com" in remote:
                # Extract org/repo from URL
                match = re.search(r"github\.com[:/](.+?)(?:\.git)?$", remote)
                if match:
                    repo_slug = match.group(1)
        except subprocess.CalledProcessError:
            repo_slug = "unknown/repo"

    print(f"Scanning {repo_slug} ({repo_path})...", file=sys.stderr)
    commits = get_commits(repo_path, limit=args.scan_depth)
    print(f"Found {len(commits)} non-merge commits", file=sys.stderr)

    candidates = filter_candidates(
        repo_path,
        commits,
        min_files=args.min_files,
        max_files=args.max_files,
        min_lines=args.min_lines,
        max_lines=args.max_lines,
    )
    print(
        f"Filtered to {len(candidates)} candidates matching criteria",
        file=sys.stderr,
    )

    shown = 0
    for i, candidate in enumerate(candidates):
        if shown >= args.limit:
            break
        print(format_yaml_stub(candidate, repo_slug, args.language, i + 1))
        shown += 1

    if shown == 0:
        print("No candidates found. Try adjusting filters.", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
