use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::config::ArchiveConfig;
use crate::types::{Chunk, ChunkType};

/// Parsed frontmatter from a human-intent record.
#[derive(Debug, Clone, Default)]
pub struct IntentMeta {
    pub id: String,
    pub timestamp: String,
    pub author: String,
    pub channel: String,
    pub context: String,
}

/// Fetch intent archive records from the filesystem and convert to Chunks.
pub fn fetch_archive(config: &ArchiveConfig) -> Result<Vec<Chunk>> {
    if !config.enabled || config.archive_path.is_empty() {
        return Ok(vec![]);
    }

    let root = Path::new(&config.archive_path);
    if !root.exists() {
        return Ok(vec![]);
    }

    let mut all_chunks = Vec::new();
    collect_records(root, root, &mut all_chunks)?;
    Ok(all_chunks)
}

/// Recursively walk the archive directory collecting .md files.
fn collect_records(root: &Path, dir: &Path, chunks: &mut Vec<Chunk>) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("archive: read dir: {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_records(root, &path, chunks)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("archive: read: {}", path.display()))?;
            if let Some(chunk) = record_to_chunk(root, &path, &content) {
                chunks.push(chunk);
            }
        }
    }
    Ok(())
}

/// Parse a single intent record file into a Chunk.
///
/// Returns None if the file doesn't have valid human-intent frontmatter.
fn record_to_chunk(root: &Path, path: &Path, content: &str) -> Option<Chunk> {
    let (meta, body) = parse_intent_frontmatter(content)?;

    let rel_path = path.strip_prefix(root).unwrap_or(path);
    let file_path = format!("archive:{}", rel_path.to_string_lossy());

    // One record = one chunk. If body exceeds 100 lines, take first 100
    // with a note — overflow splitting deferred to v2.
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    let lines: Vec<&str> = body.lines().collect();
    let end_line = lines.len() as u32;

    let mut hasher = Sha256::new();
    hasher.update(format!("archive:{}:{}", meta.id, meta.timestamp).as_bytes());
    let id = format!("{:x}", hasher.finalize());

    Some(Chunk {
        id,
        file_path,
        chunk_type: ChunkType::Section,
        name: Some(format!("{}/{}", meta.channel, meta.id)),
        start_line: 1,
        end_line,
        content: body.to_string(),
        language: "transcript".to_string(),
    })
}

/// Parse YAML frontmatter for human-intent schema.
///
/// Returns (metadata, body_text) if the file has `schema: human-intent/v*`.
fn parse_intent_frontmatter(content: &str) -> Option<(IntentMeta, &str)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }

    let after_fence = trimmed[3..].find("\n---")?;
    let fm_text = &trimmed[3..3 + after_fence];

    // Quick check: must contain human-intent schema
    if !fm_text.contains("human-intent") {
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

/// Extract key fields from YAML frontmatter text (simple line-based parsing).
fn parse_meta_fields(fm: &str) -> IntentMeta {
    let mut meta = IntentMeta::default();

    for line in fm.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("id:") {
            meta.id = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("timestamp:") {
            meta.timestamp = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("author:") {
            meta.author = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("channel:") {
            meta.channel = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("context:") {
            meta.context = val.trim().to_string();
        }
    }

    // Default ID from timestamp if not set
    if meta.id.is_empty() && !meta.timestamp.is_empty() {
        meta.id = meta.timestamp.clone();
    }

    meta
}

/// Check if file content is a human-intent transcript.
/// Used by the parser to route to transcript handling.
pub fn is_intent_record(content: &str) -> bool {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return false;
    }
    if let Some(end) = trimmed[3..].find("\n---") {
        let fm = &trimmed[3..3 + end];
        fm.contains("human-intent")
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RECORD: &str = r#"---
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

    #[test]
    fn test_is_intent_record() {
        assert!(is_intent_record(SAMPLE_RECORD));
        assert!(!is_intent_record("# Just a markdown file\n\nHello world"));
        assert!(!is_intent_record("---\ntitle: not intent\n---\nbody"));
    }

    #[test]
    fn test_parse_intent_frontmatter() {
        let (meta, body) = parse_intent_frontmatter(SAMPLE_RECORD).unwrap();
        assert_eq!(meta.id, "hi-01ARYZ6S41");
        assert_eq!(meta.timestamp, "2026-02-17T14:32:00Z");
        assert_eq!(meta.author, "stiwi");
        assert_eq!(meta.channel, "telegram");
        assert!(body.contains("Deploy bobbin to luvu"));
    }

    #[test]
    fn test_record_to_chunk() {
        let root = Path::new("/archive");
        let path = Path::new("/archive/2026/02/17/hi-01ARYZ6S41.md");
        let chunk = record_to_chunk(root, path, SAMPLE_RECORD).unwrap();
        assert_eq!(chunk.language, "transcript");
        assert_eq!(chunk.chunk_type, ChunkType::Section);
        assert!(chunk.name.unwrap().contains("hi-01ARYZ6S41"));
        assert!(chunk.content.contains("Deploy bobbin to luvu"));
        assert_eq!(chunk.file_path, "archive:2026/02/17/hi-01ARYZ6S41.md");
    }

    #[test]
    fn test_empty_body_returns_none() {
        let record = "---\nschema: human-intent/v2\nid: hi-test\n---\n";
        let root = Path::new("/archive");
        let path = Path::new("/archive/test.md");
        assert!(record_to_chunk(root, path, record).is_none());
    }
}
