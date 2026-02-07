use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::storage::VectorStore;
use crate::types::ChunkType;

/// A symbol definition found in the index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDefinition {
    pub name: String,
    pub chunk_type: ChunkType,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    /// First line of the chunk content (the signature)
    pub signature: String,
}

/// A location where a symbol is used
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolUsage {
    pub file_path: String,
    pub line: u32,
    /// The line of code containing the usage
    pub context: String,
}

/// A symbol's definition and all its usages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRefs {
    pub definition: Option<SymbolDefinition>,
    pub usages: Vec<SymbolUsage>,
}

/// All symbols defined in a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSymbols {
    pub path: String,
    pub symbols: Vec<SymbolDefinition>,
}

/// FTS-based symbol reference resolution.
///
/// Uses exact chunk name matching for definitions and full-text search for usages.
/// This is the fast, good-enough approach (Approach B from design doc)
/// that covers ~80% of use cases. Known limitations:
/// - May produce false positives (symbol name in comments/strings)
/// - Won't find renamed usages (e.g., `use foo as bar`)
/// - Case-sensitive matching
pub struct RefAnalyzer<'a> {
    vector_store: &'a mut VectorStore,
}

impl<'a> RefAnalyzer<'a> {
    pub fn new(vector_store: &'a mut VectorStore) -> Self {
        Self { vector_store }
    }

    /// Find the definition of a symbol by exact name match on indexed chunks.
    pub async fn find_definition(
        &self,
        symbol_name: &str,
        symbol_type: Option<&str>,
        repo: Option<&str>,
    ) -> Result<Option<SymbolDefinition>> {
        let defs = self.find_definitions(symbol_name, symbol_type, repo).await?;
        Ok(defs.into_iter().next())
    }

    /// Find all definitions matching a symbol name.
    ///
    /// Queries indexed chunks where the chunk name exactly matches the symbol name.
    /// If `symbol_type` is provided, also filters by chunk_type.
    async fn find_definitions(
        &self,
        symbol_name: &str,
        symbol_type: Option<&str>,
        repo: Option<&str>,
    ) -> Result<Vec<SymbolDefinition>> {
        let chunks = self
            .vector_store
            .get_chunks_by_name(symbol_name, repo)
            .await?;

        let mut definitions = Vec::new();
        for chunk in chunks {
            // Filter by symbol type if specified
            if let Some(st) = symbol_type {
                let chunk_type_str = chunk.chunk_type.to_string();
                if chunk_type_str != st {
                    continue;
                }
            }

            let signature = chunk
                .content
                .lines()
                .next()
                .unwrap_or("")
                .to_string();

            let name = chunk.name.unwrap_or_default();
            definitions.push(SymbolDefinition {
                name,
                chunk_type: chunk.chunk_type,
                file_path: chunk.file_path,
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                signature,
            });
        }

        Ok(definitions)
    }

    /// Find the definition and usages of a symbol.
    ///
    /// 1. Finds definition(s) via exact chunk name match
    /// 2. Runs FTS search for the symbol name across all chunks
    /// 3. Filters out definition chunk(s)
    /// 4. Extracts specific lines containing the symbol name
    pub async fn find_refs(
        &mut self,
        symbol_name: &str,
        symbol_type: Option<&str>,
        limit: usize,
        repo: Option<&str>,
    ) -> Result<SymbolRefs> {
        // Step 1: Find definition(s) via exact name match
        let definitions = self
            .find_definitions(symbol_name, symbol_type, repo)
            .await?;
        let definition = definitions.first().cloned();

        // Collect definition chunk identifiers to exclude from usages
        let def_keys: Vec<(String, u32, u32)> = definitions
            .iter()
            .map(|d| (d.file_path.clone(), d.start_line, d.end_line))
            .collect();

        // Step 2: FTS search for the symbol name
        let fts_results = self
            .vector_store
            .search_fts(symbol_name, limit * 3, repo)
            .await?;

        // Step 3 & 4: Filter out definitions, extract usage lines
        let mut usages = Vec::new();
        for result in fts_results {
            let chunk = &result.chunk;

            // Skip definition chunks
            let is_def = def_keys.iter().any(|(path, start, end)| {
                chunk.file_path == *path
                    && chunk.start_line == *start
                    && chunk.end_line == *end
            });
            if is_def {
                continue;
            }

            // Extract lines containing the symbol name
            for (i, line) in chunk.content.lines().enumerate() {
                if line.contains(symbol_name) {
                    let line_number = chunk.start_line + i as u32;
                    usages.push(SymbolUsage {
                        file_path: chunk.file_path.clone(),
                        line: line_number,
                        context: line.trim().to_string(),
                    });
                }
            }

            if usages.len() >= limit {
                break;
            }
        }

        usages.truncate(limit);

        Ok(SymbolRefs {
            definition,
            usages,
        })
    }

    /// List all symbols defined in a file.
    ///
    /// Returns all named chunks (functions, structs, traits, etc.) in the file.
    pub async fn list_symbols(
        &self,
        file_path: &str,
        repo: Option<&str>,
    ) -> Result<FileSymbols> {
        let chunks = self
            .vector_store
            .get_chunks_for_file(file_path, repo)
            .await?;

        let symbols = chunks
            .into_iter()
            .filter_map(|chunk| {
                let name = chunk.name?;
                let signature = chunk
                    .content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                Some(SymbolDefinition {
                    name,
                    chunk_type: chunk.chunk_type,
                    file_path: chunk.file_path,
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    signature,
                })
            })
            .collect();

        Ok(FileSymbols {
            path: file_path.to_string(),
            symbols,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::VectorStore;
    use crate::types::{Chunk, ChunkType};
    use tempfile::TempDir;

    fn sample_embedding() -> Vec<f32> {
        vec![0.0f32; 384]
    }

    fn no_contexts(n: usize) -> Vec<Option<String>> {
        vec![None; n]
    }

    /// Helper to create a VectorStore and insert test chunks
    async fn setup_test_store() -> (TempDir, VectorStore) {
        let tmp = TempDir::new().unwrap();
        let lance_path = tmp.path().join("test.lance");
        let mut store = VectorStore::open(&lance_path).await.unwrap();

        let chunks = vec![
            // Definition: parse_config function
            Chunk {
                id: "def-parse-config".to_string(),
                file_path: "src/config.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("parse_config".to_string()),
                start_line: 10,
                end_line: 30,
                content: "pub fn parse_config(path: &Path) -> Result<Config> {\n    let content = std::fs::read_to_string(path)?;\n    toml::from_str(&content)\n}".to_string(),
                language: "rust".to_string(),
            },
            // Another definition: Config struct
            Chunk {
                id: "def-config-struct".to_string(),
                file_path: "src/config.rs".to_string(),
                chunk_type: ChunkType::Struct,
                name: Some("Config".to_string()),
                start_line: 1,
                end_line: 8,
                content: "pub struct Config {\n    pub name: String,\n    pub version: String,\n}".to_string(),
                language: "rust".to_string(),
            },
            // Usage in main.rs
            Chunk {
                id: "usage-main".to_string(),
                file_path: "src/main.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("main".to_string()),
                start_line: 1,
                end_line: 10,
                content: "fn main() -> Result<()> {\n    let config = parse_config(Path::new(\"config.toml\"))?;\n    println!(\"{}\", config.name);\n    Ok(())\n}".to_string(),
                language: "rust".to_string(),
            },
            // Usage in tests
            Chunk {
                id: "usage-test".to_string(),
                file_path: "tests/config_test.rs".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("test_parse_config".to_string()),
                start_line: 5,
                end_line: 15,
                content: "#[test]\nfn test_parse_config() {\n    let config = parse_config(Path::new(\"fixtures/test.toml\")).unwrap();\n    assert_eq!(config.name, \"test\");\n}".to_string(),
                language: "rust".to_string(),
            },
            // A chunk with no name (should be excluded from list_symbols)
            Chunk {
                id: "unnamed-chunk".to_string(),
                file_path: "src/config.rs".to_string(),
                chunk_type: ChunkType::Other,
                name: None,
                start_line: 35,
                end_line: 40,
                content: "// Some comments about configuration".to_string(),
                language: "rust".to_string(),
            },
        ];

        let embeddings: Vec<Vec<f32>> = chunks.iter().map(|_| sample_embedding()).collect();

        store
            .insert(&chunks, &embeddings, &no_contexts(chunks.len()), "test-repo", "abc123", "1234567890")
            .await
            .unwrap();

        (tmp, store)
    }

    #[tokio::test]
    async fn test_find_definition() {
        let (_tmp, mut store) = setup_test_store().await;
        let analyzer = RefAnalyzer::new(&mut store);

        let def = analyzer
            .find_definition("parse_config", None, None)
            .await
            .unwrap();
        assert!(def.is_some());
        let def = def.unwrap();
        assert_eq!(def.name, "parse_config");
        assert_eq!(def.file_path, "src/config.rs");
        assert_eq!(def.start_line, 10);
        assert!(matches!(def.chunk_type, ChunkType::Function));
    }

    #[tokio::test]
    async fn test_find_definition_with_type_filter() {
        let (_tmp, mut store) = setup_test_store().await;
        let analyzer = RefAnalyzer::new(&mut store);

        // Should find Config when filtering by struct
        let def = analyzer
            .find_definition("Config", Some("struct"), None)
            .await
            .unwrap();
        assert!(def.is_some());
        assert_eq!(def.unwrap().chunk_type, ChunkType::Struct);

        // Should NOT find Config when filtering by function
        let def = analyzer
            .find_definition("Config", Some("function"), None)
            .await
            .unwrap();
        assert!(def.is_none());
    }

    #[tokio::test]
    async fn test_find_definition_not_found() {
        let (_tmp, mut store) = setup_test_store().await;
        let analyzer = RefAnalyzer::new(&mut store);

        let def = analyzer
            .find_definition("nonexistent_function", None, None)
            .await
            .unwrap();
        assert!(def.is_none());
    }

    #[tokio::test]
    async fn test_find_definition_signature() {
        let (_tmp, mut store) = setup_test_store().await;
        let analyzer = RefAnalyzer::new(&mut store);

        let def = analyzer
            .find_definition("parse_config", None, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            def.signature,
            "pub fn parse_config(path: &Path) -> Result<Config> {"
        );
    }

    #[tokio::test]
    async fn test_list_symbols() {
        let (_tmp, mut store) = setup_test_store().await;
        let analyzer = RefAnalyzer::new(&mut store);

        let file_symbols = analyzer
            .list_symbols("src/config.rs", None)
            .await
            .unwrap();

        assert_eq!(file_symbols.path, "src/config.rs");
        // Should have parse_config and Config, but NOT the unnamed chunk
        let names: Vec<&str> = file_symbols.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"parse_config"));
        assert!(names.contains(&"Config"));
        assert_eq!(
            file_symbols.symbols.len(),
            2,
            "Should only include named chunks"
        );
    }

    #[tokio::test]
    async fn test_list_symbols_empty_file() {
        let (_tmp, mut store) = setup_test_store().await;
        let analyzer = RefAnalyzer::new(&mut store);

        let file_symbols = analyzer
            .list_symbols("src/nonexistent.rs", None)
            .await
            .unwrap();

        assert_eq!(file_symbols.path, "src/nonexistent.rs");
        assert!(file_symbols.symbols.is_empty());
    }

    #[tokio::test]
    async fn test_list_symbols_sorted_by_line() {
        let (_tmp, mut store) = setup_test_store().await;
        let analyzer = RefAnalyzer::new(&mut store);

        let file_symbols = analyzer
            .list_symbols("src/config.rs", None)
            .await
            .unwrap();

        // get_chunks_for_file sorts by start_line, so Config (line 1) before parse_config (line 10)
        assert_eq!(file_symbols.symbols[0].name, "Config");
        assert_eq!(file_symbols.symbols[1].name, "parse_config");
    }
}
