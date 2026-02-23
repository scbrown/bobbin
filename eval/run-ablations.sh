#!/bin/bash
# Phase 3: Ablation runs for §66 paper (aegis-o1jqap.3)
# One method disabled at a time to isolate contribution.
#
# Run AFTER baselines complete (run-baselines.sh).
# Each -C flag creates a separate approach variant.
# Note: This also re-runs with-bobbin baseline (for consistency within run IDs).

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

LOGFILE="$EVAL_DIR/results/ablations-$(date -u +%Y%m%d-%H%M%S).log"
mkdir -p "$EVAL_DIR/results"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOGFILE"; }

log "=== Ablation Study: §66 Paper ==="
log "Tasks: ${TASKS[*]}"
log "Model: $MODEL | Attempts: $ATTEMPTS"
log ""
log "Ablation variants:"
log "  - semantic_weight=0.0 (disable semantic search)"
log "  - coupling_depth=0 (disable coupling analysis)"
log "  - recency_weight=0.0 (disable recency boosting)"
log "  - doc_demotion=0.0 (disable doc demotion)"
log "  - gate_threshold=1.0 (disable injection gating)"
log "  - blame_bridging=false (disable blame/doc bridging)"
log ""

for task in "${TASKS[@]}"; do
    log ">>> $task ablations (7 approaches × $ATTEMPTS attempts = $((7 * ATTEMPTS)) runs)"
    python3 -m runner.cli run-task "$task" \
        --approaches with-bobbin \
        -C "semantic_weight=0.0" \
        -C "coupling_depth=0" \
        -C "recency_weight=0.0" \
        -C "doc_demotion=0.0" \
        -C "gate_threshold=1.0" \
        -C "blame_bridging=false" \
        --attempts "$ATTEMPTS" \
        --results-dir "$RESULTS_DIR" \
        --model "$MODEL" \
        --budget "$BUDGET" \
        --timeout "$TIMEOUT" \
        --index-timeout "$INDEX_TIMEOUT" \
        --skip-verify \
        --force-budget 2>&1 | tee -a "$LOGFILE"
    log "<<< $task ablations done"
    log ""
done

log "=== Ablations Complete ==="

# Summary across ALL results
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

if not results:
    print('No completed results found')
else:
    print(f\"\"\"{'Task':<15} {'Approach':<40} {'N':>3} {'Pass%':>6} {'F1':>6} {'Cost':>8}\"\"\")
    print('-' * 85)
    for (task, approach), runs in sorted(results.items()):
        n = len(runs)
        passed = sum(1 for r in runs if r.get('test_result', {}).get('passed'))
        avg_f1 = sum(r.get('diff_result', {}).get('f1', 0) for r in runs) / n
        avg_cost = sum(r.get('agent_result', {}).get('cost_usd', 0) or 0 for r in runs) / n
        print(f'{task:<15} {approach:<40} {n:>3} {passed/n*100:>5.0f}% {avg_f1:>5.2f} \${avg_cost:>7.2f}')
" 2>&1 | tee -a "$LOGFILE"

log ""
log "Log: $LOGFILE"
