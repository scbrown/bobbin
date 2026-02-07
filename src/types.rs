use serde::{Deserialize, Serialize};

/// A semantic chunk extracted from a source file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub file_path: String,
    pub chunk_type: ChunkType,
    pub name: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub language: String,
}

/// Types of semantic chunks that can be extracted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkType {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Interface,
    Module,
    Impl,
    Trait,
    Doc,
    Section,
    Table,
    CodeBlock,
    Other,
}

impl std::fmt::Display for ChunkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChunkType::Function => write!(f, "function"),
            ChunkType::Method => write!(f, "method"),
            ChunkType::Class => write!(f, "class"),
            ChunkType::Struct => write!(f, "struct"),
            ChunkType::Enum => write!(f, "enum"),
            ChunkType::Interface => write!(f, "interface"),
            ChunkType::Module => write!(f, "module"),
            ChunkType::Impl => write!(f, "impl"),
            ChunkType::Trait => write!(f, "trait"),
            ChunkType::Doc => write!(f, "doc"),
            ChunkType::Section => write!(f, "section"),
            ChunkType::Table => write!(f, "table"),
            ChunkType::CodeBlock => write!(f, "code_block"),
            ChunkType::Other => write!(f, "other"),
        }
    }
}

/// Metadata about an indexed file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub language: Option<String>,
    pub mtime: i64,
    pub hash: String,
    pub indexed_at: i64,
}

/// A search result with relevance score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunk: Chunk,
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_type: Option<MatchType>,
}

/// How a result was matched
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    Semantic,
    Keyword,
    Hybrid,
}

/// Temporal coupling between two files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCoupling {
    pub file_a: String,
    pub file_b: String,
    pub score: f32,
    pub co_changes: u32,
    pub last_co_change: i64,
}

/// A raw import statement extracted from source code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawImport {
    /// The full verbatim import statement text (e.g., "use crate::auth::middleware;")
    pub statement: String,
    /// The extracted import path (e.g., "crate::auth::middleware")
    pub path: String,
    /// The categorized import type: "use", "import", "require", "from", "include"
    pub dep_type: String,
}

/// An import/dependency edge between two files (used during parsing/resolution)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportEdge {
    /// The file that contains the import statement
    pub source_file: String,
    /// The raw import specifier as written in source
    pub import_specifier: String,
    /// The resolved file path (if resolution succeeded)
    pub resolved_path: Option<String>,
    /// The language of the source file
    pub language: String,
}

/// A stored import dependency edge between two files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportDependency {
    /// Importer (source file)
    pub file_a: String,
    /// Imported (target file or "unresolved:<path>")
    pub file_b: String,
    /// Dependency type: "use", "import", "require", "from", "include"
    pub dep_type: String,
    /// Raw import statement text
    pub import_statement: String,
    /// What's imported (nullable)
    pub symbol: Option<String>,
    /// True if file_b is a real file path
    pub resolved: bool,
}

/// Statistics about the index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub total_files: u64,
    pub total_chunks: u64,
    pub total_embeddings: u64,
    pub languages: Vec<LanguageStats>,
    pub last_indexed: Option<i64>,
    pub index_size_bytes: u64,
}

/// Per-language statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageStats {
    pub language: String,
    pub file_count: u64,
    pub chunk_count: u64,
}
