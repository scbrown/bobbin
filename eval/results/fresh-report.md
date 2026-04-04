# Bobbin Eval Report

Generated: 2026-03-02 16:44 UTC
Total tasks: 13 | Total runs: 66

## Summary

| Metric | no-bobbin | with-bobbin | with-bobbin+semantic_weight=0.0 |
|--------|:-:|:-:|:-:|
| Runs | 29 | 36 | 1 |
| Test Pass Rate | 65.5% | 47.2% | 100.0% |
| Avg File Precision | 86.8% | 91.2% | 2.9% |
| Avg File Recall | 61.1% | 64.2% | 33.3% |
| Avg F1 | 69.5% | 72.2% | 5.4% |
| Avg Duration (s) | 252.3 | 209.1 | 266.7 |
| Avg Cost (USD) | $1.18 | $1.42 | $1.59 |
| Avg Input Tokens | 96 | 136 | 316 |
| Avg Output Tokens | 8,065 | 8,608 | 9,822 |

## Per-Task Breakdown

| Task | Approach | Tests Passed | File Precision | File Recall | F1 | Duration | Cost |
|------|----------|:---:|:---:|:---:|:---:|---:|---:|
| cargo-001 | no-bobbin | 100.0% | 100.0% | 100.0% | 100.0% | 314.8s | $1.04 |
| flask-001 | no-bobbin | 0.0% | 100.0% | 33.3% | 50.0% | 78.7s | n/a |
| flask-001 | with-bobbin | 0.0% | 100.0% | 33.3% | 50.0% | 78.4s | n/a |
| flask-002 | no-bobbin | 0.0% | 100.0% | 66.7% | 80.0% | 187.9s | n/a |
| flask-002 | with-bobbin | 0.0% | 100.0% | 55.6% | 70.0% | 211.0s | n/a |
| flask-003 | no-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 132.6s | n/a |
| flask-003 | with-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 146.6s | n/a |
| flask-004 | no-bobbin | 0.0% | 100.0% | 70.0% | 81.9% | 198.2s | n/a |
| flask-004 | with-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 194.4s | n/a |
| flask-005 | no-bobbin | 0.0% | 100.0% | 50.0% | 66.7% | 156.1s | n/a |
| flask-005 | with-bobbin | 0.0% | 100.0% | 58.3% | 73.0% | 113.3s | n/a |
| polars-004 | no-bobbin | 100.0% | 100.0% | 66.7% | 80.0% | 264.8s | $0.81 |
| polars-004 | with-bobbin | 0.0% | 0.0% | 0.0% | 0.0% | 0.0s | n/a |
| polars-005 | no-bobbin | 100.0% | 100.0% | 66.7% | 79.4% | 386.6s | $1.74 |
| ruff-001 | no-bobbin | 100.0% | 31.2% | 33.3% | 32.1% | 239.5s | $0.99 |
| ruff-001 | with-bobbin | 100.0% | 66.7% | 66.7% | 66.7% | 244.7s | $1.33 |
| ruff-001 | with-bobbin+semantic_weight=0.0 | 100.0% | 2.9% | 33.3% | 5.4% | 266.7s | $1.59 |
| ruff-002 | no-bobbin | 100.0% | 100.0% | 40.0% | 57.1% | 287.1s | n/a |
| ruff-002 | with-bobbin | 100.0% | 100.0% | 40.0% | 57.1% | 257.2s | $1.38 |
| ruff-003 | no-bobbin | 100.0% | 100.0% | 83.3% | 90.0% | 552.0s | n/a |
| ruff-003 | with-bobbin | 100.0% | 100.0% | 77.8% | 86.7% | 375.3s | $1.92 |
| ruff-004 | no-bobbin | 100.0% | 46.7% | 66.7% | 54.2% | 233.7s | n/a |
| ruff-004 | with-bobbin | 80.0% | 63.3% | 83.3% | 70.8% | 269.9s | $1.67 |
| ruff-005 | no-bobbin | 100.0% | 100.0% | 100.0% | 100.0% | 218.1s | n/a |
| ruff-005 | with-bobbin | 100.0% | 100.0% | 100.0% | 100.0% | 167.6s | $0.63 |
