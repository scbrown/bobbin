#!/usr/bin/env bash
# validate-frontmatter.sh — Validate YAML frontmatter in book documentation pages
#
# Checks:
#   - Required fields present (title, description, tags, status, category)
#   - Valid status values (draft, published)
#   - Related paths resolve to existing files
#   - source_files paths resolve to existing files
#
# Usage: scripts/validate-frontmatter.sh [docs/book/src]

set -euo pipefail

DOCS_DIR="${1:-docs/book/src}"
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

if [ ! -d "$DOCS_DIR" ]; then
    echo "ERROR: docs directory not found: $DOCS_DIR" >&2
    exit 1
fi

REQUIRED_FIELDS=(title description tags status category)
VALID_STATUSES=(draft published)

errors=0
warnings=0
files_checked=0

# Extract frontmatter from a file (between first --- and second ---)
extract_frontmatter() {
    local file="$1"
    sed -n '/^---$/,/^---$/p' "$file" | sed '1d;$d'
}

# Check if a file has frontmatter
has_frontmatter() {
    local file="$1"
    head -1 "$file" | grep -q '^---$'
}

# Get a field value from frontmatter text
get_field() {
    local fm="$1"
    local field="$2"
    echo "$fm" | grep "^${field}:" | sed "s/^${field}:[[:space:]]*//" || true
}

# Parse a YAML array field like [a, b, c] into newline-separated values
parse_array() {
    local value="$1"
    # Strip brackets, split on commas, trim whitespace
    echo "$value" | tr -d '[]' | tr ',' '\n' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' | grep -v '^$'
}

error() {
    echo "  ERROR: $1" >&2
    ((errors++)) || true
}

warn() {
    echo "  WARN:  $1" >&2
    ((warnings++)) || true
}

echo "Validating frontmatter in $DOCS_DIR..."
echo

while IFS= read -r -d '' file; do
    rel_path="${file#$DOCS_DIR/}"

    # Skip SUMMARY.md — it's the book index, not a content page
    if [ "$rel_path" = "SUMMARY.md" ]; then
        continue
    fi

    ((files_checked++)) || true

    if ! has_frontmatter "$file"; then
        echo "$rel_path"
        error "Missing frontmatter (no opening ---)"
        echo
        continue
    fi

    fm="$(extract_frontmatter "$file")"

    if [ -z "$fm" ]; then
        echo "$rel_path"
        error "Empty frontmatter block"
        echo
        continue
    fi

    has_errors=false

    # Check required fields
    for field in "${REQUIRED_FIELDS[@]}"; do
        value="$(get_field "$fm" "$field")"
        if [ -z "$value" ]; then
            if [ "$has_errors" = false ]; then
                echo "$rel_path"
                has_errors=true
            fi
            error "Missing required field: $field"
        fi
    done

    # Validate status value
    status_val="$(get_field "$fm" "status")"
    if [ -n "$status_val" ]; then
        valid=false
        for s in "${VALID_STATUSES[@]}"; do
            if [ "$status_val" = "$s" ]; then
                valid=true
                break
            fi
        done
        if [ "$valid" = false ]; then
            if [ "$has_errors" = false ]; then
                echo "$rel_path"
                has_errors=true
            fi
            error "Invalid status '$status_val' (expected: ${VALID_STATUSES[*]})"
        fi
    fi

    # Validate related paths
    related_val="$(get_field "$fm" "related")"
    if [ -n "$related_val" ]; then
        while IFS= read -r ref; do
            [ -z "$ref" ] && continue
            if [ ! -f "$DOCS_DIR/$ref" ]; then
                if [ "$has_errors" = false ]; then
                    echo "$rel_path"
                    has_errors=true
                fi
                error "Related path not found: $ref"
            fi
        done < <(parse_array "$related_val")
    fi

    # Validate source_files paths
    source_files_val="$(get_field "$fm" "source_files")"
    if [ -n "$source_files_val" ]; then
        while IFS= read -r sf; do
            [ -z "$sf" ] && continue
            if [ ! -f "$REPO_ROOT/$sf" ]; then
                if [ "$has_errors" = false ]; then
                    echo "$rel_path"
                    has_errors=true
                fi
                error "Source file not found: $sf"
            fi
        done < <(parse_array "$source_files_val")
    fi

    if [ "$has_errors" = true ]; then
        echo
    fi

done < <(find "$DOCS_DIR" -name '*.md' -print0 | sort -z)

echo "---"
echo "Files checked: $files_checked"
echo "Errors:        $errors"
echo "Warnings:      $warnings"

if [ "$errors" -gt 0 ]; then
    exit 1
fi
echo "All frontmatter valid."
