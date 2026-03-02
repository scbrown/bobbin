#!/bin/bash
# Quick progress check for the baseline study
# Usage: bash check-study-progress.sh

EVAL_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$EVAL_DIR"

echo "=== Baseline Study Progress ==="
echo ""

# Count completed runs by task and approach
python3 -c "
import json, glob, os
from collections import defaultdict

runs_dir = 'results/runs'
results = defaultdict(list)
errors = defaultdict(list)

for run_dir in sorted(glob.glob(os.path.join(runs_dir, '*'))):
    for f in glob.glob(os.path.join(run_dir, '*.json')):
        if f.endswith('manifest.json') or f.endswith('_metrics.jsonl'):
            continue
        try:
            d = json.load(open(f))
            key = (d['task_id'], d['approach'])
            if d.get('status') == 'completed':
                results[key].append(d)
            else:
                errors[key].append(d)
        except:
            pass

# Study targets
study_tasks = ['ruff-001', 'cargo-001', 'django-001', 'pandas-001']
baselines = ['no-bobbin', 'with-bobbin']
ablations = [
    'with-bobbin+semantic_weight=0.0',
    'with-bobbin+coupling_depth=0',
    'with-bobbin+recency_weight=0.0',
    'with-bobbin+doc_demotion=0.0',
    'with-bobbin+gate_threshold=1.0',
    'with-bobbin+blame_bridging=false',
]
all_approaches = baselines + ablations
TARGET = 3  # attempts per approach

total_needed = len(study_tasks) * len(all_approaches) * TARGET
total_done = 0
total_errors = 0

print(f'{'Task':<15} {'Approach':<45} {'Done':>4}/{TARGET} {'F1':>6} {'Pass%':>6} {'Cost':>7}')
print('-' * 95)

for task in study_tasks:
    for approach in all_approaches:
        key = (task, approach)
        runs = results.get(key, [])
        errs = errors.get(key, [])
        n = len(runs)
        total_done += n
        total_errors += len(errs)

        if n > 0:
            avg_f1 = sum(r.get('diff_result', {}).get('f1', 0) for r in runs) / n
            passed = sum(1 for r in runs if r.get('test_result', {}).get('passed'))
            avg_cost = sum(r.get('agent_result', {}).get('cost_usd', 0) or 0 for r in runs) / n
            status = '✓' if n >= TARGET else '◐'
            print(f'{task:<15} {approach:<45} {status}{n:>3}/{TARGET} {avg_f1:>5.3f} {passed/n*100:>5.0f}% \${avg_cost:>6.2f}')
        elif len(errs) > 0:
            print(f'{task:<15} {approach:<45} ✗{len(errs):>3}/{TARGET} ERROR')
        else:
            print(f'{task:<15} {approach:<45}   0/{TARGET}')
    print()

print(f'Total: {total_done}/{total_needed} completed, {total_errors} errors')
pct = total_done / total_needed * 100 if total_needed else 0
est_cost = sum(r.get('agent_result', {}).get('cost_usd', 0) or 0 for runs in results.values() for r in runs)
print(f'Progress: {pct:.0f}% | Spent: \${est_cost:.2f}')
"
