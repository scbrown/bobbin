use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::types::FileCoupling;

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

    /// Analyze git history to find files that change together
    pub fn analyze_coupling(&self, depth: usize, threshold: u32) -> Result<Vec<FileCoupling>> {
        // Get commit log with files changed
        let output = Command::new("git")
            .args([
                "log",
                "--pretty=format:%H",
                "--name-only",
                &format!("-{}", depth),
            ])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get git log")?;

        let log = String::from_utf8_lossy(&output.stdout);
        let commits = parse_git_log(&log);

        // Build co-change matrix
        let mut co_changes: HashMap<(String, String), u32> = HashMap::new();
        let mut last_seen: HashMap<(String, String), i64> = HashMap::new();

        for (commit_time, files) in commits {
            // Count co-changes for all pairs of files in this commit
            for i in 0..files.len() {
                for j in (i + 1)..files.len() {
                    let key = if files[i] < files[j] {
                        (files[i].clone(), files[j].clone())
                    } else {
                        (files[j].clone(), files[i].clone())
                    };

                    *co_changes.entry(key.clone()).or_insert(0) += 1;
                    last_seen.insert(key, commit_time);
                }
            }
        }

        // Convert to FileCoupling, filtering by threshold
        let mut couplings: Vec<FileCoupling> = co_changes
            .into_iter()
            .filter(|(_, count)| *count >= threshold)
            .map(|((file_a, file_b), count)| {
                let last_co_change = last_seen
                    .get(&(file_a.clone(), file_b.clone()))
                    .copied()
                    .unwrap_or(0);

                FileCoupling {
                    file_a,
                    file_b,
                    score: calculate_coupling_score(count, last_co_change),
                    co_changes: count,
                    last_co_change,
                }
            })
            .collect();

        // Sort by score descending
        couplings.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        Ok(couplings)
    }

    /// Get files changed in a specific commit
    pub fn get_commit_files(&self, commit_hash: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["diff-tree", "--no-commit-id", "--name-only", "-r", commit_hash])
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
}

/// Parse git log output into commits with their files
fn parse_git_log(log: &str) -> Vec<(i64, Vec<String>)> {
    let mut commits = Vec::new();
    let mut current_files = Vec::new();
    let mut current_time = 0i64;

    for line in log.lines() {
        if line.is_empty() {
            if !current_files.is_empty() {
                commits.push((current_time, std::mem::take(&mut current_files)));
            }
        } else if line.len() == 40 && line.chars().all(|c| c.is_ascii_hexdigit()) {
            // This is a commit hash
            if !current_files.is_empty() {
                commits.push((current_time, std::mem::take(&mut current_files)));
            }
            // Use commit count as proxy for time ordering
            current_time = commits.len() as i64;
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

/// Calculate coupling score based on co-change frequency and recency
fn calculate_coupling_score(co_changes: u32, _last_co_change: i64) -> f32 {
    // Simple scoring: log of co-changes
    // TODO: Factor in recency
    (co_changes as f32).ln_1p()
}
