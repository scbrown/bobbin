//! Tests for the core types (sidecar of `types.rs`).

use super::*;

#[test]
fn test_chunk_edge_type_display_matches_serde() {
    // Storage writes Display, JSON writes serde(snake_case); they must agree.
    for edge_type in ChunkEdgeType::ALL {
        let display = edge_type.to_string();
        let json = serde_json::to_string(&edge_type).unwrap();
        assert_eq!(format!("\"{}\"", display), json);
    }
}

#[test]
fn test_classify_source_files() {
    assert_eq!(classify_file("src/main.rs"), FileCategory::Source);
    assert_eq!(classify_file("src/cli/hook.rs"), FileCategory::Source);
    assert_eq!(
        classify_file("crates/ruff_linter/src/rules/mod.rs"),
        FileCategory::Source
    );
    assert_eq!(classify_file("lib/parser.py"), FileCategory::Source);
    assert_eq!(
        classify_file("app/components/Button.tsx"),
        FileCategory::Source
    );
    assert_eq!(classify_file("server.go"), FileCategory::Source);
}

#[test]
fn test_classify_documentation_by_name() {
    assert_eq!(classify_file("CHANGELOG.md"), FileCategory::Documentation);
    assert_eq!(classify_file("CHANGELOG"), FileCategory::Documentation);
    assert_eq!(classify_file("CHANGES.md"), FileCategory::Documentation);
    assert_eq!(
        classify_file("BREAKING_CHANGES.md"),
        FileCategory::Documentation
    );
    assert_eq!(classify_file("README.md"), FileCategory::Documentation);
    assert_eq!(classify_file("README"), FileCategory::Documentation);
    assert_eq!(
        classify_file("CONTRIBUTING.md"),
        FileCategory::Documentation
    );
    assert_eq!(classify_file("LICENSE"), FileCategory::Documentation);
    assert_eq!(classify_file("LICENSE.md"), FileCategory::Documentation);
    assert_eq!(
        classify_file("CODE_OF_CONDUCT.md"),
        FileCategory::Documentation
    );
}

#[test]
fn test_classify_documentation_by_directory() {
    assert_eq!(classify_file("docs/guide.md"), FileCategory::Documentation);
    assert_eq!(
        classify_file("doc/architecture.rst"),
        FileCategory::Documentation
    );
    assert_eq!(
        classify_file("changelogs/0.14.x.md"),
        FileCategory::Documentation
    );
    assert_eq!(
        classify_file("documentation/api.md"),
        FileCategory::Documentation
    );
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
    assert_eq!(
        classify_file("spec/models/user_spec.rb"),
        FileCategory::Test
    );
    assert_eq!(
        classify_file("src/__tests__/button.test.tsx"),
        FileCategory::Test
    );
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
    assert_eq!(
        classify_file("__snapshots__/button.snap"),
        FileCategory::Test
    );
    assert_eq!(
        classify_file("src/__snapshots__/app.snap"),
        FileCategory::Test
    );
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
    assert_eq!(
        classify_file(".github/workflows/ci.yml"),
        FileCategory::Config
    );
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
    assert_eq!(
        classify_file("crates/config/fixtures/test.yml"),
        FileCategory::Source
    );
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
    assert_eq!(
        format!("{}", FileCategory::Custom("generated".into())),
        "generated"
    );
}

#[test]
fn test_from_name() {
    assert_eq!(FileCategory::from_name("source"), FileCategory::Source);
    assert_eq!(FileCategory::from_name("test"), FileCategory::Test);
    assert_eq!(
        FileCategory::from_name("documentation"),
        FileCategory::Documentation
    );
    assert_eq!(FileCategory::from_name("doc"), FileCategory::Documentation);
    assert_eq!(FileCategory::from_name("docs"), FileCategory::Documentation);
    assert_eq!(FileCategory::from_name("config"), FileCategory::Config);
    assert_eq!(
        FileCategory::from_name("configuration"),
        FileCategory::Config
    );
    assert_eq!(
        FileCategory::from_name("generated"),
        FileCategory::Custom("generated".into())
    );
    assert_eq!(
        FileCategory::from_name("vendor"),
        FileCategory::Custom("vendor".into())
    );
}

#[test]
fn test_is_doc_like() {
    assert!(FileCategory::Documentation.is_doc_like());
    assert!(FileCategory::Config.is_doc_like());
    assert!(!FileCategory::Source.is_doc_like());
    assert!(!FileCategory::Test.is_doc_like());
    assert!(!FileCategory::Custom("generated".into()).is_doc_like());
}

#[test]
fn test_classify_with_rules_override() {
    use crate::config::FileTypeRule;
    let rules = vec![
        FileTypeRule {
            name: "generated".into(),
            patterns: vec!["*.pb.go".into(), "*.generated.ts".into()],
        },
        FileTypeRule {
            name: "config".into(),
            patterns: vec!["deploy/*.yaml".into()],
        },
    ];
    // Config rules match
    assert_eq!(
        classify_file_with_rules("proto/service.pb.go", &rules),
        FileCategory::Custom("generated".into())
    );
    assert_eq!(
        classify_file_with_rules("api/types.generated.ts", &rules),
        FileCategory::Custom("generated".into())
    );
    assert_eq!(
        classify_file_with_rules("deploy/staging.yaml", &rules),
        FileCategory::Config
    );
    // No rule match — falls back to built-in heuristics
    assert_eq!(
        classify_file_with_rules("src/main.rs", &rules),
        FileCategory::Source
    );
    assert_eq!(
        classify_file_with_rules("tests/test_foo.py", &rules),
        FileCategory::Test
    );
}

#[test]
fn test_classify_with_empty_rules_uses_builtin() {
    assert_eq!(
        classify_file_with_rules("src/main.rs", &[]),
        FileCategory::Source
    );
    assert_eq!(
        classify_file_with_rules("README.md", &[]),
        FileCategory::Documentation
    );
    assert_eq!(
        classify_file_with_rules("Cargo.toml", &[]),
        FileCategory::Config
    );
}

#[test]
fn test_classify_rules_first_match_wins() {
    use crate::config::FileTypeRule;
    let rules = vec![
        FileTypeRule {
            name: "vendor".into(),
            patterns: vec!["vendor/**".into()],
        },
        FileTypeRule {
            name: "test".into(),
            patterns: vec!["vendor/test/**".into()],
        },
    ];
    // First rule wins even though second also matches
    assert_eq!(
        classify_file_with_rules("vendor/test/helper.go", &rules),
        FileCategory::Custom("vendor".into())
    );
}
