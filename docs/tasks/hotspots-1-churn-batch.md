# Task: Batch File Churn Method on GitAnalyzer

## Summary

Add a method to `GitAnalyzer` that efficiently retrieves commit counts for all files in a repository in a single pass. This is the churn signal for the hotspots feature.

## Files

- `src/index/git.rs` (modify) -- add `get_file_churn()` method

## Implementation

Add to `impl GitAnalyzer`:

```rust
/// Get commit counts per file for the entire repo in one pass.
/// Returns a map of file path -> number of commits touching that file.
pub fn get_file_churn(
    &self,
    since: Option<&str>,   // e.g., "1 year ago"
) -> Result<HashMap<String, u32>>
```

**Approach:**

Run `git log --name-only --format="" --since=<since>` once. This outputs just file names (one per line, blank line between commits). Count occurrences of each file path.

Pseudocode:
```rust
let output = Command::new("git")
    .args(["log", "--name-only", "--format="])
    .arg(format!("--since={}", since.unwrap_or("1 year ago")))
    .current_dir(&self.repo_path)
    .output()?;

let mut churn: HashMap<String, u32> = HashMap::new();
for line in String::from_utf8(output.stdout)?.lines() {
    let line = line.trim();
    if !line.is_empty() {
        *churn.entry(line.to_string()).or_insert(0) += 1;
    }
}
Ok(churn)
```

**Edge cases:**
- Renamed files: `git log --follow` only works for single files. For batch mode, count renames as separate files (acceptable simplification).
- Binary files: Include them -- they still represent churn even if not indexed.
- Deleted files: Include them -- they may still be in the current index if recently deleted.

## Pattern Reference

Follow the existing `git` command execution pattern in `GitAnalyzer`. Look at `get_file_history()` for the command spawning and output parsing approach.

## Tests

- Create a test repo with known commit history, verify churn counts
- Verify `--since` filtering works
- Verify empty repo returns empty map

## Acceptance Criteria

- [ ] `get_file_churn()` method exists and compiles
- [ ] Single `git log` invocation (not per-file)
- [ ] Returns accurate commit counts
- [ ] `since` parameter filters correctly
- [ ] Handles repos with no commits gracefully
- [ ] Unit test passes
