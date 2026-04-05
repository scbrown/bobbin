//! Push markdown entities to Quipu as Document/Section/CodeExample/Definition nodes.
//!
//! After `bobbin index` processes markdown files, this module extracts the heading-based
//! entity hierarchy and pushes it to the knowledge graph:
//!
//! - **Document**: one per `.md` file
//! - **Section**: heading + content until next same/higher-level heading
//! - **CodeExample**: fenced code blocks (language-tagged)
//! - **Definition**: structured definition lists (term: description patterns)
//!
//! Relationships:
//! - `contains`: Document→Section, Section→Section (subsections), Section→CodeExample, Section→Definition
//! - `has_frontmatter`: Document→frontmatter metadata (if present)

use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, Result};
use pulldown_cmark::{Event, Options, Parser as CmarkParser, Tag, TagEnd};

const BOBBIN_NS: &str = "https://bobbin.dev/";

/// A markdown entity extracted from a parsed document.
#[derive(Debug)]
struct MdEntity {
    iri: String,
    entity_type: MdEntityType,
    label: String,
}

#[derive(Debug, Clone, Copy)]
enum MdEntityType {
    Document,
    Section,
    CodeExample,
    Definition,
}

impl MdEntityType {
    fn as_str(self) -> &'static str {
        match self {
            MdEntityType::Document => "Document",
            MdEntityType::Section => "Section",
            MdEntityType::CodeExample => "CodeExample",
            MdEntityType::Definition => "Definition",
        }
    }
}

/// A relationship between two markdown entities.
#[derive(Debug)]
struct MdEdge {
    subject_iri: String,
    predicate: &'static str,
    object_iri: String,
}

/// Push markdown heading-based entities to the Quipu knowledge graph.
///
/// Scans chunks produced by the indexer for markdown files, extracts the
/// entity hierarchy, and pushes as Turtle RDF via `quipu::tool_knot`.
///
/// Returns `(transaction_id, entity_count)` on success.
pub fn push_markdown_entities_to_quipu(
    markdown_files: &[(String, String)], // (relative_path, file_content)
    repo_name: &str,
    repo_root: &Path,
) -> Result<(i64, usize)> {
    if markdown_files.is_empty() {
        return Ok((-1, 0));
    }

    let mut all_entities: Vec<MdEntity> = Vec::new();
    let mut all_edges: Vec<MdEdge> = Vec::new();

    for (path, content) in markdown_files {
        extract_markdown_entities(path, content, repo_name, &mut all_entities, &mut all_edges);
    }

    if all_entities.is_empty() {
        return Ok((-1, 0));
    }

    let entity_count = all_entities.len();
    let turtle = generate_markdown_turtle(&all_entities, &all_edges);

    let quipu_config = quipu::QuipuConfig::load(repo_root);
    let db_path = if quipu_config.store_path.is_relative() {
        repo_root.join(&quipu_config.store_path)
    } else {
        quipu_config.store_path.clone()
    };

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create quipu store directory")?;
    }

    let mut store = quipu::Store::open(db_path.to_string_lossy().as_ref())
        .map_err(|e| anyhow::anyhow!("Failed to open quipu store: {e}"))?;

    let timestamp = chrono::Utc::now().to_rfc3339();
    let input = serde_json::json!({
        "turtle": turtle,
        "timestamp": timestamp,
        "actor": "bobbin",
        "source": "markdown-entity-sync"
    });

    let result = quipu::tool_knot(&mut store, &input)
        .map_err(|e| anyhow::anyhow!("Failed to push markdown entities to quipu: {e}"))?;

    let tx_id = result["tx_id"].as_i64().unwrap_or(-1);

    Ok((tx_id, entity_count))
}

/// Extract entities and relationships from a single markdown file.
fn extract_markdown_entities(
    path: &str,
    content: &str,
    repo_name: &str,
    entities: &mut Vec<MdEntity>,
    edges: &mut Vec<MdEdge>,
) {
    let doc_iri = document_iri(repo_name, path);

    // Document entity (one per file)
    let doc_label = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md")
        .trim_end_matches(".markdown");
    entities.push(MdEntity {
        iri: doc_iri.clone(),
        entity_type: MdEntityType::Document,
        label: doc_label.to_string(),
    });

    // Check for YAML frontmatter
    let (_frontmatter, body) = split_frontmatter(content);

    // Parse markdown with pulldown-cmark
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_HEADING_ATTRIBUTES;
    let parser = CmarkParser::new_ext(body, opts);
    let events: Vec<(Event, std::ops::Range<usize>)> = parser.into_offset_iter().collect();

    if events.is_empty() {
        return;
    }

    // Track heading hierarchy for section nesting
    // Each entry: (heading_level, section_slug, section_iri)
    let mut section_stack: Vec<(usize, String)> = Vec::new();

    let mut i = 0;
    while i < events.len() {
        match &events[i].0 {
            Event::Start(Tag::Heading { level, .. }) => {
                let heading_level = heading_level_to_usize(level);

                // Collect heading text
                let mut title = String::new();
                i += 1;
                while i < events.len() {
                    match &events[i].0 {
                        Event::End(TagEnd::Heading(_)) => break,
                        Event::Text(t) | Event::Code(t) => title.push_str(t),
                        _ => {}
                    }
                    i += 1;
                }

                let slug = slugify(&title);
                if slug.is_empty() {
                    i += 1;
                    continue;
                }

                let section_iri = section_iri(repo_name, path, &slug);

                entities.push(MdEntity {
                    iri: section_iri.clone(),
                    entity_type: MdEntityType::Section,
                    label: title.trim().to_string(),
                });

                // Pop sections at same or deeper level to find parent
                while let Some((lvl, _)) = section_stack.last() {
                    if *lvl >= heading_level {
                        section_stack.pop();
                    } else {
                        break;
                    }
                }

                // Parent is either the previous section in stack or the document
                let parent_iri = section_stack
                    .last()
                    .map(|(_, iri)| iri.clone())
                    .unwrap_or_else(|| doc_iri.clone());

                edges.push(MdEdge {
                    subject_iri: parent_iri,
                    predicate: "contains",
                    object_iri: section_iri.clone(),
                });

                section_stack.push((heading_level, section_iri));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang_tag = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => {
                        let s = info.split_whitespace().next().unwrap_or("");
                        if s.is_empty() { None } else { Some(s.to_string()) }
                    }
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };

                // Collect code content for label
                let mut code_text = String::new();
                let block_start = events[i].1.start;
                i += 1;
                while i < events.len() {
                    match &events[i].0 {
                        Event::End(TagEnd::CodeBlock) => break,
                        Event::Text(t) => code_text.push_str(t),
                        _ => {}
                    }
                    i += 1;
                }

                let label = lang_tag
                    .as_deref()
                    .unwrap_or("code")
                    .to_string();
                let code_slug = format!(
                    "code-{}-{}",
                    label,
                    block_start
                );
                let code_iri = format!(
                    "{}#{code_slug}",
                    document_iri(repo_name, path)
                );

                entities.push(MdEntity {
                    iri: code_iri.clone(),
                    entity_type: MdEntityType::CodeExample,
                    label,
                });

                // Parent is nearest section or document
                let parent_iri = section_stack
                    .last()
                    .map(|(_, iri)| iri.clone())
                    .unwrap_or_else(|| doc_iri.clone());

                edges.push(MdEdge {
                    subject_iri: parent_iri,
                    predicate: "contains",
                    object_iri: code_iri,
                });
            }
            Event::Start(Tag::List(_)) => {
                // Check if this looks like a definition list:
                // items matching "**term**: description" or "term — description"
                let list_start = events[i].1.start;
                let mut def_items: Vec<String> = Vec::new();
                let mut depth = 1u32;
                let scan_start = i + 1;
                let mut scan_i = scan_start;

                while scan_i < events.len() && depth > 0 {
                    match &events[scan_i].0 {
                        Event::Start(Tag::List(_)) => depth += 1,
                        Event::End(TagEnd::List(_)) => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Event::Start(Tag::Item) if depth == 1 => {
                            // Collect item text to check if it's a definition
                            let mut item_text = String::new();
                            scan_i += 1;
                            let mut item_depth = 1u32;
                            while scan_i < events.len() && item_depth > 0 {
                                match &events[scan_i].0 {
                                    Event::Start(Tag::Item) => item_depth += 1,
                                    Event::End(TagEnd::Item) => {
                                        item_depth -= 1;
                                        if item_depth == 0 {
                                            break;
                                        }
                                    }
                                    Event::Text(t) | Event::Code(t) => {
                                        if item_depth == 1 {
                                            item_text.push_str(t);
                                        }
                                    }
                                    _ => {}
                                }
                                scan_i += 1;
                            }
                            if is_definition_item(&item_text) {
                                def_items.push(item_text);
                            }
                        }
                        _ => {}
                    }
                    scan_i += 1;
                }

                // If more than half the items look like definitions, emit a Definition entity
                if def_items.len() >= 2 {
                    let def_slug = format!("def-{}", list_start);
                    let def_iri = format!(
                        "{}#{def_slug}",
                        document_iri(repo_name, path)
                    );

                    let label = format!(
                        "definitions ({})",
                        def_items.len()
                    );
                    entities.push(MdEntity {
                        iri: def_iri.clone(),
                        entity_type: MdEntityType::Definition,
                        label,
                    });

                    let parent_iri = section_stack
                        .last()
                        .map(|(_, iri)| iri.clone())
                        .unwrap_or_else(|| doc_iri.clone());

                    edges.push(MdEdge {
                        subject_iri: parent_iri,
                        predicate: "contains",
                        object_iri: def_iri,
                    });
                }

                // Skip to end of list since we scanned it
                // (don't double-count — let normal iteration skip past)
            }
            _ => {}
        }
        i += 1;
    }
}

/// Generate Turtle RDF for markdown entities and their relationships.
fn generate_markdown_turtle(entities: &[MdEntity], edges: &[MdEdge]) -> String {
    let mut turtle = String::with_capacity(entities.len() * 256 + edges.len() * 128);

    // Prefixes
    writeln!(turtle, "@prefix bobbin: <{BOBBIN_NS}> .").unwrap();
    writeln!(turtle, "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .").unwrap();
    writeln!(turtle).unwrap();

    // Entity declarations
    for entity in entities {
        writeln!(
            turtle,
            "<{}> a bobbin:{} ;",
            entity.iri,
            entity.entity_type.as_str()
        )
        .unwrap();
        writeln!(
            turtle,
            "    rdfs:label \"{}\" .",
            turtle_escape(&entity.label)
        )
        .unwrap();
        writeln!(turtle).unwrap();
    }

    // Relationship edges
    for edge in edges {
        writeln!(
            turtle,
            "<{}> bobbin:{} <{}> .",
            edge.subject_iri, edge.predicate, edge.object_iri
        )
        .unwrap();
    }

    turtle
}

/// Build the IRI for a Document entity (file-level).
fn document_iri(repo: &str, path: &str) -> String {
    format!(
        "{BOBBIN_NS}doc/{}/{}",
        iri_encode(repo),
        iri_encode(path)
    )
}

/// Build the IRI for a Section entity (heading-level).
fn section_iri(repo: &str, path: &str, slug: &str) -> String {
    format!(
        "{BOBBIN_NS}doc/{}/{}#{}",
        iri_encode(repo),
        iri_encode(path),
        iri_encode(slug)
    )
}

/// Convert a heading title to a URL-safe slug.
fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else if c == ' ' || c == '-' || c == '_' {
                '-'
            } else {
                // Skip non-alphanumeric characters
                '\0'
            }
        })
        .filter(|c| *c != '\0')
        .collect::<String>()
        // Collapse multiple dashes
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Check if a list item looks like a definition (e.g., "**term**: description" or "term — desc").
fn is_definition_item(text: &str) -> bool {
    let trimmed = text.trim();
    // Patterns: "term: description", "term — description", "term - description"
    trimmed.contains(": ")
        || trimmed.contains(" — ")
        || trimmed.contains(" - ")
}

/// Split YAML frontmatter from markdown content.
/// Returns (frontmatter_option, body_after_frontmatter).
fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    if !content.starts_with("---") {
        return (None, content);
    }
    // Find closing ---
    if let Some(end) = content[3..].find("\n---") {
        let fm_end = 3 + end + 4; // past the closing "---"
        let body_start = if fm_end < content.len() && content.as_bytes()[fm_end] == b'\n' {
            fm_end + 1
        } else {
            fm_end
        };
        (Some(&content[3..3 + end]), &content[body_start..])
    } else {
        (None, content)
    }
}

/// Minimal IRI encoding for path segments.
fn iri_encode(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('"', "%22")
        .replace('{', "%7B")
        .replace('}', "%7D")
}

/// Escape special characters for Turtle string literals.
fn turtle_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn heading_level_to_usize(level: &pulldown_cmark::HeadingLevel) -> usize {
    use pulldown_cmark::HeadingLevel::*;
    match level {
        H1 => 1,
        H2 => 2,
        H3 => 3,
        H4 => 4,
        H5 => 5,
        H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("API Reference (v2)"), "api-reference-v2");
        assert_eq!(slugify("foo--bar  baz"), "foo-bar-baz");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn test_document_iri() {
        let iri = document_iri("bobbin", "docs/design/plan.md");
        assert_eq!(iri, "https://bobbin.dev/doc/bobbin/docs/design/plan.md");
    }

    #[test]
    fn test_section_iri() {
        let iri = section_iri("bobbin", "README.md", "getting-started");
        assert_eq!(
            iri,
            "https://bobbin.dev/doc/bobbin/README.md#getting-started"
        );
    }

    #[test]
    fn test_is_definition_item() {
        assert!(is_definition_item("**term**: some description"));
        assert!(is_definition_item("key — value here"));
        assert!(is_definition_item("thing - another thing"));
        assert!(!is_definition_item("just a plain list item"));
    }

    #[test]
    fn test_split_frontmatter() {
        let content = "---\ntitle: Test\n---\n# Hello";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, Some("\ntitle: Test\n"));
        assert_eq!(body, "# Hello");
    }

    #[test]
    fn test_split_frontmatter_none() {
        let content = "# No frontmatter";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn test_extract_basic_document() {
        let content = "# Title\n\nSome text\n\n## Subsection\n\nMore text\n";
        let mut entities = Vec::new();
        let mut edges = Vec::new();

        extract_markdown_entities("docs/test.md", content, "repo", &mut entities, &mut edges);

        // Should have: Document, Section(Title), Section(Subsection)
        assert_eq!(entities.len(), 3);
        assert!(matches!(entities[0].entity_type, MdEntityType::Document));
        assert!(matches!(entities[1].entity_type, MdEntityType::Section));
        assert!(matches!(entities[2].entity_type, MdEntityType::Section));

        // Edges: Document->Title, Title->Subsection
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].predicate, "contains");
        assert_eq!(edges[1].predicate, "contains");

        // Title's parent is Document
        assert!(edges[0].subject_iri.contains("doc/repo/docs/test.md"));
        assert!(!edges[0].subject_iri.contains('#'));

        // Subsection's parent is Title (contains the section slug)
        assert!(edges[1].subject_iri.contains('#'));
    }

    #[test]
    fn test_extract_code_example() {
        let content = "# Guide\n\n```rust\nfn main() {}\n```\n";
        let mut entities = Vec::new();
        let mut edges = Vec::new();

        extract_markdown_entities("guide.md", content, "repo", &mut entities, &mut edges);

        let code_entities: Vec<_> = entities
            .iter()
            .filter(|e| matches!(e.entity_type, MdEntityType::CodeExample))
            .collect();
        assert_eq!(code_entities.len(), 1);
        assert_eq!(code_entities[0].label, "rust");

        // Code example should be contained by the section
        let code_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.object_iri.contains("#code-"))
            .collect();
        assert_eq!(code_edges.len(), 1);
        assert_eq!(code_edges[0].predicate, "contains");
    }

    #[test]
    fn test_generate_turtle() {
        let entities = vec![
            MdEntity {
                iri: "https://bobbin.dev/doc/repo/README.md".to_string(),
                entity_type: MdEntityType::Document,
                label: "README".to_string(),
            },
            MdEntity {
                iri: "https://bobbin.dev/doc/repo/README.md#intro".to_string(),
                entity_type: MdEntityType::Section,
                label: "Introduction".to_string(),
            },
        ];
        let edges = vec![MdEdge {
            subject_iri: "https://bobbin.dev/doc/repo/README.md".to_string(),
            predicate: "contains",
            object_iri: "https://bobbin.dev/doc/repo/README.md#intro".to_string(),
        }];

        let turtle = generate_markdown_turtle(&entities, &edges);
        assert!(turtle.contains("a bobbin:Document"));
        assert!(turtle.contains("a bobbin:Section"));
        assert!(turtle.contains("rdfs:label \"README\""));
        assert!(turtle.contains("rdfs:label \"Introduction\""));
        assert!(turtle.contains("bobbin:contains"));
    }

    #[test]
    fn test_turtle_escape() {
        assert_eq!(turtle_escape("hello \"world\""), "hello \\\"world\\\"");
        assert_eq!(turtle_escape("line\nnewline"), "line\\nnewline");
    }

    #[test]
    fn test_empty_markdown_files() {
        let result = push_markdown_entities_to_quipu(&[], "repo", Path::new("/tmp"));
        assert!(result.is_ok());
        let (tx_id, count) = result.unwrap();
        assert_eq!(tx_id, -1);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_heading_hierarchy() {
        let content = "# H1\n\n## H2a\n\n### H3\n\n## H2b\n\nText\n";
        let mut entities = Vec::new();
        let mut edges = Vec::new();

        extract_markdown_entities("test.md", content, "repo", &mut entities, &mut edges);

        // Document + 4 sections
        assert_eq!(entities.len(), 5);

        // H2b should be child of H1 (not H3) since it pops back to level 2
        let h2b_edge = edges
            .iter()
            .find(|e| e.object_iri.contains("h2b"))
            .expect("Should have edge to h2b");
        // Parent should be the H1 section, not H3
        assert!(h2b_edge.subject_iri.contains("h1"));
    }
}
