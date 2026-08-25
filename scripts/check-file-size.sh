#!/usr/bin/env bash
# check-file-size.sh — file size ratchet for .rs sources.
#
# Limits:
#   - WARN at 400 lines
#   - ERROR at 500 lines
#
# Exemptions:
#   - Test files (*tests.rs, *_test.rs) — a thorough test file is not the debt
#     this guards against.
#
# ## Why this is a RATCHET and not a limit
#
# It used to be a plain allowlist: a bare list of paths, each skipped entirely.
# That made the gate loosen on its own. Adding a line to a 9,000-line file was
# free, and so was adding a new path when a file crossed 500 — and both showed
# up as "0 errors", which is exactly what a green gate is supposed to rule out.
# Measured drift: `docs/plans/bobbin-debt.md` recorded 34 entries with
# `src/cli/hook.rs` at 9,500 lines; by 2026-08-25 it was 35 entries and 9,878
# lines, with the check reporting 0 errors throughout. The debt doc had already
# called it: "a ratchet with no retirement path is a ratchet that only ever
# loosens."
#
# So an allowlist entry is no longer an exemption. It is a CEILING:
#
#   - Entries carry the line count they were frozen at: `path <lines>`.
#   - A listed file may shrink freely. Growing past its ceiling FAILS.
#   - A file over the limit that is NOT listed fails, as before.
#   - A listed entry with no recorded ceiling fails, naming the fix — otherwise
#     hand-adding a bare path would restore the old unbounded exemption.
#
# The set of violations can therefore only shrink, and never silently.
#
# When a listed file shrinks, the file is NOT rewritten automatically: a hook
# that edits tracked files mid-commit is its own failure mode. Run
# `--update-allowlist` and commit the result. That mode never loosens an entry
# — for a file that grew it keeps the smaller recorded number, so it cannot be
# used to launder growth past the gate.
#
# Adapted from quipu's `.file-size-baseline` ratchet, which solved the same
# problem. Kept on bobbin's conventions: raw line counts (not SLOC) and the
# existing `scripts/large-file-allowlist.txt` path, so nothing else has to
# change and the diff stays readable.
#
# Usage:
#   scripts/check-file-size.sh                     # staged .rs files
#   scripts/check-file-size.sh --all               # all tracked .rs files
#   scripts/check-file-size.sh --update-allowlist  # tighten (never loosens)

set -euo pipefail

WARN_LIMIT=400
ERROR_LIMIT=500

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
ALLOWLIST="$REPO_ROOT/scripts/large-file-allowlist.txt"

mode="staged"
case "${1:-}" in
    --all) mode="all" ;;
    --update-allowlist) mode="update" ;;
    "") ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

# Recorded ceilings. A listed path with no number maps to the empty string,
# which is distinguishable from "not listed at all" and is reported as an
# error rather than treated as unbounded.
declare -A ceiling
declare -A listed
if [ -f "$ALLOWLIST" ]; then
    while IFS= read -r line; do
        line="${line%%#*}"
        # shellcheck disable=SC2001 # trimming both ends, not a substitution
        line="$(echo "$line" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
        [ -z "$line" ] && continue
        path="${line%%[[:space:]]*}"
        rest="${line#"$path"}"
        rest="$(echo "$rest" | tr -d '[:space:]')"
        listed["$path"]=1
        ceiling["$path"]="$rest"
    done < "$ALLOWLIST"
fi

is_exempt() {
    case "$1" in
        *tests.rs|*_test.rs) return 0 ;;
        *) return 1 ;;
    esac
}

if [ "$mode" = "update" ]; then
    tmp=$(mktemp)
    while IFS= read -r file; do
        [ -z "$file" ] && continue
        is_exempt "$file" && continue
        [ -f "$file" ] || continue
        lines=$(wc -l < "$file")
        [ "$lines" -gt "$ERROR_LIMIT" ] || continue
        recorded="${ceiling[$file]:-}"
        # Never loosen: for a file that grew, keep the smaller recorded number
        # so the gate still refuses it and the growth still has to be undone.
        if [ -n "$recorded" ] && [ "$recorded" -lt "$lines" ]; then
            lines="$recorded"
        fi
        printf '%s %s\n' "$file" "$lines" >> "$tmp"
    done <<< "$(git ls-files '*.rs')"
    {
        echo "# Grandfathered large files — frozen at the line count recorded here."
        echo "#"
        echo "# These are CEILINGS, not exemptions: a listed file may shrink freely,"
        echo "# but growing past its number fails the build. A file over the"
        echo "# ${ERROR_LIMIT}-line limit that is not listed fails outright."
        echo "#"
        echo "# Regenerate after a file shrinks (never loosens an entry):"
        echo "#   scripts/check-file-size.sh --update-allowlist"
        echo "#"
        echo "# Shrinking this file is the point. Remove entries by splitting files."
        sort "$tmp" 2>/dev/null || true
    } > "$ALLOWLIST"
    rm -f "$tmp"
    count=$(grep -cv '^#' "$ALLOWLIST" || true)
    echo "Allowlist updated: ${count:-0} file(s) over $ERROR_LIMIT lines."
    exit 0
fi

if [ "$mode" = "all" ]; then
    files=$(git ls-files '*.rs')
else
    files=$(git diff --cached --name-only --diff-filter=ACM -- '*.rs')
fi

warnings=0
errors=0
shrunk=0

if [ -n "$files" ]; then
    while IFS= read -r file; do
        [ -z "$file" ] && continue
        is_exempt "$file" && continue
        [ -f "$file" ] || continue

        lines=$(wc -l < "$file")
        recorded="${ceiling[$file]:-}"
        is_listed="${listed[$file]+set}"

        if [ "$is_listed" = "set" ] && [ -z "$recorded" ]; then
            echo "ERROR: $file is allowlisted with no recorded ceiling ($lines lines)" >&2
            echo "       run: scripts/check-file-size.sh --update-allowlist" >&2
            errors=$((errors + 1))
        elif [ "$lines" -gt "$ERROR_LIMIT" ]; then
            if [ -z "$recorded" ]; then
                echo "ERROR: $file has $lines lines (limit: $ERROR_LIMIT)" >&2
                errors=$((errors + 1))
            elif [ "$lines" -gt "$recorded" ]; then
                echo "ERROR: $file grew to $lines lines (ceiling: $recorded)" >&2
                errors=$((errors + 1))
            elif [ "$lines" -lt "$recorded" ]; then
                shrunk=$((shrunk + 1))
            fi
        elif [ -n "$recorded" ]; then
            # Dropped under the limit entirely — the best outcome.
            shrunk=$((shrunk + 1))
        elif [ "$lines" -gt "$WARN_LIMIT" ]; then
            echo "WARN:  $file has $lines lines (limit: $WARN_LIMIT)" >&2
            warnings=$((warnings + 1))
        fi
    done <<< "$files"
fi

# Entries whose file is gone are dead weight, and dead weight is where a future
# path silently re-acquires an exemption (a new file at that path would be
# grandfathered without anyone deciding to). Reported on a full scan only,
# where the whole list is in view.
stale=0
if [ "$mode" = "all" ]; then
    for path in "${!listed[@]}"; do
        [ -f "$REPO_ROOT/$path" ] || {
            echo "STALE: $path is allowlisted but does not exist" >&2
            stale=$((stale + 1))
        }
    done
fi

if [ "$errors" -gt 0 ] || [ "$warnings" -gt 0 ] || [ "$stale" -gt 0 ]; then
    echo "" >&2
    echo "File size check: $errors error(s), $warnings warning(s), $stale stale entry(ies)" >&2
fi
if [ "$shrunk" -gt 0 ]; then
    echo "$shrunk allowlisted file(s) shrank — run scripts/check-file-size.sh --update-allowlist to tighten." >&2
fi
if [ "$errors" -gt 0 ]; then
    echo "Split the file. Adding it to scripts/large-file-allowlist.txt is a" >&2
    echo "deliberate, reviewable act that freezes it at its current size — it is" >&2
    echo "not a way to keep growing it." >&2
    exit 1
fi
if [ "$stale" -gt 0 ]; then
    echo "Remove stale entries with: scripts/check-file-size.sh --update-allowlist" >&2
fi
exit 0
