use anyhow::Result;
use regex::Regex;
use std::path::Path;
use tree_sitter::{Language, Node};

use crate::types::{Chunk, ChunkType};

/// Parses source code using tree-sitter to extract semantic chunks
pub struct Parser {
    rust_parser: tree_sitter::Parser,
    typescript_parser: tree_sitter::Parser,
    python_parser: tree_sitter::Parser,
    go_parser: tree_sitter::Parser,
    java_parser: tree_sitter::Parser,
    cpp_parser: tree_sitter::Parser,
    header_regex: Regex,
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
            header_regex: Regex::new(r"(?m)^(#{1,6})\s+(.+)$")?,
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

    /// Extract markdown chunks based on headers
    fn chunk_markdown(&self, path: &Path, content: &str) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let matches: Vec<_> = self.header_regex.find_iter(content).collect();

        if matches.is_empty() {
            return self.chunk_by_lines(path, content);
        }

        // Handle preamble (content before first header)
        if matches[0].start() > 0 {
            let pre_content = &content[0..matches[0].start()];
            if !pre_content.trim().is_empty() {
                let end_line = byte_offset_to_line(content, matches[0].start());
                chunks.push(Chunk {
                    id: generate_chunk_id(path, 1, end_line),
                    file_path: path.to_string_lossy().to_string(),
                    chunk_type: ChunkType::Doc,
                    name: Some("Preamble".to_string()),
                    start_line: 1,
                    end_line,
                    content: pre_content.to_string(),
                    language: "markdown".to_string(),
                });
            }
        }

        // Stack of (level, title)
        let mut header_stack: Vec<(usize, String)> = Vec::new();

        for i in 0..matches.len() {
            let m = matches[i];
            let start = m.start();
            let end = if i + 1 < matches.len() {
                matches[i + 1].start()
            } else {
                content.len()
            };

            let chunk_content = &content[start..end];
            let start_line = byte_offset_to_line(content, start);
            let end_line = byte_offset_to_line(content, end);

            // Extract header level and title
            let captures = self
                .header_regex
                .captures(&content[start..m.end()])
                .unwrap();
            let hashes = captures.get(1).unwrap().as_str();
            let raw_title = captures.get(2).unwrap().as_str().trim().to_string();
            let level = hashes.len();

            // Update stack: pop headers that are same level or deeper
            while let Some((last_level, _)) = header_stack.last() {
                if *last_level >= level {
                    header_stack.pop();
                } else {
                    break;
                }
            }
            header_stack.push((level, raw_title));

            // Construct full name
            let full_name = header_stack
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join(" > ");

            chunks.push(Chunk {
                id: generate_chunk_id(path, start_line, end_line),
                file_path: path.to_string_lossy().to_string(),
                chunk_type: ChunkType::Doc,
                name: Some(full_name),
                start_line,
                end_line,
                content: chunk_content.to_string(),
                language: "markdown".to_string(),
            });
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

fn byte_offset_to_line(content: &str, offset: usize) -> u32 {
    if offset >= content.len() {
        return content.lines().count() as u32;
    }
    content[..offset].chars().filter(|&c| c == '\n').count() as u32 + 1
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

        assert_eq!(chunks.len(), 4);

        // Header 1 (Title + Preamble)
        assert_eq!(chunks[0].chunk_type, ChunkType::Doc);
        assert_eq!(chunks[0].name, Some("Title".to_string()));

        // Header 2 (Section 1)
        assert_eq!(chunks[1].chunk_type, ChunkType::Doc);
        assert_eq!(chunks[1].name, Some("Title > Section 1".to_string()));

        // Header 3 (Subsection 1.1)
        assert_eq!(chunks[2].chunk_type, ChunkType::Doc);
        assert_eq!(
            chunks[2].name,
            Some("Title > Section 1 > Subsection 1.1".to_string())
        );

        // Header 4 (Section 2)
        assert_eq!(chunks[3].chunk_type, ChunkType::Doc);
        assert_eq!(chunks[3].name, Some("Title > Section 2".to_string()));
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

        assert_eq!(chunks.len(), 2);

        assert_eq!(chunks[0].name, Some("Preamble".to_string()));
        assert_eq!(chunks[1].name, Some("Title".to_string()));
    }
}
