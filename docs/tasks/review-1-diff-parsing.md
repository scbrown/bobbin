# Task: Git Diff Parsing on GitAnalyzer

## Summary

Add a method to `GitAnalyzer` that parses git diffs into structured data: which files changed, which line ranges were added/removed, and the diff status. This is the input stage for `bobbin review`.

## Files

- `src/index/git.rs` (modify) -- add `get_diff_files()` and supporting types

## Types

```rust
pub struct DiffFile {
    pub path: String,
    pub added_lines: Vec<u32>,     // Line numbers of additions
    pub removed_lines: Vec<u32>,   // Line numbers of removals (in old version)
    pub status: DiffStatus,
}

pub enum DiffStatus {
    Added,
    Modified,
    Deleted,
    Renamed { from: String },
}

pub enum DiffSpec {
    Unstaged,                      // git diff
    Staged,                        // git diff --cached
    Branch(String),                // git diff main..branch
    Range(String),                 // git diff HEAD~3..HEAD
}
```

## Implementation

Add to `impl GitAnalyzer`:

```rust
pub fn get_diff_files(&self, spec: &DiffSpec) -> Result<Vec<DiffFile>>
```

**Steps:**

1. **Build git diff command** based on `DiffSpec`:
   - `Unstaged` → `git diff`
   - `Staged` → `git diff --cached`
   - `Branch(name)` → `git diff main..{name}` (detect default branch)
   - `Range(range)` → `git diff {range}`

2. **Parse unified diff output:** Use `git diff --unified=0 --numstat` for a summary pass (file status + line counts), then `git diff --unified=0` for exact changed line numbers.

   Alternatively, use `git diff -U0` and parse hunk headers (`@@ -old,count +new,count @@`) to extract exact line ranges.

3. **Extract line numbers from hunk headers:**
   ```
   @@ -10,3 +12,5 @@
   ```
   - Removed lines: 10, 11, 12 (old file)
   - Added lines: 12, 13, 14, 15, 16 (new file)

4. **Determine status:** Parse the `--name-status` or `--numstat` output for A/M/D/R status.

**Edge cases:**
- Binary files: Skip (no line numbers to extract)
- Renamed files: Capture old name in `DiffStatus::Renamed`
- New files: All lines are "added"
- Deleted files: All lines are "removed"

## Pattern Reference

Follow existing `git` command execution in `GitAnalyzer`. The hunk header parsing is new but straightforward regex: `@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@`.

## Tests

- Parse a diff with modifications, verify added/removed line numbers
- Parse a diff with added file, verify status = Added
- Parse a diff with deleted file, verify status = Deleted
- Parse a diff with renamed file, verify old name captured
- Verify empty diff returns empty vec

## Acceptance Criteria

- [ ] `get_diff_files()` implemented for all `DiffSpec` variants
- [ ] Hunk headers parsed correctly to extract line numbers
- [ ] File status (Added/Modified/Deleted/Renamed) detected
- [ ] Binary files handled gracefully (skipped)
- [ ] Unit tests pass
