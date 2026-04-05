use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Session Ledger: tracks chunks injected across turns for progressive reducing
// ---------------------------------------------------------------------------

/// A record of a chunk that was injected in a previous turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LedgerEntry {
    chunk_key: String,
    injection_id: String,
    pub(super) turn: u64,
}

/// Session-level ledger tracking all chunks injected so far.
/// Stored as JSONL at `.bobbin/session/<cc_session_id>/ledger.jsonl`.
pub(super) struct SessionLedger {
    pub(super) entries: HashSet<String>, // chunk_keys for fast lookup
    pub(super) turn: u64,
    pub(super) path: Option<PathBuf>,
}

impl SessionLedger {
    /// Load ledger for a Claude Code session. Returns empty ledger if session_id
    /// is empty or file doesn't exist.
    pub(super) fn load(repo_root: &Path, cc_session_id: &str) -> Self {
        if cc_session_id.is_empty() {
            return Self { entries: HashSet::new(), turn: 0, path: None };
        }
        let dir = repo_root.join(".bobbin").join("session").join(cc_session_id);
        let path = dir.join("ledger.jsonl");

        let mut entries = HashSet::new();
        let mut max_turn = 0u64;

        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    if let Ok(entry) = serde_json::from_str::<LedgerEntry>(line) {
                        if entry.turn > max_turn {
                            max_turn = entry.turn;
                        }
                        entries.insert(entry.chunk_key);
                    }
                }
            }
        }

        Self { entries, turn: max_turn, path: Some(path) }
    }

    /// Check if a chunk was already injected in a previous turn.
    pub(super) fn contains(&self, chunk_key: &str) -> bool {
        self.entries.contains(chunk_key)
    }

    /// Record newly injected chunks. Appends to the JSONL file.
    pub(super) fn record(&mut self, chunk_keys: &[String], injection_id: &str) {
        let new_turn = self.turn + 1;
        self.turn = new_turn;

        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // Append new entries
            let mut lines = String::new();
            for key in chunk_keys {
                if let Ok(json) = serde_json::to_string(&LedgerEntry {
                    chunk_key: key.clone(),
                    injection_id: injection_id.to_string(),
                    turn: new_turn,
                }) {
                    lines.push_str(&json);
                    lines.push('\n');
                }
                self.entries.insert(key.clone());
            }
            if !lines.is_empty() {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = f.write_all(lines.as_bytes());
                }
            }
        } else {
            // In-memory only (no session_id)
            for key in chunk_keys {
                self.entries.insert(key.clone());
            }
        }
    }

    /// Clear the ledger (used on compaction reset).
    pub(super) fn clear(repo_root: &Path, cc_session_id: &str) {
        if cc_session_id.is_empty() {
            return;
        }
        let path = repo_root
            .join(".bobbin")
            .join("session")
            .join(cc_session_id)
            .join("ledger.jsonl");
        let _ = std::fs::remove_file(&path);
    }

    /// Number of unique chunks tracked.
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Get unique file paths from previously injected chunks.
    /// Chunk keys are formatted as "file_path:start_line:end_line".
    pub(super) fn injected_files(&self) -> Vec<String> {
        let mut files: HashSet<String> = HashSet::new();
        for key in &self.entries {
            // Find the last two colons to extract the file path
            // Keys are "path:start:end" — path itself may contain colons on Windows
            if let Some(last_colon) = key.rfind(':') {
                if let Some(second_colon) = key[..last_colon].rfind(':') {
                    let file = &key[..second_colon];
                    files.insert(file.to_string());
                }
            }
        }
        files.into_iter().collect()
    }
}

/// Build a chunk key from file path and chunk line range.
pub(super) fn chunk_key(file_path: &str, start_line: u32, end_line: u32) -> String {
    format!("{}:{}:{}", file_path, start_line, end_line)
}

/// Tracks recent prompts within a session to build conversation-aware queries.
///
/// Stored as JSONL at `.bobbin/session/<cc_session_id>/prompts.jsonl`.
/// Each entry records a cleaned prompt and timestamp, limited to the most recent
/// N entries (configurable, default 5). When building a search query, recent
/// prompts are combined with the current prompt to capture conversational trajectory.
pub(super) struct PromptHistory {
    pub(super) entries: Vec<PromptEntry>,
    pub(super) path: Option<PathBuf>,
    pub(super) max_entries: usize,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub(super) struct PromptEntry {
    pub(super) prompt: String,
    pub(super) timestamp: u64, // unix epoch seconds
}

impl PromptHistory {
    /// Load prompt history for a Claude Code session.
    pub(super) fn load(repo_root: &Path, cc_session_id: &str, max_entries: usize) -> Self {
        if cc_session_id.is_empty() {
            return Self { entries: Vec::new(), path: None, max_entries };
        }
        let dir = repo_root.join(".bobbin").join("session").join(cc_session_id);
        let path = dir.join("prompts.jsonl");

        let mut entries = Vec::new();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    if let Ok(entry) = serde_json::from_str::<PromptEntry>(line) {
                        entries.push(entry);
                    }
                }
            }
        }

        // Keep only the most recent max_entries
        if entries.len() > max_entries {
            entries = entries.split_off(entries.len() - max_entries);
        }

        Self { entries, path: Some(path), max_entries }
    }

    /// Record a new prompt. Appends to the JSONL file and maintains window size.
    pub(super) fn record(&mut self, prompt: &str) {
        let entry = PromptEntry {
            prompt: prompt.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        self.entries.push(entry.clone());

        // Trim to max_entries
        if self.entries.len() > self.max_entries {
            self.entries = self.entries.split_off(self.entries.len() - self.max_entries);
        }

        // Append to file
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string(&entry) {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = writeln!(f, "{}", json);
                }
            }
        }
    }

    /// Build a trajectory-aware search query from recent prompts + current prompt.
    ///
    /// Combines the current prompt with up to N recent prompts, weighted by recency.
    /// Recent prompts are truncated to keep the combined query under a character limit.
    /// Returns the enriched query string.
    pub(super) fn build_trajectory_query(&self, current_prompt: &str, max_chars: usize) -> String {
        if self.entries.is_empty() {
            return current_prompt.to_string();
        }

        // Current prompt gets full weight (placed last, most influential for embeddings)
        let current_len = current_prompt.len();
        if current_len >= max_chars {
            return current_prompt.to_string();
        }

        let remaining = max_chars - current_len;

        // Collect recent prompts (excluding any that match the current prompt)
        let recent: Vec<&str> = self.entries
            .iter()
            .rev()
            .filter(|e| e.prompt != current_prompt)
            .map(|e| e.prompt.as_str())
            .take(3) // Use at most 3 recent prompts
            .collect();

        if recent.is_empty() {
            return current_prompt.to_string();
        }

        // Allocate remaining budget across recent prompts (more recent = more budget)
        let mut context_parts: Vec<String> = Vec::new();
        let per_prompt_budget = remaining / recent.len();

        for prompt in recent.iter().rev() {
            // Reverse back to chronological order
            let truncated = if prompt.len() > per_prompt_budget {
                // Take the last per_prompt_budget chars (most relevant part)
                let cutoff = prompt.len() - per_prompt_budget;
                match prompt[cutoff..].find(' ') {
                    Some(pos) => &prompt[cutoff + pos + 1..],
                    None => &prompt[cutoff..],
                }
            } else {
                prompt
            };
            if !truncated.is_empty() {
                context_parts.push(truncated.to_string());
            }
        }

        if context_parts.is_empty() {
            current_prompt.to_string()
        } else {
            // Format: "context from history... | current prompt"
            // The pipe separator helps embeddings distinguish trajectory from current focus
            format!("{} | {}", context_parts.join(" "), current_prompt)
        }
    }
}
