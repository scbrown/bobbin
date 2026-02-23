#!/bin/bash
# Fresh baseline eval for §66 paper: aegis-o1jqap.3
#
# Runs three eval types on a representative task set:
# 1. No-injection baseline (no-bobbin)
# 2. Current defaults baseline (with-bobbin)
# 3. Ablation runs: one method disabled at a time
#
# Tasks: 4 representative tasks across Rust and Python
# Attempts: 3 per approach (for statistical significance)
# Ablation variants: semantic_weight, coupling_depth, recency_weight,
#                    doc_demotion, gate_threshold, blame_bridging

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

LOGFILE="$EVAL_DIR/results/baseline-study-$(date -u +%Y%m%d-%H%M%S).log"
mkdir -p "$EVAL_DIR/results"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOGFILE"; }

log "=== Baseline Study for §66 Paper ==="
log "Tasks: ${TASKS[*]}"
log "Attempts: $ATTEMPTS per approach"
log "Model: $MODEL"
log "Budget: \$$BUDGET per run"
log ""

# Phase 1: No-injection baseline
log "=== PHASE 1: No-injection baseline ==="
for task in "${TASKS[@]}"; do
    log "Running $task (no-bobbin, $ATTEMPTS attempts)..."
    python3 -m runner.cli run-task "$task" \
        --approaches no-bobbin \
        --attempts "$ATTEMPTS" \
        --results-dir "$RESULTS_DIR" \
        --model "$MODEL" \
        --budget "$BUDGET" \
        --timeout "$TIMEOUT" \
        --index-timeout "$INDEX_TIMEOUT" \
        --skip-verify \
        --force-budget 2>&1 | tee -a "$LOGFILE"
    log "Completed $task no-bobbin"
    log ""
done

# Phase 2: With-bobbin baseline (current defaults)
log "=== PHASE 2: With-bobbin baseline (current defaults) ==="
for task in "${TASKS[@]}"; do
    log "Running $task (with-bobbin, $ATTEMPTS attempts)..."
    python3 -m runner.cli run-task "$task" \
        --approaches with-bobbin \
        --attempts "$ATTEMPTS" \
        --results-dir "$RESULTS_DIR" \
        --model "$MODEL" \
        --budget "$BUDGET" \
        --timeout "$TIMEOUT" \
        --index-timeout "$INDEX_TIMEOUT" \
        --skip-verify \
        --force-budget 2>&1 | tee -a "$LOGFILE"
    log "Completed $task with-bobbin"
    log ""
done

# Phase 3: Ablation runs (one method disabled at a time)
log "=== PHASE 3: Ablation runs ==="
for task in "${TASKS[@]}"; do
    # Build -C flags
    C_FLAGS=()
    for abl in "${ABLATIONS[@]}"; do
        C_FLAGS+=("-C" "$abl")
    done

    log "Running $task ablations (${#ABLATIONS[@]} variants × $ATTEMPTS attempts)..."
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
    log "Completed $task ablations"
    log ""
done

log "=== Study Complete ==="
log "Results in: $RESULTS_DIR/runs/"
log "Log: $LOGFILE"

# Summary
log ""
log "=== Quick Summary ==="
python3 -c "
import json, glob, os
from collections import defaultdict

runs_dir = '$RESULTS_DIR/runs'
results = defaultdict(list)
for run_dir in sorted(glob.glob(os.path.join(runs_dir, '*'))):
    for f in glob.glob(os.path.join(run_dir, '*.json')):
        if f.endswith('manifest.json'): continue
        try:
            d = json.load(open(f))
            if d.get('status') != 'completed': continue
            key = (d['task_id'], d['approach'])
            results[key].append(d)
        except: pass

print(f'{'Task':<15} {'Approach':<35} {'N':>3} {'Pass%':>6} {'F1':>6} {'Cost':>7}')
print('-' * 80)
for (task, approach), runs in sorted(results.items()):
    n = len(runs)
    passed = sum(1 for r in runs if r.get('test_result', {}).get('passed'))
    avg_f1 = sum(r.get('diff_result', {}).get('f1', 0) for r in runs) / n if n else 0
    avg_cost = sum(r.get('agent_result', {}).get('cost_usd', 0) or 0 for r in runs) / n if n else 0
    print(f'{task:<15} {approach:<35} {n:>3} {passed/n*100:>5.0f}% {avg_f1:>5.2f} \${avg_cost:>6.2f}')
" 2>&1 | tee -a "$LOGFILE"
