#!/usr/bin/env bash
# doc-coverage.sh — Check documentation coverage against CLI commands
#
# Checks:
#   - Every CLI subcommand in src/cli/mod.rs has a doc page in docs/book/src/cli/
#   - Every CLI doc page is listed in SUMMARY.md
#   - Reports undocumented commands and orphaned doc pages
#
# Usage: scripts/doc-coverage.sh

set -euo pipefail

CLI_MOD="${1:-src/cli/mod.rs}"
DOCS_CLI="${2:-docs/book/src/cli}"
SUMMARY="${3:-docs/book/src/SUMMARY.md}"

if [ ! -f "$CLI_MOD" ]; then
    echo "ERROR: CLI module not found: $CLI_MOD" >&2
    exit 1
fi

if [ ! -d "$DOCS_CLI" ]; then
    echo "ERROR: CLI docs directory not found: $DOCS_CLI" >&2
    exit 1
fi

if [ ! -f "$SUMMARY" ]; then
    echo "ERROR: SUMMARY.md not found: $SUMMARY" >&2
    exit 1
fi

errors=0

error() {
    echo "  ERROR: $1" >&2
    ((errors++)) || true
}

# Extract subcommand names from mod.rs
# Looks for lines like: mod <name>;
cli_commands=()
while IFS= read -r cmd; do
    cli_commands+=("$cmd")
done < <(grep '^mod [a-z]' "$CLI_MOD" | sed 's/^mod //;s/;.*//' | sort)

echo "CLI commands found in $CLI_MOD:"
printf "  %s\n" "${cli_commands[@]}"
echo
echo "Total commands: ${#cli_commands[@]}"
echo

# Check which commands have doc pages
echo "=== CLI Doc Coverage ==="
echo
documented=()
undocumented=()

for cmd in "${cli_commands[@]}"; do
    doc_file="$DOCS_CLI/${cmd}.md"
    if [ -f "$doc_file" ]; then
        documented+=("$cmd")
    else
        undocumented+=("$cmd")
    fi
done

if [ ${#documented[@]} -gt 0 ]; then
    echo "Documented (${#documented[@]}/${#cli_commands[@]}):"
    for cmd in "${documented[@]}"; do
        # Check if also in SUMMARY.md
        if grep -q "cli/${cmd}.md" "$SUMMARY"; then
            printf "  %-20s doc + summary\n" "$cmd"
        else
            printf "  %-20s doc only (MISSING from SUMMARY.md)\n" "$cmd"
            error "$cmd has doc page but is not listed in SUMMARY.md"
        fi
    done
    echo
fi

if [ ${#undocumented[@]} -gt 0 ]; then
    echo "Undocumented (${#undocumented[@]}/${#cli_commands[@]}):"
    for cmd in "${undocumented[@]}"; do
        printf "  %-20s NO doc page\n" "$cmd"
        error "$cmd has no documentation page at cli/${cmd}.md"
    done
    echo
fi

# Check for orphaned doc pages (docs without matching CLI command)
echo "=== Orphaned Doc Pages ==="
echo
orphaned=0
for doc_file in "$DOCS_CLI"/*.md; do
    [ -f "$doc_file" ] || continue
    basename="$(basename "$doc_file" .md)"

    # Skip overview.md — it's not a command page
    [ "$basename" = "overview" ] && continue

    found=false
    for cmd in "${cli_commands[@]}"; do
        if [ "$cmd" = "$basename" ]; then
            found=true
            break
        fi
    done

    if [ "$found" = false ]; then
        echo "  $basename.md — no matching CLI command in mod.rs"
        error "Orphaned doc page: cli/$basename.md"
        ((orphaned++)) || true
    fi
done

if [ "$orphaned" -eq 0 ]; then
    echo "  None"
fi

echo
echo "---"
echo "Coverage: ${#documented[@]}/${#cli_commands[@]} commands documented"
if [ ${#undocumented[@]} -gt 0 ]; then
    echo "Missing:  ${undocumented[*]}"
fi
echo "Errors:   $errors"

if [ "$errors" -gt 0 ]; then
    exit 1
fi
echo "Full coverage achieved."
