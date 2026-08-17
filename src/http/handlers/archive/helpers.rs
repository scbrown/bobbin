//! Filesystem and frontmatter helpers behind the archive handlers.
//!
//! Split out of the former `src/http/handlers/archive.rs` (bobbin-aoz), which
//! was 846 lines — the second-largest non-allowlisted file in the tree.

//! Archive and beads search handlers.
#![allow(private_interfaces)]

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::search::{HybridSearch, SemanticSearch};
use crate::types::{ChunkType, SearchResult};

use super::super::{bad_request, internal_error, open_vector_store, AppState, ErrorBody};
/// Extract a date string from an archive path like "{source}:YYYY/MM/DD/..."
///
/// Handles any source prefix (hla:, pensieve:, archive:, etc.)
pub(super) fn extract_date_from_archive_path(path: &str) -> Option<String> {
    // Strip the source prefix (everything before and including ':')
    let after_prefix = path.split_once(':').map(|(_, rest)| rest)?;
    // Path format: YYYY/MM/DD/filename.md
    let parts: Vec<&str> = after_prefix.splitn(4, '/').collect();
    if parts.len() >= 3
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
    {
        Some(format!("{}-{}-{}", parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

/// Get the list of archive source names (language tags) from config.
pub(super) fn archive_source_names(config: &crate::config::ArchiveConfig) -> Vec<String> {
    config.sources.iter().map(|s| s.name.clone()).collect()
}

/// Get (source_name, source_path) pairs from config.
pub(super) fn archive_source_paths(config: &crate::config::ArchiveConfig) -> Vec<(String, String)> {
    config
        .sources
        .iter()
        .filter(|s| !s.path.is_empty())
        .map(|s| (s.name.clone(), s.path.clone()))
        .collect()
}

/// Find a record file by ID (searches for filename containing the ID)
pub(super) fn find_record_by_id(
    root: &std::path::Path,
    id: &str,
) -> Option<(String, String)> {
    find_record_recursive(root, root, id)
}

pub(super) fn find_record_recursive(
    root: &std::path::Path,
    dir: &std::path::Path,
    id: &str,
) -> Option<(String, String)> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_record_recursive(root, &path, id) {
                return Some(found);
            }
        } else if path.file_stem().is_some_and(|s| s.to_string_lossy().contains(id)) {
            let content = std::fs::read_to_string(&path).ok()?;
            let rel = path.strip_prefix(root).ok()?;
            return Some((content, rel.to_string_lossy().to_string()));
        }
    }
    None
}

/// Collect archive records whose date is >= the `after` date.
///
/// Date is extracted from the path if it's date-partitioned (YYYY/MM/DD/...),
/// otherwise falls back to parsing the `timestamp:` field from YAML frontmatter.
pub(super) fn collect_recent_records(
    root: &std::path::Path,
    dir: &std::path::Path,
    after: &str,
    results: &mut Vec<(String, String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip underscore-prefixed directories (_plans/, _templates/, etc.)
            // These contain static design docs, not time-series observations.
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.starts_with('_') {
                continue;
            }
            collect_recent_records(root, &path, after, results);
        } else if path.extension().is_some_and(|e| e == "md") {
            let rel = match path.strip_prefix(root) {
                Ok(r) => r.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            // Try date from path first (cheap)
            let date = extract_date_from_archive_path(&format!("_:{}", rel));

            if let Some(ref d) = date {
                // Path has a date — filter without reading file
                if d.as_str() >= after {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let id = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        results.push((id, content, rel));
                    }
                }
            } else {
                // No date in path — read file and check frontmatter timestamp
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let fm_date = extract_timestamp_from_frontmatter(&content);
                    if fm_date.as_deref().is_some_and(|d| d >= after) {
                        let id = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        results.push((id, content, rel));
                    }
                }
            }
        }
    }
}

/// Extract a YYYY-MM-DD date from the `timestamp:` field in YAML frontmatter.
pub(super) fn extract_timestamp_from_frontmatter(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let end = trimmed[3..].find("\n---")?;
    let fm = &trimmed[3..3 + end];
    for line in fm.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("timestamp:") {
            let ts = val.trim();
            if ts.len() >= 10 {
                return Some(ts[..10].to_string());
            }
        }
    }
    None
}

/// Extract body text after YAML frontmatter
pub(super) fn extract_body(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Some(content.to_string());
    }
    let close = trimmed[3..].find("\n---")?;
    let body_start = 3 + close + 4;
    let body = if body_start < trimmed.len() {
        trimmed[body_start..].trim()
    } else {
        ""
    };
    Some(body.to_string())
}

pub(super) fn extract_bead_field(content: &str, prefix: &str) -> String {
    content
        .lines()
        .find(|line| line.contains(prefix))
        .and_then(|line| {
            let start = line.find(prefix)? + prefix.len();
            let rest = &line[start..];
            let end = rest.find(" | ").unwrap_or(rest.len());
            Some(rest[..end].trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Clean a bead snippet by removing metadata lines already in structured fields.
pub(super) fn clean_bead_snippet(content: &str, max_len: usize) -> String {
    let cleaned: String = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !(trimmed.starts_with("Status: ") && trimmed.contains(" | "))
                && !trimmed.starts_with("Priority: P")
                && !trimmed.starts_with("Assignee: ")
                && !trimmed.starts_with("Comments:")
                && !trimmed.starts_with("--- ")
                && !trimmed.starts_with("Notes:")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let trimmed = cleaned.trim();
    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &trimmed[..end])
    }
}

