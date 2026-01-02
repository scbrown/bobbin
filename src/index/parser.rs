use anyhow::{bail, Result};
use std::path::Path;
use tree_sitter::{Language, Node};

use crate::types::{Chunk, ChunkType};

/// Parses source code using tree-sitter to extract semantic chunks
pub struct Parser {
    rust_parser: tree_sitter::Parser,
    typescript_parser: tree_sitter::Parser,
    python_parser: tree_sitter::Parser,
}

impl Parser {
    /// Create a new parser with support for multiple languages
    pub fn new() -> Result<Self> {
        Ok(Self {
            rust_parser: create_parser(tree_sitter_rust::LANGUAGE.into())?,
            typescript_parser: create_parser(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?,
            python_parser: create_parser(tree_sitter_python::LANGUAGE.into())?,
        })
    }

    /// Parse a file and extract semantic chunks
    pub fn parse_file(&mut self, path: &Path, content: &str) -> Result<Vec<Chunk>> {
        let language = detect_language(path);

        let Some(lang) = language.as_deref() else {
            // Unknown language - fall back to line-based chunking
            return Ok(self.chunk_by_lines(path, content));
        };

        let parser = match lang {
            "rust" => &mut self.rust_parser,
            "typescript" | "tsx" => &mut self.typescript_parser,
            "python" => &mut self.python_parser,
            _ => return Ok(self.chunk_by_lines(path, content)),
        };

        let Some(tree) = parser.parse(content, None) else {
            bail!("Failed to parse file: {}", path.display());
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
            _ => None,
        }
    }

    /// Extract the name of a semantic unit
    fn extract_name(&self, node: &Node, content: &str, language: &str) -> Option<String> {
        let name_field = match language {
            "rust" => "name",
            "typescript" | "tsx" => "name",
            "python" => "name",
            _ => return None,
        };

        node.child_by_field_name(name_field)
            .map(|n| content[n.byte_range()].to_string())
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

fn generate_chunk_id(path: &Path, start_line: u32, end_line: u32) -> String {
    use sha2::{Digest, Sha256};
    let input = format!("{}:{}:{}", path.display(), start_line, end_line);
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(&hash[..8])
}
