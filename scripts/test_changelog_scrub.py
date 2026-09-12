#!/usr/bin/env python3
"""Render real commits to prove scrubbing preserves their release entries."""

import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parent.parent
CONFIG = Path(os.environ.get("CLIFF_CONFIG", ROOT / "cliff.toml")).resolve()


class ChangelogScrubTests(unittest.TestCase):
    def test_delimiters_children_and_entry_preservation(self):
        config = tomllib.loads(CONFIG.read_text())
        # Exercise every configured tracker prefix, without maintaining another list.
        patterns = "\n".join(
            p["pattern"] for p in config["git"]["commit_preprocessors"]
        )
        prefixes = re.search(r"\(\?:([^)]*)\)", patterns).group(1).split("|")
        cases = []
        for prefix in prefixes:
            ref = f"{prefix}-abc123"
            cases.extend([
                (f"fix: bare child {ref}.2.3", "Bare child"),
                (f"fix: public first (#42, {ref}.2)", "Public first (#42)"),
                (f"fix: bracket public [{ref}.2, #42]", "Bracket public [#42]"),
                (f"fix: bracket public first [#42, {ref}.2]", "Bracket public first [#42]"),
                (f"fix: bare reference {ref}", "Bare reference"),
                (f"fix: bracket reference [{ref}]", "Bracket reference"),
                (f"fix: bracket child [{ref}.3]", "Bracket child"),
                (f"fix: parenthesized child ({ref}.2)", "Parenthesized child"),
                (f"fix: two references [{ref}] [{prefix}-def456]", "Two references"),
                (f"fix: nested child [{ref}.2.3]", "Nested child"),
                (f"fix: retain public reference ({ref}.2, #42)", "Retain public reference (#42)"),
                (f"{ref}.2: leading child reference", "Leading child reference"),
            ])
        with tempfile.TemporaryDirectory(prefix="changelog-scrub-") as directory:
            def git(*args):
                return subprocess.check_output(
                    ["git", "-C", directory, *args], text=True
                ).strip()

            git("init", "-q")
            git("config", "user.name", "Changelog test")
            git("config", "user.email", "test@example.invalid")
            git("config", "commit.gpgsign", "false")
            git("commit", "--allow-empty", "-qm", "chore: baseline")
            git("tag", "v1.0.0")
            expected = {}
            for subject, message in cases:
                git("commit", "--allow-empty", "-qm", subject)
                expected[git("rev-parse", "HEAD")] = message
            rendered = subprocess.check_output(
                ["git-cliff", "--offline", "--config", str(CONFIG),
                 "v1.0.0..HEAD"], cwd=directory, text=True
            )

        entries = [line for line in rendered.splitlines() if line.startswith("- ")]
        self.assertEqual(len(entries), len(expected), rendered)
        for sha, message in expected.items():
            with self.subTest(message=message):
                matching = [line for line in entries if f"/commit/{sha}" in line]
                self.assertEqual(len(matching), 1, rendered)
                self.assertEqual(
                    matching[0].split("([", 1)[0].strip(), f"- {message}"
                )
        for prefix in prefixes:
            self.assertNotIn(f"{prefix}-abc123", rendered)
        self.assertNotRegex(rendered, r"(?m)^###\s+\.\d")
        self.assertIn("### Other", rendered)


if __name__ == "__main__":
    unittest.main()
