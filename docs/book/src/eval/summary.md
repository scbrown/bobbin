# Results Summary

## Overall Comparison

| Metric | no-bobbin | with-bobbin | Delta |
|--------|:---:|:---:|:---:|
| Runs | 12 | 12 | |
| Test Pass Rate | 58.3% | 58.3% | — |
| Avg Precision | 83.3% | 88.9% | +5.6pp |
| Avg Recall | 51.4% | 54.2% | +2.8pp |
| Avg F1 | 61.7% | 64.8% | +3.1pp |
| Avg Duration | 4.0m | 4.0m | -2% |

## Metric Overview

<div class="eval-chart">

![summary_metrics.svg](./charts/summary_metrics.svg)

</div>

## F1 Score by Task

<div class="eval-chart">

![summary_f1_by_task.svg](./charts/summary_f1_by_task.svg)

</div>

## Score Distribution

<div class="eval-chart">

![summary_f1_boxplot.svg](./charts/summary_f1_boxplot.svg)

</div>

## Duration

<div class="eval-chart">

![summary_duration.svg](./charts/summary_duration.svg)

</div>

## Recent Trend

<div class="eval-chart">

![summary_trend.svg](./charts/summary_trend.svg)

</div>

[Full historical trends](./trends.md)

## Per-Task Results

| Task | Language | Difficulty | Approach | Tests | Precision | Recall | F1 | Duration |
|------|----------|:----------:|----------|:-----:|:---------:|:------:|:--:|:--------:|
| flask-001 | python | medium | no-bobbin | 0.0% | 100.0% | 33.3% | 50.0% | 1.4m |
| flask-001 | python | medium | with-bobbin | 0.0% | 100.0% | 33.3% | 50.0% | 1.7m |
| flask-002 | python | medium | no-bobbin | 0.0% | 100.0% | 66.7% | 80.0% | 3.6m |
| flask-002 | python | medium | with-bobbin | 0.0% | 100.0% | 33.3% | 50.0% | 5.0m |
| flask-003 | python | medium | no-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 2.8m |
| flask-003 | python | medium | with-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 3.6m |
| flask-004 | python | medium | no-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 3.2m |
| flask-004 | python | medium | with-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 4.3m |
| flask-005 | python | easy | no-bobbin | 0.0% | 100.0% | 50.0% | 66.7% | 2.6m |
| flask-005 | python | easy | with-bobbin | 0.0% | 100.0% | 50.0% | 66.7% | 2.0m |
| ruff-001 | rust | medium | no-bobbin | 100.0% | 33.3% | 33.3% | 33.3% | 4.0m |
| ruff-001 | rust | medium | with-bobbin | 100.0% | 66.7% | 66.7% | 66.7% | 4.1m |
| ruff-002 | rust | easy | no-bobbin | 100.0% | 100.0% | 40.0% | 57.1% | 4.8m |
| ruff-002 | rust | easy | with-bobbin | 100.0% | 100.0% | 40.0% | 57.1% | 4.2m |
| ruff-003 | rust | medium | no-bobbin | 100.0% | 100.0% | 66.7% | 80.0% | 9.7m |
| ruff-003 | rust | medium | with-bobbin | 100.0% | 100.0% | 66.7% | 80.0% | 6.7m |
| ruff-004 | rust | easy | no-bobbin | 100.0% | 33.3% | 33.3% | 33.3% | 4.1m |
| ruff-004 | rust | easy | with-bobbin | 100.0% | 33.3% | 33.3% | 33.3% | 3.8m |
| ruff-005 | rust | easy | no-bobbin | 100.0% | 100.0% | 100.0% | 100.0% | 3.7m |
| ruff-005 | rust | easy | with-bobbin | 100.0% | 100.0% | 100.0% | 100.0% | 4.0m |
