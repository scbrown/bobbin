//! Error output parser: extract file paths and symbols from compiler/test output.
//!
//! Parses error messages from cargo, go, pytest, tsc, gcc/clang, and generic
//! patterns to extract referenced file paths with optional line numbers.

use regex::Regex;
use std::collections::BTreeMap;

/// A file reference extracted from error output.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ErrorRef {
    /// File path as it appeared in the error (relative or absolute).
    pub path: String,
    /// Line number, if available.
    pub line: Option<u32>,
    /// Symbol name, if available (function, type, etc.).
    pub symbol: Option<String>,
}

/// Parsed error context from build/test output.
#[derive(Debug, Clone)]
pub struct ParsedErrors {
    /// File references extracted from the error output, deduplicated.
    /// Keyed by path to avoid duplicates — keeps the first line reference per file.
    pub refs: Vec<ErrorRef>,
    /// Whether we detected this as a build/test error (vs. generic failure).
    pub is_build_error: bool,
}

/// Check if a Bash command looks like a build or test command.
pub fn is_build_test_command(command: &str) -> bool {
    let cmd = command.trim();
    let patterns = [
        // Rust
        "cargo build", "cargo test", "cargo check", "cargo clippy", "cargo run",
        // Go
        "go build", "go test", "go vet", "go run",
        // Python
        "pytest", "python -m pytest", "python3 -m pytest", "python -m unittest",
        "python3 -m unittest",
        // Node/TypeScript
        "npm test", "npm run test", "npm run build", "npx tsc", "tsc ",
        "yarn test", "yarn build", "pnpm test", "pnpm build",
        "node ", "deno test", "bun test",
        // C/C++
        "make", "cmake", "gcc ", "g++ ", "clang ", "clang++ ",
        // General
        "mvn ", "gradle ", "dotnet build", "dotnet test",
        "golangci-lint",
    ];
    patterns.iter().any(|p| cmd.starts_with(p) || cmd.contains(&format!(" && {}", p)))
}

/// Parse error output to extract file references.
///
/// Applies format-specific parsers in order of specificity, then deduplicates.
pub fn parse_error_output(error: &str, command: &str) -> ParsedErrors {
    let is_build = is_build_test_command(command);
    let mut refs_map: BTreeMap<String, ErrorRef> = BTreeMap::new();

    // Apply parsers — most specific first
    let all_refs = if command.contains("cargo") || command.contains("rustc") {
        parse_rust_errors(error)
    } else if command.contains("go ") || command.contains("golangci") {
        parse_go_errors(error)
    } else if command.contains("pytest") || command.contains("python") {
        parse_python_errors(error)
    } else if command.contains("tsc") || command.contains("node") || command.contains("deno") || command.contains("bun") {
        parse_typescript_errors(error)
    } else {
        // Generic fallback: try all parsers
        let mut combined = parse_rust_errors(error);
        combined.extend(parse_go_errors(error));
        combined.extend(parse_python_errors(error));
        combined.extend(parse_typescript_errors(error));
        combined.extend(parse_generic_file_refs(error));
        combined
    };

    // Always add generic refs as fallback
    let mut final_refs = all_refs;
    if final_refs.is_empty() {
        final_refs = parse_generic_file_refs(error);
    }

    // Deduplicate: keep first line reference per file path
    for r in final_refs {
        refs_map.entry(r.path.clone()).or_insert(r);
    }

    ParsedErrors {
        refs: refs_map.into_values().collect(),
        is_build_error: is_build,
    }
}

/// Parse Rust compiler errors.
/// Format: `error[E0425]: ... --> src/main.rs:42:5`
fn parse_rust_errors(error: &str) -> Vec<ErrorRef> {
    let mut refs = Vec::new();
    // Match "--> file:line:col" pattern (primary error location)
    let re = Regex::new(r"-->\s+([^\s:]+):(\d+):\d+").unwrap();
    for cap in re.captures_iter(error) {
        refs.push(ErrorRef {
            path: cap[1].to_string(),
            line: cap[2].parse().ok(),
            symbol: None,
        });
    }

    // Extract symbol names from "cannot find value/type/function `name`"
    let sym_re = Regex::new(r"cannot find (?:value|type|function|trait|module|struct|macro) `(\w+)`").unwrap();
    for cap in sym_re.captures_iter(error) {
        // Attach symbol to the most recent file ref
        if let Some(last) = refs.last_mut() {
            if last.symbol.is_none() {
                last.symbol = Some(cap[1].to_string());
            }
        }
    }

    // Also match "test module::test_name ... FAILED"
    let test_re = Regex::new(r"test (\S+) \.\.\. FAILED").unwrap();
    for cap in test_re.captures_iter(error) {
        let test_path = &cap[1];
        // Convert module::test_name to likely file path
        if let Some(module) = test_path.rsplit("::").nth(1) {
            // test_name -> symbol, module path -> hint
            refs.push(ErrorRef {
                path: module.replace("::", "/"),
                line: None,
                symbol: test_path.rsplit("::").next().map(|s| s.to_string()),
            });
        }
    }

    refs
}

/// Parse Go compiler/test errors.
/// Format: `./main.go:42:5: undefined: someFunc`
fn parse_go_errors(error: &str) -> Vec<ErrorRef> {
    let mut refs = Vec::new();
    let re = Regex::new(r"([./][\w/.-]+\.go):(\d+)(?::\d+)?:?\s*(.*)").unwrap();
    for cap in re.captures_iter(error) {
        let msg = cap.get(3).map_or("", |m| m.as_str());
        let symbol = if msg.contains("undefined:") {
            msg.split("undefined:").nth(1).map(|s| s.trim().to_string())
        } else {
            None
        };
        refs.push(ErrorRef {
            path: cap[1].to_string(),
            line: cap[2].parse().ok(),
            symbol,
        });
    }

    // Go test failures: "--- FAIL: TestName (0.00s)"
    let test_re = Regex::new(r"--- FAIL: (\w+)").unwrap();
    for cap in test_re.captures_iter(error) {
        refs.push(ErrorRef {
            path: String::new(), // No file path from this pattern
            line: None,
            symbol: Some(cap[1].to_string()),
        });
    }

    refs.retain(|r| !r.path.is_empty() || r.symbol.is_some());
    refs
}

/// Parse Python/pytest errors.
/// Format: `FAILED test_foo.py::TestClass::test_method`
/// Format: `  File "path/to/file.py", line 42, in func_name`
fn parse_python_errors(error: &str) -> Vec<ErrorRef> {
    let mut refs = Vec::new();

    // pytest FAILED lines
    let pytest_re = Regex::new(r"FAILED\s+([^\s:]+\.py)::(\S+)").unwrap();
    for cap in pytest_re.captures_iter(error) {
        refs.push(ErrorRef {
            path: cap[1].to_string(),
            line: None,
            symbol: Some(cap[2].to_string()),
        });
    }

    // Python traceback: File "path", line N
    let tb_re = Regex::new(r#"File "([^"]+\.py)", line (\d+)(?:, in (\w+))?"#).unwrap();
    for cap in tb_re.captures_iter(error) {
        let path = &cap[1];
        // Skip stdlib/site-packages
        if path.contains("site-packages") || path.contains("/lib/python") {
            continue;
        }
        refs.push(ErrorRef {
            path: path.to_string(),
            line: cap[2].parse().ok(),
            symbol: cap.get(3).map(|m| m.as_str().to_string()),
        });
    }

    refs
}

/// Parse TypeScript/tsc errors.
/// Format: `src/index.ts(42,5): error TS2304: Cannot find name 'foo'.`
/// Also: Node.js stack traces like `at Object.<anonymous> (/path/to/file.js:42:5)`
fn parse_typescript_errors(error: &str) -> Vec<ErrorRef> {
    let mut refs = Vec::new();

    // tsc format: file(line,col): error TSxxxx
    let tsc_re = Regex::new(r"([^\s(]+\.(?:ts|tsx|js|jsx))\((\d+),\d+\):\s*error").unwrap();
    for cap in tsc_re.captures_iter(error) {
        refs.push(ErrorRef {
            path: cap[1].to_string(),
            line: cap[2].parse().ok(),
            symbol: None,
        });
    }

    // Node stack trace: at ... (file:line:col) or at ... file:line:col
    let stack_re = Regex::new(r"at\s+\S+\s+\(?([^\s():]+\.(?:ts|tsx|js|jsx)):(\d+):\d+\)?").unwrap();
    for cap in stack_re.captures_iter(error) {
        let path = &cap[1];
        if path.contains("node_modules") || path.contains("internal/") {
            continue;
        }
        refs.push(ErrorRef {
            path: path.to_string(),
            line: cap[2].parse().ok(),
            symbol: None,
        });
    }

    refs
}

/// Generic file reference extraction — works across languages.
/// Matches patterns like `file.ext:line:col` or `file.ext:line`.
fn parse_generic_file_refs(error: &str) -> Vec<ErrorRef> {
    let mut refs = Vec::new();

    // Generic: filepath:line or filepath:line:col
    // Must look like a source file (has a recognized extension)
    let re = Regex::new(
        r"([./]?[\w/.-]+\.(?:rs|go|py|ts|tsx|js|jsx|java|kt|c|cpp|h|hpp|rb|ex|exs|zig|swift|cs|lua|sh)):(\d+)(?::\d+)?"
    ).unwrap();
    for cap in re.captures_iter(error) {
        let path = &cap[1];
        // Skip paths that look like URLs or build artifacts
        if path.contains("://") || path.contains("/target/") || path.contains("__pycache__") {
            continue;
        }
        refs.push(ErrorRef {
            path: path.to_string(),
            line: cap[2].parse().ok(),
            symbol: None,
        });
    }

    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_error_parsing() {
        let error = r#"error[E0425]: cannot find value `foo` in this scope
   --> src/main.rs:42:5
    |
42  |     foo.bar();
    |     ^^^ not found in this scope

error[E0308]: mismatched types
   --> src/lib.rs:10:12
    |
10  |     return "hello";
    |            ^^^^^^^ expected i32, found &str"#;
        let parsed = parse_error_output(error, "cargo build");
        assert!(parsed.is_build_error);
        assert_eq!(parsed.refs.len(), 2);
        assert_eq!(parsed.refs[0].path, "src/lib.rs");
        assert_eq!(parsed.refs[1].path, "src/main.rs");
    }

    #[test]
    fn test_go_error_parsing() {
        let error = r#"./cmd/main.go:42:5: undefined: processRequest
./internal/handler.go:15:2: cannot use x (type string) as type int"#;
        let parsed = parse_error_output(error, "go build ./...");
        assert!(parsed.is_build_error);
        assert_eq!(parsed.refs.len(), 2);
        assert_eq!(parsed.refs[0].path, "./cmd/main.go");
        assert_eq!(parsed.refs[0].symbol.as_deref(), Some("processRequest"));
    }

    #[test]
    fn test_python_error_parsing() {
        let error = r#"FAILED tests/test_api.py::TestAuth::test_login_invalid
  File "src/auth.py", line 42, in validate
    raise ValueError("invalid token")
  File "src/handlers.py", line 15, in handle_login
    auth.validate(token)"#;
        let parsed = parse_error_output(error, "pytest tests/");
        assert!(parsed.is_build_error);
        assert!(parsed.refs.len() >= 3);
    }

    #[test]
    fn test_typescript_error_parsing() {
        let error = r#"src/index.ts(42,5): error TS2304: Cannot find name 'foo'.
src/utils.ts(10,3): error TS2345: Argument of type 'string' is not assignable."#;
        let parsed = parse_error_output(error, "npx tsc --noEmit");
        assert!(parsed.is_build_error);
        assert_eq!(parsed.refs.len(), 2);
    }

    #[test]
    fn test_is_build_command() {
        assert!(is_build_test_command("cargo build"));
        assert!(is_build_test_command("cargo test -- --nocapture"));
        assert!(is_build_test_command("go test ./..."));
        assert!(is_build_test_command("pytest tests/"));
        assert!(is_build_test_command("make all"));
        assert!(!is_build_test_command("ls -la"));
        assert!(!is_build_test_command("git status"));
    }

    #[test]
    fn test_deduplication() {
        let error = r#"error[E0425]: ...
   --> src/main.rs:42:5
error[E0308]: ...
   --> src/main.rs:50:10"#;
        let parsed = parse_error_output(error, "cargo build");
        // Same file should be deduplicated (keeps first line ref)
        assert_eq!(parsed.refs.len(), 1);
        assert_eq!(parsed.refs[0].path, "src/main.rs");
        assert_eq!(parsed.refs[0].line, Some(42));
    }

    #[test]
    fn test_generic_fallback() {
        let error = "error in src/config.rs:25:3: expected `;`";
        let parsed = parse_error_output(error, "some-custom-build-tool");
        assert_eq!(parsed.refs.len(), 1);
        assert_eq!(parsed.refs[0].path, "src/config.rs");
    }
}
