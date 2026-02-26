# Calibration Sweep Results

## Round 2: Post Commit-Chunk Filter (2026-02-26)

180-config grid (5 sw × 3 dd × 1 k × 3 b × 4 sl), 20 samples per repo.
Commit chunks filtered from context injection (a975e76) — they remain indexed
but no longer consume budget or compete in search rankings.

### Summary

| Repo | Chunks | Best Config | Best F1 | Round 1 F1 | Change |
|------|-------:|-------------|--------:|-----------:|-------:|
| Flask | 7,171 | sw=0.50 dd=0.1 sl=10 b=500 | 0.138 | 0.092 | **+50%** |
| Ruff | 63,462 | sw=0.50 dd=0.5 sl=20 b=300 | 0.246 | 0.129 | **+91%** |
| Bobbin | 5,103 | sw=0.30 dd=0.1 sl=30 b=300 | 0.217 | 0.117 | **+85%** |

### Key Findings

1. **Filtering commit chunks from context is the single biggest quality win.**
   F1 improved 50-91% across all repos with no other changes. Commits were
   50-98% of chunks, consuming budget and diluting rankings.

2. **doc_demotion now differentiates** (partially). On ruff, dd=0.5 beats dd=0.1
   by +7%. On bobbin, dd=0.1 beats dd=0.3 by +8% (docs are relevant there).
   Flask still shows no differentiation (too few doc/config chunks remain).

3. **Semantic search rehabilitated.** Ruff flipped from pure-keyword (sw=0.0)
   to balanced (sw=0.50). Bobbin shifted from sw=0.90 to sw=0.30. Commit chunks
   were masking semantic signal — BM25 matched commit message identifiers, making
   keyword search appear superior.

4. **Precision improved dramatically.** Flask P: 0.071→0.166 (+134%), Ruff P:
   0.099→0.235 (+137%), Bobbin P: 0.064→0.174 (+172%). Removing commit noise
   improved precision more than recall.

5. **search_limit and budget shifted.** Ruff optimal sl went 10→20, bobbin
   sl went 40→30. Budget: ruff prefers b=300 (not 500), bobbin also b=300.

### Top 5 Results Per Repo

#### Flask
```
sw=0.50 dd=0.10 sl=10 b=500  F1=0.138  P=0.166  R=0.141
sw=0.50 dd=0.30 sl=10 b=500  F1=0.138  P=0.166  R=0.141
sw=0.50 dd=0.50 sl=10 b=500  F1=0.138  P=0.166  R=0.141
sw=0.50 dd=0.10 sl=10 b=300  F1=0.132  P=0.163  R=0.133
sw=0.50 dd=0.30 sl=10 b=300  F1=0.132  P=0.163  R=0.133
```

#### Ruff
```
sw=0.50 dd=0.50 sl=20 b=300  F1=0.246  P=0.235  R=0.318
sw=0.00 dd=0.30 sl=20 b=300  F1=0.240  P=0.265  R=0.305
sw=0.00 dd=0.10 sl=20 b=300  F1=0.239  P=0.267  R=0.294
sw=0.00 dd=0.50 sl=20 b=300  F1=0.238  P=0.266  R=0.294
sw=0.00 dd=0.50 sl=40 b=500  F1=0.237  P=0.252  R=0.254
```

#### Bobbin
```
sw=0.30 dd=0.10 sl=30 b=300  F1=0.217  P=0.174  R=0.351
sw=0.90 dd=0.10 sl=40 b=150  F1=0.216  P=0.230  R=0.267
sw=0.70 dd=0.30 sl=30 b=150  F1=0.212  P=0.243  R=0.257
sw=0.70 dd=0.10 sl=30 b=150  F1=0.206  P=0.216  R=0.245
sw=0.30 dd=0.10 sl=30 b=150  F1=0.205  P=0.233  R=0.229
```

---

## Round 1: Pre Commit-Chunk Filter (2026-02-26, historical)

180-config grid, 20 samples per repo. Commit chunks competed in search rankings
and consumed context budget. Included here for comparison.

### Summary

| Repo | Chunks | Best Config | Best F1 | Default F1 | Improvement |
|------|-------:|-------------|--------:|-----------:|------------:|
| Flask | 7,171 | sw=0.50 sl=10 | 0.092 | 0.082 | +10% |
| Ruff | 63,462 | sw=0.00 sl=10 b=500 | 0.129 | 0.065 | +78% |
| Bobbin | 5,103 | sw=0.90 sl=40 | 0.117 | 0.094 | +0% |

### Observations (now explained by commit chunk pollution)

1. doc_demotion had zero impact — commit chunks were classified as Source,
   bypassing demotion. The knob only touched the 2-50% non-commit minority.

2. Keyword search dominated on ruff — BM25 matched commit message identifiers
   directly, making it appear superior to semantic search.

3. F1 values were low (0.06-0.13) — commit chunks consumed budget slots that
   should have gone to source/doc files.

---

## Next Steps

- [ ] Expand rrf_k search space (try 20, 40, 60, 80)
- [ ] Run --full sweep (recency + coupling) post commit-filter
- [ ] Explore commit-to-source bridging (use matching commits to boost their touched files)
- [ ] Evaluate chunk-level scoring vs file-level for more granular feedback
- [x] ~~Investigate why doc_demotion has no effect~~ → Commit chunks bypassed it (fixed a975e76)
