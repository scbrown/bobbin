use anyhow::Result;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser as CmarkParser, Tag, TagEnd};
use std::path::Path;
use tree_sitter::{Language, Node};

use crate::types::{Chunk, ChunkEdge, ChunkEdgeType, ChunkType, ImportEdge, RawImport};

/// Default lines per line-based chunk (unknown languages). Overridable via
/// `[index].chunk_size`.
pub const DEFAULT_CHUNK_SIZE: usize = 50;
/// Default overlapping lines between consecutive line-based chunks.
/// Overridable via `[index].chunk_overlap`.
pub const DEFAULT_CHUNK_OVERLAP: usize = 10;

/// Estimate the token count of a text fragment for embedder-window clamping.
/// Uses the standard `chars / 4` heuristic (≈4 chars/token) — deterministic,
/// needs no tokenizer files, and is accurate enough to keep chunks under the
/// model window. Matches the budget estimator used in context assembly.
fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Parses source code using tree-sitter to extract semantic chunks
pub struct Parser {
    rust_parser: tree_sitter::Parser,
    typescript_parser: tree_sitter::Parser,
    python_parser: tree_sitter::Parser,
    go_parser: tree_sitter::Parser,
    java_parser: tree_sitter::Parser,
    cpp_parser: tree_sitter::Parser,
    /// Lines per line-based chunk.
    chunk_size: usize,
    /// Overlapping lines between consecutive line-based chunks.
    chunk_overlap: usize,
    /// Embedder token window. When > 0, line/markdown chunks are split so none
    /// exceeds this many estimated tokens — preventing silent embedding
    /// truncation. 0 = no clamp (window unknown / API backend).
    max_chunk_tokens: usize,
}

impl Parser {
    /// Create a new parser with support for multiple languages
    pub fn new() -> Result<Self> {
        Ok(Self {
            rust_parser: create_parser(tree_sitter_rust::LANGUAGE.into())?,
            typescript_parser: create_parser(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?,
            python_parser: create_parser(tree_sitter_python::LANGUAGE.into())?,
            go_parser: create_parser(tree_sitter_go::LANGUAGE.into())?,
            java_parser: create_parser(tree_sitter_java::LANGUAGE.into())?,
            cpp_parser: create_parser(tree_sitter_cpp::LANGUAGE.into())?,
            chunk_size: DEFAULT_CHUNK_SIZE,
            chunk_overlap: DEFAULT_CHUNK_OVERLAP,
            max_chunk_tokens: 0,
        })
    }

    /// Configure line-chunk sizing and the embedder token window for clamping.
    /// `max_chunk_tokens` is typically the embedder's `max_seq` (0 to disable
    /// clamping). Invalid pairings are sanitized: `chunk_size` is forced to at
    /// least 1, and `chunk_overlap` is capped below `chunk_size` to guarantee
    /// forward progress.
    pub fn with_chunking(
        mut self,
        chunk_size: usize,
        chunk_overlap: usize,
        max_chunk_tokens: usize,
    ) -> Self {
        self.chunk_size = chunk_size.max(1);
        self.chunk_overlap = chunk_overlap.min(self.chunk_size - 1);
        self.max_chunk_tokens = max_chunk_tokens;
        self
    }

    /// Parse a file and extract semantic chunks
    pub fn parse_file(&mut self, path: &Path, content: &str) -> Result<Vec<Chunk>> {
        let language = detect_language(path);

        let Some(lang) = language.as_deref() else {
            // Unknown language - fall back to line-based chunking
            return Ok(self.chunk_by_lines(path, content));
        };

        if lang == "markdown" {
            // Check if this is an archive record (has schema: in frontmatter)
            if has_schema_frontmatter(content) {
                return Ok(self.chunk_transcript(path, content));
            }
            return Ok(self.chunk_markdown(path, content));
        }

        if lang == "html" {
            // Content arrives already converted to markdown-ish text by
            // index::documents (upstream, like PDF extraction), so the
            // markdown chunker recovers heading structure; retag the chunks
            // with the real source language so search filters stay honest.
            let mut chunks = self.chunk_markdown(path, content);
            for chunk in &mut chunks {
                chunk.language = "html".to_string();
            }
            return Ok(chunks);
        }

        let parser = match lang {
            "rust" => &mut self.rust_parser,
            "typescript" | "tsx" => &mut self.typescript_parser,
            "python" => &mut self.python_parser,
            "go" => &mut self.go_parser,
            "java" => &mut self.java_parser,
            "cpp" => &mut self.cpp_parser,
            _ => return Ok(self.chunk_by_lines(path, content)),
        };

        let Some(tree) = parser.parse(content, None) else {
            // Gracefully handle parse errors - fall back to line-based chunking
            return Ok(self.chunk_by_lines(path, content));
        };

        let mut chunks = Vec::new();
        let root = tree.root_node();
        self.extract_chunks(&root, content, path, lang, &mut chunks);

        // If no semantic chunks found, fall back to line-based
        if chunks.is_empty() {
            return Ok(self.chunk_by_lines(path, content));
        }

        Ok(chunks)
    }

    /// Extract import statements from a file using tree-sitter
    pub fn extract_imports(&mut self, path: &Path, content: &str) -> Vec<ImportEdge> {
        let language = detect_language(path);

        let Some(lang) = language.as_deref() else {
            return Vec::new();
        };

        // Markdown and unknown languages don't have imports
        if lang == "markdown" {
            return Vec::new();
        }

        let parser = match lang {
            "rust" => &mut self.rust_parser,
            "typescript" | "tsx" => &mut self.typescript_parser,
            "python" => &mut self.python_parser,
            "go" => &mut self.go_parser,
            "java" => &mut self.java_parser,
            "cpp" => &mut self.cpp_parser,
            _ => return Vec::new(),
        };

        let Some(tree) = parser.parse(content, None) else {
            return Vec::new();
        };

        let file_path = path.to_string_lossy().to_string();
        let root = tree.root_node();
        let mut imports = Vec::new();
        collect_imports(&root, content, &file_path, lang, &mut imports);
        imports
    }

    /// Extract raw import statements from a file using tree-sitter.
    ///
    /// Returns `RawImport` structs with the verbatim statement text, extracted
    /// import path, and categorized dep_type for all 6 language families.
    pub fn extract_raw_imports(&mut self, path: &Path, content: &str) -> Vec<RawImport> {
        let language = detect_language(path);

        let Some(lang) = language.as_deref() else {
            return Vec::new();
        };

        if lang == "markdown" {
            return Vec::new();
        }

        let parser = match lang {
            "rust" => &mut self.rust_parser,
            "typescript" | "tsx" => &mut self.typescript_parser,
            "python" => &mut self.python_parser,
            "go" => &mut self.go_parser,
            "java" => &mut self.java_parser,
            "cpp" => &mut self.cpp_parser,
            _ => return Vec::new(),
        };

        let Some(tree) = parser.parse(content, None) else {
            return Vec::new();
        };

        let root = tree.root_node();
        let mut imports = Vec::new();
        collect_raw_imports(&root, content, lang, &mut imports);
        imports
    }

    /// Extract typed chunk-to-chunk edges from parsed source.
    ///
    /// Uses tree-sitter AST to find structural relationships:
    /// - Rust: `impl Trait for Struct` → Implements edge, `impl Struct` → ImplFor edge
    /// - Python: `class Foo(Bar)` → Extends edge
    /// - Java/TS: `class Foo extends Bar` → Extends edge
    /// - Test inference: `test_foo` / `Foo_test` → Tests edge to `foo` / `Foo` chunk
    pub fn extract_chunk_edges(
        &mut self,
        path: &Path,
        content: &str,
        chunks: &[Chunk],
    ) -> Vec<ChunkEdge> {
        let file_path = path.to_string_lossy().to_string();

        // Structural edges (next_chunk / part_of) and name-convention Tests
        // edges are language-agnostic — derived from the chunk list alone,
        // for every file type.
        let mut structural = crate::index::structural::extract_structural_edges(&file_path, chunks);
        structural.extend(crate::index::test_edges::extract_test_edges(
            &file_path, chunks,
        ));

        let language = detect_language(path);
        let Some(lang) = language.as_deref() else {
            return structural;
        };

        let parser = match lang {
            "rust" => &mut self.rust_parser,
            "typescript" | "tsx" => &mut self.typescript_parser,
            "python" => &mut self.python_parser,
            "go" => &mut self.go_parser,
            "java" => &mut self.java_parser,
            "cpp" => &mut self.cpp_parser,
            _ => return structural,
        };

        let Some(tree) = parser.parse(content, None) else {
            return structural;
        };

        // Build a lookup from chunk name → chunk (for resolving targets)
        let chunk_by_name: std::collections::HashMap<&str, &Chunk> = chunks
            .iter()
            .filter_map(|c| c.name.as_deref().map(|n| (n, c)))
            .collect();

        let root = tree.root_node();
        let mut edges = Vec::new();
        collect_chunk_edges(
            &root,
            content,
            &file_path,
            lang,
            chunks,
            &chunk_by_name,
            &mut edges,
        );
        structural.extend(edges);
        structural
    }

    /// Extract markdown chunks using pulldown-cmark for semantic parsing.
    ///
    /// Strategy:
    /// 1. Extract YAML frontmatter as a Doc chunk if present
    /// 2. Walk pulldown-cmark events to identify headings, tables, and code blocks
    /// 3. Split content at heading boundaries into Section chunks
    /// 4. Emit standalone Table and CodeBlock chunks for those elements
    ///    while also including them in the parent section's content
    fn chunk_markdown(&self, path: &Path, content: &str) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let file_path = path.to_string_lossy().to_string();

        // 1. Extract YAML frontmatter (--- delimited)
        let (frontmatter, body, body_start_line) = extract_frontmatter(content);

        if let Some(fm) = frontmatter {
            let fm_end_line = body_start_line.saturating_sub(1).max(1);
            chunks.push(Chunk {
                id: generate_chunk_id(path, 1, fm_end_line as u32),
                file_path: file_path.clone(),
                chunk_type: ChunkType::Doc,
                name: Some("Frontmatter".to_string()),
                start_line: 1,
                end_line: fm_end_line as u32,
                content: fm,
                language: "markdown".to_string(),
                tags: String::new(),
            });
        }

        // 2. Parse markdown body with pulldown-cmark
        let opts = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_HEADING_ATTRIBUTES;
        let parser = CmarkParser::new_ext(body, opts);

        // Collect events with byte offsets into body
        let events: Vec<(Event, std::ops::Range<usize>)> = parser.into_offset_iter().collect();

        if events.is_empty() {
            if chunks.is_empty() {
                return self.chunk_by_lines(path, content);
            }
            return chunks;
        }

        // 3. Find heading boundaries and standalone blocks
        let mut sections: Vec<MarkdownSection> = Vec::new();
        let mut standalone_blocks: Vec<StandaloneBlock> = Vec::new();
        let mut header_stack: Vec<(usize, String)> = Vec::new();

        let mut i = 0;
        while i < events.len() {
            match &events[i].0 {
                Event::Start(Tag::Heading { level, .. }) => {
                    let heading_level = heading_level_to_usize(level);
                    let section_start = events[i].1.start;

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

                    // Update header stack
                    while let Some((last_level, _)) = header_stack.last() {
                        if *last_level >= heading_level {
                            header_stack.pop();
                        } else {
                            break;
                        }
                    }
                    header_stack.push((heading_level, title.trim().to_string()));

                    let full_name = header_stack
                        .iter()
                        .map(|(_, t)| t.as_str())
                        .collect::<Vec<_>>()
                        .join(" > ");

                    sections.push(MarkdownSection {
                        name: full_name,
                        body_offset: section_start,
                    });
                }
                Event::Start(Tag::Table(_)) => {
                    let table_start = events[i].1.start;
                    let mut table_end = events[i].1.end;
                    i += 1;
                    while i < events.len() {
                        match &events[i].0 {
                            Event::End(TagEnd::Table) => {
                                table_end = events[i].1.end;
                                break;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    standalone_blocks.push(StandaloneBlock {
                        chunk_type: ChunkType::Table,
                        body_offset_start: table_start,
                        body_offset_end: table_end,
                        name: None,
                    });
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    let cb_start = events[i].1.start;
                    let mut cb_end = events[i].1.end;
                    let lang_tag = match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(info) => {
                            let s = info.split_whitespace().next().unwrap_or("");
                            if s.is_empty() {
                                None
                            } else {
                                Some(s.to_string())
                            }
                        }
                        pulldown_cmark::CodeBlockKind::Indented => None,
                    };
                    i += 1;
                    while i < events.len() {
                        match &events[i].0 {
                            Event::End(TagEnd::CodeBlock) => {
                                cb_end = events[i].1.end;
                                break;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    standalone_blocks.push(StandaloneBlock {
                        chunk_type: ChunkType::CodeBlock,
                        body_offset_start: cb_start,
                        body_offset_end: cb_end,
                        name: lang_tag,
                    });
                }
                _ => {}
            }
            i += 1;
        }

        // 4. Emit section chunks at heading boundaries
        let line_offset = body_start_line.saturating_sub(1);

        if sections.is_empty() {
            // No headings found — check for preamble content
            let trimmed = body.trim();
            if !trimmed.is_empty() {
                let start_line = (line_offset + 1) as u32;
                let end_line = (line_offset + body.lines().count()) as u32;
                chunks.push(Chunk {
                    id: generate_chunk_id(path, start_line, end_line),
                    file_path: file_path.clone(),
                    chunk_type: ChunkType::Doc,
                    name: Some("Preamble".to_string()),
                    start_line,
                    end_line,
                    content: body.to_string(),
                    language: "markdown".to_string(),
                    tags: String::new(),
                });
            }
        } else {
            // Preamble before first heading
            if sections[0].body_offset > 0 {
                let pre = &body[..sections[0].body_offset];
                if !pre.trim().is_empty() {
                    let start_line = (line_offset + 1) as u32;
                    let end_line = (line_offset
                        + byte_offset_to_line_in(body, sections[0].body_offset))
                        as u32;
                    chunks.push(Chunk {
                        id: generate_chunk_id(path, start_line, end_line),
                        file_path: file_path.clone(),
                        chunk_type: ChunkType::Doc,
                        name: Some("Preamble".to_string()),
                        start_line,
                        end_line,
                        content: pre.to_string(),
                        language: "markdown".to_string(),
                        tags: String::new(),
                    });
                }
            }

            // Section chunks
            for si in 0..sections.len() {
                let sec = &sections[si];
                let sec_start = sec.body_offset;
                let sec_end = if si + 1 < sections.len() {
                    sections[si + 1].body_offset
                } else {
                    body.len()
                };
                let section_content = &body[sec_start..sec_end];

                let start_line = (line_offset + byte_offset_to_line_in(body, sec_start)) as u32;
                let end_line = (line_offset + byte_offset_to_line_in(body, sec_end)) as u32;

                chunks.push(Chunk {
                    id: generate_chunk_id(path, start_line, end_line),
                    file_path: file_path.clone(),
                    chunk_type: ChunkType::Section,
                    name: Some(sec.name.clone()),
                    start_line,
                    end_line,
                    content: section_content.to_string(),
                    language: "markdown".to_string(),
                    tags: String::new(),
                });
            }
        }

        // 5. Emit standalone table and code_block chunks
        for block in &standalone_blocks {
            let block_content = &body[block.body_offset_start..block.body_offset_end];
            let start_line =
                (line_offset + byte_offset_to_line_in(body, block.body_offset_start)) as u32;
            let end_line =
                (line_offset + byte_offset_to_line_in(body, block.body_offset_end)) as u32;

            let name = match block.chunk_type {
                ChunkType::Table => {
                    // Try to use preceding section heading
                    let parent_section = sections
                        .iter()
                        .rev()
                        .find(|s| s.body_offset <= block.body_offset_start);
                    parent_section.map(|s| format!("{} (table)", s.name))
                }
                ChunkType::CodeBlock => block.name.as_ref().map(|lang| format!("code: {}", lang)),
                _ => None,
            };

            chunks.push(Chunk {
                id: generate_chunk_id(path, start_line, end_line),
                file_path: file_path.clone(),
                chunk_type: block.chunk_type,
                name,
                start_line,
                end_line,
                content: block_content.to_string(),
                language: "markdown".to_string(),
                tags: String::new(),
            });
        }

        if chunks.is_empty() {
            return self.chunk_by_lines(path, content);
        }

        chunks
    }

    /// Extract semantic chunks from a syntax tree
    fn extract_chunks(
        &self,
        node: &Node,
        content: &str,
        path: &Path,
        language: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        let chunk_type = self.node_to_chunk_type(node, language);

        if let Some(chunk_type) = chunk_type {
            let name = self.extract_name(node, content, language);
            let start_line = node.start_position().row as u32 + 1;
            let end_line = node.end_position().row as u32 + 1;
            let node_content = &content[node.byte_range()];

            chunks.push(Chunk {
                id: generate_chunk_id(path, start_line, end_line),
                file_path: path.to_string_lossy().to_string(),
                chunk_type,
                name,
                start_line,
                end_line,
                content: node_content.to_string(),
                language: language.to_string(),
                tags: String::new(),
            });
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_chunks(&child, content, path, language, chunks);
        }
    }

    /// Map a tree-sitter node to a chunk type
    fn node_to_chunk_type(&self, node: &Node, language: &str) -> Option<ChunkType> {
        let kind = node.kind();

        match language {
            "rust" => match kind {
                "function_item" => Some(ChunkType::Function),
                "impl_item" => Some(ChunkType::Impl),
                "struct_item" => Some(ChunkType::Struct),
                "enum_item" => Some(ChunkType::Enum),
                "trait_item" => Some(ChunkType::Trait),
                "mod_item" => Some(ChunkType::Module),
                _ => None,
            },
            "typescript" | "tsx" => match kind {
                "function_declaration" | "arrow_function" => Some(ChunkType::Function),
                "method_definition" => Some(ChunkType::Method),
                "class_declaration" => Some(ChunkType::Class),
                "interface_declaration" => Some(ChunkType::Interface),
                _ => None,
            },
            "python" => match kind {
                "function_definition" => Some(ChunkType::Function),
                "class_definition" => Some(ChunkType::Class),
                _ => None,
            },
            "go" => match kind {
                "function_declaration" => Some(ChunkType::Function),
                "method_declaration" => Some(ChunkType::Method),
                "type_declaration" => Some(ChunkType::Struct), // covers struct, interface
                _ => None,
            },
            "java" => match kind {
                "method_declaration" => Some(ChunkType::Method),
                "constructor_declaration" => Some(ChunkType::Method),
                "class_declaration" => Some(ChunkType::Class),
                "interface_declaration" => Some(ChunkType::Interface),
                "enum_declaration" => Some(ChunkType::Enum),
                _ => None,
            },
            "cpp" => match kind {
                "function_definition" => Some(ChunkType::Function),
                "class_specifier" => Some(ChunkType::Class),
                "struct_specifier" => Some(ChunkType::Struct),
                "enum_specifier" => Some(ChunkType::Enum),
                _ => None,
            },
            _ => None,
        }
    }

    /// Extract the name of a semantic unit
    fn extract_name(&self, node: &Node, content: &str, language: &str) -> Option<String> {
        match language {
            "rust" | "typescript" | "tsx" | "python" | "java" => node
                .child_by_field_name("name")
                .map(|n| content[n.byte_range()].to_string()),
            "go" => {
                // Go functions use "name", methods use "name" too
                // type_declaration has a nested type_spec with name
                if node.kind() == "type_declaration" {
                    // Find the type_spec child and get its name
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "type_spec" {
                            return child
                                .child_by_field_name("name")
                                .map(|n| content[n.byte_range()].to_string());
                        }
                    }
                    None
                } else {
                    node.child_by_field_name("name")
                        .map(|n| content[n.byte_range()].to_string())
                }
            }
            "cpp" => {
                // C++ class/struct use "name", functions use "declarator"
                if let Some(name_node) = node.child_by_field_name("name") {
                    return Some(content[name_node.byte_range()].to_string());
                }
                // For function_definition, the name is inside the declarator
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    // The declarator can be a function_declarator, get its declarator field
                    if let Some(inner) = declarator.child_by_field_name("declarator") {
                        return Some(content[inner.byte_range()].to_string());
                    }
                    // Or the declarator itself might be the identifier
                    if declarator.kind() == "identifier" {
                        return Some(content[declarator.byte_range()].to_string());
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Chunk an archive record (any file with `schema:` in frontmatter).
    ///
    /// One record = one chunk (preserving atomic record content).
    /// If body exceeds 100 lines, split at paragraph breaks.
    fn chunk_transcript(&self, path: &Path, content: &str) -> Vec<Chunk> {
        let (fm, body, body_start_line) = extract_frontmatter(content);

        let body = body.trim();
        if body.is_empty() {
            return vec![];
        }

        // Extract record ID from frontmatter for the chunk name
        let record_name = fm.as_deref().and_then(|fm| {
            fm.lines()
                .find(|l| l.trim().starts_with("id:"))
                .map(|l| l.trim().trim_start_matches("id:").trim().to_string())
        });

        let lines: Vec<&str> = body.lines().collect();

        if lines.len() <= 100 {
            // Single chunk — the common case
            let end_line = body_start_line + lines.len() - 1;
            return vec![Chunk {
                id: generate_chunk_id(path, body_start_line as u32, end_line as u32),
                file_path: path.to_string_lossy().to_string(),
                chunk_type: ChunkType::Section,
                name: record_name,
                start_line: body_start_line as u32,
                end_line: end_line as u32,
                content: body.to_string(),
                language: "archive".to_string(),
                tags: String::new(),
            }];
        }

        // Overflow: split at paragraph breaks (empty lines)
        let mut chunks = Vec::new();
        let mut chunk_start = 0;
        let overlap = 10;

        while chunk_start < lines.len() {
            let chunk_end = if chunk_start + 100 >= lines.len() {
                lines.len()
            } else {
                // Find nearest paragraph break within 80-100 line range
                let search_start = chunk_start + 80;
                let search_end = (chunk_start + 100).min(lines.len());
                let mut break_at = search_end;
                for i in search_start..search_end {
                    if lines[i].trim().is_empty() {
                        break_at = i;
                        break;
                    }
                }
                break_at
            };

            let chunk_content = lines[chunk_start..chunk_end].join("\n");
            let sl = body_start_line + chunk_start;
            let el = body_start_line + chunk_end - 1;

            let name = record_name.as_ref().map(|n| {
                if chunks.is_empty() {
                    n.clone()
                } else {
                    format!("{}:{}", n, chunks.len())
                }
            });

            chunks.push(Chunk {
                id: generate_chunk_id(path, sl as u32, el as u32),
                file_path: path.to_string_lossy().to_string(),
                chunk_type: ChunkType::Section,
                name,
                start_line: sl as u32,
                end_line: el as u32,
                content: chunk_content,
                language: "archive".to_string(),
                tags: String::new(),
            });

            if chunk_end >= lines.len() {
                break;
            }
            chunk_start = chunk_end.saturating_sub(overlap);
        }

        chunks
    }

    /// Fall back to line-based chunking for unknown languages.
    ///
    /// Chunk size and overlap come from `[index]` config. When `max_chunk_tokens`
    /// is set (the embedder window), each chunk is shrunk to fit so the embedder
    /// never silently truncates it — a 50-line dense-code chunk can otherwise
    /// blow past a 256/512-token window.
    fn chunk_by_lines(&self, path: &Path, content: &str) -> Vec<Chunk> {
        let lines: Vec<&str> = content.lines().collect();
        let overlap = self.chunk_overlap;
        let language = detect_language(path).unwrap_or_else(|| "unknown".to_string());

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < lines.len() {
            let mut end = (start + self.chunk_size).min(lines.len());

            // Clamp to the embedder token window: shrink the chunk until it fits.
            // Always keep at least one line so we make forward progress (a lone
            // over-window line can't be split without breaking it; the embedder
            // truncates that pathological case as before).
            if self.max_chunk_tokens > 0 {
                while end > start + 1
                    && estimate_tokens(&lines[start..end].join("\n")) > self.max_chunk_tokens
                {
                    end -= 1;
                }
            }

            chunks.push(Chunk {
                id: generate_chunk_id(path, start as u32 + 1, end as u32),
                file_path: path.to_string_lossy().to_string(),
                chunk_type: ChunkType::Other,
                name: None,
                start_line: start as u32 + 1,
                end_line: end as u32,
                content: lines[start..end].join("\n"),
                language: language.clone(),
                tags: String::new(),
            });

            if end >= lines.len() {
                break;
            }
            // Advance with overlap, but never backwards or in place — the clamp
            // can make a chunk shorter than `overlap`.
            start = end.saturating_sub(overlap).max(start + 1);
        }

        chunks
    }
}

/// A heading-delimited section in a markdown document
struct MarkdownSection {
    name: String,
    body_offset: usize,
}

/// A standalone block (table or code block) extracted from markdown
struct StandaloneBlock {
    chunk_type: ChunkType,
    body_offset_start: usize,
    body_offset_end: usize,
    name: Option<String>,
}

fn heading_level_to_usize(level: &HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Extract YAML frontmatter from markdown content.
/// Returns (frontmatter_content, body, body_start_line).
fn extract_frontmatter(content: &str) -> (Option<String>, &str, usize) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (None, content, 1);
    }

    // Find the leading whitespace/newlines before ---
    let leading = content.len() - trimmed.len();
    let after_first_fence = leading + 3;

    // Find closing ---
    if let Some(close_pos) = trimmed[3..].find("\n---") {
        let fm_end = after_first_fence + close_pos;
        let fm_content = content[after_first_fence..fm_end].trim().to_string();

        // Body starts after closing --- and its newline
        let body_start = fm_end + 4; // skip \n---
        let body_start = if body_start < content.len() && content.as_bytes()[body_start] == b'\n' {
            body_start + 1
        } else {
            body_start
        };

        let body_start_line = content[..body_start].lines().count() + 1;
        let body = &content[body_start..];
        (Some(fm_content), body, body_start_line)
    } else {
        (None, content, 1)
    }
}

/// Convert a byte offset in a string to a 1-based line number
fn byte_offset_to_line_in(content: &str, offset: usize) -> usize {
    if offset >= content.len() {
        return content.lines().count().max(1);
    }
    content[..offset].chars().filter(|&c| c == '\n').count() + 1
}

fn create_parser(language: Language) -> Result<tree_sitter::Parser> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language)?;
    Ok(parser)
}

fn detect_language(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some("rust".to_string()),
        "ts" => Some("typescript".to_string()),
        "tsx" => Some("tsx".to_string()),
        "js" | "jsx" | "mjs" => Some("javascript".to_string()),
        "py" => Some("python".to_string()),
        "go" => Some("go".to_string()),
        "java" => Some("java".to_string()),
        "c" | "h" => Some("c".to_string()),
        "cpp" | "cc" | "hpp" => Some("cpp".to_string()),
        "md" => Some("markdown".to_string()),
        // IaC / config / scripts. No tree-sitter parser is wired for these, so they
        // fall through to line-based chunking in parse_file (the `_ => chunk_by_lines`
        // arm). Tagging them here — rather than letting them fall through to `None`
        // and chunk as "unknown" — makes them searchable AND surfaces them as a real
        // language in /status instead of a "unknown" blob (bobbin-ywzq8).
        "yml" | "yaml" => Some("yaml".to_string()),
        "j2" => Some("jinja".to_string()),
        "tf" => Some("terraform".to_string()),
        "sh" => Some("shell".to_string()),
        // PDF text (extracted upstream by index::multimodal). Falls through to
        // line-based chunking in parse_file, tagged with language = "pdf".
        "pdf" => Some("pdf".to_string()),
        // HTML text (converted upstream by index::documents to markdown-ish
        // text). Routed to the markdown chunker in parse_file, tagged "html".
        "html" | "htm" => Some("html".to_string()),
        _ => None,
    }
}

/// Walk a tree-sitter AST and collect import specifiers
fn collect_imports(
    node: &Node,
    content: &str,
    file_path: &str,
    language: &str,
    imports: &mut Vec<ImportEdge>,
) {
    match language {
        "rust" => collect_rust_imports(node, content, file_path, imports),
        "typescript" | "tsx" => collect_ts_imports(node, content, file_path, language, imports),
        "python" => collect_python_imports(node, content, file_path, imports),
        "go" => collect_go_imports(node, content, file_path, imports),
        "java" => collect_java_imports(node, content, file_path, imports),
        "cpp" => collect_cpp_imports(node, content, file_path, imports),
        _ => {}
    }
}

fn collect_rust_imports(
    node: &Node,
    content: &str,
    file_path: &str,
    imports: &mut Vec<ImportEdge>,
) {
    // Rust: use_declaration nodes (e.g., `use std::path::Path;`, `use crate::types::Chunk;`)
    // Also: extern_crate_item, mod_item with path
    if node.kind() == "use_declaration" {
        // The argument child holds the path (e.g., `std::path::Path`)
        if let Some(arg) = node.child_by_field_name("argument") {
            let specifier = content[arg.byte_range()].to_string();
            imports.push(ImportEdge {
                source_file: file_path.to_string(),
                import_specifier: specifier,
                resolved_path: None,
                language: "rust".to_string(),
            });
        }
    } else if node.kind() == "extern_crate_declaration" {
        if let Some(name) = node.child_by_field_name("name") {
            let specifier = content[name.byte_range()].to_string();
            imports.push(ImportEdge {
                source_file: file_path.to_string(),
                import_specifier: specifier,
                resolved_path: None,
                language: "rust".to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_imports(&child, content, file_path, imports);
    }
}

fn collect_ts_imports(
    node: &Node,
    content: &str,
    file_path: &str,
    language: &str,
    imports: &mut Vec<ImportEdge>,
) {
    // TypeScript/JavaScript: import_statement nodes
    // e.g., `import { Foo } from './bar'`, `import * as x from 'lib'`
    // Also: require() calls, dynamic import()
    if node.kind() == "import_statement" {
        if let Some(source) = node.child_by_field_name("source") {
            let raw = content[source.byte_range()].to_string();
            // Strip surrounding quotes
            let specifier = raw.trim_matches(|c| c == '\'' || c == '"').to_string();
            imports.push(ImportEdge {
                source_file: file_path.to_string(),
                import_specifier: specifier,
                resolved_path: None,
                language: language.to_string(),
            });
        }
    } else if node.kind() == "export_statement" {
        // Re-exports: `export { Foo } from './bar'`
        if let Some(source) = node.child_by_field_name("source") {
            let raw = content[source.byte_range()].to_string();
            let specifier = raw.trim_matches(|c| c == '\'' || c == '"').to_string();
            imports.push(ImportEdge {
                source_file: file_path.to_string(),
                import_specifier: specifier,
                resolved_path: None,
                language: language.to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ts_imports(&child, content, file_path, language, imports);
    }
}

fn collect_python_imports(
    node: &Node,
    content: &str,
    file_path: &str,
    imports: &mut Vec<ImportEdge>,
) {
    // Python: import_statement, import_from_statement
    // e.g., `import os`, `from pathlib import Path`, `from . import utils`
    if node.kind() == "import_statement" {
        // `import foo, bar` — each dotted_name is a module
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "dotted_name" || child.kind() == "aliased_import" {
                let name_node = if child.kind() == "aliased_import" {
                    child.child_by_field_name("name")
                } else {
                    Some(child)
                };
                if let Some(n) = name_node {
                    let specifier = content[n.byte_range()].to_string();
                    imports.push(ImportEdge {
                        source_file: file_path.to_string(),
                        import_specifier: specifier,
                        resolved_path: None,
                        language: "python".to_string(),
                    });
                }
            }
        }
    } else if node.kind() == "import_from_statement" {
        // `from foo.bar import Baz` — module_name is `foo.bar`
        if let Some(module) = node.child_by_field_name("module_name") {
            let specifier = content[module.byte_range()].to_string();
            imports.push(ImportEdge {
                source_file: file_path.to_string(),
                import_specifier: specifier,
                resolved_path: None,
                language: "python".to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_python_imports(&child, content, file_path, imports);
    }
}

fn collect_go_imports(node: &Node, content: &str, file_path: &str, imports: &mut Vec<ImportEdge>) {
    // Go: import_declaration with import_spec children
    // e.g., `import "fmt"`, `import ("fmt"; "os")`
    if node.kind() == "import_spec" {
        if let Some(path_node) = node.child_by_field_name("path") {
            let raw = content[path_node.byte_range()].to_string();
            let specifier = raw.trim_matches('"').to_string();
            imports.push(ImportEdge {
                source_file: file_path.to_string(),
                import_specifier: specifier,
                resolved_path: None,
                language: "go".to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_go_imports(&child, content, file_path, imports);
    }
}

fn collect_java_imports(
    node: &Node,
    content: &str,
    file_path: &str,
    imports: &mut Vec<ImportEdge>,
) {
    // Java: import_declaration
    // e.g., `import java.util.List;`, `import static org.junit.Assert.*;`
    if node.kind() == "import_declaration" {
        // Get the full import text, strip `import `, `static `, and trailing `;`
        let text = content[node.byte_range()].to_string();
        let specifier = text
            .trim_start_matches("import")
            .trim()
            .trim_start_matches("static")
            .trim()
            .trim_end_matches(';')
            .trim()
            .to_string();
        imports.push(ImportEdge {
            source_file: file_path.to_string(),
            import_specifier: specifier,
            resolved_path: None,
            language: "java".to_string(),
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_java_imports(&child, content, file_path, imports);
    }
}

fn collect_cpp_imports(node: &Node, content: &str, file_path: &str, imports: &mut Vec<ImportEdge>) {
    // C/C++: preproc_include
    // e.g., `#include <iostream>`, `#include "myheader.h"`
    if node.kind() == "preproc_include" {
        if let Some(path_node) = node.child_by_field_name("path") {
            let raw = content[path_node.byte_range()].to_string();
            // Strip <> or ""
            let specifier = raw
                .trim_matches(|c| c == '<' || c == '>' || c == '"')
                .to_string();
            imports.push(ImportEdge {
                source_file: file_path.to_string(),
                import_specifier: specifier,
                resolved_path: None,
                language: "cpp".to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_cpp_imports(&child, content, file_path, imports);
    }
}

/// Walk a tree-sitter AST and collect raw import statements with statement text, path, and dep_type
fn collect_raw_imports(node: &Node, content: &str, language: &str, imports: &mut Vec<RawImport>) {
    match language {
        "rust" => collect_raw_rust_imports(node, content, imports),
        "typescript" | "tsx" => collect_raw_ts_imports(node, content, imports),
        "python" => collect_raw_python_imports(node, content, imports),
        "go" => collect_raw_go_imports(node, content, imports),
        "java" => collect_raw_java_imports(node, content, imports),
        "cpp" => collect_raw_cpp_imports(node, content, imports),
        _ => {}
    }
}

fn collect_raw_rust_imports(node: &Node, content: &str, imports: &mut Vec<RawImport>) {
    if node.kind() == "use_declaration" {
        let statement = content[node.byte_range()].to_string();
        if let Some(arg) = node.child_by_field_name("argument") {
            let path = content[arg.byte_range()].to_string();
            imports.push(RawImport {
                statement,
                path,
                dep_type: "use".to_string(),
            });
        }
    } else if node.kind() == "extern_crate_declaration" {
        let statement = content[node.byte_range()].to_string();
        if let Some(name) = node.child_by_field_name("name") {
            let path = content[name.byte_range()].to_string();
            imports.push(RawImport {
                statement,
                path,
                dep_type: "use".to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_raw_rust_imports(&child, content, imports);
    }
}

fn collect_raw_ts_imports(node: &Node, content: &str, imports: &mut Vec<RawImport>) {
    if node.kind() == "import_statement" {
        let statement = content[node.byte_range()].to_string();
        if let Some(source) = node.child_by_field_name("source") {
            let raw = content[source.byte_range()].to_string();
            let path = raw.trim_matches(|c| c == '\'' || c == '"').to_string();
            imports.push(RawImport {
                statement,
                path,
                dep_type: "import".to_string(),
            });
        }
    } else if node.kind() == "export_statement" {
        if let Some(source) = node.child_by_field_name("source") {
            let statement = content[node.byte_range()].to_string();
            let raw = content[source.byte_range()].to_string();
            let path = raw.trim_matches(|c| c == '\'' || c == '"').to_string();
            imports.push(RawImport {
                statement,
                path,
                dep_type: "import".to_string(),
            });
        }
    } else if node.kind() == "call_expression" {
        // Detect require() calls: `const x = require('foo')`
        if let Some(func) = node.child_by_field_name("function") {
            if content[func.byte_range()] == *"require" {
                if let Some(args) = node.child_by_field_name("arguments") {
                    // First child of argument_list after "(" is the string argument
                    let mut cursor = args.walk();
                    for arg in args.children(&mut cursor) {
                        if arg.kind() == "string" {
                            let statement = content[node.byte_range()].to_string();
                            let raw = content[arg.byte_range()].to_string();
                            let path = raw.trim_matches(|c| c == '\'' || c == '"').to_string();
                            imports.push(RawImport {
                                statement,
                                path,
                                dep_type: "require".to_string(),
                            });
                            break;
                        }
                    }
                }
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_raw_ts_imports(&child, content, imports);
    }
}

fn collect_raw_python_imports(node: &Node, content: &str, imports: &mut Vec<RawImport>) {
    if node.kind() == "import_statement" {
        let statement = content[node.byte_range()].to_string();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "dotted_name" || child.kind() == "aliased_import" {
                let name_node = if child.kind() == "aliased_import" {
                    child.child_by_field_name("name")
                } else {
                    Some(child)
                };
                if let Some(n) = name_node {
                    let path = content[n.byte_range()].to_string();
                    imports.push(RawImport {
                        statement: statement.clone(),
                        path,
                        dep_type: "import".to_string(),
                    });
                }
            }
        }
    } else if node.kind() == "import_from_statement" {
        let statement = content[node.byte_range()].to_string();
        if let Some(module) = node.child_by_field_name("module_name") {
            let path = content[module.byte_range()].to_string();
            imports.push(RawImport {
                statement,
                path,
                dep_type: "from".to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_raw_python_imports(&child, content, imports);
    }
}

fn collect_raw_go_imports(node: &Node, content: &str, imports: &mut Vec<RawImport>) {
    if node.kind() == "import_spec" {
        let statement = content[node.byte_range()].to_string();
        if let Some(path_node) = node.child_by_field_name("path") {
            let raw = content[path_node.byte_range()].to_string();
            let path = raw.trim_matches('"').to_string();
            imports.push(RawImport {
                statement,
                path,
                dep_type: "import".to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_raw_go_imports(&child, content, imports);
    }
}

fn collect_raw_java_imports(node: &Node, content: &str, imports: &mut Vec<RawImport>) {
    if node.kind() == "import_declaration" {
        let statement = content[node.byte_range()].to_string();
        let path = statement
            .trim_start_matches("import")
            .trim()
            .trim_start_matches("static")
            .trim()
            .trim_end_matches(';')
            .trim()
            .to_string();
        imports.push(RawImport {
            statement,
            path,
            dep_type: "import".to_string(),
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_raw_java_imports(&child, content, imports);
    }
}

fn collect_raw_cpp_imports(node: &Node, content: &str, imports: &mut Vec<RawImport>) {
    if node.kind() == "preproc_include" {
        let statement = content[node.byte_range()].to_string();
        if let Some(path_node) = node.child_by_field_name("path") {
            let raw = content[path_node.byte_range()].to_string();
            let path = raw
                .trim_matches(|c| c == '<' || c == '>' || c == '"')
                .to_string();
            imports.push(RawImport {
                statement,
                path,
                dep_type: "include".to_string(),
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_raw_cpp_imports(&child, content, imports);
    }
}

fn generate_chunk_id(path: &Path, start_line: u32, end_line: u32) -> String {
    use sha2::{Digest, Sha256};
    let input = format!("{}:{}:{}", path.display(), start_line, end_line);
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(&hash[..8])
}

/// Check if a markdown file has a `schema:` field in its YAML frontmatter.
///
/// Any file with `schema:` in frontmatter is treated as an archive record
/// and chunked as a whole transcript rather than by markdown headings.
fn has_schema_frontmatter(content: &str) -> bool {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return false;
    }
    if let Some(end) = trimmed[3..].find("\n---") {
        let fm = &trimmed[3..3 + end];
        fm.lines().any(|l| l.trim().starts_with("schema:"))
    } else {
        false
    }
}

/// Find the chunk whose span contains the given line.
fn find_chunk_at_line<'a>(chunks: &'a [Chunk], line: u32) -> Option<&'a Chunk> {
    chunks
        .iter()
        .find(|c| c.start_line <= line && line <= c.end_line)
}

/// Recursively collect chunk-level edges from the tree-sitter AST.
fn collect_chunk_edges(
    node: &Node,
    content: &str,
    file_path: &str,
    language: &str,
    chunks: &[Chunk],
    chunk_by_name: &std::collections::HashMap<&str, &Chunk>,
    edges: &mut Vec<ChunkEdge>,
) {
    match language {
        "rust" => collect_rust_chunk_edges(node, content, file_path, chunks, chunk_by_name, edges),
        "python" => {
            collect_python_chunk_edges(node, content, file_path, chunks, chunk_by_name, edges)
        }
        "typescript" | "tsx" | "java" => collect_class_extends_edges(
            node,
            content,
            file_path,
            language,
            chunks,
            chunk_by_name,
            edges,
        ),
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_chunk_edges(
            &child,
            content,
            file_path,
            language,
            chunks,
            chunk_by_name,
            edges,
        );
    }
}

/// Extract edges from Rust impl items: `impl Trait for Struct` and `impl Struct`.
fn collect_rust_chunk_edges(
    node: &Node,
    content: &str,
    file_path: &str,
    chunks: &[Chunk],
    chunk_by_name: &std::collections::HashMap<&str, &Chunk>,
    edges: &mut Vec<ChunkEdge>,
) {
    if node.kind() != "impl_item" {
        return;
    }

    let impl_line = node.start_position().row as u32 + 1;
    let Some(source_chunk) = find_chunk_at_line(chunks, impl_line) else {
        return;
    };
    let source_name = source_chunk.name.as_deref().unwrap_or("<impl>").to_string();

    // Look for `impl Trait for Type` pattern by checking child nodes.
    // In tree-sitter-rust, impl_item has:
    //   - "trait" field: the trait being implemented (if `impl Trait for Type`)
    //   - "type" field: the type being implemented
    let trait_node = node.child_by_field_name("trait");
    let type_node = node.child_by_field_name("type");

    if let Some(trait_n) = trait_node {
        let trait_name = &content[trait_n.byte_range()];
        // Extract just the identifier (strip generics like `Trait<T>`)
        let trait_ident = trait_name.split('<').next().unwrap_or(trait_name).trim();

        if let Some(target) = chunk_by_name.get(trait_ident) {
            edges.push(ChunkEdge {
                source_chunk: source_chunk.id.clone(),
                target_chunk: target.id.clone(),
                source_name: source_name.clone(),
                target_name: trait_ident.to_string(),
                edge_type: ChunkEdgeType::Implements,
                file_path: file_path.to_string(),
            });
        }
    }

    if let Some(type_n) = type_node {
        let type_name = &content[type_n.byte_range()];
        let type_ident = type_name.split('<').next().unwrap_or(type_name).trim();

        if let Some(target) = chunk_by_name.get(type_ident) {
            edges.push(ChunkEdge {
                source_chunk: source_chunk.id.clone(),
                target_chunk: target.id.clone(),
                source_name,
                target_name: type_ident.to_string(),
                edge_type: ChunkEdgeType::ImplFor,
                file_path: file_path.to_string(),
            });
        }
    }
}

/// Extract edges from Python class inheritance: `class Foo(Bar, Baz)`.
fn collect_python_chunk_edges(
    node: &Node,
    content: &str,
    file_path: &str,
    chunks: &[Chunk],
    chunk_by_name: &std::collections::HashMap<&str, &Chunk>,
    edges: &mut Vec<ChunkEdge>,
) {
    if node.kind() != "class_definition" {
        return;
    }

    let class_line = node.start_position().row as u32 + 1;
    let Some(source_chunk) = find_chunk_at_line(chunks, class_line) else {
        return;
    };
    let source_name = source_chunk
        .name
        .as_deref()
        .unwrap_or("<class>")
        .to_string();

    // Python class_definition has an "superclasses" / "argument_list" child
    // with the base classes: class Foo(Bar, Baz):
    if let Some(args) = node.child_by_field_name("superclasses") {
        let mut cursor = args.walk();
        for child in args.children(&mut cursor) {
            if child.kind() == "identifier" {
                let base_name = &content[child.byte_range()];
                if let Some(target) = chunk_by_name.get(base_name) {
                    edges.push(ChunkEdge {
                        source_chunk: source_chunk.id.clone(),
                        target_chunk: target.id.clone(),
                        source_name: source_name.clone(),
                        target_name: base_name.to_string(),
                        edge_type: ChunkEdgeType::Extends,
                        file_path: file_path.to_string(),
                    });
                }
            }
        }
    }
}

/// Extract extends edges from TS/Java class declarations.
fn collect_class_extends_edges(
    node: &Node,
    content: &str,
    file_path: &str,
    language: &str,
    chunks: &[Chunk],
    chunk_by_name: &std::collections::HashMap<&str, &Chunk>,
    edges: &mut Vec<ChunkEdge>,
) {
    let is_class = match language {
        "typescript" | "tsx" => node.kind() == "class_declaration",
        "java" => node.kind() == "class_declaration",
        _ => false,
    };
    if !is_class {
        return;
    }

    let class_line = node.start_position().row as u32 + 1;
    let Some(source_chunk) = find_chunk_at_line(chunks, class_line) else {
        return;
    };
    let source_name = source_chunk
        .name
        .as_deref()
        .unwrap_or("<class>")
        .to_string();

    // TS/Java: class_heritage or superclass field
    // Look for extends clause in the text (tree-sitter field varies)
    let node_text = &content[node.byte_range()];
    // Quick scan for `extends SomeClass` in the class header (first line)
    if let Some(first_line) = node_text.lines().next() {
        if let Some(pos) = first_line.find("extends ") {
            let after = &first_line[pos + 8..];
            // Take the identifier (until space, {, <, or comma)
            let base: &str = after
                .split(|c: char| c.is_whitespace() || c == '{' || c == '<' || c == ',')
                .next()
                .unwrap_or("");
            let base = base.trim();
            if !base.is_empty() {
                if let Some(target) = chunk_by_name.get(base) {
                    edges.push(ChunkEdge {
                        source_chunk: source_chunk.id.clone(),
                        target_chunk: target.id.clone(),
                        source_name,
                        target_name: base.to_string(),
                        edge_type: ChunkEdgeType::Extends,
                        file_path: file_path.to_string(),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
