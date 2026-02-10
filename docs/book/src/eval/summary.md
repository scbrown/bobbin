# Results Summary

## Overall Comparison

| Metric | no-bobbin | with-bobbin | Delta |
|--------|:---:|:---:|:---:|
| Runs | 5 | 5 | |
| Test Pass Rate | 20.0% | 20.0% | — |
| Avg Precision | 100.0% | 100.0% | — |
| Avg Recall | 50.7% | 50.7% | — |
| Avg F1 | 66.0% | 66.0% | — |
| Avg Duration | 2.0m | 2.1m | +6% |

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
| flask-001 | python | medium | no-bobbin | 50.0% | 100.0% | 33.3% | 50.0% | 57s |
| flask-001 | python | medium | with-bobbin | 50.0% | 100.0% | 33.3% | 50.0% | 1.4m |
| flask-002 | python | medium | no-bobbin | 0.0% | 100.0% | 66.7% | 80.0% | 2.9m |
| flask-002 | python | medium | with-bobbin | 0.0% | 100.0% | 66.7% | 80.0% | 2.7m |
| flask-003 | python | medium | no-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 2.4m |
| flask-003 | python | medium | with-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 2.0m |
| flask-004 | python | medium | no-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 2.5m |
| flask-004 | python | medium | with-bobbin | 0.0% | 100.0% | 60.0% | 75.0% | 3.0m |
