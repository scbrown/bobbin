//! Tests for the HTML documents extractor (sidecar of `documents.rs`).
//!
//! The conversion is pure and deterministic, so most tests are direct
//! string assertions; the pipeline tests run the converted text through
//! the real markdown chunker to prove HTML lands as section chunks.

use super::*;
use std::path::PathBuf;

#[test]
fn detects_html_extensions_case_insensitively() {
    assert!(is_document_file(&PathBuf::from("docs/page.html")));
    assert!(is_document_file(&PathBuf::from("docs/PAGE.HTML")));
    assert!(is_document_file(&PathBuf::from("legacy/index.htm")));
    assert!(!is_document_file(&PathBuf::from("src/main.rs")));
    assert!(!is_document_file(&PathBuf::from("README.md")));
    assert!(!is_document_file(&PathBuf::from("no_extension")));
}

#[test]
fn headings_become_markdown_headings() {
    let html = "<h1>Guide</h1><p>Intro.</p><h2>Setup</h2><p>Steps.</p>";
    let text = html_to_markdownish(html);
    assert_eq!(text, "# Guide\n\nIntro.\n\n## Setup\n\nSteps.\n");
}

#[test]
fn script_style_head_and_comments_are_dropped() {
    let html = r#"<html><head><title>T</title><style>p{color:red}</style>
<script>var x = "<p>not text</p>";</script></head>
<body><p>Visible.</p><!-- hidden --><script>alert(1)</script></body></html>"#;
    let text = html_to_markdownish(html);
    assert_eq!(text, "Visible.\n");
}

#[test]
fn entities_are_decoded() {
    let html = "<p>a &amp; b &lt;c&gt; &quot;d&quot; &#65; &#x42; &nbsp;e</p>";
    let text = html_to_markdownish(html);
    assert_eq!(text, "a & b <c> \"d\" A B e\n");
}

#[test]
fn unknown_or_malformed_entities_stay_literal() {
    let text = html_to_markdownish("<p>&bogus; &noSemicolon and &#xZZ;</p>");
    assert_eq!(text, "&bogus; &noSemicolon and &#xZZ;\n");
}

#[test]
fn lists_become_bullets() {
    let html = "<ul><li>one</li><li>two</li></ul>";
    let text = html_to_markdownish(html);
    assert_eq!(text, "- one\n- two\n");
}

#[test]
fn pre_blocks_become_fenced_and_preserve_whitespace() {
    let html = "<p>Before</p><pre>fn main() {\n    body();\n}</pre><p>After</p>";
    let text = html_to_markdownish(html);
    assert_eq!(
        text,
        "Before\n\n```\nfn main() {\n    body();\n}\n```\nAfter\n"
    );
}

#[test]
fn inline_tags_do_not_split_words_and_whitespace_collapses() {
    let html = "<p>\n    Multi\n    line   <b>bo</b>ld and <a href=\"x\">a link</a>.\n  </p>";
    let text = html_to_markdownish(html);
    assert_eq!(text, "Multi line bold and a link.\n");
}

#[test]
fn table_cells_are_space_separated_rows() {
    let html = "<table><tr><th>k</th><th>v</th></tr><tr><td>a</td><td>1</td></tr></table>";
    let text = html_to_markdownish(html);
    assert_eq!(text, "k v\na 1\n");
}

#[test]
fn bare_less_than_is_kept_as_text() {
    let text = html_to_markdownish("<p>a < b and c</p>");
    assert_eq!(text, "a < b and c\n");
}

#[test]
fn head_without_closing_tag_still_yields_body_text() {
    let html = "<html><head><title>T</title><body><p>Body text.</p></body></html>";
    let text = html_to_markdownish(html);
    assert_eq!(text, "Body text.\n");
}

#[test]
fn empty_and_degenerate_html_yield_empty_text() {
    assert_eq!(html_to_markdownish(""), "");
    assert_eq!(html_to_markdownish("<html><head></head><body></body>"), "");
    assert_eq!(html_to_markdownish("<div><span></span></div>"), "");
    // Unterminated comment swallows the rest without panicking.
    assert_eq!(html_to_markdownish("<p>x</p><!-- oops"), "x\n");
    // Unterminated script likewise.
    assert_eq!(html_to_markdownish("<p>x</p><script>var a=1;"), "x\n");
}

// ── Pipeline: converted HTML through the real chunkers ────────────────

#[test]
fn converted_html_chunks_into_sections() {
    let html = "<html><head><title>ignored</title></head><body>\
        <h1>Guide</h1><p>Intro paragraph.</p>\
        <h2>Install</h2><p>Install steps here.</p>\
        <h2>Usage</h2><p>Usage notes here.</p></body></html>";
    let text = html_to_markdownish(html);

    let mut parser = crate::index::Parser::new().unwrap();
    let chunks = parser
        .parse_file(Path::new("docs/guide.html"), &text)
        .unwrap();

    // Heading structure survives: breadcrumb section names, html language.
    let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
    assert!(names.contains(&"Guide"), "names: {names:?}");
    assert!(names.contains(&"Guide > Install"), "names: {names:?}");
    assert!(names.contains(&"Guide > Usage"), "names: {names:?}");
    assert!(chunks.iter().all(|c| c.language == "html"), "{chunks:#?}");

    let install = chunks
        .iter()
        .find(|c| c.name.as_deref() == Some("Guide > Install"))
        .unwrap();
    assert!(install.content.contains("Install steps here."));
}

#[test]
fn headingless_html_still_chunks() {
    let text = html_to_markdownish("<body><p>Just one paragraph of text.</p></body>");
    let mut parser = crate::index::Parser::new().unwrap();
    let chunks = parser
        .parse_file(Path::new("docs/flat.html"), &text)
        .unwrap();
    assert!(!chunks.is_empty());
    assert!(chunks[0].content.contains("Just one paragraph"));
    assert!(chunks.iter().all(|c| c.language == "html"));
}

#[test]
fn extract_text_reads_file_and_converts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.html");
    std::fs::write(&path, "<h1>Title</h1><p>Body.</p>").unwrap();
    assert_eq!(extract_text(&path).unwrap(), "# Title\n\nBody.\n");

    // Missing file is an IO error (caller degrades to skip-with-warning).
    assert!(extract_text(&dir.path().join("missing.html")).is_err());
}
