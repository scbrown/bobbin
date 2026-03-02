#!/bin/bash
# Focused ablation study for §66 paper: aegis-o1jqap.3
#
# Skips no-bobbin runs (already have sufficient data).
# Runs ONLY:
# 1. With-bobbin baseline (current defaults)
# 2. Ablation runs: one method disabled at a time
#
# Tasks: 4 representative tasks across Rust and Python
# Attempts: 3 per approach (for statistical significance)

set -euo pipefail

export PATH="$HOME/.cargo/bin:$HOME/go/bin:$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin"

EVAL_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$EVAL_DIR"

RESULTS_DIR="results"
ATTEMPTS=3
MODEL="claude-sonnet-4-5-20250929"
BUDGET=5.00
TIMEOUT=600
INDEX_TIMEOUT=300

# Representative tasks: 2 Rust, 2 Python
TASKS=(
    "ruff-001"
    "cargo-001"
    "django-001"
    "pandas-001"
)

# Ablation overrides (one method disabled at a time)
ABLATIONS=(
    "semantic_weight=0.0"
    "coupling_depth=0"
    "recency_weight=0.0"
    "doc_demotion=0.0"
    "gate_threshold=1.0"
    "blame_bridging=false"
)

LOGFILE="$EVAL_DIR/results/ablation-study-$(date -u +%Y%m%d-%H%M%S).log"
mkdir -p "$EVAL_DIR/results"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOGFILE"; }

log "=== Focused Ablation Study for §66 Paper ==="
log "Tasks: ${TASKS[*]}"
log "Attempts: $ATTEMPTS per approach"
log "Model: $MODEL"
log "Budget: \$$BUDGET per run"
log "Approaches: with-bobbin + ${#ABLATIONS[@]} ablation variants"
log "Estimated runs: ${#TASKS[@]} tasks × $((1 + ${#ABLATIONS[@]})) approaches × $ATTEMPTS attempts = $(( ${#TASKS[@]} * (1 + ${#ABLATIONS[@]}) * ATTEMPTS )) runs"
log ""

# Verify bobbin binary is available
if ! command -v bobbin &>/dev/null; then
    log "ERROR: bobbin not found in PATH"
    exit 1
fi
log "Bobbin version: $(bobbin --version)"

# Verify settings files exist
for sf in settings-with-bobbin.json settings-no-bobbin.json; do
    if [[ ! -f "$EVAL_DIR/$sf" ]]; then
        log "ERROR: Missing $sf"
        exit 1
    fi
done
log ""

for task in "${TASKS[@]}"; do
    # Build -C flags for ablation variants
    C_FLAGS=()
    for abl in "${ABLATIONS[@]}"; do
        C_FLAGS+=("-C" "$abl")
    done

    log "=== Running $task: with-bobbin + ${#ABLATIONS[@]} ablations × $ATTEMPTS attempts ==="
    python3 -m runner.cli run-task "$task" \
        --approaches with-bobbin \
        "${C_FLAGS[@]}" \
        --attempts "$ATTEMPTS" \
        --results-dir "$RESULTS_DIR" \
        --model "$MODEL" \
        --budget "$BUDGET" \
        --timeout "$TIMEOUT" \
        --index-timeout "$INDEX_TIMEOUT" \
        --skip-verify \
        --force-budget 2>&1 | tee -a "$LOGFILE"
    log "Completed $task"
    log ""
done

log "=== Study Complete ==="
log "Results in: $RESULTS_DIR/runs/"
log "Log: $LOGFILE"

# Run ablation analysis
log ""
log "=== Ablation Analysis ==="
python3 analysis/ablation_analysis.py "$RESULTS_DIR" 2>&1 | tee -a "$LOGFILE"
