use anyhow::Result;
use tree_sitter::{Language, Node};

use crate::types::ChunkType;

/// Complexity metrics for a single AST chunk
#[derive(Debug, Clone)]
pub struct ComplexityMetrics {
    /// Maximum nesting depth
    pub ast_depth: u32,
    /// Total AST nodes
    pub node_count: u32,
    /// Branch points count (base 1 + branches)
    pub cyclomatic: u32,
    /// Normalized combined score [0, 1]
    pub combined: f32,
}

/// File-level complexity summary
#[derive(Debug, Clone)]
pub struct FileComplexity {
    pub path: String,
    /// Average of chunk complexities, weighted by size
    pub complexity: f32,
    pub chunk_count: usize,
    pub chunks: Vec<ChunkComplexity>,
}

/// Complexity metrics for a single chunk within a file
#[derive(Debug, Clone)]
pub struct ChunkComplexity {
    pub name: Option<String>,
    pub chunk_type: ChunkType,
    pub start_line: u32,
    pub end_line: u32,
    pub metrics: ComplexityMetrics,
}

/// Analyzes AST complexity metrics from Tree-sitter parse trees
pub struct ComplexityAnalyzer {
    rust_parser: tree_sitter::Parser,
    typescript_parser: tree_sitter::Parser,
    python_parser: tree_sitter::Parser,
    go_parser: tree_sitter::Parser,
    java_parser: tree_sitter::Parser,
    cpp_parser: tree_sitter::Parser,
}

impl ComplexityAnalyzer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            rust_parser: create_parser(tree_sitter_rust::LANGUAGE.into())?,
            typescript_parser: create_parser(
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            )?,
            python_parser: create_parser(tree_sitter_python::LANGUAGE.into())?,
            go_parser: create_parser(tree_sitter_go::LANGUAGE.into())?,
            java_parser: create_parser(tree_sitter_java::LANGUAGE.into())?,
            cpp_parser: create_parser(tree_sitter_cpp::LANGUAGE.into())?,
        })
    }

    /// Compute complexity for a single chunk's content.
    pub fn analyze_chunk(&mut self, content: &str, language: &str) -> Result<ComplexityMetrics> {
        let parser = match language {
            "rust" => &mut self.rust_parser,
            "typescript" | "tsx" | "javascript" => &mut self.typescript_parser,
            "python" => &mut self.python_parser,
            "go" => &mut self.go_parser,
            "java" => &mut self.java_parser,
            "cpp" => &mut self.cpp_parser,
            _ => anyhow::bail!("Unsupported language for complexity analysis: {}", language),
        };

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse content"))?;

        let root = tree.root_node();

        let ast_depth = compute_max_depth(&root);
        let node_count = compute_node_count(&root);
        let cyclomatic = 1 + count_branch_points(&root, language);
        let combined = compute_combined_score(ast_depth, node_count, cyclomatic);

        Ok(ComplexityMetrics {
            ast_depth,
            node_count,
            cyclomatic,
            combined,
        })
    }

    /// Compute file-level complexity by parsing the file and analyzing each semantic chunk.
    pub fn analyze_file(
        &mut self,
        path: &str,
        content: &str,
        language: &str,
    ) -> Result<FileComplexity> {
        let parser = match language {
            "rust" => &mut self.rust_parser,
            "typescript" | "tsx" | "javascript" => &mut self.typescript_parser,
            "python" => &mut self.python_parser,
            "go" => &mut self.go_parser,
            "java" => &mut self.java_parser,
            "cpp" => &mut self.cpp_parser,
            _ => anyhow::bail!("Unsupported language for complexity analysis: {}", language),
        };

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse file: {}", path))?;

        let root = tree.root_node();
        let mut chunks = Vec::new();
        collect_chunk_complexities(&root, content, language, &mut chunks);

        // If no semantic chunks found, treat the whole file as one chunk
        if chunks.is_empty() {
            let ast_depth = compute_max_depth(&root);
            let node_count = compute_node_count(&root);
            let cyclomatic = 1 + count_branch_points(&root, language);
            let combined = compute_combined_score(ast_depth, node_count, cyclomatic);

            let total_lines = content.lines().count() as u32;
            chunks.push(ChunkComplexity {
                name: None,
                chunk_type: ChunkType::Other,
                start_line: 1,
                end_line: total_lines.max(1),
                metrics: ComplexityMetrics {
                    ast_depth,
                    node_count,
                    cyclomatic,
                    combined,
                },
            });
        }

        // Weighted average: weight each chunk by its line count
        let total_lines: u32 = chunks
            .iter()
            .map(|c| (c.end_line - c.start_line + 1).max(1))
            .sum();

        let complexity = if total_lines == 0 {
            0.0
        } else {
            chunks
                .iter()
                .map(|c| {
                    let lines = (c.end_line - c.start_line + 1).max(1) as f32;
                    c.metrics.combined * lines
                })
                .sum::<f32>()
                / total_lines as f32
        };

        Ok(FileComplexity {
            path: path.to_string(),
            complexity,
            chunk_count: chunks.len(),
            chunks,
        })
    }
}

fn create_parser(language: Language) -> Result<tree_sitter::Parser> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language)?;
    Ok(parser)
}

/// Compute maximum nesting depth of the AST
fn compute_max_depth(node: &Node) -> u32 {
    let mut max_child_depth = 0u32;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let child_depth = compute_max_depth(&child);
        max_child_depth = max_child_depth.max(child_depth);
    }
    max_child_depth + 1
}

/// Count total nodes in the AST
fn compute_node_count(node: &Node) -> u32 {
    let mut count = 1u32;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += compute_node_count(&child);
    }
    count
}

/// Count branch point nodes for cyclomatic complexity
fn count_branch_points(node: &Node, language: &str) -> u32 {
    let mut count = 0u32;

    if is_branch_point(node, language) {
        count += 1;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_branch_points(&child, language);
    }
    count
}

/// Determine if a node is a branch point for the given language
fn is_branch_point(node: &Node, language: &str) -> bool {
    let kind = node.kind();

    match language {
        "rust" => matches!(
            kind,
            "if_expression"
                | "match_expression"
                | "for_expression"
                | "while_expression"
                | "loop_expression"
        ) || is_rust_logical_op(node),
        "typescript" | "tsx" | "javascript" => matches!(
            kind,
            "if_statement"
                | "switch_case"
                | "for_statement"
                | "while_statement"
                | "do_statement"
                | "ternary_expression"
                | "for_in_statement"
        ) || is_ts_logical_op(node),
        "python" => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "while_statement"
                | "elif_clause"
        ) || is_python_logical_op(node),
        "go" => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "select_statement"
                | "case_clause"
                | "default_case"
        ) || is_go_logical_op(node),
        "java" => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "enhanced_for_statement"
                | "while_statement"
                | "do_statement"
                | "switch_expression"
                | "catch_clause"
                | "ternary_expression"
        ) || is_java_logical_op(node),
        "cpp" => matches!(
            kind,
            "if_statement"
                | "for_statement"
                | "while_statement"
                | "do_statement"
                | "case_statement"
                | "catch_clause"
                | "conditional_expression"
        ) || is_cpp_logical_op(node),
        _ => false,
    }
}

/// Check if a Rust binary_expression uses || or &&
fn is_rust_logical_op(node: &Node) -> bool {
    if node.kind() != "binary_expression" {
        return false;
    }
    if let Some(op) = node.child_by_field_name("operator") {
        let op_kind = op.kind();
        return op_kind == "||" || op_kind == "&&";
    }
    false
}

/// Check if a TS/JS binary_expression uses ||, &&, or ??
fn is_ts_logical_op(node: &Node) -> bool {
    if node.kind() != "binary_expression" {
        return false;
    }
    if let Some(op) = node.child_by_field_name("operator") {
        let op_kind = op.kind();
        return op_kind == "||" || op_kind == "&&" || op_kind == "??";
    }
    false
}

/// Check if a Python node is an `and` or `or` boolean_operator
fn is_python_logical_op(node: &Node) -> bool {
    if node.kind() != "boolean_operator" {
        return false;
    }
    if let Some(op) = node.child_by_field_name("operator") {
        let op_kind = op.kind();
        return op_kind == "and" || op_kind == "or";
    }
    false
}

/// Check if a Go binary_expression uses || or &&
fn is_go_logical_op(node: &Node) -> bool {
    if node.kind() != "binary_expression" {
        return false;
    }
    if let Some(op) = node.child_by_field_name("operator") {
        let op_kind = op.kind();
        return op_kind == "||" || op_kind == "&&";
    }
    false
}

/// Check if a Java binary_expression uses || or &&
fn is_java_logical_op(node: &Node) -> bool {
    if node.kind() != "binary_expression" {
        return false;
    }
    if let Some(op) = node.child_by_field_name("operator") {
        let op_kind = op.kind();
        return op_kind == "||" || op_kind == "&&";
    }
    false
}

/// Check if a C++ binary_expression uses || or &&
fn is_cpp_logical_op(node: &Node) -> bool {
    if node.kind() != "binary_expression" {
        return false;
    }
    if let Some(op) = node.child_by_field_name("operator") {
        let op_kind = op.kind();
        return op_kind == "||" || op_kind == "&&";
    }
    false
}

/// Normalize and combine metrics into a [0, 1] score.
/// Weights: 0.4 * cyclomatic_norm + 0.3 * depth_norm + 0.3 * node_count_norm
fn compute_combined_score(ast_depth: u32, node_count: u32, cyclomatic: u32) -> f32 {
    let depth_norm = (ast_depth as f32 / 10.0).min(1.0);
    let cyclomatic_norm = (cyclomatic as f32 / 20.0).min(1.0);
    let node_count_norm = (node_count as f32 / 200.0).min(1.0);

    let score = 0.4 * cyclomatic_norm + 0.3 * depth_norm + 0.3 * node_count_norm;
    score.min(1.0)
}

/// Walk the AST and collect complexity for each semantic chunk (function, method, class, etc.)
fn collect_chunk_complexities(
    node: &Node,
    content: &str,
    language: &str,
    chunks: &mut Vec<ChunkComplexity>,
) {
    if let Some(chunk_type) = node_to_chunk_type(node, language) {
        let name = extract_name(node, content, language);
        let start_line = node.start_position().row as u32 + 1;
        let end_line = node.end_position().row as u32 + 1;

        let ast_depth = compute_max_depth(node);
        let node_count = compute_node_count(node);
        let cyclomatic = 1 + count_branch_points(node, language);
        let combined = compute_combined_score(ast_depth, node_count, cyclomatic);

        chunks.push(ChunkComplexity {
            name,
            chunk_type,
            start_line,
            end_line,
            metrics: ComplexityMetrics {
                ast_depth,
                node_count,
                cyclomatic,
                combined,
            },
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_chunk_complexities(&child, content, language, chunks);
    }
}

/// Map a tree-sitter node to a chunk type (mirrors parser.rs logic)
fn node_to_chunk_type(node: &Node, language: &str) -> Option<ChunkType> {
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
        "typescript" | "tsx" | "javascript" => match kind {
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
            "type_declaration" => Some(ChunkType::Struct),
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

/// Extract name from a semantic node (mirrors parser.rs logic)
fn extract_name(node: &Node, content: &str, language: &str) -> Option<String> {
    match language {
        "rust" | "typescript" | "tsx" | "javascript" | "python" | "java" => {
            node.child_by_field_name("name")
                .map(|n| content[n.byte_range()].to_string())
        }
        "go" => {
            if node.kind() == "type_declaration" {
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
            if let Some(name_node) = node.child_by_field_name("name") {
                return Some(content[name_node.byte_range()].to_string());
            }
            if let Some(declarator) = node.child_by_field_name("declarator") {
                if let Some(inner) = declarator.child_by_field_name("declarator") {
                    return Some(content[inner.byte_range()].to_string());
                }
                if declarator.kind() == "identifier" {
                    return Some(content[declarator.byte_range()].to_string());
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_rust_function() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"fn hello() {
    println!("Hello, world!");
}"#;
        let metrics = analyzer.analyze_chunk(content, "rust").unwrap();

        // Simple function: no branches, cyclomatic = 1
        assert_eq!(metrics.cyclomatic, 1);
        assert!(metrics.ast_depth > 0);
        assert!(metrics.node_count > 0);
        assert!(metrics.combined >= 0.0 && metrics.combined <= 1.0);
    }

    #[test]
    fn test_rust_function_with_branches() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"fn complex(x: i32, y: bool) -> i32 {
    if x > 0 {
        if y {
            for i in 0..x {
                println!("{}", i);
            }
            42
        } else {
            0
        }
    } else {
        match x {
            -1 => -1,
            _ => -2,
        }
    }
}"#;
        let metrics = analyzer.analyze_chunk(content, "rust").unwrap();

        // Branches: if, if, for, match = 4 branch points, cyclomatic = 5
        assert_eq!(metrics.cyclomatic, 5);
        assert!(metrics.ast_depth >= 5);
        assert!(metrics.combined > 0.0);
    }

    #[test]
    fn test_rust_logical_operators() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"fn check(a: bool, b: bool, c: bool) -> bool {
    if a && b || c {
        true
    } else {
        false
    }
}"#;
        let metrics = analyzer.analyze_chunk(content, "rust").unwrap();

        // if + && + || = 3 branch points, cyclomatic = 4
        assert_eq!(metrics.cyclomatic, 4);
    }

    #[test]
    fn test_typescript_complexity() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"function process(items: number[]): number {
    let total = 0;
    for (let i = 0; i < items.length; i++) {
        if (items[i] > 10) {
            total += items[i];
        } else if (items[i] > 0) {
            total += 1;
        }
    }
    return total;
}"#;
        let metrics = analyzer.analyze_chunk(content, "typescript").unwrap();

        // for + if + if = 3 branch points, cyclomatic = 4
        assert_eq!(metrics.cyclomatic, 4);
        assert!(metrics.combined > 0.0);
    }

    #[test]
    fn test_typescript_ternary_and_nullish() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"function getValue(x: number | null): number {
    const val = x ?? 0;
    return val > 0 ? val : -val;
}"#;
        let metrics = analyzer.analyze_chunk(content, "typescript").unwrap();

        // ?? + ternary = 2 branch points, cyclomatic = 3
        assert_eq!(metrics.cyclomatic, 3);
    }

    #[test]
    fn test_python_complexity() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"def process(items):
    total = 0
    for item in items:
        if item > 10:
            total += item
        elif item > 0:
            total += 1
    return total
"#;
        let metrics = analyzer.analyze_chunk(content, "python").unwrap();

        // for + if + elif = 3 branch points, cyclomatic = 4
        assert_eq!(metrics.cyclomatic, 4);
    }

    #[test]
    fn test_python_logical_operators() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"def check(a, b, c):
    if a and b or c:
        return True
    return False
"#;
        let metrics = analyzer.analyze_chunk(content, "python").unwrap();

        // if + and + or = 3 branch points, cyclomatic = 4
        assert_eq!(metrics.cyclomatic, 4);
    }

    #[test]
    fn test_go_complexity() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"package main

func process(items []int) int {
    total := 0
    for _, item := range items {
        if item > 10 {
            total += item
        }
    }
    return total
}
"#;
        let metrics = analyzer.analyze_chunk(content, "go").unwrap();

        // for + if = 2 branch points, cyclomatic = 3
        assert_eq!(metrics.cyclomatic, 3);
    }

    #[test]
    fn test_normalization_bounds() {
        // Low complexity
        let score = compute_combined_score(2, 10, 1);
        assert!(score >= 0.0 && score <= 1.0);

        // High complexity
        let score = compute_combined_score(15, 300, 25);
        assert!(score >= 0.0 && score <= 1.0);
        assert!((score - 1.0).abs() < 0.001); // Should be capped at 1.0

        // Zero
        let score = compute_combined_score(0, 0, 0);
        assert!((score - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_file_level_complexity_rust() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"fn simple() {
    println!("hello");
}

fn complex(x: i32) -> i32 {
    if x > 0 {
        for i in 0..x {
            if i % 2 == 0 {
                println!("{}", i);
            }
        }
        x
    } else {
        -x
    }
}
"#;
        let file = analyzer.analyze_file("test.rs", content, "rust").unwrap();

        assert_eq!(file.path, "test.rs");
        assert_eq!(file.chunk_count, 2);
        assert!(file.complexity >= 0.0 && file.complexity <= 1.0);

        // First chunk (simple) should have lower complexity than second
        assert!(file.chunks[0].metrics.cyclomatic < file.chunks[1].metrics.cyclomatic);
        assert_eq!(file.chunks[0].name, Some("simple".to_string()));
        assert_eq!(file.chunks[1].name, Some("complex".to_string()));
    }

    #[test]
    fn test_file_level_complexity_python() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"class Calculator:
    def add(self, a, b):
        return a + b

    def divide(self, a, b):
        if b == 0:
            raise ValueError("Cannot divide by zero")
        return a / b
"#;
        let file = analyzer
            .analyze_file("calc.py", content, "python")
            .unwrap();

        assert_eq!(file.path, "calc.py");
        // class + 2 functions = 3 chunks
        assert_eq!(file.chunk_count, 3);
        assert!(file.complexity >= 0.0 && file.complexity <= 1.0);
    }

    #[test]
    fn test_unsupported_language() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let result = analyzer.analyze_chunk("some content", "haskell");
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_max_depth() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let content = "fn f() { }";
        let tree = parser.parse(content, None).unwrap();
        let depth = compute_max_depth(&tree.root_node());
        assert!(depth >= 3); // source_file > function_item > block > ...
    }

    #[test]
    fn test_compute_node_count() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let content = "fn f() { }";
        let tree = parser.parse(content, None).unwrap();
        let count = compute_node_count(&tree.root_node());
        assert!(count >= 4); // source_file, function_item, identifier, parameters, block
    }

    #[test]
    fn test_java_complexity() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"public class Example {
    public int process(int[] items) {
        int total = 0;
        for (int item : items) {
            if (item > 10) {
                total += item;
            } else if (item > 0) {
                total += 1;
            }
        }
        return total;
    }
}
"#;
        let metrics = analyzer.analyze_chunk(content, "java").unwrap();

        // for + if + if = 3, cyclomatic = 4
        assert_eq!(metrics.cyclomatic, 4);
    }

    #[test]
    fn test_cpp_complexity() {
        let mut analyzer = ComplexityAnalyzer::new().unwrap();
        let content = r#"int process(int* items, int len) {
    int total = 0;
    for (int i = 0; i < len; i++) {
        if (items[i] > 10) {
            total += items[i];
        } else if (items[i] > 0) {
            total += 1;
        }
    }
    return total;
}
"#;
        let metrics = analyzer.analyze_chunk(content, "cpp").unwrap();

        // for + if + if = 3, cyclomatic = 4
        assert_eq!(metrics.cyclomatic, 4);
    }
}
