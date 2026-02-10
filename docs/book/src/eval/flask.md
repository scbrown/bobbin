# Flask (Python)

## flask-001 <span class="eval-medium">medium</span>

**Commit**: [8646edca6f](https://github.com/pallets/flask/commit/8646edca6f47e2cd57464081b3911218d4734f8d)

<details>
<summary>Task prompt</summary>

> Fix the Vary: Cookie header to be set consistently when the session is
accessed, modified, or refreshed. Previously, the header was only set when
the session was modified but not when it was merely accessed or when a
session cookie was being deleted. Move the Vary header logic so it covers
all code paths in save_session, including cookie deletion and session
refresh without modification.

</details>

| Approach | Tests Pass | Precision | Recall | F1 | Duration |
|----------|:----------:|:---------:|:------:|:--:|:--------:|
| no-bobbin | 100.0% | 100.0% | 33.3% | 50.0% | 1.1m |
| with-bobbin | 100.0% | 100.0% | 33.3% | 50.0% | 1.2m |

**Ground truth files**: `CHANGES.rst`, `src/flask/sessions.py`, `tests/test_basic.py`

**Files touched (no-bobbin)**: `src/flask/sessions.py`
**Files touched (with-bobbin)**: `src/flask/sessions.py`

<div class="eval-chart">

<svg width="300" height="180" xmlns="http://www.w3.org/2000/svg">
<rect width="300" height="180" fill="#282a36" rx="6"/>
<text x="150.0" y="24" text-anchor="middle" fill="#f8f8f2" font-size="14" font-weight="600">flask-001 F1 Score</text>
<line x1="50" y1="120.0" x2="280" y2="120.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="124.0" text-anchor="end" fill="#6272a4" font-size="10">0%</text>
<line x1="50" y1="104.0" x2="280" y2="104.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="108.0" text-anchor="end" fill="#6272a4" font-size="10">10%</text>
<line x1="50" y1="88.0" x2="280" y2="88.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="92.0" text-anchor="end" fill="#6272a4" font-size="10">20%</text>
<line x1="50" y1="72.0" x2="280" y2="72.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="76.0" text-anchor="end" fill="#6272a4" font-size="10">30%</text>
<line x1="50" y1="56.0" x2="280" y2="56.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="60.0" text-anchor="end" fill="#6272a4" font-size="10">40%</text>
<line x1="50" y1="40.0" x2="280" y2="40.0" stroke="#44475a" stroke-width="1"/>
<text x="44" y="44.0" text-anchor="end" fill="#6272a4" font-size="10">50%</text>
<rect x="54.0" y="40.0" width="109.0" height="80.0" fill="#bd93f9" rx="2"/>
<text x="108.5" y="36.0" text-anchor="middle" fill="#bd93f9" font-size="9">50%</text>
<rect x="167.0" y="40.0" width="109.0" height="80.0" fill="#50fa7b" rx="2"/>
<text x="221.5" y="36.0" text-anchor="middle" fill="#50fa7b" font-size="9">50%</text>
<text x="165.0" y="136.0" text-anchor="middle" fill="#f8f8f2" font-size="11">flask-001</text>
<rect x="50" y="160" width="12" height="12" rx="2" fill="#bd93f9"/>
<text x="66" y="170" fill="#f8f8f2" font-size="11">no-bobbin</text>
<rect x="190" y="160" width="12" height="12" rx="2" fill="#50fa7b"/>
<text x="206" y="170" fill="#f8f8f2" font-size="11">with-bobbin</text>
</svg>

</div>

---
