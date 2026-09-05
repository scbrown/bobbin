//! Listing recent archive records — the read path shared by the HTTP
//! `/archive/recent` handler and the `archive_recent` MCP tool.
//!
//! Split out of `index::archive` (aegis-g69atf's file-size ratchet) so the
//! indexing half and the listing half stay separately readable. The two used
//! to be two *implementations* rather than two modules, which is how they came
//! to disagree — see the note on `collect_recent`.

use std::path::Path;

use crate::config::ArchiveConfig;

// ---------------------------------------------------------------------------
// Recent-record listing (shared by the HTTP handler and the MCP tool)
// ---------------------------------------------------------------------------
//
// `/archive/recent` and the `archive_recent` MCP tool used to be two separate
// implementations of the same question, and they disagreed: the HTTP handler
// walked the configured source directories, while the MCP tool ran an FTS
// query for the literal token `"*"` and post-filtered the hits. FTS has no
// match-all token, so `"*"` matched nothing and the MCP tool returned "No
// archive records found" for every input, for every date, forever — while the
// HTTP handler answered the identical query with rows (aegis-44n1cy).
//
// Both callers now go through `collect_recent` so there is one answer to the
// question. Listing recent records is a filesystem walk, not a search: it must
// not depend on a query string at all.

/// One archive record, as returned by [`collect_recent`].
#[derive(Debug, Clone)]
pub struct RecentRecord {
    /// Configured source name (e.g. "hla-records", "pensieve").
    pub source: String,
    /// Record id — the file stem.
    pub id: String,
    /// Record body, with YAML frontmatter stripped.
    pub body: String,
    /// Prefixed path, `"{source}:{relative/path.md}"`.
    pub file_path: String,
    /// `YYYY-MM-DD`, from the frontmatter `timestamp:` or the dated path.
    pub timestamp: String,
}

/// Collect archive records dated on or after `after`, newest first.
///
/// `after` defaults to 30 days ago when `None`; `source` restricts the walk to
/// one configured source name. Records with identical bodies are deduplicated
/// (the same record is often captured by several agents), keeping the newest.
pub fn collect_recent(
    config: &ArchiveConfig,
    after: Option<&str>,
    source: Option<&str>,
    limit: usize,
) -> Vec<RecentRecord> {
    if !config.enabled {
        return Vec::new();
    }

    let default_after = (chrono::Utc::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let after = after.unwrap_or(&default_after);

    // (source, id, body, rel_path, sort_date)
    let mut records: Vec<(String, String, String, String, String)> = Vec::new();

    for src in &config.sources {
        if src.path.is_empty() {
            continue;
        }
        if source.is_some_and(|want| want != src.name) {
            continue;
        }

        let root = Path::new(&src.path);
        let mut found = Vec::new();
        collect_recent_records(root, root, after, &mut found);

        for (id, content, rel_path) in found {
            // Prefer the frontmatter timestamp; fall back to the dated path.
            let sort_date = extract_timestamp_from_frontmatter(&content)
                .or_else(|| extract_date_from_archive_path(&format!("_:{}", rel_path)))
                .unwrap_or_default();
            let body = extract_body(&content).unwrap_or_default();
            records.push((src.name.clone(), id, body, rel_path, sort_date));
        }
    }

    // Newest first, path as a stable tiebreak.
    records.sort_by(|a, b| b.4.cmp(&a.4).then_with(|| b.3.cmp(&a.3)));

    // Content dedup on the body — duplicates differ only in frontmatter.
    let mut seen = std::collections::HashSet::new();
    records.retain(|(_, _, body, _, _)| seen.insert(dedup_key(body)));

    records.truncate(limit);

    records
        .into_iter()
        .map(|(source, id, body, rel_path, sort_date)| {
            let file_path = format!("{}:{}", source, rel_path);
            let timestamp = if sort_date.is_empty() {
                extract_date_from_archive_path(&file_path).unwrap_or_default()
            } else {
                sort_date
            };
            RecentRecord {
                source,
                id,
                body,
                file_path,
                timestamp,
            }
        })
        .collect()
}

/// Dedup key for record bodies: the first 200 bytes, normalised.
///
/// Truncated at a char boundary — slicing a multi-byte char panics.
pub fn dedup_key(body: &str) -> String {
    let key = body.trim().to_lowercase();
    let mut end = key.len().min(200);
    while end > 0 && !key.is_char_boundary(end) {
        end -= 1;
    }
    key[..end].to_string()
}

/// Extract a date from an archive path like `"{source}:YYYY/MM/DD/..."`.
///
/// Handles any source prefix (`hla:`, `pensieve:`, …). Returns `None` when the
/// path carries no `{prefix}:` or is not date-partitioned.
pub fn extract_date_from_archive_path(path: &str) -> Option<String> {
    let after_prefix = path.split_once(':').map(|(_, rest)| rest)?;
    let parts: Vec<&str> = after_prefix.splitn(4, '/').collect();
    if parts.len() >= 3 && parts[0].len() == 4 && parts[1].len() == 2 && parts[2].len() == 2 {
        Some(format!("{}-{}-{}", parts[0], parts[1], parts[2]))
    } else {
        None
    }
}

/// Extract a `YYYY-MM-DD` date from the `timestamp:` field in YAML frontmatter.
pub fn extract_timestamp_from_frontmatter(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let end = trimmed[3..].find("\n---")?;
    for line in trimmed[3..3 + end].lines() {
        if let Some(val) = line.trim().strip_prefix("timestamp:") {
            let ts = val.trim();
            if ts.len() >= 10 {
                return Some(ts[..10].to_string());
            }
        }
    }
    None
}

/// Extract the body text following YAML frontmatter.
pub fn extract_body(content: &str) -> Option<String> {
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

/// Recursively collect `(id, content, rel_path)` for records dated >= `after`.
///
/// The date comes from the path when it is date-partitioned (cheap — no read),
/// otherwise from the frontmatter `timestamp:`. An unreadable directory yields
/// nothing rather than an error: a missing mount must not fail the whole walk.
pub fn collect_recent_records(
    root: &Path,
    dir: &Path,
    after: &str,
    results: &mut Vec<(String, String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip underscore-prefixed dirs (_plans/, _templates/): static
            // design docs, not time-series observations.
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('_') {
                continue;
            }
            collect_recent_records(root, &path, after, results);
        } else if path.extension().is_some_and(|e| e == "md") {
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel = rel.to_string_lossy().to_string();

            // A dated path settles the window without opening the file.
            if let Some(d) = extract_date_from_archive_path(&format!("_:{}", rel)) {
                if d.as_str() < after {
                    continue;
                }
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let date = extract_date_from_archive_path(&format!("_:{}", rel))
                .or_else(|| extract_timestamp_from_frontmatter(&content));
            if date.as_deref().is_some_and(|d| d >= after) {
                let id = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                results.push((id, content, rel));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ArchiveSource;

    const SAMPLE_HLA: &str = r#"---
schema: human-intent/v2
id: hi-01ARYZ6S41
timestamp: 2026-02-17T14:32:00Z
author: stiwi
source:
  channel: telegram
---

Deploy bobbin to node-5, not the old CT.
Make sure traefik points to the new host.
"#;

    // -- Recent listing (aegis-44n1cy) --
    //
    // The MCP `archive_recent` tool used to answer this question with an FTS
    // query for the literal token `"*"`. FTS has no match-all token, so it
    // matched nothing and the tool reported "No archive records found" for
    // every input, for every date, indefinitely — while `/archive/recent`
    // answered the identical query with rows. These tests pin the property
    // that broke: listing recent records is a filesystem walk and must never
    // depend on a query string matching anything.

    fn write_record(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
    }

    fn recent_config(root: &Path) -> ArchiveConfig {
        ArchiveConfig {
            enabled: true,
            webhook_secret: String::new(),
            sources: vec![ArchiveSource {
                name: "hla-records".to_string(),
                path: root.to_string_lossy().to_string(),
                schema: "human-intent".to_string(),
                name_field: "channel".to_string(),
            }],
        }
    }

    #[test]
    fn collect_recent_returns_records_without_any_query() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), "2026/03/06/hi-aaa.md", SAMPLE_HLA);

        let found = collect_recent(&recent_config(dir.path()), Some("2026-03-01"), None, 10);

        assert_eq!(found.len(), 1, "a record in the window must be listed");
        assert_eq!(found[0].id, "hi-aaa");
        assert_eq!(found[0].source, "hla-records");
        assert_eq!(found[0].file_path, "hla-records:2026/03/06/hi-aaa.md");
        assert!(found[0].body.contains("Deploy bobbin to node-5"));
        // The frontmatter timestamp wins over the path date.
        assert_eq!(found[0].timestamp, "2026-02-17");
    }

    #[test]
    fn collect_recent_honours_the_after_window() {
        let dir = tempfile::tempdir().unwrap();
        // Dated paths, so the window is decided without reading frontmatter.
        write_record(dir.path(), "2026/03/06/hi-old.md", "no frontmatter, old");
        write_record(dir.path(), "2026/05/06/hi-new.md", "no frontmatter, new");

        let found = collect_recent(&recent_config(dir.path()), Some("2026-04-01"), None, 10);

        assert_eq!(found.len(), 1, "only the record after the window");
        assert_eq!(found[0].id, "hi-new");
    }

    #[test]
    fn collect_recent_filters_by_source_name() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), "2026/03/06/hi-aaa.md", SAMPLE_HLA);
        let config = recent_config(dir.path());

        let matched = collect_recent(&config, Some("2026-01-01"), Some("hla-records"), 10);
        assert_eq!(matched.len(), 1, "the configured source must match");

        // An unconfigured source is empty for a different reason than an empty
        // window — the caller is told which sources exist, not just "none".
        let unmatched = collect_recent(&config, Some("2026-01-01"), Some("pensieve"), 10);
        assert!(
            unmatched.is_empty(),
            "a source that is not configured cannot return records"
        );
    }

    #[test]
    fn collect_recent_dedups_identical_bodies() {
        let dir = tempfile::tempdir().unwrap();
        // Same body, different ids and timestamps — the duplicate-capture case.
        write_record(dir.path(), "2026/03/06/hi-aaa.md", SAMPLE_HLA);
        write_record(
            dir.path(),
            "2026/03/06/hi-bbb.md",
            &SAMPLE_HLA.replace("hi-01ARYZ6S41", "hi-bbb"),
        );

        let found = collect_recent(&recent_config(dir.path()), Some("2026-01-01"), None, 10);
        assert_eq!(found.len(), 1, "identical bodies collapse to one record");
    }

    #[test]
    fn collect_recent_survives_a_missing_source_directory() {
        // A source path that does not exist (an unmounted archive) must yield
        // nothing rather than panicking or aborting the whole walk.
        let config = recent_config(Path::new("/nonexistent/archive/root"));
        assert!(collect_recent(&config, Some("2026-01-01"), None, 10).is_empty());
    }

    #[test]
    fn collect_recent_is_empty_when_archive_disabled() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), "2026/03/06/hi-aaa.md", SAMPLE_HLA);
        let mut config = recent_config(dir.path());
        config.enabled = false;
        assert!(collect_recent(&config, Some("2026-01-01"), None, 10).is_empty());
    }

    #[test]
    fn collect_recent_skips_underscore_directories() {
        let dir = tempfile::tempdir().unwrap();
        write_record(dir.path(), "_templates/2026/03/06/hi-tpl.md", SAMPLE_HLA);

        let found = collect_recent(&recent_config(dir.path()), Some("2026-01-01"), None, 10);
        assert!(
            found.is_empty(),
            "_-prefixed dirs hold templates, not records"
        );
    }
}
