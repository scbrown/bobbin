use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::types::FileCoupling;

/// A single entry in a file's commit history
#[derive(Debug, Clone)]
pub struct FileHistoryEntry {
    pub date: String,
    pub author: String,
    pub message: String,
    pub issues: Vec<String>,
    pub timestamp: i64,
}

/// A full commit entry for semantic indexing
#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub files: Vec<String>,
    pub timestamp: i64,
    /// Git trailers (e.g. Co-Authored-By, Bead-ID)
    pub trailers: Vec<(String, String)>,
}

/// A single line from git blame output, mapping a line to its originating commit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameEntry {
    pub commit_hash: String,
    pub line_number: u32,
}

/// Specifies which diff to analyze
#[derive(Debug, Clone)]
pub enum DiffSpec {
    /// Unstaged working tree changes
    Unstaged,
    /// Staged (cached) changes only
    Staged,
    /// Compare a branch against its merge base with the current branch
    Branch(String),
    /// A commit range, e.g. "HEAD~3..HEAD"
    Range(String),
}

/// Status of a file in a diff
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl std::fmt::Display for DiffStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffStatus::Added => write!(f, "added"),
            DiffStatus::Modified => write!(f, "modified"),
            DiffStatus::Deleted => write!(f, "deleted"),
            DiffStatus::Renamed => write!(f, "renamed"),
        }
    }
}

/// A file and its changed line ranges from a git diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFile {
    /// File path (relative to repo root)
    pub path: String,
    /// Line numbers of added lines in the new version
    pub added_lines: Vec<u32>,
    /// Line numbers of removed lines in the old version
    pub removed_lines: Vec<u32>,
    /// Whether the file was added, modified, deleted, or renamed
    pub status: DiffStatus,
}

/// Analyzes git history to find temporal coupling between files
pub struct GitAnalyzer {
    repo_root: std::path::PathBuf,
}

impl GitAnalyzer {
    /// Create a new git analyzer for the given repository
    pub fn new(repo_root: &Path) -> Result<Self> {
        // Verify this is a git repository
        let output = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(repo_root)
            .output()
            .context("Failed to run git command")?;

        if !output.status.success() {
            anyhow::bail!("Not a git repository: {}", repo_root.display());
        }

        Ok(Self {
            repo_root: repo_root.to_path_buf(),
        })
    }

    /// Get the repository root path
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// Analyze git history to find files that change together
    pub fn analyze_coupling(
        &self,
        depth: usize,
        threshold: u32,
        freq_weight: f32,
        recency_days: f32,
    ) -> Result<Vec<FileCoupling>> {
        // Get commit log with files changed
        // Format: COMMIT:<hash>:<timestamp>
        // followed by list of files
        let mut args = vec![
            "log".to_string(),
            "--pretty=format:COMMIT:%H:%ct".to_string(),
            "--name-only".to_string(),
            "--no-merges".to_string(),
        ];
        if depth > 0 {
            args.push(format!("-{}", depth));
        }

        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get git log")?;

        let log = String::from_utf8_lossy(&output.stdout);
        let commits = parse_git_log(&log);

        // String interning: map paths to u32 IDs to avoid millions of String clones
        let mut path_to_id: HashMap<String, u32> = HashMap::new();
        let mut id_to_path: Vec<String> = Vec::new();

        let mut intern = |path: &str| -> u32 {
            if let Some(&id) = path_to_id.get(path) {
                id
            } else {
                let id = id_to_path.len() as u32;
                id_to_path.push(path.to_string());
                path_to_id.insert(path.to_string(), id);
                id
            }
        };

        // Build co-change matrix using interned IDs
        let mut co_changes: HashMap<(u32, u32), u32> = HashMap::new();
        let mut last_seen: HashMap<(u32, u32), i64> = HashMap::new();
        let mut max_co_changes = 0u32;

        /// Max files per commit before we skip it (avoids O(n²) from reformats)
        const MAX_FILES_PER_COMMIT: usize = 50;

        for (commit_time, files) in commits {
            // Skip mega-commits (reformats, renames) to prevent pair explosion
            if files.len() > MAX_FILES_PER_COMMIT {
                continue;
            }

            let ids: Vec<u32> = files.iter().map(|f| intern(f)).collect();

            for i in 0..ids.len() {
                for j in (i + 1)..ids.len() {
                    let key = if ids[i] < ids[j] {
                        (ids[i], ids[j])
                    } else {
                        (ids[j], ids[i])
                    };

                    let count = co_changes.entry(key).or_insert(0);
                    *count += 1;
                    if *count > max_co_changes {
                        max_co_changes = *count;
                    }

                    let last = last_seen.entry(key).or_insert(0);
                    if commit_time > *last {
                        *last = commit_time;
                    }
                }
            }
        }

        // Current time for recency calculation
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Convert to FileCoupling, filtering by threshold
        let mut couplings: Vec<FileCoupling> = co_changes
            .into_iter()
            .filter(|(_, count)| *count >= threshold)
            .map(|((id_a, id_b), count)| {
                let file_a = id_to_path[id_a as usize].clone();
                let file_b = id_to_path[id_b as usize].clone();
                let last_co_change = last_seen
                    .get(&(id_a, id_b))
                    .copied()
                    .unwrap_or(0);

                FileCoupling {
                    file_a,
                    file_b,
                    score: calculate_coupling_score(
                        count,
                        max_co_changes,
                        last_co_change,
                        now,
                        freq_weight,
                        recency_days,
                    ),
                    co_changes: count,
                    last_co_change,
                }
            })
            .collect();

        // Sort by score descending
        couplings.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(couplings)
    }

    /// Extract `bead_id -> (touched files, latest commit timestamp)` for this
    /// repo from its git history (bo-oqny cross-repo coupling).
    ///
    /// Reuses the same bead-trailer parsing as lineage (`extract_bead_refs`), so
    /// only explicit `Bead*` trailers count. A bead may span several commits; the
    /// file set is the union and the timestamp is the most recent. Mega-beads
    /// (more than `MAX_FILES_PER_BEAD` distinct files) are skipped to avoid pair
    /// explosion, mirroring the per-repo coupling guard.
    pub fn bead_file_map(
        &self,
        depth: usize,
    ) -> Result<HashMap<String, (std::collections::BTreeSet<String>, i64)>> {
        const MAX_FILES_PER_BEAD: usize = 50;
        let commits = self.get_commit_log(depth, None)?;
        let mut map: HashMap<String, (std::collections::BTreeSet<String>, i64)> = HashMap::new();
        for entry in commits {
            let beads = extract_bead_refs(&entry.trailers);
            if beads.is_empty() || entry.files.is_empty() {
                continue;
            }
            for bead in beads {
                let e = map
                    .entry(bead)
                    .or_insert_with(|| (std::collections::BTreeSet::new(), 0));
                for f in &entry.files {
                    e.0.insert(f.clone());
                }
                if entry.timestamp > e.1 {
                    e.1 = entry.timestamp;
                }
            }
        }
        // Drop mega-beads (sweeping reformats/renames tagged with one bead).
        map.retain(|_, (files, _)| files.len() <= MAX_FILES_PER_BEAD);
        Ok(map)
    }

    /// Get files changed in a specific commit
    // TODO(bobbin-6vq): For incremental indexing
    #[allow(dead_code)]
    pub fn get_commit_files(&self, commit_hash: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args([
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                commit_hash,
            ])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get commit files")?;

        let files = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(files)
    }

    /// Get list of files that have changed since the last index
    // TODO(bobbin-6vq): For incremental indexing
    #[allow(dead_code)]
    pub fn get_changed_files(&self, since_commit: Option<&str>) -> Result<Vec<String>> {
        let args = match since_commit {
            Some(commit) => vec!["diff", "--name-only", commit, "HEAD"],
            None => vec!["ls-files"],
        };

        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get changed files")?;

        let files = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(files)
    }

    /// Get the current HEAD commit hash
    pub fn get_head_commit(&self) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get HEAD commit")?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Get the committer timestamp (unix seconds) of the current HEAD commit.
    ///
    /// Used as a freshness reference: if HEAD is newer than the last index run,
    /// committed changes have not yet been picked up (bobbin #44). Returns None
    /// when there is no HEAD (empty repo) or git is unavailable.
    pub fn get_head_commit_time(&self) -> Option<i64> {
        let output = Command::new("git")
            .args(["log", "-1", "--format=%ct"])
            .current_dir(&self.repo_root)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout).trim().parse::<i64>().ok()
    }

    /// Get the last commit timestamp for every file in the repo in one pass.
    ///
    /// Returns a map of relative file path -> unix timestamp of the most recent
    /// commit that touched that file. Uses reverse-chronological git log order
    /// so the first occurrence of each file is its most recent commit.
    pub fn get_file_last_modified(&self) -> Result<HashMap<String, i64>> {
        let output = Command::new("git")
            .args(["log", "--format=COMMIT %ct", "--name-only"])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get file timestamps from git log")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut timestamps: HashMap<String, i64> = HashMap::new();
        let mut current_ts: Option<i64> = None;

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(ts_str) = line.strip_prefix("COMMIT ") {
                current_ts = ts_str.parse::<i64>().ok();
            } else if let Some(ts) = current_ts {
                // First occurrence = most recent commit (git log is reverse-chronological)
                timestamps.entry(line.to_string()).or_insert(ts);
            }
        }

        Ok(timestamps)
    }

    /// Get commit log for semantic indexing.
    ///
    /// Returns commit entries with hash, author, date, message, and touched files.
    /// If `since_commit` is provided, only returns commits after that commit.
    pub fn get_commit_log(
        &self,
        depth: usize,
        since_commit: Option<&str>,
    ) -> Result<Vec<CommitEntry>> {
        // Format: ENTRY<sep>hash<sep>timestamp<sep>author<sep>subject<sep>trailers
        // followed by list of files (--name-only)
        // Trailers are separated by RS (\x1e) via %(trailers) format
        let sep = "\x1f"; // unit separator
        let format_str = format!(
            "ENTRY{}%H{}%ct{}%an{}%s{}%(trailers:separator=%x1e,unfold)",
            sep, sep, sep, sep, sep
        );

        let mut args = vec![
            "log".to_string(),
            format!("--pretty=format:{}", format_str),
            "--name-only".to_string(),
        ];

        if let Some(commit) = since_commit {
            args.push(format!("{}..HEAD", commit));
        } else if depth > 0 {
            args.push(format!("-{}", depth));
        }
        // depth == 0 means "all commits" — no -N flag needed

        let output = Command::new("git")
            .args(&args)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get commit log")?;

        let log = String::from_utf8_lossy(&output.stdout);
        Ok(parse_commit_log(&log, sep))
    }

    /// Get commit counts per file for the entire repo in one pass.
    /// Returns a map of file path -> number of commits touching that file.
    pub fn get_file_churn(
        &self,
        since: Option<&str>,
    ) -> Result<HashMap<String, u32>> {
        let since_val = since.unwrap_or("1 year ago");
        let output = Command::new("git")
            .args(["log", "--name-only", "--format=", &format!("--since={}", since_val)])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get file churn from git log")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut churn: HashMap<String, u32> = HashMap::new();
        for line in stdout.lines() {
            let line = line.trim();
            if !line.is_empty() {
                *churn.entry(line.to_string()).or_insert(0) += 1;
            }
        }
        Ok(churn)
    }

    /// Extract changed files with line-level detail from a git diff.
    ///
    /// Parses the unified diff output to determine which files changed
    /// and exactly which lines were added or removed.
    pub fn get_diff_files(&self, spec: &DiffSpec) -> Result<Vec<DiffFile>> {
        // Step 1: Get the list of files and their statuses via --name-status
        let name_status_args = match spec {
            DiffSpec::Unstaged => vec!["diff", "--name-status"],
            DiffSpec::Staged => vec!["diff", "--cached", "--name-status"],
            DiffSpec::Branch(branch) => {
                // Find the merge base between the branch and HEAD
                let merge_base_output = Command::new("git")
                    .args(["merge-base", "HEAD", branch])
                    .current_dir(&self.repo_root)
                    .output()
                    .context("Failed to find merge base")?;
                let merge_base = String::from_utf8_lossy(&merge_base_output.stdout)
                    .trim()
                    .to_string();
                if merge_base.is_empty() {
                    anyhow::bail!(
                        "Could not find merge base between HEAD and '{}'",
                        branch
                    );
                }
                return self.get_diff_files(&DiffSpec::Range(format!("{}..{}", merge_base, branch)));
            }
            DiffSpec::Range(range) => vec!["diff", "--name-status", range.as_str()],
        };

        let status_output = Command::new("git")
            .args(&name_status_args)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get diff name-status")?;

        let status_text = String::from_utf8_lossy(&status_output.stdout);
        let file_statuses = parse_name_status(&status_text);

        if file_statuses.is_empty() {
            return Ok(Vec::new());
        }

        // Step 2: Get the unified diff with line numbers
        let diff_args: Vec<&str> = match spec {
            DiffSpec::Unstaged => vec!["diff", "-U0"],
            DiffSpec::Staged => vec!["diff", "--cached", "-U0"],
            DiffSpec::Range(range) => vec!["diff", "-U0", range.as_str()],
            DiffSpec::Branch(_) => unreachable!("Branch is converted to Range above"),
        };

        let diff_output = Command::new("git")
            .args(&diff_args)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get unified diff")?;

        let diff_text = String::from_utf8_lossy(&diff_output.stdout);
        let line_changes = parse_unified_diff(&diff_text);

        // Step 3: Combine status info with line-level changes
        let mut results: Vec<DiffFile> = Vec::new();
        for (path, status) in &file_statuses {
            let (added, removed) = line_changes
                .get(path.as_str())
                .cloned()
                .unwrap_or_default();

            results.push(DiffFile {
                path: path.clone(),
                added_lines: added,
                removed_lines: removed,
                status: *status,
            });
        }

        results.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(results)
    }

    /// Get commit history for a specific file
    pub fn get_file_history(&self, file_path: &str, limit: usize) -> Result<Vec<FileHistoryEntry>> {
        // Format: hash|timestamp|author|subject
        let output = Command::new("git")
            .args([
                "log",
                "--pretty=format:%H|%ct|%an|%s",
                &format!("-{}", limit),
                "--follow",
                "--",
                file_path,
            ])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get file history")?;

        let log = String::from_utf8_lossy(&output.stdout);
        let entries = parse_file_history(&log);

        Ok(entries)
    }

    /// Blame a specific line range to find the commits that introduced those lines.
    /// Returns one BlameEntry per line with the commit hash that last modified it.
    pub fn blame_lines(&self, file_path: &str, start: u32, end: u32) -> Result<Vec<BlameEntry>> {
        let range = format!("{},{}", start, end);
        let output = Command::new("git")
            .args(["blame", "-L", &range, "--porcelain", "--", file_path])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to run git blame")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git blame failed for {}:{}-{}: {}", file_path, start, end, stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_blame_porcelain(&stdout))
    }

    /// Blame a line range as of a specific revision (instead of the working tree).
    /// Used by SZZ-style culprit detection: blame the pre-fix file (`<fix>^`) at
    /// the lines a fix removed to find the commit that introduced them.
    pub fn blame_lines_at_rev(
        &self,
        rev: &str,
        file_path: &str,
        start: u32,
        end: u32,
    ) -> Result<Vec<BlameEntry>> {
        let range = format!("{},{}", start, end);
        let output = Command::new("git")
            .args(["blame", rev, "-L", &range, "--porcelain", "--", file_path])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to run git blame")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "git blame {} failed for {}:{}-{}: {}",
                rev,
                file_path,
                start,
                end,
                stderr.trim()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_blame_porcelain(&stdout))
    }

    /// SZZ-lite culprit detection: find the commits that last touched the lines a
    /// fix commit removed or modified in `file`.
    ///
    /// Diffs `fix_sha` against its first parent (zero context) to get the
    /// old-side line ranges the fix deleted, then blames those ranges against
    /// `fix_sha^`. Returns one [`BlameEntry`] per blamed line (the introducing
    /// commit + its old line number). Returns an empty vec when the fix only
    /// *added* lines, when `file` is newly created by the fix, or when blame is
    /// unavailable (e.g. the fix is a root commit with no parent).
    pub fn blame_fix_culprits(&self, fix_sha: &str, file: &str) -> Result<Vec<BlameEntry>> {
        // Diff the fix against its first parent, no context, for this file only.
        let output = Command::new("git")
            .args([
                "show",
                fix_sha,
                "--unified=0",
                "--pretty=format:",
                "--",
                file,
            ])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to run git show")?;

        if !output.status.success() {
            anyhow::bail!(
                "git show failed for {} {}: {}",
                fix_sha,
                file,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let diff = String::from_utf8_lossy(&output.stdout);
        let parsed = parse_unified_diff(&diff);
        // removed_lines are old-side line numbers in the pre-fix file.
        let removed = match parsed.get(file) {
            Some((_added, removed)) if !removed.is_empty() => removed.clone(),
            _ => return Ok(Vec::new()),
        };

        let parent = format!("{fix_sha}^");
        let mut entries = Vec::new();
        for (start, end) in group_consecutive(&removed) {
            // A missing parent (root commit) or a file absent at the parent blames
            // to nothing — skip those ranges rather than failing the whole bug.
            if let Ok(mut e) = self.blame_lines_at_rev(&parent, file, start, end) {
                entries.append(&mut e);
            }
        }
        Ok(entries)
    }
}

/// Parse file history log into entries
fn parse_file_history(log: &str) -> Vec<FileHistoryEntry> {
    let mut entries = Vec::new();

    // Match issue IDs like "bobbin-123", "bobbin-xyz", "JIRA-456", "GH-789", "#123"
    // Support both numeric IDs (JIRA-123) and alphanumeric IDs (bobbin-abc)
    let issue_regex = Regex::new(r"(?i)([a-z]+-[a-z0-9]+|#\d+)").unwrap();

    for line in log.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: hash|timestamp|author|subject
        let parts: Vec<&str> = line.splitn(4, '|').collect();
        if parts.len() < 4 {
            continue;
        }

        let timestamp = parts[1].parse::<i64>().unwrap_or(0);
        let author = parts[2].to_string();
        let message = parts[3].to_string();

        // Extract issue IDs from commit message
        let issues: Vec<String> = issue_regex
            .find_iter(&message)
            .map(|m| m.as_str().to_string())
            .collect();

        // Format date as YYYY-MM-DD
        let date = format_timestamp(timestamp);

        entries.push(FileHistoryEntry {
            date,
            author,
            message,
            issues,
            timestamp,
        });
    }

    entries
}

/// Parse commit log output into CommitEntry structs
fn parse_commit_log(log: &str, sep: &str) -> Vec<CommitEntry> {
    let mut entries = Vec::new();
    let mut current: Option<(String, i64, String, String, Vec<(String, String)>)> = None;
    let mut current_files: Vec<String> = Vec::new();

    let entry_prefix = format!("ENTRY{}", sep);

    for line in log.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with(&entry_prefix) {
            // Save previous entry if any
            if let Some((hash, timestamp, author, message, trailers)) = current.take() {
                entries.push(CommitEntry {
                    hash,
                    author,
                    date: format_timestamp(timestamp),
                    message,
                    files: std::mem::take(&mut current_files),
                    timestamp,
                    trailers,
                });
            }

            // Parse: ENTRY<sep>hash<sep>timestamp<sep>author<sep>subject<sep>trailers
            let parts: Vec<&str> = line.splitn(6, sep).collect();
            if parts.len() >= 5 {
                let hash = parts[1].to_string();
                let timestamp = parts[2].parse::<i64>().unwrap_or(0);
                let author = parts[3].to_string();
                let message = parts[4].to_string();
                let trailers = if parts.len() >= 6 && !parts[5].is_empty() {
                    parse_trailers(parts[5])
                } else {
                    Vec::new()
                };
                current = Some((hash, timestamp, author, message, trailers));
            }
        } else {
            // This is a file path
            current_files.push(line.to_string());
        }
    }

    // Don't forget the last entry
    if let Some((hash, timestamp, author, message, trailers)) = current.take() {
        entries.push(CommitEntry {
            hash,
            author,
            date: format_timestamp(timestamp),
            message,
            files: current_files,
            timestamp,
            trailers,
        });
    }

    entries
}

/// Parse trailers from a RS-separated string (from git %(trailers:separator=%x1e,unfold))
/// Each trailer is "Key: Value". Returns vec of (key, value) pairs.
fn parse_trailers(raw: &str) -> Vec<(String, String)> {
    raw.split('\x1e')
        .filter_map(|t| {
            let t = t.trim();
            let colon = t.find(':')?;
            let key = t[..colon].trim().to_string();
            let value = t[colon + 1..].trim().to_string();
            if key.is_empty() { None } else { Some((key, value)) }
        })
        .collect()
}

/// Extract bead references a commit declares, for workflow-telemetry lineage
/// (GH#9 auto-association). High-precision by design: only explicit `Bead*`
/// trailers (e.g. `Bead-ID: bo-abc`, `Bead: aegis-h8x`), so incidental
/// `word-word` tokens in the message never pollute the lineage table.
///
/// A trailer value may list several ids separated by commas/whitespace; a
/// leading `b:` (bundle/bead label form) is stripped.
pub fn extract_bead_refs(trailers: &[(String, String)]) -> Vec<String> {
    let mut ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (key, value) in trailers {
        if key.to_lowercase().contains("bead") {
            for tok in value.split([',', ' ', '\t', ';']) {
                let t = tok.trim().trim_start_matches("b:");
                if !t.is_empty() {
                    ids.insert(t.to_string());
                }
            }
        }
    }
    ids.into_iter().collect()
}

/// Format unix timestamp as YYYY-MM-DD
fn format_timestamp(timestamp: i64) -> String {
    // Simple date formatting without external crate
    let secs = timestamp;
    let days = secs / 86400;

    // Calculate year, month, day from days since epoch
    // This is a simplified calculation
    let mut year = 1970;
    let mut remaining_days = days;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for days_in_month in days_in_months.iter() {
        if remaining_days < *days_in_month {
            break;
        }
        remaining_days -= *days_in_month;
        month += 1;
    }

    let day = remaining_days + 1;

    format!("{:04}-{:02}-{:02}", year, month, day)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Parse git log output into commits with their files
fn parse_git_log(log: &str) -> Vec<(i64, Vec<String>)> {
    let mut commits = Vec::new();
    let mut current_files = Vec::new();
    let mut current_time = 0i64;

    for line in log.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("COMMIT:") {
            // If we have accumulated files for the previous commit, push them
            if !current_files.is_empty() {
                commits.push((current_time, std::mem::take(&mut current_files)));
            }

            // Parse new commit header: COMMIT:<hash>:<timestamp>
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                if let Ok(ts) = parts[2].parse::<i64>() {
                    current_time = ts;
                }
            }
        } else {
            // This is a file path
            current_files.push(line.to_string());
        }
    }

    // Don't forget the last commit
    if !current_files.is_empty() {
        commits.push((current_time, current_files));
    }

    commits
}

/// Parse `git diff --name-status` output into (path, DiffStatus) pairs.
///
/// Format per line: `<status>\t<path>` or `R<score>\t<old>\t<new>` for renames.
fn parse_name_status(output: &str) -> Vec<(String, DiffStatus)> {
    let mut results = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let status_char = parts[0].chars().next().unwrap_or('M');
        let status = match status_char {
            'A' => DiffStatus::Added,
            'D' => DiffStatus::Deleted,
            'R' => DiffStatus::Renamed,
            _ => DiffStatus::Modified, // M, C, T, U all treated as modified
        };
        // For renames, use the new path (parts[2])
        let path = if status_char == 'R' && parts.len() >= 3 {
            parts[2].to_string()
        } else {
            parts[1].to_string()
        };
        results.push((path, status));
    }
    results
}

/// Parse unified diff output to extract per-file added/removed line numbers.
///
/// Looks for `--- a/<path>` / `+++ b/<path>` headers and `@@ -old,count +new,count @@` hunks.
/// Returns a map from file path to (added_lines, removed_lines).
fn parse_unified_diff(diff: &str) -> HashMap<String, (Vec<u32>, Vec<u32>)> {
    let mut results: HashMap<String, (Vec<u32>, Vec<u32>)> = HashMap::new();
    let mut current_file: Option<String> = None;

    for line in diff.lines() {
        if line.starts_with("+++ b/") {
            current_file = Some(line[6..].to_string());
        } else if line.starts_with("+++ /dev/null") {
            // Deleted file — lines tracked under the old name from --- header
            current_file = None;
        } else if line.starts_with("--- a/") && current_file.is_none() {
            // For deleted files, we'll use this as the path
            // (current_file stays None, but we set up an entry for removed lines)
        } else if line.starts_with("@@ ") {
            if let Some(ref file) = current_file {
                let entry = results.entry(file.clone()).or_default();
                parse_hunk_header(line, &mut entry.0, &mut entry.1);
            } else {
                // Deleted file: parse removed lines only. Extract path from --- header.
                // We won't have added lines for deleted files.
            }
        } else if line.starts_with("diff --git") {
            // Reset for next file
            current_file = None;
        }
    }

    // Handle deleted files: re-parse looking for --- a/ headers paired with +++ /dev/null
    let mut deleted_file: Option<String> = None;
    for line in diff.lines() {
        if line.starts_with("--- a/") {
            deleted_file = Some(line[6..].to_string());
        } else if line.starts_with("+++ /dev/null") {
            // confirmed deletion — keep deleted_file
        } else if line.starts_with("+++ b/") {
            deleted_file = None; // not a deletion
        } else if line.starts_with("@@ ") {
            if let Some(ref file) = deleted_file {
                let entry = results.entry(file.clone()).or_default();
                parse_hunk_header(line, &mut entry.0, &mut entry.1);
            }
        } else if line.starts_with("diff --git") {
            deleted_file = None;
        }
    }

    results
}

/// Parse a single `@@ -old_start[,old_count] +new_start[,new_count] @@` hunk header.
///
/// Populates `added_lines` with the new-side line range and `removed_lines` with the old-side range.
fn parse_hunk_header(line: &str, added_lines: &mut Vec<u32>, removed_lines: &mut Vec<u32>) {
    // Format: @@ -<old_start>[,<old_count>] +<new_start>[,<new_count>] @@
    let Some(at_end) = line.find(" @@") else {
        return;
    };
    let header = &line[3..at_end]; // skip leading "@@ "

    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }

    // Parse old range: -start[,count]
    if let Some(old_spec) = parts[0].strip_prefix('-') {
        let (start, count) = parse_range_spec(old_spec);
        for i in 0..count {
            removed_lines.push(start + i);
        }
    }

    // Parse new range: +start[,count]
    if let Some(new_spec) = parts[1].strip_prefix('+') {
        let (start, count) = parse_range_spec(new_spec);
        for i in 0..count {
            added_lines.push(start + i);
        }
    }
}

/// Collapse a set of line numbers into contiguous `(start, end)` inclusive
/// ranges. Input is sorted and de-duplicated first, so callers may pass the
/// removed-line lists from multiple hunks in any order. Empty input → no ranges.
fn group_consecutive(lines: &[u32]) -> Vec<(u32, u32)> {
    let mut sorted: Vec<u32> = lines.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut ranges = Vec::new();
    let mut iter = sorted.into_iter();
    let Some(first) = iter.next() else {
        return ranges;
    };
    let (mut start, mut end) = (first, first);
    for n in iter {
        if n == end + 1 {
            end = n;
        } else {
            ranges.push((start, end));
            start = n;
            end = n;
        }
    }
    ranges.push((start, end));
    ranges
}

/// Parse a range spec like "42" (1 line at 42) or "42,5" (5 lines starting at 42).
/// A count of 0 means no lines (pure addition or deletion on the other side).
fn parse_range_spec(spec: &str) -> (u32, u32) {
    if let Some((start_str, count_str)) = spec.split_once(',') {
        let start = start_str.parse::<u32>().unwrap_or(0);
        let count = count_str.parse::<u32>().unwrap_or(1);
        (start, count)
    } else {
        let start = spec.parse::<u32>().unwrap_or(0);
        (start, 1)
    }
}

/// Calculate coupling score based on frequency and recency.
///
/// `freq_weight` is the weight on the normalized co-change count (0.0–1.0); the
/// recency component is weighted `1.0 - freq_weight`. `recency_days` is the knee
/// of the recency decay: a pair last changed `recency_days` ago scores 0.5 on
/// recency. See `[git].coupling_freq_weight` / `coupling_recency_days`.
pub(crate) fn calculate_coupling_score(
    co_changes: u32,
    max_co_changes: u32,
    last_co_change: i64,
    now: i64,
    freq_weight: f32,
    recency_days: f32,
) -> f32 {
    if max_co_changes == 0 {
        return 0.0;
    }

    // Frequency score: normalized count (0.0 - 1.0)
    let freq_score = co_changes as f32 / max_co_changes as f32;

    // Recency score: slow decay based on days since last co-change.
    // At `recency_days` old the score is 0.5; at 0 days it is 1.0.
    // Guard against a non-positive knee collapsing the decay.
    let knee = if recency_days > 0.0 { recency_days } else { 30.0 };
    let days_diff = ((now - last_co_change) as f32 / 86400.0).max(0.0);
    let recency_score = 1.0 / (1.0 + days_diff / knee);

    // Weighted combination: emphasizes pairs that change often, with a boost if
    // they changed recently. Recency weight is the complement of freq_weight.
    freq_weight * freq_score + (1.0 - freq_weight) * recency_score
}

/// Parse git blame --porcelain output into BlameEntry records.
/// Porcelain format: each blamed line starts with "<hash> <orig_line> <final_line> [<group_lines>]"
/// followed by header lines, then a tab-prefixed content line.
fn parse_blame_porcelain(output: &str) -> Vec<BlameEntry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        // Porcelain blame lines start with a 40-char hex hash
        if line.len() >= 40 && line.as_bytes()[0].is_ascii_hexdigit() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Format: <hash> <orig_line> <final_line> [<group_count>]
            if parts.len() >= 3 {
                if let Ok(final_line) = parts[2].parse::<u32>() {
                    entries.push(BlameEntry {
                        commit_hash: parts[0].to_string(),
                        line_number: final_line,
                    });
                }
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_consecutive() {
        // Unsorted + duplicate input collapses into contiguous inclusive ranges.
        assert_eq!(
            group_consecutive(&[5, 1, 2, 3, 7, 6, 3]),
            vec![(1, 3), (5, 7)]
        );
        // Single isolated lines each form their own range.
        assert_eq!(group_consecutive(&[10, 20, 30]), vec![(10, 10), (20, 20), (30, 30)]);
        // One run.
        assert_eq!(group_consecutive(&[4, 5, 6]), vec![(4, 6)]);
        // Empty input → no ranges.
        assert_eq!(group_consecutive(&[]), Vec::<(u32, u32)>::new());
    }

    #[test]
    fn test_parse_git_log() {
        let log = "COMMIT:hash1:1000\nfile1.rs\nfile2.rs\n\nCOMMIT:hash2:2000\nfile2.rs\nfile3.rs";
        let commits = parse_git_log(log);

        assert_eq!(commits.len(), 2);

        assert_eq!(commits[0].0, 1000);
        assert_eq!(commits[0].1, vec!["file1.rs", "file2.rs"]);

        assert_eq!(commits[1].0, 2000);
        assert_eq!(commits[1].1, vec!["file2.rs", "file3.rs"]);
    }

    #[test]
    fn test_calculate_coupling_score() {
        let now = 10000;
        let max_co_changes = 10;

        // Default weights: 0.7 frequency, 0.3 recency, 30-day knee.
        let fw = 0.7;
        let rd = 30.0;

        // Case 1: High frequency, recent
        let score1 = calculate_coupling_score(10, max_co_changes, now, now, fw, rd);
        // freq = 1.0, recency = 1.0 -> 1.0
        assert!((score1 - 1.0).abs() < 0.001);

        // Case 2: Low frequency, recent
        let score2 = calculate_coupling_score(1, max_co_changes, now, now, fw, rd);
        // freq = 0.1, recency = 1.0 -> 0.07 + 0.3 = 0.37
        assert!((score2 - 0.37).abs() < 0.001);

        // Case 3: High frequency, old
        // 30 days old = 2592000 seconds
        let old = now - 30 * 86400;
        let score3 = calculate_coupling_score(10, max_co_changes, old, now, fw, rd);
        // freq = 1.0, recency = 1/(1+1) = 0.5 -> 0.7 + 0.15 = 0.85
        assert!((score3 - 0.85).abs() < 0.001);

        // Case 4: Custom weights — recency-dominant (freq_weight = 0.2).
        // freq = 1.0, recency = 0.5 -> 0.2 + 0.4 = 0.6
        let score4 = calculate_coupling_score(10, max_co_changes, old, now, 0.2, rd);
        assert!((score4 - 0.6).abs() < 0.001);

        // Case 5: Custom knee — 60-day knee makes 30-day-old pairs decay less.
        // freq = 1.0, recency = 1/(1+0.5) = 0.667 -> 0.7 + 0.3*0.667 = 0.9
        let score5 = calculate_coupling_score(10, max_co_changes, old, now, fw, 60.0);
        assert!((score5 - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_parse_file_history() {
        let log = "abc123|1704067200|Alice|Initial commit\ndef456|1704153600|Bob|Fix bug (bobbin-123)";
        let entries = parse_file_history(log);

        assert_eq!(entries.len(), 2);

        // First entry
        assert_eq!(entries[0].author, "Alice");
        assert_eq!(entries[0].message, "Initial commit");
        assert_eq!(entries[0].timestamp, 1704067200);
        assert!(entries[0].issues.is_empty());

        // Second entry with issue reference
        assert_eq!(entries[1].author, "Bob");
        assert_eq!(entries[1].message, "Fix bug (bobbin-123)");
        assert_eq!(entries[1].timestamp, 1704153600);
        assert_eq!(entries[1].issues, vec!["bobbin-123"]);
    }

    #[test]
    fn test_parse_file_history_multiple_issues() {
        let log = "abc123|1704067200|Dev|Fixes #42 and JIRA-99, also bobbin-xyz";
        let entries = parse_file_history(log);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].issues, vec!["#42", "JIRA-99", "bobbin-xyz"]);
    }

    #[test]
    fn test_format_timestamp() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(format_timestamp(1704067200), "2024-01-01");

        // 1970-01-01 00:00:00 UTC = 0
        assert_eq!(format_timestamp(0), "1970-01-01");
    }

    #[test]
    fn test_parse_commit_log() {
        let sep = "\x1f";
        let log = format!(
            "ENTRY{s}abc123{s}1704067200{s}Alice{s}Initial commit\nfile1.rs\nfile2.rs\n\nENTRY{s}def456{s}1704153600{s}Bob{s}Add feature\nfile3.rs",
            s = sep
        );
        let entries = parse_commit_log(&log, sep);

        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].hash, "abc123");
        assert_eq!(entries[0].author, "Alice");
        assert_eq!(entries[0].message, "Initial commit");
        assert_eq!(entries[0].timestamp, 1704067200);
        assert_eq!(entries[0].files, vec!["file1.rs", "file2.rs"]);

        assert_eq!(entries[1].hash, "def456");
        assert_eq!(entries[1].author, "Bob");
        assert_eq!(entries[1].message, "Add feature");
        assert_eq!(entries[1].files, vec!["file3.rs"]);
    }

    #[test]
    fn test_parse_commit_log_empty() {
        let entries = parse_commit_log("", "\x1f");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_commit_log_no_files() {
        let sep = "\x1f";
        let log = format!("ENTRY{s}abc123{s}1704067200{s}Alice{s}Empty commit", s = sep);
        let entries = parse_commit_log(&log, sep);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hash, "abc123");
        assert!(entries[0].files.is_empty());
    }

    #[test]
    fn test_parse_commit_log_with_trailers() {
        let sep = "\x1f";
        let trailers = "Co-Authored-By: Alice <alice@test.com>\x1eBead-ID: aegis-xyz";
        let log = format!(
            "ENTRY{s}abc123{s}1704067200{s}Bob{s}Fix bug{s}{t}\nfile1.rs",
            s = sep,
            t = trailers
        );
        let entries = parse_commit_log(&log, sep);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].trailers.len(), 2);
        assert_eq!(entries[0].trailers[0].0, "Co-Authored-By");
        assert_eq!(entries[0].trailers[0].1, "Alice <alice@test.com>");
        assert_eq!(entries[0].trailers[1].0, "Bead-ID");
        assert_eq!(entries[0].trailers[1].1, "aegis-xyz");
    }

    #[test]
    fn test_parse_trailers() {
        let raw = "Co-Authored-By: Claude <noreply@anthropic.com>\x1eBead-ID: bo-123";
        let trailers = parse_trailers(raw);
        assert_eq!(trailers.len(), 2);
        assert_eq!(trailers[0], ("Co-Authored-By".to_string(), "Claude <noreply@anthropic.com>".to_string()));
        assert_eq!(trailers[1], ("Bead-ID".to_string(), "bo-123".to_string()));

        // Empty input
        assert!(parse_trailers("").is_empty());
    }

    /// Helper: create a temp git repo and return the path
    fn setup_test_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        Command::new("git").args(["init"]).current_dir(path).output().unwrap();
        Command::new("git").args(["config", "user.email", "test@test.com"]).current_dir(path).output().unwrap();
        Command::new("git").args(["config", "user.name", "Test"]).current_dir(path).output().unwrap();
        dir
    }

    #[test]
    fn test_get_file_churn_counts() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Create file and commit it 3 times
        std::fs::write(path.join("a.rs"), "v1").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "c1"]).current_dir(path).output().unwrap();

        std::fs::write(path.join("a.rs"), "v2").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "c2"]).current_dir(path).output().unwrap();

        std::fs::write(path.join("b.rs"), "v1").unwrap();
        std::fs::write(path.join("a.rs"), "v3").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "c3"]).current_dir(path).output().unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let churn = analyzer.get_file_churn(None).unwrap();

        assert_eq!(churn.get("a.rs"), Some(&3));
        assert_eq!(churn.get("b.rs"), Some(&1));
    }

    #[test]
    fn test_get_file_churn_since_filter() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Create a commit
        std::fs::write(path.join("old.rs"), "v1").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "old commit"]).current_dir(path).output().unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();

        // "1 second ago" should still include the commit we just made
        let churn = analyzer.get_file_churn(Some("1 year ago")).unwrap();
        assert!(churn.contains_key("old.rs"));

        // "1 second" in the future effectively means nothing is older
        // A very restrictive since should return empty or same
        let churn_future = analyzer.get_file_churn(Some("2099-01-01")).unwrap();
        assert!(churn_future.is_empty());
    }

    #[test]
    fn test_blame_fix_culprits_finds_introducing_commit() {
        // SZZ-lite: a "culprit" commit introduces a buggy line; a later "fix"
        // commit changes that line. blame_fix_culprits should attribute the
        // removed line to the culprit commit, not the fix.
        let dir = setup_test_repo();
        let path = dir.path();
        let run = |args: &[&str]| {
            Command::new("git").args(args).current_dir(path).output().unwrap()
        };

        // c0: baseline file.
        std::fs::write(path.join("auth.rs"), "fn ok() {}\nlet x = 1;\nfn end() {}\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "baseline"]);

        // culprit: introduce the buggy middle line.
        std::fs::write(path.join("auth.rs"), "fn ok() {}\nlet x = BUGGY;\nfn end() {}\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "introduce bug"]);
        let culprit_sha = String::from_utf8_lossy(&run(&["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();

        // fix: change the buggy line (removes the culprit's version).
        std::fs::write(path.join("auth.rs"), "fn ok() {}\nlet x = 2;\nfn end() {}\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "fix bug"]);
        let fix_sha = String::from_utf8_lossy(&run(&["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let entries = analyzer.blame_fix_culprits(&fix_sha, "auth.rs").unwrap();

        // The removed line was last touched by the culprit commit.
        assert!(!entries.is_empty(), "expected blame to attribute removed lines");
        assert!(
            entries.iter().all(|e| e.commit_hash == culprit_sha),
            "all blamed lines should point at the culprit, got {:?}",
            entries
        );
    }

    #[test]
    fn test_blame_fix_culprits_pure_addition_is_empty() {
        // A fix that only *adds* lines has no removed lines to blame → empty.
        let dir = setup_test_repo();
        let path = dir.path();
        let run = |args: &[&str]| {
            Command::new("git").args(args).current_dir(path).output().unwrap()
        };

        std::fs::write(path.join("a.rs"), "one\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "c0"]);

        std::fs::write(path.join("a.rs"), "one\ntwo\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "append only"]);
        let fix_sha = String::from_utf8_lossy(&run(&["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let entries = analyzer.blame_fix_culprits(&fix_sha, "a.rs").unwrap();
        assert!(entries.is_empty(), "pure addition should yield no culprits");
    }

    #[test]
    fn test_parse_name_status() {
        let output = "M\tsrc/main.rs\nA\tsrc/new.rs\nD\told.rs\nR100\tsrc/old.rs\tsrc/renamed.rs\n";
        let results = parse_name_status(output);

        assert_eq!(results.len(), 4);
        assert_eq!(results[0], ("src/main.rs".to_string(), DiffStatus::Modified));
        assert_eq!(results[1], ("src/new.rs".to_string(), DiffStatus::Added));
        assert_eq!(results[2], ("old.rs".to_string(), DiffStatus::Deleted));
        assert_eq!(results[3], ("src/renamed.rs".to_string(), DiffStatus::Renamed));
    }

    #[test]
    fn test_parse_name_status_empty() {
        let results = parse_name_status("");
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_hunk_header_simple() {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        parse_hunk_header("@@ -10,3 +20,5 @@ fn foo()", &mut added, &mut removed);

        assert_eq!(removed, vec![10, 11, 12]);
        assert_eq!(added, vec![20, 21, 22, 23, 24]);
    }

    #[test]
    fn test_parse_hunk_header_single_line() {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        parse_hunk_header("@@ -5 +5 @@", &mut added, &mut removed);

        assert_eq!(removed, vec![5]);
        assert_eq!(added, vec![5]);
    }

    #[test]
    fn test_parse_hunk_header_pure_addition() {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        // count 0 on old side means pure addition
        parse_hunk_header("@@ -10,0 +11,2 @@", &mut added, &mut removed);

        assert!(removed.is_empty());
        assert_eq!(added, vec![11, 12]);
    }

    #[test]
    fn test_parse_hunk_header_pure_deletion() {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        // count 0 on new side means pure deletion
        parse_hunk_header("@@ -10,2 +9,0 @@", &mut added, &mut removed);

        assert_eq!(removed, vec![10, 11]);
        assert!(added.is_empty());
    }

    #[test]
    fn test_parse_range_spec() {
        assert_eq!(parse_range_spec("42"), (42, 1));
        assert_eq!(parse_range_spec("42,5"), (42, 5));
        assert_eq!(parse_range_spec("0,0"), (0, 0));
        assert_eq!(parse_range_spec("1,0"), (1, 0));
    }

    #[test]
    fn test_parse_unified_diff_modification() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,2 +10,3 @@ fn main() {
@@ -25,1 +26,1 @@ fn helper()
";
        let changes = parse_unified_diff(diff);
        let (added, removed) = changes.get("src/main.rs").unwrap();

        // First hunk: removed 2 lines at 10, added 3 lines at 10
        // Second hunk: removed 1 line at 25, added 1 line at 26
        assert_eq!(*removed, vec![10, 11, 25]);
        assert_eq!(*added, vec![10, 11, 12, 26]);
    }

    #[test]
    fn test_parse_unified_diff_new_file() {
        let diff = "\
diff --git a/new.rs b/new.rs
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,5 @@
";
        let changes = parse_unified_diff(diff);
        let (added, removed) = changes.get("new.rs").unwrap();

        assert_eq!(*added, vec![1, 2, 3, 4, 5]);
        assert!(removed.is_empty());
    }

    #[test]
    fn test_parse_unified_diff_multiple_files() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,2 @@
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -5,3 +5,1 @@
";
        let changes = parse_unified_diff(diff);

        let (a_added, a_removed) = changes.get("a.rs").unwrap();
        assert_eq!(*a_removed, vec![1]);
        assert_eq!(*a_added, vec![1, 2]);

        let (b_added, b_removed) = changes.get("b.rs").unwrap();
        assert_eq!(*b_removed, vec![5, 6, 7]);
        assert_eq!(*b_added, vec![5]);
    }

    #[test]
    fn test_diff_status_display() {
        assert_eq!(format!("{}", DiffStatus::Added), "added");
        assert_eq!(format!("{}", DiffStatus::Modified), "modified");
        assert_eq!(format!("{}", DiffStatus::Deleted), "deleted");
        assert_eq!(format!("{}", DiffStatus::Renamed), "renamed");
    }

    #[test]
    fn test_get_diff_files_unstaged() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Create initial file and commit
        std::fs::write(path.join("a.rs"), "line1\nline2\nline3\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(path).output().unwrap();

        // Modify the file (unstaged)
        std::fs::write(path.join("a.rs"), "line1\nchanged\nline3\n").unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let files = analyzer.get_diff_files(&DiffSpec::Unstaged).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.rs");
        assert_eq!(files[0].status, DiffStatus::Modified);
        assert!(!files[0].added_lines.is_empty() || !files[0].removed_lines.is_empty());
    }

    #[test]
    fn test_get_diff_files_staged() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Create initial file and commit
        std::fs::write(path.join("a.rs"), "line1\nline2\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(path).output().unwrap();

        // Modify and stage
        std::fs::write(path.join("a.rs"), "line1\nline2\nline3\n").unwrap();
        Command::new("git").args(["add", "a.rs"]).current_dir(path).output().unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let files = analyzer.get_diff_files(&DiffSpec::Staged).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.rs");
        assert_eq!(files[0].status, DiffStatus::Modified);
    }

    #[test]
    fn test_get_diff_files_new_file() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Initial commit
        std::fs::write(path.join("existing.rs"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(path).output().unwrap();

        // Add a new file and stage
        std::fs::write(path.join("new.rs"), "new content\n").unwrap();
        Command::new("git").args(["add", "new.rs"]).current_dir(path).output().unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let files = analyzer.get_diff_files(&DiffSpec::Staged).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "new.rs");
        assert_eq!(files[0].status, DiffStatus::Added);
        assert!(!files[0].added_lines.is_empty());
    }

    #[test]
    fn test_get_diff_files_deleted_file() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Create and commit a file
        std::fs::write(path.join("to_delete.rs"), "line1\nline2\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(path).output().unwrap();

        // Delete and stage
        std::fs::remove_file(path.join("to_delete.rs")).unwrap();
        Command::new("git").args(["add", "to_delete.rs"]).current_dir(path).output().unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let files = analyzer.get_diff_files(&DiffSpec::Staged).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "to_delete.rs");
        assert_eq!(files[0].status, DiffStatus::Deleted);
    }

    #[test]
    fn test_get_diff_files_range() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Initial commit
        std::fs::write(path.join("a.rs"), "v1").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "c1"]).current_dir(path).output().unwrap();

        // Second commit
        std::fs::write(path.join("a.rs"), "v2").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "c2"]).current_dir(path).output().unwrap();

        // Third commit with new file
        std::fs::write(path.join("b.rs"), "new").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "c3"]).current_dir(path).output().unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let files = analyzer.get_diff_files(&DiffSpec::Range("HEAD~2..HEAD".to_string())).unwrap();

        // Should see changes from the last 2 commits
        assert!(files.len() >= 1);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"a.rs") || paths.contains(&"b.rs"));
    }

    #[test]
    fn test_get_diff_files_empty_diff() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Commit a file
        std::fs::write(path.join("a.rs"), "content").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(path).output().unwrap();

        // No changes — unstaged diff should be empty
        let analyzer = GitAnalyzer::new(path).unwrap();
        let files = analyzer.get_diff_files(&DiffSpec::Unstaged).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_get_diff_files_multiple_changes() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Initial commit with two files
        std::fs::write(path.join("a.rs"), "a-v1\n").unwrap();
        std::fs::write(path.join("b.rs"), "b-v1\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(path).output().unwrap();

        // Modify both files (unstaged)
        std::fs::write(path.join("a.rs"), "a-v2\n").unwrap();
        std::fs::write(path.join("b.rs"), "b-v2\n").unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let files = analyzer.get_diff_files(&DiffSpec::Unstaged).unwrap();

        assert_eq!(files.len(), 2);
        // Results are sorted by path
        assert_eq!(files[0].path, "a.rs");
        assert_eq!(files[1].path, "b.rs");
        assert_eq!(files[0].status, DiffStatus::Modified);
        assert_eq!(files[1].status, DiffStatus::Modified);
    }

    #[test]
    fn test_get_file_churn_empty_repo() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Make an initial empty commit so HEAD exists
        Command::new("git").args(["commit", "--allow-empty", "-m", "init"]).current_dir(path).output().unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let churn = analyzer.get_file_churn(None).unwrap();
        assert!(churn.is_empty());
    }

    #[test]
    fn test_parse_blame_porcelain() {
        let porcelain = "\
abc1234567890123456789012345678901234567 1 1 3
author John Doe
author-mail <john@example.com>
author-time 1700000000
author-tz +0000
committer John Doe
committer-mail <john@example.com>
committer-time 1700000000
committer-tz +0000
summary Initial commit
filename src/main.rs
\tline 1 content
abc1234567890123456789012345678901234567 2 2
\tline 2 content
def9876543210987654321098765432109876543 3 3 1
author Jane Doe
author-mail <jane@example.com>
author-time 1700001000
author-tz +0000
committer Jane Doe
committer-mail <jane@example.com>
committer-time 1700001000
committer-tz +0000
summary Fix bug
filename src/main.rs
\tline 3 content
";
        let entries = parse_blame_porcelain(porcelain);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].commit_hash, "abc1234567890123456789012345678901234567");
        assert_eq!(entries[0].line_number, 1);
        assert_eq!(entries[1].commit_hash, "abc1234567890123456789012345678901234567");
        assert_eq!(entries[1].line_number, 2);
        assert_eq!(entries[2].commit_hash, "def9876543210987654321098765432109876543");
        assert_eq!(entries[2].line_number, 3);
    }

    #[test]
    fn test_parse_blame_porcelain_empty() {
        let entries = parse_blame_porcelain("");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_blame_lines_integration() {
        let dir = setup_test_repo();
        let path = dir.path();

        // Create a file with multiple lines
        std::fs::write(path.join("src.rs"), "line1\nline2\nline3\nline4\nline5\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "initial"]).current_dir(path).output().unwrap();

        // Modify lines 2-3 in a second commit
        std::fs::write(path.join("src.rs"), "line1\nmodified2\nmodified3\nline4\nline5\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).output().unwrap();
        Command::new("git").args(["commit", "-m", "modify lines"]).current_dir(path).output().unwrap();

        let analyzer = GitAnalyzer::new(path).unwrap();
        let entries = analyzer.blame_lines("src.rs", 1, 5).unwrap();

        assert_eq!(entries.len(), 5);
        // Lines 1, 4, 5 should be from first commit; lines 2, 3 from second
        let first_commit = &entries[0].commit_hash;
        let second_commit = &entries[1].commit_hash;
        assert_ne!(first_commit, second_commit);
        assert_eq!(&entries[3].commit_hash, first_commit); // line 4 unchanged
        assert_eq!(&entries[4].commit_hash, first_commit); // line 5 unchanged
        assert_eq!(&entries[2].commit_hash, second_commit); // line 3 modified
    }

    #[test]
    fn test_extract_bead_refs_from_trailers() {
        let trailers = vec![
            ("Bead-ID".to_string(), "bo-abc123".to_string()),
            ("Co-Authored-By".to_string(), "Someone <x@y.z>".to_string()),
            ("Bead".to_string(), "aegis-h8x, b:bo-def".to_string()),
        ];
        let ids = extract_bead_refs(&trailers);
        assert!(ids.contains(&"bo-abc123".to_string()));
        assert!(ids.contains(&"aegis-h8x".to_string()));
        assert!(ids.contains(&"bo-def".to_string())); // `b:` stripped
        // Non-bead trailers ignored.
        assert!(!ids.iter().any(|i| i.contains("Someone")));
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn test_extract_bead_refs_none_when_no_bead_trailers() {
        let trailers = vec![("Signed-off-by".to_string(), "x".to_string())];
        assert!(extract_bead_refs(&trailers).is_empty());
    }
}
