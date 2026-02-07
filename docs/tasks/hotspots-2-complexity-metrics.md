# Task: AST Complexity Metrics Module

## Summary

Create a complexity analysis module that computes structural complexity metrics from Tree-sitter ASTs. These metrics combine with git churn to identify hotspots.

## Files

- `src/analysis/complexity.rs` (new) -- complexity computation
- `src/analysis/mod.rs` (modify) -- add `pub mod complexity;`

## Types

```rust
pub struct ComplexityMetrics {
    pub ast_depth: u32,           // Maximum nesting depth
    pub node_count: u32,          // Total AST nodes
    pub cyclomatic: u32,          // Branch points count
    pub combined: f32,            // Normalized combined score [0, 1]
}

pub struct FileComplexity {
    pub path: String,
    pub complexity: f32,          // Average of chunk complexities, weighted by size
    pub chunk_count: usize,
    pub chunks: Vec<ChunkComplexity>,
}

pub struct ChunkComplexity {
    pub name: Option<String>,
    pub chunk_type: ChunkType,
    pub start_line: u32,
    pub end_line: u32,
    pub metrics: ComplexityMetrics,
}
```

## Implementation

Add `ComplexityAnalyzer`:

```rust
pub struct ComplexityAnalyzer {
    parser: CodeParser,
}

impl ComplexityAnalyzer {
    /// Compute complexity for a single chunk's content.
    pub fn analyze_chunk(&self, content: &str, language: &str) -> Result<ComplexityMetrics>

    /// Compute file-level complexity by averaging chunk complexities.
    pub fn analyze_file(&self, path: &str, content: &str, language: &str) -> Result<FileComplexity>
}
```

**Metrics computation (from Tree-sitter AST):**

1. **AST depth:** Walk the tree recursively, track max depth.

2. **Node count:** Count all nodes in the tree.

3. **Cyclomatic complexity:** Count branch point nodes. Per language:
   - Rust: `if_expression`, `match_expression`, `for_expression`, `while_expression`, `loop_expression`, `binary_expression` (when operator is `||` or `&&`)
   - TypeScript/JS: `if_statement`, `switch_case`, `for_statement`, `while_statement`, `do_statement`, `ternary_expression`, `binary_expression` (`||`, `&&`, `??`)
   - Python: `if_statement`, `for_statement`, `while_statement`, `elif_clause`, `and`, `or`
   - Go: `if_statement`, `for_statement`, `select_statement`, `case_clause`
   - Base cyclomatic = 1 + branch_count

4. **Combined score:** Normalize each metric to [0, 1] using reasonable maximums (e.g., depth > 10 = 1.0, cyclomatic > 20 = 1.0). Combined = weighted average: `0.4 * cyclomatic_norm + 0.3 * depth_norm + 0.3 * node_count_norm`.

**On-the-fly vs pre-computed:** Compute on-the-fly by re-parsing chunk content with Tree-sitter. This avoids schema changes. If performance is a concern for large repos, a follow-up task can add pre-computed metrics to the index.

## Pattern Reference

Follow the same Tree-sitter usage pattern as `CodeParser::extract_chunks()` in `src/index/parser.rs`. Use the existing language detection and parser initialization.

## Dependencies

None -- standalone module.

## Tests

- Compute metrics for a simple function (known depth, known branches)
- Compute metrics for a complex function with nested control flow
- Verify cyclomatic complexity count for each supported language
- Verify normalization produces values in [0, 1]

## Acceptance Criteria

- [ ] `ComplexityAnalyzer` computes all three metrics
- [ ] Cyclomatic complexity counts correct branch nodes per language
- [ ] Combined score normalized to [0, 1]
- [ ] File-level complexity correctly averages chunk complexities
- [ ] Works for Rust, TypeScript, Python, Go at minimum
- [ ] Tests pass
