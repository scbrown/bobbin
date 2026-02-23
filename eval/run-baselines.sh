#!/bin/bash
# Phase 1+2: Baseline eval for §66 paper (aegis-o1jqap.3)
# No-injection baseline + current defaults baseline
#
# Run this first, check results, then run ablations separately.

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

TASKS=(
    "ruff-001"
    "cargo-001"
    "django-001"
    "pandas-001"
)

LOGFILE="$EVAL_DIR/results/baselines-$(date -u +%Y%m%d-%H%M%S).log"
mkdir -p "$EVAL_DIR/results"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOGFILE"; }

log "=== Baseline Eval: §66 Paper ==="
log "Tasks: ${TASKS[*]}"
log "Model: $MODEL | Attempts: $ATTEMPTS | Budget: \$$BUDGET"
log ""

# Phase 1: No-injection baseline
log "=== PHASE 1: No-injection baseline ==="
for task in "${TASKS[@]}"; do
    log ">>> $task no-bobbin ($ATTEMPTS attempts)"
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
    log "<<< $task no-bobbin done"
done

# Phase 2: With-bobbin baseline
log ""
log "=== PHASE 2: With-bobbin baseline ==="
for task in "${TASKS[@]}"; do
    log ">>> $task with-bobbin ($ATTEMPTS attempts)"
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
    log "<<< $task with-bobbin done"
done

log ""
log "=== Baselines Complete ==="

# Quick summary
python3 -c "
import json, glob, os
from collections import defaultdict
from datetime import datetime

runs_dir = '$RESULTS_DIR/runs'
# Only look at recent runs (last hour)
cutoff = datetime.utcnow().timestamp() - 7200
results = defaultdict(list)
for run_dir in sorted(glob.glob(os.path.join(runs_dir, '*'))):
    if os.path.getmtime(run_dir) < cutoff:
        continue
    for f in glob.glob(os.path.join(run_dir, '*.json')):
        if f.endswith('manifest.json'): continue
        try:
            d = json.load(open(f))
            if d.get('status') != 'completed': continue
            key = (d['task_id'], d['approach'])
            results[key].append(d)
        except: pass

if not results:
    print('No completed results found')
else:
    print(f\"\"\"{'Task':<15} {'Approach':<20} {'N':>3} {'Pass%':>6} {'F1':>6} {'AvgCost':>8}\"\"\")
    print('-' * 65)
    for (task, approach), runs in sorted(results.items()):
        n = len(runs)
        passed = sum(1 for r in runs if r.get('test_result', {}).get('passed'))
        avg_f1 = sum(r.get('diff_result', {}).get('f1', 0) for r in runs) / n
        avg_cost = sum(r.get('agent_result', {}).get('cost_usd', 0) or 0 for r in runs) / n
        print(f'{task:<15} {approach:<20} {n:>3} {passed/n*100:>5.0f}% {avg_f1:>5.2f} \${avg_cost:>7.2f}')
" 2>&1 | tee -a "$LOGFILE"

log ""
log "Log: $LOGFILE"
