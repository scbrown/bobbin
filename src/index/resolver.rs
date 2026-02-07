use std::collections::HashSet;
use std::path::Path;

use crate::types::ImportEdge;

/// Resolve import specifiers to file paths using heuristic matching.
///
/// This does NOT perform full module resolution (no tsconfig paths, no Cargo.toml
/// features, etc.). It uses the set of known indexed files to find likely matches.
pub fn resolve_imports(
    imports: &mut [ImportEdge],
    indexed_files: &HashSet<String>,
    source_root: &Path,
) {
    for edge in imports.iter_mut() {
        edge.resolved_path = resolve_one(&edge.import_specifier, &edge.source_file, &edge.language, indexed_files, source_root);
    }
}

fn resolve_one(
    specifier: &str,
    source_file: &str,
    language: &str,
    indexed_files: &HashSet<String>,
    _source_root: &Path,
) -> Option<String> {
    match language {
        "rust" => resolve_rust(specifier, source_file, indexed_files),
        "typescript" | "tsx" => resolve_typescript(specifier, source_file, indexed_files),
        "python" => resolve_python(specifier, source_file, indexed_files),
        "go" => resolve_go(specifier, indexed_files),
        "java" => resolve_java(specifier, indexed_files),
        "cpp" => resolve_cpp(specifier, source_file, indexed_files),
        _ => None,
    }
}

/// Rust: `crate::foo::bar` → `src/foo/bar.rs` or `src/foo/bar/mod.rs`
fn resolve_rust(specifier: &str, source_file: &str, indexed_files: &HashSet<String>) -> Option<String> {
    // Strip leading path qualifiers
    let path_part = if specifier.starts_with("crate::") {
        specifier.strip_prefix("crate::")
    } else if specifier.starts_with("self::") {
        specifier.strip_prefix("self::")
    } else if specifier.starts_with("super::") {
        // Resolve relative to parent of source file
        let parent = Path::new(source_file).parent()?;
        let rest = specifier.strip_prefix("super::")?;
        let grandparent = parent.parent()?;
        return try_rust_path(&grandparent.to_string_lossy(), rest, indexed_files);
    } else {
        // External crate or nested path — check if first segment is in our tree
        Some(specifier)
    }?;

    // For crate:: paths, try from src/
    if specifier.starts_with("crate::") {
        return try_rust_path("src", path_part, indexed_files);
    }

    // For self:: paths, try relative to current file's directory
    if specifier.starts_with("self::") {
        let parent = Path::new(source_file).parent()?;
        return try_rust_path(&parent.to_string_lossy(), path_part, indexed_files);
    }

    // For bare paths, try as top-level module
    try_rust_path("src", path_part, indexed_files)
}

fn try_rust_path(base: &str, path_part: &str, indexed_files: &HashSet<String>) -> Option<String> {
    // Take only the first few segments (module path, not the imported item)
    let segments: Vec<&str> = path_part.split("::").collect();

    // Try progressively longer prefixes
    for len in (1..=segments.len()).rev() {
        let module_path = segments[..len].join("/");

        // Try as file.rs
        let candidate = format!("{}/{}.rs", base, module_path);
        if indexed_files.contains(&candidate) {
            return Some(candidate);
        }

        // Try as directory/mod.rs
        let candidate = format!("{}/{}/mod.rs", base, module_path);
        if indexed_files.contains(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// TypeScript: `./foo/bar` → `foo/bar.ts`, `foo/bar/index.ts`, etc.
fn resolve_typescript(specifier: &str, source_file: &str, indexed_files: &HashSet<String>) -> Option<String> {
    // Only resolve relative imports
    if !specifier.starts_with('.') {
        return None;
    }

    let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));

    // Normalize the relative path
    let resolved = source_dir.join(specifier);
    let normalized = normalize_path(&resolved);

    let extensions = ["", ".ts", ".tsx", ".js", ".jsx", ".mjs"];
    let index_files = ["/index.ts", "/index.tsx", "/index.js", "/index.jsx"];

    for ext in &extensions {
        let candidate = format!("{}{}", normalized, ext);
        if indexed_files.contains(&candidate) {
            return Some(candidate);
        }
    }

    for idx in &index_files {
        let candidate = format!("{}{}", normalized, idx);
        if indexed_files.contains(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Python: `foo.bar.baz` → `foo/bar/baz.py` or `foo/bar/baz/__init__.py`
fn resolve_python(specifier: &str, source_file: &str, indexed_files: &HashSet<String>) -> Option<String> {
    // Handle relative imports (leading dots)
    let (dots, module) = count_leading_dots(specifier);

    if dots > 0 {
        // Relative import
        let mut base = Path::new(source_file).parent()?;
        for _ in 1..dots {
            base = base.parent()?;
        }
        let path_part = module.replace('.', "/");
        return try_python_path(&base.to_string_lossy(), &path_part, indexed_files);
    }

    // Absolute import
    let path_part = specifier.replace('.', "/");
    try_python_path("", &path_part, indexed_files)
}

fn count_leading_dots(s: &str) -> (usize, &str) {
    let dots = s.chars().take_while(|&c| c == '.').count();
    (dots, &s[dots..])
}

fn try_python_path(base: &str, path_part: &str, indexed_files: &HashSet<String>) -> Option<String> {
    if path_part.is_empty() {
        // `from . import something` — refers to __init__.py in the directory
        let candidate = if base.is_empty() {
            "__init__.py".to_string()
        } else {
            format!("{}/__init__.py", base)
        };
        if indexed_files.contains(&candidate) {
            return Some(candidate);
        }
        return None;
    }

    let full = if base.is_empty() {
        path_part.to_string()
    } else {
        format!("{}/{}", base, path_part)
    };

    // Try as .py file
    let candidate = format!("{}.py", full);
    if indexed_files.contains(&candidate) {
        return Some(candidate);
    }

    // Try as package (__init__.py)
    let candidate = format!("{}/__init__.py", full);
    if indexed_files.contains(&candidate) {
        return Some(candidate);
    }

    None
}

/// Go: package imports map to directory paths, not individual files.
/// We resolve to the first file found in the matching directory.
fn resolve_go(specifier: &str, indexed_files: &HashSet<String>) -> Option<String> {
    // Standard library and external packages won't resolve
    // Only resolve local packages (those that match indexed paths)
    let last_segment = specifier.rsplit('/').next()?;

    // Look for any .go file in a directory matching the import path suffix
    for file in indexed_files {
        if file.ends_with(".go") {
            let parent = Path::new(file).parent()?.to_string_lossy();
            if parent.ends_with(last_segment) || parent == last_segment {
                return Some(file.clone());
            }
        }
    }

    None
}

/// Java: `java.util.List` — only resolve project-local classes
fn resolve_java(specifier: &str, indexed_files: &HashSet<String>) -> Option<String> {
    // Convert dots to path separators
    let path = specifier.replace('.', "/");

    // Try as direct file
    let candidate = format!("{}.java", path);
    if indexed_files.contains(&candidate) {
        return Some(candidate);
    }

    // Try with src/main/java prefix (Maven convention)
    let candidate = format!("src/main/java/{}.java", path);
    if indexed_files.contains(&candidate) {
        return Some(candidate);
    }

    None
}

/// C/C++: `#include "foo.h"` → resolve relative to source file or project root
fn resolve_cpp(specifier: &str, source_file: &str, indexed_files: &HashSet<String>) -> Option<String> {
    // Direct match in indexed files
    if indexed_files.contains(specifier) {
        return Some(specifier.to_string());
    }

    // Relative to source file's directory
    let source_dir = Path::new(source_file).parent().unwrap_or(Path::new(""));
    let candidate = source_dir.join(specifier);
    let normalized = normalize_path(&candidate);
    if indexed_files.contains(&normalized) {
        return Some(normalized);
    }

    // Try common include directories
    for prefix in &["include", "src", "lib"] {
        let candidate = format!("{}/{}", prefix, specifier);
        if indexed_files.contains(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// Normalize a path by resolving `.` and `..` components (without filesystem access)
fn normalize_path(path: &Path) -> String {
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::Normal(s) => {
                components.push(s.to_string_lossy().to_string());
            }
            _ => {}
        }
    }
    components.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_files(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_resolve_rust_crate_import() {
        let files = make_files(&["src/types.rs", "src/index/parser.rs", "src/index/mod.rs"]);
        let result = resolve_rust("crate::types::Chunk", "src/main.rs", &files);
        assert_eq!(result, Some("src/types.rs".to_string()));
    }

    #[test]
    fn test_resolve_rust_mod() {
        let files = make_files(&["src/index/mod.rs", "src/index/parser.rs"]);
        let result = resolve_rust("crate::index", "src/main.rs", &files);
        assert_eq!(result, Some("src/index/mod.rs".to_string()));
    }

    #[test]
    fn test_resolve_typescript_relative() {
        let files = make_files(&["src/utils.ts", "src/components/Button.tsx"]);
        let result = resolve_typescript("./utils", "src/index.ts", &files);
        assert_eq!(result, Some("src/utils.ts".to_string()));
    }

    #[test]
    fn test_resolve_typescript_parent() {
        let files = make_files(&["src/utils.ts", "src/components/Button.tsx"]);
        let result = resolve_typescript("../utils", "src/components/Button.tsx", &files);
        assert_eq!(result, Some("src/utils.ts".to_string()));
    }

    #[test]
    fn test_resolve_typescript_external_skipped() {
        let files = make_files(&["src/index.ts"]);
        let result = resolve_typescript("react", "src/index.ts", &files);
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_python_absolute() {
        let files = make_files(&["foo/bar.py", "foo/__init__.py"]);
        let result = resolve_python("foo.bar", "main.py", &files);
        assert_eq!(result, Some("foo/bar.py".to_string()));
    }

    #[test]
    fn test_resolve_python_relative() {
        let files = make_files(&["pkg/utils.py", "pkg/core.py"]);
        let result = resolve_python(".utils", "pkg/core.py", &files);
        assert_eq!(result, Some("pkg/utils.py".to_string()));
    }

    #[test]
    fn test_resolve_cpp_include() {
        let files = make_files(&["include/mylib.h", "src/main.cpp"]);
        let result = resolve_cpp("mylib.h", "src/main.cpp", &files);
        assert_eq!(result, Some("include/mylib.h".to_string()));
    }

    #[test]
    fn test_resolve_java_class() {
        let files = make_files(&["com/example/Foo.java"]);
        let result = resolve_java("com.example.Foo", &files);
        assert_eq!(result, Some("com/example/Foo.java".to_string()));
    }
}
