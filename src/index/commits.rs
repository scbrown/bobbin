//! Git commits as a [`WatermarkSource`] (W4.P1 follow-up, bobbin-d5e).
//!
//! Commit history is append-only, so the per-repo watermark
//! (`last_indexed_commit:{repo}`) replaces per-item hash bookkeeping and no
//! removal sweep applies; the `git:` file-path prefix is what keeps the file
//! sweep in `cli/index.rs` from judging commit rows "deleted files". The
//! chunk format here is byte-identical to the bespoke block this replaced.

use std::sync::Mutex;

use anyhow::Result;

use crate::index::git::{CommitEntry, GitAnalyzer};
use crate::index::source::WatermarkSource;
use crate::types::{Chunk, ChunkType};

/// Watermark-tracked source over a repo's commit log.
pub struct CommitsSource {
    analyzer: GitAnalyzer,
    repo: String,
    depth: usize,
    /// Entries from the last fetch, kept so the caller can record
    /// bead→commit lineage after a successful insert — git-specific
    /// bookkeeping that stays outside the seam.
    entries: Mutex<Vec<CommitEntry>>,
}

impl CommitsSource {
    pub fn new(analyzer: GitAnalyzer, repo: &str, depth: usize) -> Self {
        Self {
            analyzer,
            repo: repo.to_string(),
            depth,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Take the entries fetched by the last [`WatermarkSource::fetch_since`]
    /// call (the newly indexed increment).
    pub fn take_entries(&self) -> Vec<CommitEntry> {
        std::mem::take(&mut *self.entries.lock().expect("commits entries lock poisoned"))
    }
}

impl WatermarkSource for CommitsSource {
    fn name(&self) -> &str {
        "git-commits"
    }

    fn repo(&self) -> &str {
        &self.repo
    }

    fn source_label(&self) -> &str {
        "git-commits"
    }

    fn watermark_key(&self) -> String {
        // Per-repo: the old global key held ONE SHA shared by all repos, so
        // each was asked for "commits since" a commit from whichever repo
        // happened to index last.
        format!("last_indexed_commit:{}", self.repo)
    }

    fn fetch_since(&self, since: Option<&str>) -> Result<(Vec<Chunk>, Option<String>)> {
        let commit_entries = self.analyzer.get_commit_log(self.depth, since)?;
        let chunks: Vec<Chunk> = commit_entries.iter().map(commit_entry_to_chunk).collect();
        // Entries come newest-first; the first is the next watermark.
        let watermark = commit_entries.first().map(|e| e.hash.clone());
        *self.entries.lock().expect("commits entries lock poisoned") = commit_entries;
        Ok((chunks, watermark))
    }
}

/// Build the searchable chunk for one commit: message + author/date
/// metadata + trailers + changed files, with trailer keys as tags.
pub fn commit_entry_to_chunk(entry: &CommitEntry) -> Chunk {
    let files_str = if entry.files.is_empty() {
        String::new()
    } else {
        format!("\n\nFiles changed:\n{}", entry.files.join("\n"))
    };
    let trailers_str = if entry.trailers.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = entry
            .trailers
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        format!("\n\nTrailers:\n{}", lines.join("\n"))
    };
    let content = format!(
        "{}\n\nAuthor: {}\nDate: {}{}{}",
        entry.message, entry.author, entry.date, trailers_str, files_str
    );

    // Store trailer keys as tags for structured filtering
    let tags = entry
        .trailers
        .iter()
        .map(|(k, v)| format!("{}={}", k.to_lowercase().replace(' ', "-"), v))
        .collect::<Vec<_>>()
        .join(",");

    Chunk {
        id: format!("commit:{}", entry.hash),
        file_path: format!("git:{}", &entry.hash[..7.min(entry.hash.len())]),
        chunk_type: ChunkType::Commit,
        name: Some(truncate_message(&entry.message, 80)),
        start_line: 0,
        end_line: 0,
        content,
        language: "git".to_string(),
        tags,
    }
}

/// Truncate a commit message to max_len, appending "..." if truncated
fn truncate_message(msg: &str, max_len: usize) -> String {
    // Take only the first line (subject line)
    let first_line = msg.lines().next().unwrap_or(msg);
    if first_line.len() <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_len - 3).collect();
        format!("{}...", truncated.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> CommitEntry {
        CommitEntry {
            hash: "abcdef0123456789".to_string(),
            author: "Test Author".to_string(),
            date: "2026-08-22".to_string(),
            message: "feat: add thing\n\nBody line".to_string(),
            files: vec!["src/lib.rs".to_string()],
            timestamp: 0,
            trailers: vec![("Bead-ID".to_string(), "bo-123".to_string())],
        }
    }

    #[test]
    fn test_truncate_message_short() {
        assert_eq!(truncate_message("short msg", 80), "short msg");
    }

    #[test]
    fn test_truncate_message_long() {
        let long_msg = "a".repeat(100);
        let result = truncate_message(&long_msg, 20);
        assert_eq!(result.len(), 20); // 17 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_message_multiline() {
        let msg = "First line subject\n\nLong body with details";
        assert_eq!(truncate_message(msg, 80), "First line subject");
    }

    #[test]
    fn test_commit_entry_to_chunk_shape() {
        let chunk = commit_entry_to_chunk(&entry());
        assert_eq!(chunk.id, "commit:abcdef0123456789");
        assert_eq!(chunk.file_path, "git:abcdef0");
        assert_eq!(chunk.chunk_type, ChunkType::Commit);
        assert_eq!(chunk.name.as_deref(), Some("feat: add thing"));
        assert_eq!(chunk.language, "git");
        assert!(chunk.content.contains("Author: Test Author"));
        assert!(chunk.content.contains("Trailers:\nBead-ID: bo-123"));
        assert!(chunk.content.contains("Files changed:\nsrc/lib.rs"));
        assert_eq!(chunk.tags, "bead-id=bo-123");
    }
}
