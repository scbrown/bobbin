# Results Summary

## Overall Comparison

| Metric | no-bobbin | with-bobbin | Delta |
|--------|:---:|:---:|:---:|
| Runs | 1 | 1 | |
| Test Pass Rate | 100.0% | 100.0% | — |
| Avg Precision | 50.0% | 100.0% | +50.0pp |
| Avg Recall | 33.3% | 33.3% | — |
| Avg F1 | 40.0% | 50.0% | +10.0pp |
| Avg Duration | 2.3m | 1.4m | -39% |

## F1 Score by Task

<div class="eval-chart">

<svg width="600" height="300" xmlns="http://www.w3.org/2000/svg">
<rect width="600" height="300" fill="#282a36" rx="6"/>
<text x="300.0" y="24" text-anchor="middle" fill="#f8f8f2" font-size="14" font-weight="600">F1 Score Comparison</text>
<line x1="50" y1="240.0" x2="580" y2="240.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="244.0" text-anchor="end" fill="#6272a4" font-size="10">0%</text>
<line x1="50" y1="200.0" x2="580" y2="200.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="204.0" text-anchor="end" fill="#6272a4" font-size="10">10%</text>
<line x1="50" y1="160.0" x2="580" y2="160.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="164.0" text-anchor="end" fill="#6272a4" font-size="10">20%</text>
<line x1="50" y1="120.0" x2="580" y2="120.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="124.0" text-anchor="end" fill="#6272a4" font-size="10">30%</text>
<line x1="50" y1="80.0" x2="580" y2="80.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="84.0" text-anchor="end" fill="#6272a4" font-size="10">40%</text>
<line x1="50" y1="40.0" x2="580" y2="40.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="44.0" text-anchor="end" fill="#6272a4" font-size="10">50%</text>
<rect x="54.0" y="80.0" width="259.0" height="160.0" fill="#bd93f9" rx="2"/>
<text x="183.5" y="76.0" text-anchor="middle" fill="#bd93f9" font-size="9">40%</text>
<rect x="317.0" y="40.0" width="259.0" height="200.0" fill="#50fa7b" rx="2"/>
<text x="446.5" y="36.0" text-anchor="middle" fill="#50fa7b" font-size="9">50%</text>
<text x="315.0" y="256.0" text-anchor="middle" fill="#f8f8f2" font-size="11">flask-001</text>
<rect x="50" y="280" width="12" height="12" rx="2" fill="#bd93f9"/>
<text x="66" y="290" fill="#f8f8f2" font-size="11">no-bobbin</text>
<rect x="190" y="280" width="12" height="12" rx="2" fill="#50fa7b"/>
<text x="206" y="290" fill="#f8f8f2" font-size="11">with-bobbin</text>
</svg>

</div>

## Per-Task Results

| Task | Language | Difficulty | Approach | Tests | Precision | Recall | F1 | Duration |
|------|----------|:----------:|----------|:-----:|:---------:|:------:|:--:|:--------:|
| flask-001 | python | medium | no-bobbin | 100.0% | 50.0% | 33.3% | 40.0% | 2.3m |
| flask-001 | python | medium | with-bobbin | 100.0% | 100.0% | 33.3% | 50.0% | 1.4m |

