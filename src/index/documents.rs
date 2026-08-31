//! Generic documents ingest: extract searchable text from HTML files.
//!
//! Mirrors the multimodal (PDF) lifecycle exactly: an opt-in `[index]`
//! flag (`documents = true`) adds the extensions to the walk, the text is
//! extracted upstream of the parser in `read_indexable_content`, and the
//! result flows through the normal chunking pipeline — the markdown
//! chunker, since the conversion below emits markdown-ish text whose
//! `#` headings become section chunks with breadcrumb names.
//!
//! The converter is a deliberately dependency-free, deterministic tag
//! stripper (no model in the loop, no HTML parser crate): script/style
//! and comments are dropped, `<head>` is skipped, headings become `#`
//! prefixes, block elements become line breaks, `<pre>` becomes a fenced
//! code block, list items become bullets, and entities are decoded.
//! Malformed input degrades to whatever text survives — never an error —
//! so the indexer's empty-content skip handles pathological files.

use std::path::Path;

use anyhow::{Context, Result};

/// File extensions handled by the documents extractor.
///
/// Centralized so the indexer's include-pattern injection and the per-file
/// routing decision agree on one list (same shape as
/// [`crate::index::multimodal::MULTIMODAL_EXTENSIONS`]).
pub const DOCUMENT_EXTENSIONS: &[&str] = &["html", "htm"];

/// Returns true if `path` has an extension the documents extractor handles.
/// Comparison is case-insensitive (`.HTML` counts).
pub fn is_document_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let ext = ext.to_ascii_lowercase();
            DOCUMENT_EXTENSIONS.contains(&ext.as_str())
        }
        None => false,
    }
}

/// Read an HTML file and convert it to markdown-ish text for indexing.
///
/// IO/encoding failures are the only error path; the conversion itself
/// never fails. Empty or text-free HTML yields an empty string, which the
/// indexer already treats as "skip this file".
pub fn extract_text(path: &Path) -> Result<String> {
    let html = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read HTML document {}", path.display()))?;
    Ok(html_to_markdownish(&html))
}

/// Which structural role a tag plays in the emitted text.
enum TagRole {
    /// `h1`..`h6`: heading prefix with the given level
    Heading(usize),
    /// Paragraph-ish: line break before and after
    Block,
    /// `<li>`: line break plus a bullet
    ListItem,
    /// `<br>`: a single line break
    LineBreak,
    /// `<pre>`: fenced block, whitespace preserved inside
    Pre,
    /// `<td>`/`<th>`: a cell separator space
    Cell,
    /// Anything else: inline, contributes nothing itself
    Inline,
}

fn tag_role(name: &str) -> TagRole {
    match name {
        "h1" => TagRole::Heading(1),
        "h2" => TagRole::Heading(2),
        "h3" => TagRole::Heading(3),
        "h4" => TagRole::Heading(4),
        "h5" => TagRole::Heading(5),
        "h6" => TagRole::Heading(6),
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "aside" | "nav"
        | "blockquote" | "table" | "thead" | "tbody" | "tr" | "ul" | "ol" | "dl" | "dt" | "dd"
        | "figure" | "figcaption" | "hr" | "form" | "fieldset" | "details" | "summary" => {
            TagRole::Block
        }
        "li" => TagRole::ListItem,
        "br" => TagRole::LineBreak,
        "pre" => TagRole::Pre,
        "td" | "th" => TagRole::Cell,
        _ => TagRole::Inline,
    }
}

/// Convert HTML to deterministic markdown-ish plain text.
pub fn html_to_markdownish(html: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    let mut in_pre = false;
    // Pending inter-word whitespace from the source, emitted lazily so
    // collapsed runs never produce trailing spaces before a block break.
    let mut pending_space = false;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Comment?
            if html[i..].starts_with("<!--") {
                i = match html[i + 4..].find("-->") {
                    Some(p) => i + 4 + p + 3,
                    None => bytes.len(), // unterminated comment: drop the rest
                };
                continue;
            }
            // Parse the tag name.
            let (closing, name, tag_end) = match parse_tag(html, i) {
                Some(t) => t,
                None => {
                    // A bare '<' that opens no tag ("a < b"): literal text.
                    flush_space(&mut out, &mut pending_space, in_pre);
                    out.push('<');
                    i += 1;
                    continue;
                }
            };

            // Containers whose *content* is dropped wholesale.
            if !closing && matches!(name.as_str(), "script" | "style" | "head" | "title") {
                i = skip_container(html, tag_end, &name);
                continue;
            }

            match tag_role(&name) {
                TagRole::Heading(level) => {
                    if closing {
                        push_newline(&mut out);
                        out.push('\n');
                    } else {
                        ensure_blank_line(&mut out);
                        out.push_str(&"#".repeat(level));
                        out.push(' ');
                    }
                    pending_space = false;
                }
                TagRole::Block => {
                    push_newline(&mut out);
                    pending_space = false;
                }
                TagRole::ListItem => {
                    if closing {
                        push_newline(&mut out);
                    } else {
                        push_newline(&mut out);
                        out.push_str("- ");
                    }
                    pending_space = false;
                }
                TagRole::LineBreak => {
                    if !closing {
                        out.push('\n');
                    }
                    pending_space = false;
                }
                TagRole::Pre => {
                    if closing {
                        push_newline(&mut out);
                        out.push_str("```\n");
                        in_pre = false;
                    } else {
                        ensure_blank_line(&mut out);
                        out.push_str("```\n");
                        in_pre = true;
                    }
                    pending_space = false;
                }
                TagRole::Cell => {
                    if !closing {
                        pending_space = true;
                    }
                }
                TagRole::Inline => {}
            }
            i = tag_end;
        } else {
            // Text content: decode one entity or copy one character.
            let (decoded, next) = if bytes[i] == b'&' {
                decode_entity(html, i)
            } else {
                let ch = html[i..].chars().next().unwrap();
                (ch, i + ch.len_utf8())
            };
            if in_pre {
                out.push(decoded);
            } else if decoded.is_whitespace() {
                pending_space = true;
            } else {
                flush_space(&mut out, &mut pending_space, in_pre);
                out.push(decoded);
            }
            i = next;
        }
    }

    tidy(&out)
}

/// Parse a tag starting at `start` (which is '<'). Returns
/// (is_closing, lowercased_name, index just past '>'), or None if this is
/// not a tag.
fn parse_tag(html: &str, start: usize) -> Option<(bool, String, usize)> {
    let rest = &html[start + 1..];
    let (closing, rest_off) = match rest.strip_prefix('/') {
        Some(_) => (true, 1),
        None => (false, 0),
    };
    let name_start = start + 1 + rest_off;
    let name: String = html[name_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if name.is_empty() {
        // Not a real tag unless it's a doctype/processing blob — skip those.
        if rest.starts_with('!') || rest.starts_with('?') {
            let end = html[start..].find('>').map(|p| start + p + 1)?;
            return Some((false, String::new(), end));
        }
        return None;
    }
    let end = html[start..].find('>').map(|p| start + p + 1)?;
    Some((closing, name.to_ascii_lowercase(), end))
}

/// Skip everything through the matching `</name>`. Degrades to end-of-input
/// when unterminated, except `<head>`, which also ends at `<body` because
/// real-world documents omit `</head>` often enough to matter.
fn skip_container(html: &str, from: usize, name: &str) -> usize {
    let lower = html.to_ascii_lowercase();
    let close = format!("</{name}");
    if let Some(p) = lower[from..].find(&close) {
        let after = from + p;
        return lower[after..]
            .find('>')
            .map(|q| after + q + 1)
            .unwrap_or(html.len());
    }
    if name == "head" {
        if let Some(p) = lower[from..].find("<body") {
            return from + p;
        }
    }
    html.len()
}

/// Decode one HTML entity at `start` (which is '&'). Returns the decoded
/// char and the index after the entity; an unknown or malformed entity
/// yields the literal '&' and advances one byte.
fn decode_entity(html: &str, start: usize) -> (char, usize) {
    let rest = &html[start + 1..];
    let Some(semi) = rest.find(';') else {
        return ('&', start + 1);
    };
    // Entities are short; anything long is not an entity.
    if semi > 10 {
        return ('&', start + 1);
    }
    let body = &rest[..semi];
    let end = start + 1 + semi + 1;
    let decoded = match body {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => body.strip_prefix("#").and_then(|num| {
            let cp = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                num.parse::<u32>().ok()
            };
            cp.and_then(char::from_u32)
        }),
    };
    match decoded {
        Some(c) => (c, end),
        None => ('&', start + 1),
    }
}

/// Emit a pending collapsed space, unless we're at a line start.
fn flush_space(out: &mut String, pending: &mut bool, in_pre: bool) {
    if *pending && !in_pre {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push(' ');
        }
        *pending = false;
    }
}

/// Terminate the current line, if any.
fn push_newline(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Ensure the output ends with a blank line (or is empty).
fn ensure_blank_line(out: &mut String) {
    while !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
}

/// Final cleanup: strip trailing spaces per line and collapse 3+ blank
/// lines, so the markdown chunker sees tidy, stable input.
fn tidy(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0;
    for line in lines {
        if line.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
#[path = "documents_tests.rs"]
mod tests;
