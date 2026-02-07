# Task: Transitive Impact Expansion

## Summary

Add depth-based transitive expansion to impact analysis. When `--depth 2`, run impact analysis on the depth-1 results, decaying scores at each level. Uses visited set to prevent cycles.

## Files

- `src/analysis/impact.rs` (modify) -- add transitive expansion to `ImpactAnalyzer`

## Implementation

Modify `ImpactAnalyzer::analyze()` to accept a `depth` parameter:

```rust
pub async fn analyze(
    &self,
    target: &str,
    config: &ImpactConfig,
    depth: u32,              // 1 = direct only, 2+ = transitive
    repo: Option<&str>,
) -> Result<Vec<ImpactResult>>
```

**Algorithm:**

```rust
let mut all_results: HashMap<String, ImpactResult> = HashMap::new();
let mut visited: HashSet<String> = HashSet::new();
let mut current_targets = vec![target.to_string()];
let decay_factor = 0.5;

for level in 0..depth {
    let decay = decay_factor.powi(level as i32);
    let mut next_targets = Vec::new();

    for t in &current_targets {
        if !visited.insert(t.clone()) {
            continue; // Already analyzed
        }
        let results = self.analyze_single(t, config, repo).await?;
        for mut r in results {
            r.score *= decay;
            next_targets.push(r.path.clone());
            all_results
                .entry(r.path.clone())
                .and_modify(|existing| {
                    if r.score > existing.score {
                        *existing = r.clone();
                    }
                })
                .or_insert(r);
        }
    }
    current_targets = next_targets;
}

// Sort, filter, limit
```

**Key decisions:**
- Decay factor 0.5 per depth level (depth 1 = 1.0x, depth 2 = 0.5x, depth 3 = 0.25x)
- If a file appears at multiple depths, keep the highest score
- Visited set prevents re-analyzing the same file (cycle prevention)
- Max depth capped at 3 to prevent runaway expansion

**Refactoring:** Extract the current single-level logic into `analyze_single()` (private method), then `analyze()` becomes the public wrapper that handles transitive expansion.

## Dependencies

- Requires `impact-1-signal-merger`

## Tests

- Verify depth=1 returns only direct impacts
- Verify depth=2 includes transitive impacts with decayed scores
- Verify cycle prevention (A impacts B, B impacts A -- no infinite loop)
- Verify max depth cap
- Verify highest score kept when file appears at multiple depths

## Acceptance Criteria

- [ ] Transitive expansion works for depth > 1
- [ ] Scores decay by 0.5x per level
- [ ] Visited set prevents cycles
- [ ] Depth capped at 3
- [ ] Highest score kept for duplicate files
- [ ] Tests pass
