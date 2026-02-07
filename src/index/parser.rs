use anyhow::Result;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser as CmarkParser, Tag, TagEnd};
use std::path::Path;
use tree_sitter::{Language, Node};

use crate::types::{Chunk, ChunkType, ImportEdge, RawImport};

/// Parses source code using tree-sitter to extract semantic chunks
pub struct Parser {
    rust_parser: tree_sitter::Parser,
    typescript_parser: tree_sitter::Parser,
    python_parser: tree_sitter::Parser,
    go_parser: tree_sitter::Parser,
    java_parser: tree_sitter::Parser,
    cpp_parser: tree_sitter::Parser,
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
        })
    }

    /// Parse a file and extract semantic chunks
    pub fn parse_file(&mut self, path: &Path, content: &str) -> Result<Vec<Chunk>> {
        let language = detect_language(path);

        let Some(lang) = language.as_deref() else {
            // Unknown language - fall back to line-based chunking
            return Ok(self.chunk_by_lines(path, content));
        };

        if lang == "markdown" {
            return Ok(self.chunk_markdown(path, content));
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
                            if s.is_empty() { None } else { Some(s.to_string()) }
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
                });
            }
        } else {
            // Preamble before first heading
            if sections[0].body_offset > 0 {
                let pre = &body[..sections[0].body_offset];
                if !pre.trim().is_empty() {
                    let start_line = (line_offset + 1) as u32;
                    let end_line = (line_offset + byte_offset_to_line_in(body, sections[0].body_offset)) as u32;
                    chunks.push(Chunk {
                        id: generate_chunk_id(path, start_line, end_line),
                        file_path: file_path.clone(),
                        chunk_type: ChunkType::Doc,
                        name: Some("Preamble".to_string()),
                        start_line,
                        end_line,
                        content: pre.to_string(),
                        language: "markdown".to_string(),
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
                });
            }
        }

        // 5. Emit standalone table and code_block chunks
        for block in &standalone_blocks {
            let block_content = &body[block.body_offset_start..block.body_offset_end];
            let start_line = (line_offset + byte_offset_to_line_in(body, block.body_offset_start)) as u32;
            let end_line = (line_offset + byte_offset_to_line_in(body, block.body_offset_end)) as u32;

            let name = match block.chunk_type {
                ChunkType::Table => {
                    // Try to use preceding section heading
                    let parent_section = sections.iter().rev().find(|s| s.body_offset <= block.body_offset_start);
                    parent_section.map(|s| format!("{} (table)", s.name))
                }
                ChunkType::CodeBlock => {
                    block.name.as_ref().map(|lang| format!("code: {}", lang))
                }
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
            "rust" | "typescript" | "tsx" | "python" | "java" => {
                node.child_by_field_name("name")
                    .map(|n| content[n.byte_range()].to_string())
            }
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

    /// Fall back to line-based chunking for unknown languages
    fn chunk_by_lines(&self, path: &Path, content: &str) -> Vec<Chunk> {
        let lines: Vec<&str> = content.lines().collect();
        let chunk_size = 50; // lines per chunk
        let overlap = 10;

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < lines.len() {
            let end = (start + chunk_size).min(lines.len());
            let chunk_content = lines[start..end].join("\n");

            chunks.push(Chunk {
                id: generate_chunk_id(path, start as u32 + 1, end as u32),
                file_path: path.to_string_lossy().to_string(),
                chunk_type: ChunkType::Other,
                name: None,
                start_line: start as u32 + 1,
                end_line: end as u32,
                content: chunk_content,
                language: detect_language(path).unwrap_or_else(|| "unknown".to_string()),
            });

            if end >= lines.len() {
                break;
            }
            start = end - overlap;
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

fn collect_rust_imports(node: &Node, content: &str, file_path: &str, imports: &mut Vec<ImportEdge>) {
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

fn collect_ts_imports(node: &Node, content: &str, file_path: &str, language: &str, imports: &mut Vec<ImportEdge>) {
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

fn collect_python_imports(node: &Node, content: &str, file_path: &str, imports: &mut Vec<ImportEdge>) {
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

fn collect_java_imports(node: &Node, content: &str, file_path: &str, imports: &mut Vec<ImportEdge>) {
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
fn collect_raw_imports(
    node: &Node,
    content: &str,
    language: &str,
    imports: &mut Vec<RawImport>,
) {
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


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_rust_function() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
fn hello_world() {
    println!("Hello, world!");
}
"#;
        let path = PathBuf::from("test.rs");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_type, ChunkType::Function);
        assert_eq!(chunks[0].name, Some("hello_world".to_string()));
        assert_eq!(chunks[0].language, "rust");
    }

    #[test]
    fn test_parse_rust_struct_and_impl() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}
"#;
        let path = PathBuf::from("test.rs");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert_eq!(chunks.len(), 3); // struct, impl, function inside impl
        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Struct));
        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Impl));
        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Function && c.name == Some("new".to_string())));
    }

    #[test]
    fn test_parse_typescript_class() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
class Greeter {
    greeting: string;

    constructor(message: string) {
        this.greeting = message;
    }

    greet() {
        return "Hello, " + this.greeting;
    }
}
"#;
        let path = PathBuf::from("test.ts");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Class));
        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Method));
    }

    #[test]
    fn test_parse_typescript_interface() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
interface User {
    name: string;
    age: number;
}

function greet(user: User): string {
    return `Hello, ${user.name}`;
}
"#;
        let path = PathBuf::from("test.ts");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Interface));
        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Function));
    }

    #[test]
    fn test_parse_python_class() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
class Dog:
    def __init__(self, name):
        self.name = name

    def bark(self):
        print(f"{self.name} says woof!")
"#;
        let path = PathBuf::from("test.py");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Class && c.name == Some("Dog".to_string())));
        // Functions inside class are also extracted
        assert!(
            chunks
                .iter()
                .any(|c| c.chunk_type == ChunkType::Function
                    && c.name == Some("__init__".to_string()))
        );
        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Function && c.name == Some("bark".to_string())));
    }

    #[test]
    fn test_parse_go_function() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
package main

func hello() {
    fmt.Println("Hello, world!")
}
"#;
        let path = PathBuf::from("test.go");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Function && c.name == Some("hello".to_string())));
        assert!(chunks.iter().all(|c| c.language == "go"));
    }

    #[test]
    fn test_parse_go_struct_and_method() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
package main

type Point struct {
    X float64
    Y float64
}

func (p Point) Distance() float64 {
    return math.Sqrt(p.X*p.X + p.Y*p.Y)
}
"#;
        let path = PathBuf::from("test.go");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Struct && c.name == Some("Point".to_string())));
        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Method
            && c.name == Some("Distance".to_string())));
    }

    #[test]
    fn test_parse_java_class() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
public class Greeter {
    private String name;

    public Greeter(String name) {
        this.name = name;
    }

    public void greet() {
        System.out.println("Hello, " + name);
    }
}
"#;
        let path = PathBuf::from("Greeter.java");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Class && c.name == Some("Greeter".to_string())));
        // Constructor and method
        assert!(chunks.iter().filter(|c| c.chunk_type == ChunkType::Method).count() >= 2);
        assert!(chunks.iter().all(|c| c.language == "java"));
    }

    #[test]
    fn test_parse_java_interface() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
public interface Runnable {
    void run();
}
"#;
        let path = PathBuf::from("Runnable.java");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Interface
            && c.name == Some("Runnable".to_string())));
    }

    #[test]
    fn test_parse_cpp_function() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
#include <iostream>

void hello() {
    std::cout << "Hello, world!" << std::endl;
}
"#;
        let path = PathBuf::from("test.cpp");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Function && c.name == Some("hello".to_string())));
        assert!(chunks.iter().all(|c| c.language == "cpp"));
    }

    #[test]
    fn test_parse_cpp_class() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
class Point {
public:
    double x;
    double y;

    Point(double x, double y) : x(x), y(y) {}

    double distance() {
        return sqrt(x*x + y*y);
    }
};
"#;
        let path = PathBuf::from("test.cpp");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Class && c.name == Some("Point".to_string())));
    }

    #[test]
    fn test_parse_cpp_struct() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
struct Vec3 {
    float x, y, z;
};
"#;
        let path = PathBuf::from("test.cpp");
        let chunks = parser.parse_file(&path, content).unwrap();

        assert!(chunks
            .iter()
            .any(|c| c.chunk_type == ChunkType::Struct && c.name == Some("Vec3".to_string())));
    }

    #[test]
    fn test_unknown_language_fallback() {
        let mut parser = Parser::new().unwrap();
        let content = "line1\nline2\nline3\nline4\nline5";
        let path = PathBuf::from("test.xyz");
        let chunks = parser.parse_file(&path, content).unwrap();

        // Should fall back to line-based chunking
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| c.chunk_type == ChunkType::Other));
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(
            detect_language(Path::new("foo.rs")),
            Some("rust".to_string())
        );
        assert_eq!(
            detect_language(Path::new("foo.ts")),
            Some("typescript".to_string())
        );
        assert_eq!(
            detect_language(Path::new("foo.tsx")),
            Some("tsx".to_string())
        );
        assert_eq!(
            detect_language(Path::new("foo.py")),
            Some("python".to_string())
        );
        assert_eq!(
            detect_language(Path::new("foo.js")),
            Some("javascript".to_string())
        );
        assert_eq!(detect_language(Path::new("foo.go")), Some("go".to_string()));
        assert_eq!(
            detect_language(Path::new("foo.java")),
            Some("java".to_string())
        );
        assert_eq!(
            detect_language(Path::new("foo.cpp")),
            Some("cpp".to_string())
        );
        assert_eq!(
            detect_language(Path::new("foo.cc")),
            Some("cpp".to_string())
        );
        assert_eq!(
            detect_language(Path::new("foo.hpp")),
            Some("cpp".to_string())
        );
        assert_eq!(detect_language(Path::new("foo.unknown")), None);
    }

    #[test]
    fn test_chunk_id_deterministic() {
        let path = Path::new("test.rs");
        let id1 = generate_chunk_id(path, 1, 10);
        let id2 = generate_chunk_id(path, 1, 10);
        let id3 = generate_chunk_id(path, 1, 11);

        assert_eq!(id1, id2); // Same inputs = same ID
        assert_ne!(id1, id3); // Different inputs = different ID
    }

    #[test]
    fn test_empty_file() {
        let mut parser = Parser::new().unwrap();
        let content = "";
        let path = PathBuf::from("test.rs");
        let chunks = parser.parse_file(&path, content).unwrap();

        // Empty file should return empty chunks (no fallback needed)
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_file_with_only_comments() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
// This is a comment
// Another comment
/* Block comment */
"#;
        let path = PathBuf::from("test.rs");
        let chunks = parser.parse_file(&path, content).unwrap();

        // No semantic chunks, should fall back to line-based
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| c.chunk_type == ChunkType::Other));
    }

    #[test]
    fn test_parse_markdown() {
        let mut parser = Parser::new().unwrap();
        let content = r#"# Title

Preamble content.

## Section 1
Content 1.

### Subsection 1.1
Content 1.1.

## Section 2
Content 2.
"#;
        let path = PathBuf::from("README.md");
        let chunks = parser.parse_file(&path, content).unwrap();

        // Section chunks for each heading
        let sections: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Section).collect();
        assert_eq!(sections.len(), 4);

        assert_eq!(sections[0].name, Some("Title".to_string()));
        assert_eq!(sections[1].name, Some("Title > Section 1".to_string()));
        assert_eq!(
            sections[2].name,
            Some("Title > Section 1 > Subsection 1.1".to_string())
        );
        assert_eq!(sections[3].name, Some("Title > Section 2".to_string()));
    }

    #[test]
    fn test_markdown_preamble() {
        let mut parser = Parser::new().unwrap();
        let content = r#"Preamble content without header.

# Title
Content.
"#;
        let path = PathBuf::from("README.md");
        let chunks = parser.parse_file(&path, content).unwrap();

        let doc_chunks: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Doc).collect();
        let section_chunks: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Section).collect();

        assert_eq!(doc_chunks.len(), 1);
        assert_eq!(doc_chunks[0].name, Some("Preamble".to_string()));

        assert_eq!(section_chunks.len(), 1);
        assert_eq!(section_chunks[0].name, Some("Title".to_string()));
    }

    #[test]
    fn test_markdown_frontmatter() {
        let mut parser = Parser::new().unwrap();
        let content = r#"---
title: Test Document
author: Bobbin
tags: [rust, search]
---

# Introduction

Welcome to the document.
"#;
        let path = PathBuf::from("doc.md");
        let chunks = parser.parse_file(&path, content).unwrap();

        // Should have frontmatter chunk
        let fm: Vec<_> = chunks.iter().filter(|c| c.name == Some("Frontmatter".to_string())).collect();
        assert_eq!(fm.len(), 1);
        assert_eq!(fm[0].chunk_type, ChunkType::Doc);
        assert!(fm[0].content.contains("title: Test Document"));

        // And a section for the heading
        let sections: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Section).collect();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, Some("Introduction".to_string()));
    }

    #[test]
    fn test_markdown_code_blocks() {
        let mut parser = Parser::new().unwrap();
        let content = r#"# Setup

Install the package:

```bash
npm install bobbin
```

Then configure:

```json
{
  "key": "value"
}
```
"#;
        let path = PathBuf::from("guide.md");
        let chunks = parser.parse_file(&path, content).unwrap();

        let code_blocks: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::CodeBlock).collect();
        assert_eq!(code_blocks.len(), 2);
        assert_eq!(code_blocks[0].name, Some("code: bash".to_string()));
        assert_eq!(code_blocks[1].name, Some("code: json".to_string()));
        assert!(code_blocks[0].content.contains("npm install bobbin"));
    }

    // ---- Import extraction tests ----

    #[test]
    fn test_extract_rust_imports() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
use std::path::Path;
use crate::types::Chunk;
use anyhow::Result;

fn main() {}
"#;
        let path = PathBuf::from("test.rs");
        let imports = parser.extract_imports(&path, content);

        assert_eq!(imports.len(), 3);
        assert!(imports.iter().any(|i| i.import_specifier == "std::path::Path"));
        assert!(imports.iter().any(|i| i.import_specifier == "crate::types::Chunk"));
        assert!(imports.iter().any(|i| i.import_specifier == "anyhow::Result"));
        assert!(imports.iter().all(|i| i.language == "rust"));
    }

    #[test]
    fn test_extract_typescript_imports() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
import { Foo } from './bar';
import * as React from 'react';
import defaultExport from '../utils';

export { Thing } from './thing';

function hello() {}
"#;
        let path = PathBuf::from("test.ts");
        let imports = parser.extract_imports(&path, content);

        assert_eq!(imports.len(), 4);
        assert!(imports.iter().any(|i| i.import_specifier == "./bar"));
        assert!(imports.iter().any(|i| i.import_specifier == "react"));
        assert!(imports.iter().any(|i| i.import_specifier == "../utils"));
        assert!(imports.iter().any(|i| i.import_specifier == "./thing"));
    }

    #[test]
    fn test_extract_python_imports() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
import os
import sys
from pathlib import Path
from . import utils

def main():
    pass
"#;
        let path = PathBuf::from("test.py");
        let imports = parser.extract_imports(&path, content);

        assert!(imports.iter().any(|i| i.import_specifier == "os"));
        assert!(imports.iter().any(|i| i.import_specifier == "sys"));
        assert!(imports.iter().any(|i| i.import_specifier == "pathlib"));
        assert!(imports.iter().all(|i| i.language == "python"));
    }

    #[test]
    fn test_extract_go_imports() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
package main

import (
    "fmt"
    "os"
)

func main() {}
"#;
        let path = PathBuf::from("test.go");
        let imports = parser.extract_imports(&path, content);

        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.import_specifier == "fmt"));
        assert!(imports.iter().any(|i| i.import_specifier == "os"));
    }

    #[test]
    fn test_extract_java_imports() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
import java.util.List;
import java.io.File;

public class Main {
    public static void main(String[] args) {}
}
"#;
        let path = PathBuf::from("Main.java");
        let imports = parser.extract_imports(&path, content);

        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.import_specifier == "java.util.List"));
        assert!(imports.iter().any(|i| i.import_specifier == "java.io.File"));
    }

    #[test]
    fn test_extract_cpp_includes() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
#include <iostream>
#include "myheader.h"

void hello() {
    std::cout << "Hello" << std::endl;
}
"#;
        let path = PathBuf::from("test.cpp");
        let imports = parser.extract_imports(&path, content);

        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.import_specifier == "iostream"));
        assert!(imports.iter().any(|i| i.import_specifier == "myheader.h"));
    }

    #[test]
    fn test_extract_no_imports_markdown() {
        let mut parser = Parser::new().unwrap();
        let content = "# Title\n\nSome content.";
        let path = PathBuf::from("README.md");
        let imports = parser.extract_imports(&path, content);
        assert!(imports.is_empty());
    }

    // ---- Raw import extraction tests ----

    #[test]
    fn test_raw_imports_rust() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
use std::path::Path;
use crate::types::Chunk;
use anyhow::Result;

fn main() {}
"#;
        let path = PathBuf::from("test.rs");
        let imports = parser.extract_raw_imports(&path, content);

        assert_eq!(imports.len(), 3);

        let i = imports.iter().find(|i| i.path == "std::path::Path").unwrap();
        assert_eq!(i.statement, "use std::path::Path;");
        assert_eq!(i.dep_type, "use");

        let i = imports.iter().find(|i| i.path == "crate::types::Chunk").unwrap();
        assert_eq!(i.statement, "use crate::types::Chunk;");
        assert_eq!(i.dep_type, "use");

        let i = imports.iter().find(|i| i.path == "anyhow::Result").unwrap();
        assert_eq!(i.dep_type, "use");
    }

    #[test]
    fn test_raw_imports_typescript() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
import { Foo } from './bar';
import * as React from 'react';

export { Thing } from './thing';

function hello() {}
"#;
        let path = PathBuf::from("test.ts");
        let imports = parser.extract_raw_imports(&path, content);

        assert_eq!(imports.len(), 3);

        let i = imports.iter().find(|i| i.path == "./bar").unwrap();
        assert!(i.statement.contains("import"));
        assert_eq!(i.dep_type, "import");

        let i = imports.iter().find(|i| i.path == "react").unwrap();
        assert_eq!(i.dep_type, "import");

        let i = imports.iter().find(|i| i.path == "./thing").unwrap();
        assert!(i.statement.contains("export"));
        assert_eq!(i.dep_type, "import");
    }

    #[test]
    fn test_raw_imports_typescript_require() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
const fs = require('fs');
const path = require('path');

function hello() {}
"#;
        let path = PathBuf::from("test.ts");
        let imports = parser.extract_raw_imports(&path, content);

        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.path == "fs" && i.dep_type == "require"));
        assert!(imports.iter().any(|i| i.path == "path" && i.dep_type == "require"));
    }

    #[test]
    fn test_raw_imports_python() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
import os
import sys
from pathlib import Path
from . import utils

def main():
    pass
"#;
        let path = PathBuf::from("test.py");
        let imports = parser.extract_raw_imports(&path, content);

        let i = imports.iter().find(|i| i.path == "os").unwrap();
        assert_eq!(i.dep_type, "import");
        assert!(i.statement.contains("import os"));

        let i = imports.iter().find(|i| i.path == "pathlib").unwrap();
        assert_eq!(i.dep_type, "from");
        assert!(i.statement.contains("from pathlib import Path"));
    }

    #[test]
    fn test_raw_imports_go() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
package main

import (
    "fmt"
    "os"
)

func main() {}
"#;
        let path = PathBuf::from("test.go");
        let imports = parser.extract_raw_imports(&path, content);

        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.path == "fmt" && i.dep_type == "import"));
        assert!(imports.iter().any(|i| i.path == "os" && i.dep_type == "import"));
    }

    #[test]
    fn test_raw_imports_java() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
import java.util.List;
import java.io.File;

public class Main {
    public static void main(String[] args) {}
}
"#;
        let path = PathBuf::from("Main.java");
        let imports = parser.extract_raw_imports(&path, content);

        assert_eq!(imports.len(), 2);

        let i = imports.iter().find(|i| i.path == "java.util.List").unwrap();
        assert_eq!(i.dep_type, "import");
        assert!(i.statement.contains("import java.util.List;"));

        let i = imports.iter().find(|i| i.path == "java.io.File").unwrap();
        assert_eq!(i.dep_type, "import");
    }

    #[test]
    fn test_raw_imports_cpp() {
        let mut parser = Parser::new().unwrap();
        let content = r#"
#include <iostream>
#include "myheader.h"

void hello() {
    std::cout << "Hello" << std::endl;
}
"#;
        let path = PathBuf::from("test.cpp");
        let imports = parser.extract_raw_imports(&path, content);

        assert_eq!(imports.len(), 2);

        let i = imports.iter().find(|i| i.path == "iostream").unwrap();
        assert_eq!(i.dep_type, "include");
        assert!(i.statement.contains("#include"));

        let i = imports.iter().find(|i| i.path == "myheader.h").unwrap();
        assert_eq!(i.dep_type, "include");
    }

    #[test]
    fn test_raw_imports_no_false_positives() {
        let mut parser = Parser::new().unwrap();
        // A function called "import" should not trigger import detection
        let content = r#"
function doStuff() {
    const result = import_data();
    return result;
}
"#;
        let path = PathBuf::from("test.ts");
        let imports = parser.extract_raw_imports(&path, content);
        assert!(imports.is_empty());
    }

    #[test]
    fn test_raw_imports_markdown_empty() {
        let mut parser = Parser::new().unwrap();
        let content = "# Title\n\nSome content.";
        let path = PathBuf::from("README.md");
        let imports = parser.extract_raw_imports(&path, content);
        assert!(imports.is_empty());
    }

    #[test]
    fn test_markdown_tables() {
        let mut parser = Parser::new().unwrap();
        let content = r#"# API Reference

## Methods

| Method | Description |
|--------|-------------|
| get    | Fetch data  |
| set    | Store data  |

More content here.
"#;
        let path = PathBuf::from("api.md");
        let chunks = parser.parse_file(&path, content).unwrap();

        let tables: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Table).collect();
        assert_eq!(tables.len(), 1);
        assert!(tables[0].content.contains("Method"));
        assert!(tables[0].name.as_ref().unwrap().contains("table"));
    }
}
