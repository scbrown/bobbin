# Calibration Sweep Results

Real-world calibration runs on three repos of different sizes and languages.
Run on 2026-02-26 with 20 samples per repo, 15-point core grid (5×3×1).

## Summary

| Repo | Files | Chunks | Languages | Best sw | Best F1 | Default F1 | Improvement |
|------|------:|-------:|-----------|--------:|--------:|-----------:|------------:|
| Flask | 5,599 | 7,171 | Python (1%), git (98%) | 0.50 | 0.082 | 0.061 | +33% |
| Ruff | 9,989 | 63,462 | Rust (17%), Python (27%), git (50%) | 0.00 | 0.125 | 0.065 | +92% |
| Bobbin | 679 | 5,103 | Rust (9%), markdown (23%), git (62%) | 0.90 | 0.094 | 0.094 | +0% |

## Key Observations

1. **Default sw=0.90 is suboptimal for most repos.** Only bobbin itself (small,
   Rust-heavy) was optimal at the default. Flask preferred 0.50, ruff preferred
   pure keyword (0.00).

2. **doc_demotion has minimal impact.** Across all repos, dd values (0.10, 0.30,
   0.50) produced identical or near-identical F1 when sw and k were held constant.
   This suggests the doc/non-doc distinction isn't affecting ranking meaningfully.

3. **rrf_k=60 dominates.** All top configs used k=60 (the only k value tested).
   This warrants expanding the k search space in future sweeps.

4. **Recall > Precision across the board.** All configs produced recall 2-4x
   higher than precision. The system retrieves many files but the right ones are
   in there. Budget/ranking improvements could help precision.

5. **Keyword search won on the largest repo (ruff).** With 63k chunks and mixed
   Rust/Python, pure BM25 outperformed semantic search by 92%. Hypothesis:
   commit messages reference specific identifiers/file names that BM25 matches
   directly, while semantic search dilutes with conceptually similar but
   irrelevant files.

6. **F1 values are low overall (0.06-0.13).** This is expected for file-level
   retrieval — commit messages are imprecise queries and ground truth includes
   all changed files regardless of relevance to the message. The relative
   improvement between configs is more meaningful than absolute F1.

## Top 5 Results Per Repo

### Flask
```
sw=0.50 dd=0.10 k=60  F1=0.082  P=0.071  R=0.130
sw=0.50 dd=0.30 k=60  F1=0.082  P=0.071  R=0.130
sw=0.50 dd=0.50 k=60  F1=0.082  P=0.071  R=0.130
sw=0.00 dd=0.10 k=60  F1=0.075  P=0.063  R=0.128
sw=0.00 dd=0.30 k=60  F1=0.075  P=0.063  R=0.128
```

### Ruff
```
sw=0.00 dd=0.10 k=60  F1=0.125  P=0.099  R=0.251
sw=0.00 dd=0.30 k=60  F1=0.125  P=0.099  R=0.251
sw=0.00 dd=0.50 k=60  F1=0.125  P=0.099  R=0.251
sw=0.50 dd=0.50 k=60  F1=0.111  P=0.075  R=0.259
sw=0.30 dd=0.30 k=60  F1=0.109  P=0.076  R=0.251
```

### Bobbin
```
sw=0.90 dd=0.10 k=60  F1=0.094  P=0.064  R=0.217
sw=0.90 dd=0.30 k=60  F1=0.094  P=0.065  R=0.192
sw=0.90 dd=0.50 k=60  F1=0.094  P=0.065  R=0.192
sw=0.30 dd=0.50 k=60  F1=0.094  P=0.061  R=0.226
sw=0.70 dd=0.10 k=60  F1=0.093  P=0.062  R=0.217
```

## Next Steps

- [ ] Expand rrf_k search space (try 20, 40, 60, 80)
- [ ] Run --full sweep on flask (recency + coupling) to see if extended params help
- [ ] Investigate why doc_demotion has no effect — are doc files being indexed?
- [ ] Consider commit quality filter — exclude merge commits, trivial changes
- [ ] Evaluate chunk-level scoring vs file-level for more granular feedback
