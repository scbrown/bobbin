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
        // Format: COMMIT:<hash>:<timestamp>
        // followed by list of files
        let output = Command::new("git")
            .args([
                "log",
                "--pretty=format:COMMIT:%H:%ct",
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
        
        // Track max co-changes for normalization
        let mut max_co_changes = 0;

        for (commit_time, files) in commits {
            // Count co-changes for all pairs of files in this commit
            for i in 0..files.len() {
                for j in (i + 1)..files.len() {
                    let key = if files[i] < files[j] {
                        (files[i].clone(), files[j].clone())
                    } else {
                        (files[j].clone(), files[i].clone())
                    };

                    let count = co_changes.entry(key.clone()).or_insert(0);
                    *count += 1;
                    if *count > max_co_changes {
                        max_co_changes = *count;
                    }
                    
                    // Keep the most recent time
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
            .map(|((file_a, file_b), count)| {
                let last_co_change = last_seen
                    .get(&(file_a.clone(), file_b.clone()))
                    .copied()
                    .unwrap_or(0);

                FileCoupling {
                    file_a,
                    file_b,
                    score: calculate_coupling_score(count, max_co_changes, last_co_change, now),
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

/// Calculate coupling score based on frequency and recency
fn calculate_coupling_score(co_changes: u32, max_co_changes: u32, last_co_change: i64, now: i64) -> f32 {
    if max_co_changes == 0 {
        return 0.0;
    }

    // Frequency score: normalized count (0.0 - 1.0)
    let freq_score = co_changes as f32 / max_co_changes as f32;

    // Recency score: decay based on days since last co-change
    // Decay factor: 0.99 per day? Or simpler: 1 / (1 + days)
    let days_diff = ((now - last_co_change) as f32 / 86400.0).max(0.0);
    // Use a slow decay: at 30 days, score is ~0.5. at 0 days, score is 1.0
    // 30 days * k = 1 => k = 1/30? 
    // Let's use 1 / (1 + days/30)
    let recency_score = 1.0 / (1.0 + days_diff / 30.0);

    // Weighted combination: 70% frequency, 30% recency
    // This emphasizes pairs that change often, with a boost if they changed recently
    0.7 * freq_score + 0.3 * recency_score
}

#[cfg(test)]
mod tests {
    use super::*;

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
        
        // Case 1: High frequency, recent
        let score1 = calculate_coupling_score(10, max_co_changes, now, now);
        // freq = 1.0, recency = 1.0 -> 1.0
        assert!((score1 - 1.0).abs() < 0.001);

        // Case 2: Low frequency, recent
        let score2 = calculate_coupling_score(1, max_co_changes, now, now);
        // freq = 0.1, recency = 1.0 -> 0.07 + 0.3 = 0.37
        assert!((score2 - 0.37).abs() < 0.001);

        // Case 3: High frequency, old
        // 30 days old = 2592000 seconds
        let old = now - 30 * 86400;
        let score3 = calculate_coupling_score(10, max_co_changes, old, now);
        // freq = 1.0, recency = 1/(1+1) = 0.5 -> 0.7 + 0.15 = 0.85
        assert!((score3 - 0.85).abs() < 0.001);
    }
}
