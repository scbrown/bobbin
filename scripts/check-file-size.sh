#!/usr/bin/env bash
# check-file-size.sh — Enforce file size limits on staged .rs files
# WARN_LIMIT: prints a warning (non-blocking)
# ERROR_LIMIT: exits 1 (blocking)
# Test files and allowlisted files are exempt.

set -euo pipefail

WARN_LIMIT=400
ERROR_LIMIT=500
ALLOWLIST="scripts/large-file-allowlist.txt"

# Load allowlist paths into an associative array
declare -A allowed
if [[ -f "$ALLOWLIST" ]]; then
    while IFS= read -r line; do
        # Skip comments and blank lines
        [[ -z "$line" || "$line" == \#* ]] && continue
        allowed["$line"]=1
    done < "$ALLOWLIST"
fi

errors=0
warnings=0

# Get staged .rs files (added or modified)
staged_files=$(git diff --cached --name-only --diff-filter=ACM -- '*.rs' 2>/dev/null || true)

if [[ -z "$staged_files" ]]; then
    exit 0
fi

while IFS= read -r file; do
    [[ -z "$file" ]] && continue

    # Exempt test files
    case "$file" in
        *tests.rs|*_test.rs|*/tests/*) continue ;;
    esac

    # Exempt allowlisted files
    if [[ -n "${allowed[$file]:-}" ]]; then
        continue
    fi

    # Count lines
    lines=$(wc -l < "$file" 2>/dev/null || echo 0)

    if (( lines > ERROR_LIMIT )); then
        echo "ERROR: $file has $lines lines (limit: $ERROR_LIMIT)"
        errors=$((errors + 1))
    elif (( lines > WARN_LIMIT )); then
        echo "WARN:  $file has $lines lines (limit: $WARN_LIMIT)"
        warnings=$((warnings + 1))
    fi
done <<< "$staged_files"

if (( errors > 0 )); then
    echo ""
    echo "$errors file(s) exceed the $ERROR_LIMIT-line limit."
    echo "Split large files or add to $ALLOWLIST if grandfathered."
    exit 1
fi

if (( warnings > 0 )); then
    echo ""
    echo "$warnings file(s) approaching the limit (>${WARN_LIMIT} lines)."
fi

exit 0
