#!/usr/bin/env bash
# check-file-size.sh — Enforce file size limits on staged .rs files
#
# Limits:
#   - WARN at 400 lines
#   - ERROR at 500 lines
#
# Exemptions:
#   - Test files (*tests.rs, *_test.rs)
#   - Files listed in scripts/large-file-allowlist.txt
#
# Usage: scripts/check-file-size.sh [--all]
#   --all   Check all tracked .rs files (not just staged)

set -euo pipefail

WARN_LIMIT=400
ERROR_LIMIT=500

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ALLOWLIST="$REPO_ROOT/scripts/large-file-allowlist.txt"

warnings=0
errors=0

# Load allowlist (strip comments and blank lines)
declare -A allowed
if [ -f "$ALLOWLIST" ]; then
    while IFS= read -r line; do
        # Skip comments and blank lines
        line="${line%%#*}"
        line="${line// /}"
        [ -z "$line" ] && continue
        allowed["$line"]=1
    done < "$ALLOWLIST"
fi

# Get file list
if [ "${1:-}" = "--all" ]; then
    files=$(git ls-files '*.rs')
else
    files=$(git diff --cached --name-only --diff-filter=ACM -- '*.rs')
fi

if [ -z "$files" ]; then
    exit 0
fi

while IFS= read -r file; do
    # Skip test files
    case "$file" in
        *tests.rs|*_test.rs) continue ;;
    esac

    # Skip allowlisted files
    if [ "${allowed[$file]+set}" = "set" ]; then
        continue
    fi

    # Count lines
    if [ ! -f "$file" ]; then
        continue
    fi
    lines=$(wc -l < "$file")

    if [ "$lines" -gt "$ERROR_LIMIT" ]; then
        echo "ERROR: $file has $lines lines (limit: $ERROR_LIMIT)" >&2
        errors=$((errors + 1))
    elif [ "$lines" -gt "$WARN_LIMIT" ]; then
        echo "WARN:  $file has $lines lines (limit: $WARN_LIMIT)" >&2
        warnings=$((warnings + 1))
    fi
done <<< "$files"

if [ "$errors" -gt 0 ] || [ "$warnings" -gt 0 ]; then
    echo "" >&2
    echo "File size check: $errors error(s), $warnings warning(s)" >&2
    if [ "$errors" -gt 0 ]; then
        echo "Split large files or add to scripts/large-file-allowlist.txt" >&2
        exit 1
    fi
fi
