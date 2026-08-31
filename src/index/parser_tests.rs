//! Tests for the chunking parser (sidecar of `parser.rs`).

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
    assert!(chunks
        .iter()
        .any(|c| c.chunk_type == ChunkType::Function && c.name == Some("__init__".to_string())));
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
    assert!(chunks
        .iter()
        .any(|c| c.chunk_type == ChunkType::Method && c.name == Some("Distance".to_string())));
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
    assert!(
        chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Method)
            .count()
            >= 2
    );
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

    assert!(chunks
        .iter()
        .any(|c| c.chunk_type == ChunkType::Interface && c.name == Some("Runnable".to_string())));
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
    // IaC / config / scripts — line-chunked but tagged so they are searchable
    // and show up as a language, not "unknown" (bobbin-ywzq8).
    assert_eq!(
        detect_language(Path::new("roles/x/defaults/main.yml")),
        Some("yaml".to_string())
    );
    assert_eq!(
        detect_language(Path::new("group_vars/all.yaml")),
        Some("yaml".to_string())
    );
    assert_eq!(
        detect_language(Path::new("templates/nftables.conf.j2")),
        Some("jinja".to_string())
    );
    assert_eq!(
        detect_language(Path::new("main.tf")),
        Some("terraform".to_string())
    );
    assert_eq!(
        detect_language(Path::new("scripts/deploy.sh")),
        Some("shell".to_string())
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
    let sections: Vec<_> = chunks
        .iter()
        .filter(|c| c.chunk_type == ChunkType::Section)
        .collect();
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

    let doc_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c.chunk_type == ChunkType::Doc)
        .collect();
    let section_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c.chunk_type == ChunkType::Section)
        .collect();

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
    let fm: Vec<_> = chunks
        .iter()
        .filter(|c| c.name == Some("Frontmatter".to_string()))
        .collect();
    assert_eq!(fm.len(), 1);
    assert_eq!(fm[0].chunk_type, ChunkType::Doc);
    assert!(fm[0].content.contains("title: Test Document"));

    // And a section for the heading
    let sections: Vec<_> = chunks
        .iter()
        .filter(|c| c.chunk_type == ChunkType::Section)
        .collect();
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

    let code_blocks: Vec<_> = chunks
        .iter()
        .filter(|c| c.chunk_type == ChunkType::CodeBlock)
        .collect();
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
    assert!(imports
        .iter()
        .any(|i| i.import_specifier == "std::path::Path"));
    assert!(imports
        .iter()
        .any(|i| i.import_specifier == "crate::types::Chunk"));
    assert!(imports
        .iter()
        .any(|i| i.import_specifier == "anyhow::Result"));
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
    assert!(imports
        .iter()
        .any(|i| i.import_specifier == "java.util.List"));
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

    let i = imports
        .iter()
        .find(|i| i.path == "std::path::Path")
        .unwrap();
    assert_eq!(i.statement, "use std::path::Path;");
    assert_eq!(i.dep_type, "use");

    let i = imports
        .iter()
        .find(|i| i.path == "crate::types::Chunk")
        .unwrap();
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
    assert!(imports
        .iter()
        .any(|i| i.path == "fs" && i.dep_type == "require"));
    assert!(imports
        .iter()
        .any(|i| i.path == "path" && i.dep_type == "require"));
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
    assert!(imports
        .iter()
        .any(|i| i.path == "fmt" && i.dep_type == "import"));
    assert!(imports
        .iter()
        .any(|i| i.path == "os" && i.dep_type == "import"));
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

    let tables: Vec<_> = chunks
        .iter()
        .filter(|c| c.chunk_type == ChunkType::Table)
        .collect();
    assert_eq!(tables.len(), 1);
    assert!(tables[0].content.contains("Method"));
    assert!(tables[0].name.as_ref().unwrap().contains("table"));
}

#[test]
fn test_chunk_edges_rust_impl_trait() {
    let mut parser = Parser::new().unwrap();
    let content = r#"
trait Greeter {
    fn greet(&self);
}

struct Person {
    name: String,
}

impl Greeter for Person {
    fn greet(&self) {
        println!("Hello, {}", self.name);
    }
}
"#;
    let path = PathBuf::from("test.rs");
    let chunks = parser.parse_file(&path, content).unwrap();
    let edges = parser.extract_chunk_edges(&path, content, &chunks);

    // Should find: impl → Greeter (Implements), impl → Person (ImplFor)
    let implements: Vec<_> = edges
        .iter()
        .filter(|e| e.edge_type == ChunkEdgeType::Implements)
        .collect();
    let impl_for: Vec<_> = edges
        .iter()
        .filter(|e| e.edge_type == ChunkEdgeType::ImplFor)
        .collect();

    assert_eq!(implements.len(), 1, "expected 1 implements edge");
    assert_eq!(implements[0].target_name, "Greeter");

    assert_eq!(impl_for.len(), 1, "expected 1 impl_for edge");
    assert_eq!(impl_for[0].target_name, "Person");
}

#[test]
fn test_chunk_edges_rust_impl_no_trait() {
    let mut parser = Parser::new().unwrap();
    let content = r#"
struct Counter {
    count: u32,
}

impl Counter {
    fn new() -> Self {
        Self { count: 0 }
    }
}
"#;
    let path = PathBuf::from("test.rs");
    let chunks = parser.parse_file(&path, content).unwrap();
    let edges = parser.extract_chunk_edges(&path, content, &chunks);

    // Should find: impl → Counter (ImplFor), no Implements
    let implements: Vec<_> = edges
        .iter()
        .filter(|e| e.edge_type == ChunkEdgeType::Implements)
        .collect();
    let impl_for: Vec<_> = edges
        .iter()
        .filter(|e| e.edge_type == ChunkEdgeType::ImplFor)
        .collect();

    assert!(implements.is_empty(), "no trait = no implements edge");
    assert_eq!(impl_for.len(), 1);
    assert_eq!(impl_for[0].target_name, "Counter");
}

#[test]
fn test_chunk_edges_python_extends() {
    let mut parser = Parser::new().unwrap();
    let content = r#"
class Animal:
    def speak(self):
        pass

class Dog(Animal):
    def speak(self):
        return "Woof"
"#;
    let path = PathBuf::from("test.py");
    let chunks = parser.parse_file(&path, content).unwrap();
    let edges = parser.extract_chunk_edges(&path, content, &chunks);

    let extends: Vec<_> = edges
        .iter()
        .filter(|e| e.edge_type == ChunkEdgeType::Extends)
        .collect();
    assert_eq!(extends.len(), 1, "expected 1 extends edge");
    assert_eq!(extends[0].source_name, "Dog");
    assert_eq!(extends[0].target_name, "Animal");
}

#[test]
fn test_chunk_edges_no_edges_for_standalone() {
    let mut parser = Parser::new().unwrap();
    let content = r#"
fn standalone_function() {
    println!("no edges here");
}
"#;
    let path = PathBuf::from("test.rs");
    let chunks = parser.parse_file(&path, content).unwrap();
    let edges = parser.extract_chunk_edges(&path, content, &chunks);
    assert!(edges.is_empty());
}

#[test]
fn test_estimate_tokens_heuristic() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("abcd"), 1); // 4 chars => 1
    assert_eq!(estimate_tokens("abcde"), 2); // 5 chars => ceil(5/4) = 2
}

#[test]
fn test_with_chunking_sanitizes_overlap() {
    // overlap >= size would stall the chunker; it must be capped below size.
    let parser = Parser::new().unwrap().with_chunking(5, 10, 0);
    assert_eq!(parser.chunk_size, 5);
    assert_eq!(parser.chunk_overlap, 4);
    // size 0 is forced to at least 1.
    let parser = Parser::new().unwrap().with_chunking(0, 0, 0);
    assert_eq!(parser.chunk_size, 1);
    assert_eq!(parser.chunk_overlap, 0);
}

#[test]
fn test_chunk_by_lines_configurable_size_overlap() {
    // 10-line chunks, 2-line overlap, no token clamp.
    let parser = Parser::new().unwrap().with_chunking(10, 2, 0);
    let content = (1..=25)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let path = PathBuf::from("data.unknownext");
    let chunks = parser.chunk_by_lines(&path, &content);

    // start 0..10, then 8..18, then 16..25 => 3 chunks
    assert_eq!(chunks.len(), 3);
    assert_eq!((chunks[0].start_line, chunks[0].end_line), (1, 10));
    assert_eq!(chunks[1].start_line, 9); // 10 - 2 overlap + 1 (1-based)
    assert_eq!(chunks[2].end_line, 25);
}

#[test]
fn test_chunk_by_lines_clamps_to_token_window() {
    // 50-line window but an 8-token clamp: dense lines force smaller chunks
    // so nothing exceeds the embedder window (no silent truncation).
    let cap = 8;
    let parser = Parser::new().unwrap().with_chunking(50, 1, cap);
    // 12 lines, 8 chars each (~2 tokens/line). The full 50-line window would
    // be ~30+ tokens — must be split.
    let content = (0..12)
        .map(|_| "yyyyyyyy".to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let path = PathBuf::from("dense.unknownext");
    let chunks = parser.chunk_by_lines(&path, &content);

    assert!(
        chunks.len() > 1,
        "clamp should split the window into multiple chunks"
    );
    for c in &chunks {
        assert!(
            estimate_tokens(&c.content) <= cap,
            "chunk exceeds token window: {} tokens",
            estimate_tokens(&c.content)
        );
    }
    // Coverage: chunks span the whole file start-to-end.
    assert_eq!(chunks.first().unwrap().start_line, 1);
    assert_eq!(chunks.last().unwrap().end_line, 12);
}
