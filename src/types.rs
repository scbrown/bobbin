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
    Commit,
    Issue,
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
            ChunkType::Commit => write!(f, "commit"),
            ChunkType::Issue => write!(f, "issue"),
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

/// Classification of a file by its role in the project
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    Source,
    Test,
    Documentation,
    Config,
}

impl std::fmt::Display for FileCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileCategory::Source => write!(f, "source"),
            FileCategory::Test => write!(f, "test"),
            FileCategory::Documentation => write!(f, "documentation"),
            FileCategory::Config => write!(f, "config"),
        }
    }
}

/// Classify a file path into a category based on path heuristics.
/// Default is Source (conservative — only well-known patterns trigger other categories).
pub fn classify_file(path: &str) -> FileCategory {
    let lower = path.to_lowercase();
    let parts: Vec<&str> = lower.split('/').collect();
    let filename = parts.last().copied().unwrap_or("");

    // Documentation: known doc filenames and extensions
    let doc_names = [
        "changelog", "changelog.md", "changelog.rst", "changelog.txt",
        "changes", "changes.md", "changes.rst",
        "breaking_changes.md", "breaking_changes.rst",
        "history.md", "history.rst",
        "readme", "readme.md", "readme.rst", "readme.txt",
        "contributing.md", "contributing.rst",
        "license", "license.md", "license.txt",
        "code_of_conduct.md",
    ];
    if doc_names.contains(&filename) {
        return FileCategory::Documentation;
    }

    // Documentation: doc directories
    let doc_dirs = ["docs", "doc", "changelogs", "documentation"];
    for part in &parts[..parts.len().saturating_sub(1)] {
        if doc_dirs.contains(part) {
            // Files in doc dirs with code extensions are still source
            if has_code_extension(filename) {
                return FileCategory::Source;
            }
            return FileCategory::Documentation;
        }
    }

    // Documentation: doc extensions (only if not in a source-like directory)
    let doc_extensions = [".md", ".mdx", ".rst", ".txt"];
    for ext in &doc_extensions {
        if lower.ends_with(ext) {
            return FileCategory::Documentation;
        }
    }

    // Test: test directories and naming patterns
    let test_dirs = ["test", "tests", "spec", "specs", "__tests__", "test_fixtures", "testdata"];
    for part in &parts[..parts.len().saturating_sub(1)] {
        if test_dirs.contains(part) {
            return FileCategory::Test;
        }
    }
    // Test: file naming patterns
    if filename.starts_with("test_")
        || filename.contains("_test.")
        || filename.contains("_spec.")
        || filename.contains(".test.")
        || filename.contains(".spec.")
        || filename.ends_with("_test.rs")
        || filename.ends_with("_test.py")
        || filename.ends_with("_test.go")
    {
        return FileCategory::Test;
    }
    // Snapshot directories
    if parts.iter().any(|p| *p == "__snapshots__" || *p == "snapshots") {
        return FileCategory::Test;
    }

    // Config: known config files and extensions
    let config_names = [
        "cargo.toml", "cargo.lock",
        "package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml",
        "makefile", "justfile",
        ".gitignore", ".gitattributes", ".editorconfig",
        "pyproject.toml", "setup.py", "setup.cfg",
        "tsconfig.json", "babel.config.js", "webpack.config.js",
        "dockerfile", "docker-compose.yml", "docker-compose.yaml",
        ".eslintrc.js", ".eslintrc.json", ".prettierrc",
        "renovate.json", "dependabot.yml",
        "rustfmt.toml", "clippy.toml", ".clippy.toml",
    ];
    if config_names.contains(&filename) {
        return FileCategory::Config;
    }
    // Config: config directories
    if parts.iter().any(|p| *p == ".github" || *p == ".circleci" || *p == ".vscode") {
        return FileCategory::Config;
    }
    // Config: YAML/TOML at root level (1 part = just filename)
    let config_extensions = [".yaml", ".yml", ".toml", ".ini", ".cfg"];
    if parts.len() <= 2 {
        for ext in &config_extensions {
            if lower.ends_with(ext) {
                return FileCategory::Config;
            }
        }
    }

    FileCategory::Source
}

/// Check if a filename has a code extension (used to avoid classifying source in doc dirs)
fn has_code_extension(filename: &str) -> bool {
    let code_extensions = [
        ".rs", ".py", ".js", ".ts", ".tsx", ".jsx", ".go", ".java", ".c", ".cpp", ".h",
        ".hpp", ".cs", ".rb", ".swift", ".kt", ".scala", ".zig", ".hs", ".ml", ".ex",
        ".exs", ".sh", ".bash", ".zsh", ".fish", ".lua", ".r", ".jl", ".pl", ".php",
    ];
    code_extensions.iter().any(|ext| filename.ends_with(ext))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_source_files() {
        assert_eq!(classify_file("src/main.rs"), FileCategory::Source);
        assert_eq!(classify_file("src/cli/hook.rs"), FileCategory::Source);
        assert_eq!(classify_file("crates/ruff_linter/src/rules/mod.rs"), FileCategory::Source);
        assert_eq!(classify_file("lib/parser.py"), FileCategory::Source);
        assert_eq!(classify_file("app/components/Button.tsx"), FileCategory::Source);
        assert_eq!(classify_file("server.go"), FileCategory::Source);
    }

    #[test]
    fn test_classify_documentation_by_name() {
        assert_eq!(classify_file("CHANGELOG.md"), FileCategory::Documentation);
        assert_eq!(classify_file("CHANGELOG"), FileCategory::Documentation);
        assert_eq!(classify_file("CHANGES.md"), FileCategory::Documentation);
        assert_eq!(classify_file("BREAKING_CHANGES.md"), FileCategory::Documentation);
        assert_eq!(classify_file("README.md"), FileCategory::Documentation);
        assert_eq!(classify_file("README"), FileCategory::Documentation);
        assert_eq!(classify_file("CONTRIBUTING.md"), FileCategory::Documentation);
        assert_eq!(classify_file("LICENSE"), FileCategory::Documentation);
        assert_eq!(classify_file("LICENSE.md"), FileCategory::Documentation);
        assert_eq!(classify_file("CODE_OF_CONDUCT.md"), FileCategory::Documentation);
    }

    #[test]
    fn test_classify_documentation_by_directory() {
        assert_eq!(classify_file("docs/guide.md"), FileCategory::Documentation);
        assert_eq!(classify_file("doc/architecture.rst"), FileCategory::Documentation);
        assert_eq!(classify_file("changelogs/0.14.x.md"), FileCategory::Documentation);
        assert_eq!(classify_file("documentation/api.md"), FileCategory::Documentation);
    }

    #[test]
    fn test_classify_source_in_doc_directory() {
        // Code files in docs/ should still be classified as Source
        assert_eq!(classify_file("docs/examples/demo.py"), FileCategory::Source);
        assert_eq!(classify_file("docs/src/helper.rs"), FileCategory::Source);
    }

    #[test]
    fn test_classify_documentation_by_extension() {
        assert_eq!(classify_file("notes.md"), FileCategory::Documentation);
        assert_eq!(classify_file("guide.rst"), FileCategory::Documentation);
        assert_eq!(classify_file("info.txt"), FileCategory::Documentation);
        assert_eq!(classify_file("src/notes.mdx"), FileCategory::Documentation);
    }

    #[test]
    fn test_classify_test_files() {
        assert_eq!(classify_file("tests/test_parser.py"), FileCategory::Test);
        assert_eq!(classify_file("test/helper_test.go"), FileCategory::Test);
        assert_eq!(classify_file("spec/models/user_spec.rb"), FileCategory::Test);
        assert_eq!(classify_file("src/__tests__/button.test.tsx"), FileCategory::Test);
    }

    #[test]
    fn test_classify_test_by_naming() {
        assert_eq!(classify_file("test_utils.py"), FileCategory::Test);
        assert_eq!(classify_file("parser_test.rs"), FileCategory::Test);
        assert_eq!(classify_file("auth_spec.js"), FileCategory::Test);
        assert_eq!(classify_file("button.test.tsx"), FileCategory::Test);
        assert_eq!(classify_file("app.spec.ts"), FileCategory::Test);
    }

    #[test]
    fn test_classify_snapshot_directories() {
        assert_eq!(classify_file("__snapshots__/button.snap"), FileCategory::Test);
        assert_eq!(classify_file("src/__snapshots__/app.snap"), FileCategory::Test);
        assert_eq!(classify_file("snapshots/output.snap"), FileCategory::Test);
    }

    #[test]
    fn test_classify_config_files() {
        assert_eq!(classify_file("Cargo.toml"), FileCategory::Config);
        assert_eq!(classify_file("package.json"), FileCategory::Config);
        assert_eq!(classify_file("Makefile"), FileCategory::Config);
        assert_eq!(classify_file(".gitignore"), FileCategory::Config);
        assert_eq!(classify_file("pyproject.toml"), FileCategory::Config);
        assert_eq!(classify_file("Dockerfile"), FileCategory::Config);
        assert_eq!(classify_file("docker-compose.yml"), FileCategory::Config);
        assert_eq!(classify_file("rustfmt.toml"), FileCategory::Config);
    }

    #[test]
    fn test_classify_config_directories() {
        assert_eq!(classify_file(".github/workflows/ci.yml"), FileCategory::Config);
        assert_eq!(classify_file(".circleci/config.yml"), FileCategory::Config);
        assert_eq!(classify_file(".vscode/settings.json"), FileCategory::Config);
    }

    #[test]
    fn test_classify_root_yaml_as_config() {
        assert_eq!(classify_file("config.yaml"), FileCategory::Config);
        assert_eq!(classify_file("settings.yml"), FileCategory::Config);
    }

    #[test]
    fn test_classify_nested_yaml_as_source() {
        // YAML deep in the tree is likely source/data, not project config
        assert_eq!(classify_file("src/data/schema.yaml"), FileCategory::Source);
        assert_eq!(classify_file("crates/config/fixtures/test.yml"), FileCategory::Source);
    }

    #[test]
    fn test_classify_case_insensitive() {
        assert_eq!(classify_file("CHANGELOG.MD"), FileCategory::Documentation);
        assert_eq!(classify_file("Readme.md"), FileCategory::Documentation);
        assert_eq!(classify_file("TESTS/test_foo.py"), FileCategory::Test);
        assert_eq!(classify_file("cargo.toml"), FileCategory::Config);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", FileCategory::Source), "source");
        assert_eq!(format!("{}", FileCategory::Test), "test");
        assert_eq!(format!("{}", FileCategory::Documentation), "documentation");
        assert_eq!(format!("{}", FileCategory::Config), "config");
    }
}
