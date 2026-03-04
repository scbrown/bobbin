#!/usr/bin/env bash
# run-format-comparison.sh — Compare injection format modes
#
# Tests 4 format modes (standard, minimal, verbose, xml) across
# selected tasks to measure which format is most useful to agents.
#
# Usage: ./run-format-comparison.sh [--tasks TASK1,TASK2,...] [--attempts N]

set -euo pipefail

cd "$(dirname "$0")"

TASKS="${TASKS:-cargo-001,ruff-001,django-001,go-001}"
ATTEMPTS="${ATTEMPTS:-3}"
FORMAT_MODES="standard minimal verbose xml"

# Parse CLI args
while [[ $# -gt 0 ]]; do
    case "$1" in
        --tasks) TASKS="$2"; shift 2 ;;
        --attempts) ATTEMPTS="$2"; shift 2 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

IFS=',' read -ra TASK_LIST <<< "$TASKS"

echo "=== Injection Format Comparison Study ==="
echo "Tasks:    ${TASK_LIST[*]}"
echo "Modes:    $FORMAT_MODES"
echo "Attempts: $ATTEMPTS"
echo ""

TOTAL_RUNS=$(( ${#TASK_LIST[@]} * (1 + 4) * ATTEMPTS ))
echo "Total runs: $TOTAL_RUNS (no-bobbin baseline + 4 format modes)"
echo ""

# Phase 1: No-bobbin baseline (same across all format comparisons)
echo "--- Phase 1: No-bobbin baseline ---"
for task in "${TASK_LIST[@]}"; do
    for attempt in $(seq 1 "$ATTEMPTS"); do
        echo "[$(date +%H:%M)] no-bobbin: $task (attempt $attempt/$ATTEMPTS)"
        python3 -m runner.cli run-task "$task" \
            --approach no-bobbin \
            --attempt "$attempt" \
            2>&1 | tail -1
    done
done

# Phase 2: Format mode variants
for mode in $FORMAT_MODES; do
    echo ""
    echo "--- Phase 2: format_mode=$mode ---"
    for task in "${TASK_LIST[@]}"; do
        for attempt in $(seq 1 "$ATTEMPTS"); do
            echo "[$(date +%H:%M)] format_mode=$mode: $task (attempt $attempt/$ATTEMPTS)"
            python3 -m runner.cli run-task "$task" \
                --approach with-bobbin \
                -C "format_mode=$mode" \
                --attempt "$attempt" \
                2>&1 | tail -1
        done
    done
done

echo ""
echo "=== Format comparison study complete ==="
echo "Run analysis with:"
echo "  python3 analysis/controlled_comparison.py results/ --group-by format_mode"
