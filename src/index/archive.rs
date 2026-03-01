use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::config::{ArchiveConfig, ArchiveSource};
use crate::types::{Chunk, ChunkType};

/// Parsed frontmatter fields from an archive record.
///
/// Generic across all archive types — each source uses a schema match
/// to identify its records, and a name_field to choose which metadata
/// field becomes the chunk name prefix.
#[derive(Debug, Clone, Default)]
pub struct ArchiveMeta {
    /// Record ID (from frontmatter `id:`)
    pub id: String,
    /// Timestamp (from frontmatter `timestamp:`)
    pub timestamp: String,
    /// All simple key-value fields from frontmatter
    pub fields: Vec<(String, String)>,
}

/// Fetch archive records from all configured sources and convert to Chunks.
pub fn fetch_archive(config: &ArchiveConfig) -> Result<Vec<Chunk>> {
    if !config.enabled {
        return Ok(vec![]);
    }

    let mut all_chunks = Vec::new();

    for source in &config.sources {
        if source.path.is_empty() {
            continue;
        }
        let root = Path::new(&source.path);
        if root.exists() {
            collect_records(root, root, source, &mut all_chunks)?;
        }
    }

    Ok(all_chunks)
}

/// Recursively walk an archive directory collecting .md files that match the source schema.
fn collect_records(
    root: &Path,
    dir: &Path,
    source: &ArchiveSource,
    chunks: &mut Vec<Chunk>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("archive: read dir: {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_records(root, &path, source, chunks)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("archive: read: {}", path.display()))?;
            if let Some(chunk) = record_to_chunk(root, &path, &content, source) {
                chunks.push(chunk);
            }
        }
    }
    Ok(())
}

/// Parse a single archive record into a Chunk for the given source.
///
/// Returns None if the frontmatter doesn't contain the source's schema string.
fn record_to_chunk(
    root: &Path,
    path: &Path,
    content: &str,
    source: &ArchiveSource,
) -> Option<Chunk> {
    let (meta, body) = parse_frontmatter(content, &source.schema)?;

    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    // Build file_path with date prefix for consistent date extraction.
    // If the filesystem path is already date-partitioned (e.g., YYYY/MM/DD/file.md),
    // use it directly. Otherwise, extract date from the timestamp field.
    let rel_path = path.strip_prefix(root).unwrap_or(path);
    let rel_str = rel_path.to_string_lossy();
    let file_path = if looks_like_date_partitioned(&rel_str) {
        format!("{}:{}", source.name, rel_str)
    } else {
        let date_prefix = timestamp_to_date_path(&meta.timestamp);
        format!("{}:{}/{}", source.name, date_prefix, rel_str)
    };

    let lines: Vec<&str> = body.lines().collect();
    let end_line = lines.len() as u32;

    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}:{}", source.name, meta.id, meta.timestamp).as_bytes());
    let id = format!("{:x}", hasher.finalize());

    // Build chunk name: "{name_field_value}/{record_id}" or just "{record_id}"
    let name_prefix = if source.name_field.is_empty() {
        String::new()
    } else {
        meta.fields
            .iter()
            .find(|(k, _)| k == &source.name_field)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let chunk_name = if name_prefix.is_empty() {
        meta.id.clone()
    } else {
        format!("{}/{}", name_prefix, meta.id)
    };

    Some(Chunk {
        id,
        file_path,
        chunk_type: ChunkType::Section,
        name: Some(chunk_name),
        start_line: 1,
        end_line,
        content: body.to_string(),
        language: source.name.clone(),
    })
}

/// Parse YAML frontmatter for a given schema identifier.
///
/// Returns (metadata, body_text) if the frontmatter contains the schema string.
fn parse_frontmatter<'a>(content: &'a str, schema: &str) -> Option<(ArchiveMeta, &'a str)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }

    let after_fence = trimmed[3..].find("\n---")?;
    let fm_text = &trimmed[3..3 + after_fence];

    if !fm_text.contains(schema) {
        return None;
    }

    let body_start = 3 + after_fence + 4; // skip \n---
    let body = if body_start < trimmed.len() {
        &trimmed[body_start..]
    } else {
        ""
    };

    let meta = parse_meta_fields(fm_text);
    Some((meta, body))
}

/// Extract key fields from YAML frontmatter (simple line-based parsing).
///
/// Handles simple `key: value` pairs and YAML list items under the last key.
/// Nested keys under `source:` are flattened (e.g., `source:\n  channel: x`
/// becomes a field named `channel`).
fn parse_meta_fields(fm: &str) -> ArchiveMeta {
    let mut meta = ArchiveMeta::default();
    let mut last_key = String::new();

    for line in fm.lines() {
        let trimmed = line.trim();

        // Skip empty lines and schema line
        if trimmed.is_empty() || trimmed.starts_with("schema:") {
            continue;
        }

        // YAML list item (e.g., "  - mcp-tools")
        if let Some(item) = trimmed.strip_prefix("- ") {
            if !last_key.is_empty() {
                // Append to last key's value as comma-separated
                if let Some(entry) = meta.fields.iter_mut().find(|(k, _)| k == &last_key) {
                    if entry.1.is_empty() {
                        entry.1 = item.trim().to_string();
                    } else {
                        entry.1 = format!("{},{}", entry.1, item.trim());
                    }
                }
            }
            continue;
        }

        // Key: value pair
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim().to_string();
            let val = trimmed[colon_pos + 1..].trim().to_string();

            // Special handling for well-known fields
            if key == "id" {
                meta.id = val.clone();
            } else if key == "timestamp" {
                meta.timestamp = val.clone();
            }

            // Skip "null" values
            let store_val = if val == "null" {
                String::new()
            } else {
                val
            };

            last_key = key.clone();
            meta.fields.push((key, store_val));
        }
    }

    // Default ID from timestamp if not set
    if meta.id.is_empty() && !meta.timestamp.is_empty() {
        meta.id = meta.timestamp.clone();
    }

    meta
}

/// Check if a relative path starts with a date-like pattern (YYYY/MM/DD/).
fn looks_like_date_partitioned(rel_path: &str) -> bool {
    let parts: Vec<&str> = rel_path.splitn(4, '/').collect();
    if parts.len() < 3 {
        return false;
    }
    parts[0].len() == 4
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && parts[1].len() == 2
        && parts[1].chars().all(|c| c.is_ascii_digit())
        && parts[2].len() == 2
        && parts[2].chars().all(|c| c.is_ascii_digit())
}

/// Extract YYYY/MM/DD from an ISO timestamp like "2026-02-27T01:00:00Z".
fn timestamp_to_date_path(timestamp: &str) -> String {
    if timestamp.len() >= 10 {
        let date_part = &timestamp[..10];
        let parts: Vec<&str> = date_part.split('-').collect();
        if parts.len() == 3 {
            return format!("{}/{}/{}", parts[0], parts[1], parts[2]);
        }
    }
    "unknown".to_string()
}

/// Check if file content is an archive record matching a given schema.
pub fn matches_schema(content: &str, schema: &str) -> bool {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return false;
    }
    if let Some(end) = trimmed[3..].find("\n---") {
        let fm = &trimmed[3..3 + end];
        fm.contains(schema)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HLA: &str = r#"---
schema: human-intent/v2
id: hi-01ARYZ6S41
timestamp: 2026-02-17T14:32:00Z
author: stiwi
source:
  channel: telegram
  context: dm
---

Deploy bobbin to luvu, not the old CT.
Make sure traefik points to the new host.
"#;

    const SAMPLE_PENSIEVE: &str = r#"---
schema: agent-memory/v1
id: pm-01BXYZ7T52
timestamp: 2026-02-27T01:00:00Z
agent: aegis/crew/arnold
topics:
  - mcp-tools
  - performance
supersedes: null
confidence: high
---

Bobbin search latency improved from 200ms to 45ms after switching to hybrid mode.
The semantic_weight of 0.7 gives the best results for code search.
"#;

    fn hla_source() -> ArchiveSource {
        ArchiveSource {
            name: "hla".to_string(),
            path: "/archive".to_string(),
            schema: "human-intent".to_string(),
            name_field: "channel".to_string(),
        }
    }

    fn pensieve_source() -> ArchiveSource {
        ArchiveSource {
            name: "pensieve".to_string(),
            path: "/pensieve".to_string(),
            schema: "agent-memory".to_string(),
            name_field: "agent".to_string(),
        }
    }

    // -- Schema matching --

    #[test]
    fn test_matches_schema() {
        assert!(matches_schema(SAMPLE_HLA, "human-intent"));
        assert!(!matches_schema(SAMPLE_HLA, "agent-memory"));
        assert!(matches_schema(SAMPLE_PENSIEVE, "agent-memory"));
        assert!(!matches_schema(SAMPLE_PENSIEVE, "human-intent"));
        assert!(!matches_schema("# Not a record", "human-intent"));
    }

    // -- Frontmatter parsing --

    #[test]
    fn test_parse_hla_frontmatter() {
        let (meta, body) = parse_frontmatter(SAMPLE_HLA, "human-intent").unwrap();
        assert_eq!(meta.id, "hi-01ARYZ6S41");
        assert_eq!(meta.timestamp, "2026-02-17T14:32:00Z");
        assert!(body.contains("Deploy bobbin to luvu"));

        // Check that channel is extracted from nested source
        let channel = meta.fields.iter().find(|(k, _)| k == "channel");
        assert_eq!(channel.unwrap().1, "telegram");
    }

    #[test]
    fn test_parse_pensieve_frontmatter() {
        let (meta, body) = parse_frontmatter(SAMPLE_PENSIEVE, "agent-memory").unwrap();
        assert_eq!(meta.id, "pm-01BXYZ7T52");
        assert_eq!(meta.timestamp, "2026-02-27T01:00:00Z");
        assert!(body.contains("Bobbin search latency"));

        let agent = meta.fields.iter().find(|(k, _)| k == "agent");
        assert_eq!(agent.unwrap().1, "aegis/crew/arnold");

        let topics = meta.fields.iter().find(|(k, _)| k == "topics");
        assert_eq!(topics.unwrap().1, "mcp-tools,performance");
    }

    #[test]
    fn test_parse_frontmatter_wrong_schema() {
        assert!(parse_frontmatter(SAMPLE_HLA, "agent-memory").is_none());
        assert!(parse_frontmatter(SAMPLE_PENSIEVE, "human-intent").is_none());
    }

    // -- Chunk creation --

    #[test]
    fn test_hla_record_to_chunk() {
        let source = hla_source();
        let root = Path::new("/archive");
        let path = Path::new("/archive/2026/02/17/hi-01ARYZ6S41.md");
        let chunk = record_to_chunk(root, path, SAMPLE_HLA, &source).unwrap();

        assert_eq!(chunk.language, "hla");
        assert_eq!(chunk.chunk_type, ChunkType::Section);
        assert_eq!(chunk.file_path, "hla:2026/02/17/hi-01ARYZ6S41.md");
        assert!(chunk.name.as_deref().unwrap().contains("hi-01ARYZ6S41"));
        assert!(chunk.name.as_deref().unwrap().starts_with("telegram/"));
        assert!(chunk.content.contains("Deploy bobbin to luvu"));
    }

    #[test]
    fn test_pensieve_record_to_chunk() {
        let source = pensieve_source();
        let root = Path::new("/pensieve");
        let path = Path::new("/pensieve/aegis-crew-arnold/pm-01BXYZ7T52.md");
        let chunk = record_to_chunk(root, path, SAMPLE_PENSIEVE, &source).unwrap();

        assert_eq!(chunk.language, "pensieve");
        assert_eq!(chunk.chunk_type, ChunkType::Section);
        assert!(chunk.file_path.starts_with("pensieve:2026/02/27/"));
        assert!(chunk.file_path.contains("pm-01BXYZ7T52.md"));
        assert_eq!(
            chunk.name.as_deref().unwrap(),
            "aegis/crew/arnold/pm-01BXYZ7T52"
        );
        assert!(chunk.content.contains("Bobbin search latency"));
    }

    #[test]
    fn test_empty_body_returns_none() {
        let source = hla_source();
        let record = "---\nschema: human-intent/v2\nid: hi-test\n---\n";
        let root = Path::new("/archive");
        let path = Path::new("/archive/test.md");
        assert!(record_to_chunk(root, path, record, &source).is_none());
    }

    // -- Helpers --

    #[test]
    fn test_looks_like_date_partitioned() {
        assert!(looks_like_date_partitioned("2026/02/17/hi-xxx.md"));
        assert!(looks_like_date_partitioned("2026/12/01/file.md"));
        assert!(!looks_like_date_partitioned("aegis-crew-arnold/pm-xxx.md"));
        assert!(!looks_like_date_partitioned("records/pm-xxx.md"));
        assert!(!looks_like_date_partitioned("file.md"));
    }

    #[test]
    fn test_timestamp_to_date_path() {
        assert_eq!(
            timestamp_to_date_path("2026-02-27T01:00:00Z"),
            "2026/02/27"
        );
        assert_eq!(timestamp_to_date_path("2026-12-01"), "2026/12/01");
        assert_eq!(timestamp_to_date_path("bad"), "unknown");
        assert_eq!(timestamp_to_date_path(""), "unknown");
    }

    // -- Custom source --

    #[test]
    fn test_custom_source() {
        let custom_record = r#"---
schema: field-notes/v1
id: fn-001
timestamp: 2026-03-01T12:00:00Z
project: alpha
---

Field observation: deployment went smoothly.
"#;
        let source = ArchiveSource {
            name: "fieldnotes".to_string(),
            path: "/notes".to_string(),
            schema: "field-notes".to_string(),
            name_field: "project".to_string(),
        };
        let root = Path::new("/notes");
        let path = Path::new("/notes/fn-001.md");
        let chunk = record_to_chunk(root, path, custom_record, &source).unwrap();

        assert_eq!(chunk.language, "fieldnotes");
        assert_eq!(chunk.name.as_deref().unwrap(), "alpha/fn-001");
        assert!(chunk.file_path.starts_with("fieldnotes:2026/03/01/"));
    }
}
