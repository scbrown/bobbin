//! Filesystem and frontmatter helpers behind the archive handlers.
//!
//! Split out of the former `src/http/handlers/archive.rs` (bobbin-aoz), which
//! was 846 lines — the second-largest non-allowlisted file in the tree.

//! Archive and beads search handlers.
#![allow(private_interfaces)]

// The archive record helpers live with the archive domain logic in
// `index::archive`, so the HTTP handler and the MCP tool share one
// implementation instead of two that drift apart (aegis-44n1cy).
pub(super) use crate::index::archive::{extract_body, extract_date_from_archive_path};

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
pub(super) fn find_record_by_id(root: &std::path::Path, id: &str) -> Option<(String, String)> {
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
        } else if path
            .file_stem()
            .is_some_and(|s| s.to_string_lossy().contains(id))
        {
            let content = std::fs::read_to_string(&path).ok()?;
            let rel = path.strip_prefix(root).ok()?;
            return Some((content, rel.to_string_lossy().to_string()));
        }
    }
    None
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
