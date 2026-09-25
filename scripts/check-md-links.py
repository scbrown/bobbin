#!/usr/bin/env python3
"""Fail when a Markdown link points at a file that does not exist.

Offline and deterministic, so it can gate every commit: it checks relative
links, and links of the form https://github.com/scbrown/bobbin/(blob|tree)/main/<path>,
against the working tree. Other URLs are not fetched. Fenced code blocks are
skipped, and anchors (#...) are not checked, only the file they belong to.

Usage: check-md-links.py [FILE.md ...]   (no arguments: README, CONTRIBUTING and tracked book Markdown)
"""

import os
import re
import subprocess
import sys
from urllib.parse import unquote

REPO_URL = re.compile(r"^https://github\.com/scbrown/bobbin/(?:blob|tree)/main/(.+)$")
LINK = re.compile(r"\]\(\s*<?([^)\s>]+)>?(?:\s+\"[^\"]*\")?\s*\)")
HTML = re.compile(r"""(?:src|href)=["']([^"']+)["']""")
FENCE = re.compile(r"^\s*(```|~~~)")


def targets(text):
    in_fence = False
    for number, line in enumerate(text.splitlines(), 1):
        if FENCE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        line = re.sub(r"`[^`]*`", "", line)  # inline code is not a link
        for match in LINK.finditer(line):
            yield number, match.group(1)
        for match in HTML.finditer(line):
            yield number, match.group(1)


def resolve(source, target, root):
    """The file a link points at, or None when this check does not apply."""
    target = unquote(target.split("#", 1)[0].split("?", 1)[0])
    if not target:
        return None  # a same-page anchor
    repo = REPO_URL.match(target)
    if repo:
        return os.path.join(root, repo.group(1))
    if re.match(r"^[a-z][a-z0-9+.-]*:", target):
        return None  # another scheme or site: not fetched
    base = root if target.startswith("/") else os.path.dirname(source)
    return os.path.normpath(os.path.join(base, target.lstrip("/")))


def main(argv):
    root = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True
    ).stdout.strip()
    files = argv or ["README.md", "CONTRIBUTING.md", *subprocess.run(
        ["git", "ls-files", "docs/book/src/*.md"], capture_output=True, text=True, check=True, cwd=root
    ).stdout.splitlines()]
    broken = []
    for name in files:
        path = os.path.join(root, name)
        if not os.path.isfile(path):
            broken.append(f"{name}: missing source file")
            continue
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
        for number, target in targets(text):
            resolved = resolve(path, target, root)
            if resolved is not None and not os.path.exists(resolved):
                broken.append(f"{name}:{number}: {target}")
    for line in broken:
        print(f"broken link: {line}")
    if broken:
        print(f"check-md-links: {len(broken)} broken link(s)", file=sys.stderr)
        return 1
    print(f"check-md-links: {len(files)} file(s), no broken links")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
