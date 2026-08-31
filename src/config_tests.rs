//! Tests for configuration loading and defaults (sidecar of `config.rs`).

use super::*;

#[test]
fn test_default_config_backward_compatible() {
    let config = EmbeddingConfig::default();
    assert_eq!(config.backend, EmbeddingBackend::Onnx);
    assert_eq!(config.model, "all-MiniLM-L6-v2");
    assert_eq!(config.batch_size, 32);
    assert!(config.dimensions.is_none());
    assert!(config.api.is_none());
    assert!(config.custom_model.is_none());
}

#[test]
fn test_default_index_include_has_java_and_cpp() {
    // Java + C++ have wired tree-sitter parsers; they must be in the default
    // include set so their structural extraction actually runs (bo-esn4).
    let include = IndexConfig::default().include;
    for pat in [
        "**/*.java",
        "**/*.cpp",
        "**/*.cc",
        "**/*.hpp",
        "**/*.rs",
        "**/*.py",
    ] {
        assert!(
            include.contains(&pat.to_string()),
            "default include missing {pat}"
        );
    }
}

#[test]
fn test_default_index_include_has_iac_extensions() {
    // An ansible/IaC repo (yaml + jinja templates, terraform, shell) must be
    // visible to code search. Before bobbin-ywzq8 the include list stopped at
    // code + markdown, so an entire IaC corpus was silently uncovered and
    // searches answered from markdown/commit-messages only, confidently.
    let include = IndexConfig::default().include;
    for pat in ["**/*.yml", "**/*.yaml", "**/*.j2", "**/*.tf", "**/*.sh"] {
        assert!(
            include.contains(&pat.to_string()),
            "default include missing {pat}"
        );
    }
}

#[test]
fn test_index_chunk_defaults_and_override() {
    let def = IndexConfig::default();
    assert_eq!(def.chunk_size, 50);
    assert_eq!(def.chunk_overlap, 10);

    let toml_str = r#"
[index]
chunk_size = 80
chunk_overlap = 16
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.index.chunk_size, 80);
    assert_eq!(config.index.chunk_overlap, 16);
    // Unspecified index keys still fall back to defaults.
    assert!(config.index.use_gitignore);
}

#[test]
fn test_parse_legacy_config() {
    // Old-style config without backend field should still work
    let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.embedding.backend, EmbeddingBackend::Onnx);
    assert_eq!(config.embedding.model, "all-MiniLM-L6-v2");
}

#[test]
fn test_parse_openai_api_config() {
    let toml_str = r#"
[embedding]
backend = "openai-api"
model = "nomic-embed-text"
dimensions = 768

[embedding.api]
url = "http://localhost:11434/v1/embeddings"
api_key = "test-key"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.embedding.backend, EmbeddingBackend::OpenaiApi);
    assert_eq!(config.embedding.model, "nomic-embed-text");
    assert_eq!(config.embedding.dimensions, Some(768));
    let api = config.embedding.api.unwrap();
    assert_eq!(api.url, "http://localhost:11434/v1/embeddings");
    assert_eq!(api.api_key, Some("test-key".to_string()));
}

#[test]
fn test_parse_custom_onnx_config() {
    let toml_str = r#"
[embedding]
model = "custom"
dimensions = 1024

[embedding.custom_model]
model_path = "/path/to/model.onnx"
tokenizer_path = "/path/to/tokenizer.json"
max_seq_len = 512
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.embedding.backend, EmbeddingBackend::Onnx);
    assert_eq!(config.embedding.dimensions, Some(1024));
    let custom = config.embedding.custom_model.unwrap();
    assert_eq!(custom.model_path, "/path/to/model.onnx");
    assert_eq!(custom.tokenizer_path, "/path/to/tokenizer.json");
    assert_eq!(custom.max_seq_len, Some(512));
}

#[test]
fn test_api_key_resolve_literal() {
    let api = ApiEmbeddingConfig {
        url: "http://example.com".to_string(),
        api_key: Some("literal-key".to_string()),
    };
    assert_eq!(api.resolve_api_key(), Some("literal-key".to_string()));
}

#[test]
fn test_api_key_resolve_env() {
    std::env::set_var("TEST_BOBBIN_API_KEY", "env-value");
    let api = ApiEmbeddingConfig {
        url: "http://example.com".to_string(),
        api_key: Some("env:TEST_BOBBIN_API_KEY".to_string()),
    };
    assert_eq!(api.resolve_api_key(), Some("env-value".to_string()));
    std::env::remove_var("TEST_BOBBIN_API_KEY");
}

#[test]
fn test_api_key_resolve_empty() {
    let api = ApiEmbeddingConfig {
        url: "http://example.com".to_string(),
        api_key: Some("".to_string()),
    };
    assert!(api.resolve_api_key().is_none());
}

#[test]
fn test_api_key_resolve_none() {
    let api = ApiEmbeddingConfig {
        url: "http://example.com".to_string(),
        api_key: None,
    };
    assert!(api.resolve_api_key().is_none());
}

#[test]
fn test_dependencies_config_default_enabled() {
    let config = Config::default();
    assert!(config.dependencies.enabled);
}

#[test]
fn test_dependencies_config_disabled() {
    let toml_str = r#"
[dependencies]
enabled = false
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(!config.dependencies.enabled);
}

#[test]
fn test_legacy_config_without_dependencies_section() {
    // Config without [dependencies] should default to enabled
    let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.dependencies.enabled);
}

#[test]
fn test_git_commits_config_defaults() {
    let config = Config::default();
    assert!(config.git.commits_enabled);
    assert_eq!(config.git.commits_depth, 0);
}

#[test]
fn test_git_commits_config_custom() {
    let toml_str = r#"
[git]
commits_enabled = false
commits_depth = 500
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(!config.git.commits_enabled);
    assert_eq!(config.git.commits_depth, 500);
}

#[test]
fn test_legacy_git_config_without_commits_fields() {
    // Old config without commits fields should default to enabled, depth=0 (all)
    let toml_str = r#"
[git]
coupling_enabled = true
coupling_depth = 500
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.git.commits_enabled);
    assert_eq!(config.git.commits_depth, 0);
}

#[test]
fn test_search_config_defaults() {
    let config = Config::default();
    assert_eq!(config.search.default_limit, 10);
    assert!((config.search.semantic_weight - 0.9).abs() < f32::EPSILON);
    assert!((config.search.recency_half_life_days - 30.0).abs() < f32::EPSILON);
    assert!((config.search.recency_weight - 0.3).abs() < f32::EPSILON);
    assert!((config.search.rrf_k - 60.0).abs() < f32::EPSILON);
    assert!((config.search.doc_demotion - 0.3).abs() < f32::EPSILON);
}

// bo-b0nn: doc_demotion has one source of truth. SearchConfig is canonical;
// ContextConfig::default() must match it so the two never drift (a stale 0.5
// comment + a 0.5 ContextConfig default previously disagreed with the 0.3
// effective default).
#[test]
fn test_doc_demotion_single_source_of_truth() {
    let search_default = SearchConfig::default().doc_demotion;
    let context_default = crate::search::context::ContextConfig::default().doc_demotion;
    assert!(
        (search_default - 0.3).abs() < f32::EPSILON,
        "SearchConfig::doc_demotion default should be 0.3, got {search_default}"
    );
    assert!(
        (search_default - context_default).abs() < f32::EPSILON,
        "ContextConfig default ({context_default}) must match SearchConfig ({search_default})"
    );
}

// bo-qlfu: context-assembly knobs are config-backed via [context]/[feedback].
#[test]
fn test_context_tuning_defaults() {
    let c = Config::default();
    assert!((c.context.bridge_boost_factor - 0.3).abs() < f32::EPSILON);
    assert_eq!(c.context.max_bridged_files, 2);
    assert_eq!(c.context.max_bridged_chunks_per_file, 1);
    assert!((c.context.coupling_threshold - 0.1).abs() < f32::EPSILON);
    assert!((c.context.knowledge_budget_pct - 15.0).abs() < f32::EPSILON);
    assert_eq!(c.context.knowledge_max_hops, 2);
    assert_eq!(
        c.context.budget_unit,
        crate::search::context::BudgetUnit::Line
    );
    assert!((c.feedback.boost_max - 0.3).abs() < f32::EPSILON);
    assert!((c.feedback.boost_weight - 0.2).abs() < f32::EPSILON);
}

#[test]
fn test_context_tuning_custom() {
    let toml_str = r#"
[context]
bridge_boost_factor = 0.5
max_bridged_files = 4
max_bridged_chunks_per_file = 3
coupling_threshold = 0.25
knowledge_budget_pct = 20.0
knowledge_max_hops = 3
budget_unit = "token"

[feedback]
boost_max = 0.4
boost_weight = 0.15
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!((config.context.bridge_boost_factor - 0.5).abs() < f32::EPSILON);
    assert_eq!(config.context.max_bridged_files, 4);
    assert_eq!(config.context.max_bridged_chunks_per_file, 3);
    assert!((config.context.coupling_threshold - 0.25).abs() < f32::EPSILON);
    assert!((config.context.knowledge_budget_pct - 20.0).abs() < f32::EPSILON);
    assert_eq!(config.context.knowledge_max_hops, 3);
    assert_eq!(
        config.context.budget_unit,
        crate::search::context::BudgetUnit::Token
    );
    assert!((config.feedback.boost_max - 0.4).abs() < f32::EPSILON);
    assert!((config.feedback.boost_weight - 0.15).abs() < f32::EPSILON);
}

#[test]
fn test_legacy_config_without_context_sections() {
    // Config predating [context]/[feedback] must fall back to defaults.
    let toml_str = r#"
[search]
semantic_weight = 0.8
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.context.max_bridged_files, 2);
    assert!((config.feedback.boost_max - 0.3).abs() < f32::EPSILON);
}

#[test]
fn test_search_config_custom() {
    let toml_str = r#"
[search]
semantic_weight = 0.5
rrf_k = 40.0
doc_demotion = 0.3
recency_weight = 0.1
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!((config.search.semantic_weight - 0.5).abs() < f32::EPSILON);
    assert!((config.search.rrf_k - 40.0).abs() < f32::EPSILON);
    assert!((config.search.doc_demotion - 0.3).abs() < f32::EPSILON);
    assert!((config.search.recency_weight - 0.1).abs() < f32::EPSILON);
}

#[test]
fn test_legacy_config_without_search_tuning_fields() {
    // Old config without rrf_k/doc_demotion should use defaults
    let toml_str = r#"
[search]
semantic_weight = 0.8
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!((config.search.semantic_weight - 0.8).abs() < f32::EPSILON);
    assert!((config.search.rrf_k - 60.0).abs() < f32::EPSILON);
    assert!((config.search.doc_demotion - 0.3).abs() < f32::EPSILON);
}

#[test]
fn test_hooks_config_defaults() {
    let config = Config::default();
    assert!((config.hooks.threshold - 0.5).abs() < f32::EPSILON);
    assert_eq!(config.hooks.budget, 300);
    assert_eq!(config.hooks.content_mode, "full");
    assert_eq!(config.hooks.min_prompt_length, 20);
    assert!((config.hooks.gate_threshold - 0.45).abs() < f32::EPSILON);
    assert!(config.hooks.dedup_enabled);
    assert!((config.hooks.repo_affinity_boost - 2.0).abs() < f32::EPSILON);
}

// bo-ruuc: repo_affinity_boost must be configurable and parsed from [hooks];
// `bobbin context` now sources it from config.hooks.repo_affinity_boost rather
// than a hardcoded 2.0.
#[test]
fn test_hooks_repo_affinity_boost_custom() {
    let toml_str = r#"
[hooks]
repo_affinity_boost = 3.5
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!((config.hooks.repo_affinity_boost - 3.5).abs() < f32::EPSILON);
}

// bo-u962: the agent-facing context/search paths (MCP + HTTP context/review,
// cli review) must source repo_affinity_boost from config, not a hardcoded 2.0
// — otherwise the knob is silently ignored where agents actually hit it. These
// ContextConfig blocks are built inline in async handlers with no extractable
// seam, so guard against regression at the source: each agent-facing site reads
// the config field and none hardcode `repo_affinity_boost: 2.0`. calibrate.rs is
// deliberately excluded (calibration stays config-independent / deterministic).
#[test]
fn test_repo_affinity_boost_not_hardcoded_in_agent_paths() {
    let root = env!("CARGO_MANIFEST_DIR");
    for rel in [
        "src/mcp/server.rs",
        "src/http/handlers/context.rs",
        "src/http/handlers/review.rs",
        "src/cli/review.rs",
        "src/cli/context.rs",
    ] {
        let src = std::fs::read_to_string(format!("{root}/{rel}")).unwrap();
        assert!(
                !src.contains("repo_affinity_boost: 2.0"),
                "{rel} hardcodes repo_affinity_boost: 2.0 — thread config.hooks.repo_affinity_boost instead"
            );
        assert!(
            src.contains("repo_affinity_boost: config.hooks.repo_affinity_boost")
                || src.contains("repo_affinity_boost: state.config.hooks.repo_affinity_boost"),
            "{rel} must source repo_affinity_boost from config"
        );
    }
}

#[test]
fn test_hooks_config_custom() {
    let toml_str = r#"
[hooks]
threshold = 0.7
budget = 200
content_mode = "preview"
min_prompt_length = 20
gate_threshold = 0.9
dedup_enabled = false
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!((config.hooks.threshold - 0.7).abs() < f32::EPSILON);
    assert_eq!(config.hooks.budget, 200);
    assert_eq!(config.hooks.content_mode, "preview");
    assert_eq!(config.hooks.min_prompt_length, 20);
    assert!((config.hooks.gate_threshold - 0.9).abs() < f32::EPSILON);
    assert!(!config.hooks.dedup_enabled);
}

#[test]
fn test_legacy_config_without_hooks_section() {
    let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
batch_size = 32
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!((config.hooks.threshold - 0.5).abs() < f32::EPSILON);
    assert_eq!(config.hooks.budget, 300);
    assert!((config.hooks.gate_threshold - 0.45).abs() < f32::EPSILON);
}

#[test]
fn test_hooks_gate_threshold_backward_compatible() {
    // Config with hooks section but no gate_threshold should default to 0.45
    let toml_str = r#"
[hooks]
threshold = 0.5
budget = 300
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!((config.hooks.gate_threshold - 0.45).abs() < f32::EPSILON);
    assert!(config.hooks.dedup_enabled);
}

#[test]
fn test_backend_serde_roundtrip() {
    let config = EmbeddingConfig {
        backend: EmbeddingBackend::OpenaiApi,
        model: "test".to_string(),
        batch_size: 16,
        dimensions: Some(768),
        api: Some(ApiEmbeddingConfig {
            url: "http://localhost:8080/v1/embeddings".to_string(),
            api_key: None,
        }),
        custom_model: None,
        context: ContextualEmbeddingConfig::default(),
        ..Default::default()
    };
    let serialized = toml::to_string_pretty(&config).unwrap();
    let deserialized: EmbeddingConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(deserialized.backend, EmbeddingBackend::OpenaiApi);
    assert_eq!(deserialized.dimensions, Some(768));
}

#[test]
fn test_server_config_default_is_none() {
    let config = Config::default();
    assert!(config.server.url.is_none());
}

#[test]
fn test_server_config_parse() {
    let toml_str = r#"
[server]
url = "http://search.example"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.server.url.as_deref(), Some("http://search.example"));
}

#[test]
fn test_legacy_config_without_server_section() {
    // Old config without [server] should still parse fine
    let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.server.url.is_none());
}

#[test]
fn test_global_config_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("config.toml");

    let mut config = Config::default();
    config.server.url = Some("http://search.example".to_string());

    let content = toml::to_string_pretty(&config).unwrap();
    std::fs::write(&config_path, &content).unwrap();

    let loaded: Config = toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(loaded.server.url.as_deref(), Some("http://search.example"));
}

#[test]
fn test_groups_config_parse() {
    let toml_str = r#"
[[groups]]
name = "infra"
repos = ["goldblum", "homelab-mcp", "aegis"]

[[groups]]
name = "apps"
repos = ["reckoning", "tapestry"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.groups.len(), 2);
    assert_eq!(config.groups[0].name, "infra");
    assert_eq!(
        config.groups[0].repos,
        vec!["goldblum", "homelab-mcp", "aegis"]
    );
    assert_eq!(config.groups[1].name, "apps");
    assert_eq!(config.groups[1].repos, vec!["reckoning", "tapestry"]);
}

#[test]
fn test_groups_default_empty() {
    let config = Config::default();
    assert!(config.groups.is_empty());
}

#[test]
fn test_legacy_config_without_groups() {
    let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.groups.is_empty());
}

#[test]
fn test_resolve_group() {
    let toml_str = r#"
[[groups]]
name = "infra"
repos = ["goldblum", "aegis"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        config.resolve_group("infra"),
        Some(&["goldblum".to_string(), "aegis".to_string()][..])
    );
    assert_eq!(config.resolve_group("nonexistent"), None);
}

#[test]
fn test_group_filter_sql() {
    let toml_str = r#"
[[groups]]
name = "infra"
repos = ["goldblum", "homelab-mcp"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        config.group_filter("infra"),
        Some("repo IN ('goldblum', 'homelab-mcp')".to_string())
    );
    assert_eq!(config.group_filter("nonexistent"), None);
}

#[test]
fn test_group_filter_empty_repos() {
    let toml_str = r#"
[[groups]]
name = "empty"
repos = []
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.group_filter("empty"), None);
}

#[test]
fn test_keyword_repos_config_parse() {
    let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible", "playbook"]
repos = ["goldblum"]

[[hooks.keyword_repos]]
keywords = ["beads", "bd "]
repos = ["beads"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.hooks.keyword_repos.len(), 2);
    assert_eq!(
        config.hooks.keyword_repos[0].keywords,
        vec!["ansible", "playbook"]
    );
    assert_eq!(config.hooks.keyword_repos[0].repos, vec!["goldblum"]);
}

#[test]
fn test_keyword_repos_resolve_match() {
    let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible", "playbook"]
repos = ["goldblum"]

[[hooks.keyword_repos]]
keywords = ["beads", "bd "]
repos = ["beads"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let repos = config
        .hooks
        .resolve_keyword_repos("deploy the ansible playbook");
    assert_eq!(repos, vec!["goldblum"]);
}

#[test]
fn test_keyword_repos_resolve_multiple() {
    let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible"]
repos = ["goldblum"]

[[hooks.keyword_repos]]
keywords = ["beads"]
repos = ["beads"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let repos = config
        .hooks
        .resolve_keyword_repos("check ansible and beads status");
    assert_eq!(repos, vec!["goldblum", "beads"]);
}

#[test]
fn test_keyword_repos_resolve_case_insensitive() {
    let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["Ansible"]
repos = ["goldblum"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let repos = config.hooks.resolve_keyword_repos("ANSIBLE playbook");
    assert_eq!(repos, vec!["goldblum"]);
}

#[test]
fn test_keyword_repos_resolve_no_match() {
    let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible"]
repos = ["goldblum"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let repos = config.hooks.resolve_keyword_repos("fix the bobbin build");
    assert!(repos.is_empty());
}

#[test]
fn test_keyword_repos_dedup() {
    let toml_str = r#"
[hooks]

[[hooks.keyword_repos]]
keywords = ["ansible"]
repos = ["goldblum"]

[[hooks.keyword_repos]]
keywords = ["iac"]
repos = ["goldblum"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let repos = config.hooks.resolve_keyword_repos("ansible iac deployment");
    assert_eq!(repos, vec!["goldblum"]);
}

#[test]
fn test_keyword_repos_default_empty() {
    let config = Config::default();
    assert!(config.hooks.keyword_repos.is_empty());
    assert!(config.hooks.resolve_keyword_repos("anything").is_empty());
}

#[test]
fn test_legacy_config_without_keyword_repos() {
    let toml_str = r#"
[hooks]
threshold = 0.5
budget = 300
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.hooks.keyword_repos.is_empty());
}

#[test]
fn test_file_types_config_parse() {
    let toml_str = r#"
[[file_types]]
name = "generated"
patterns = ["*.pb.go", "*.generated.ts"]

[[file_types]]
name = "config"
patterns = ["deploy/*.yaml", "*.toml"]

[[file_types]]
name = "vendor"
patterns = ["vendor/**"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.file_types.len(), 3);
    assert_eq!(config.file_types[0].name, "generated");
    assert_eq!(
        config.file_types[0].patterns,
        vec!["*.pb.go", "*.generated.ts"]
    );
    assert_eq!(config.file_types[1].name, "config");
    assert_eq!(config.file_types[2].name, "vendor");
}

#[test]
fn test_file_types_default_empty() {
    let config = Config::default();
    assert!(config.file_types.is_empty());
}

#[test]
fn test_legacy_config_without_file_types() {
    let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.file_types.is_empty());
}

#[test]
fn test_deep_merge_toml_overlay_scalar() {
    let base: toml::Value = toml::from_str(
        r#"
[search]
semantic_weight = 0.9
doc_demotion = 0.3
"#,
    )
    .unwrap();
    let overlay: toml::Value = toml::from_str(
        r#"
[search]
semantic_weight = 0.7
"#,
    )
    .unwrap();
    let merged = deep_merge_toml(base, overlay);
    let config: Config = merged.try_into().unwrap();
    assert!((config.search.semantic_weight - 0.7).abs() < f32::EPSILON);
    // doc_demotion should retain the base value
    assert!((config.search.doc_demotion - 0.3).abs() < f32::EPSILON);
}

#[test]
fn test_deep_merge_toml_overlay_adds_section() {
    let base: toml::Value = toml::from_str(
        r#"
[search]
semantic_weight = 0.9
"#,
    )
    .unwrap();
    let overlay: toml::Value = toml::from_str(
        r#"
[server]
url = "http://search.example"
"#,
    )
    .unwrap();
    let merged = deep_merge_toml(base, overlay);
    let config: Config = merged.try_into().unwrap();
    assert_eq!(config.server.url.as_deref(), Some("http://search.example"));
    // search.semantic_weight from base is preserved
    assert!((config.search.semantic_weight - 0.9).abs() < f32::EPSILON);
}

#[test]
fn test_deep_merge_toml_array_replaces() {
    let base: toml::Value = toml::from_str(
        r#"
[index]
include = ["**/*.rs"]
"#,
    )
    .unwrap();
    let overlay: toml::Value = toml::from_str(
        r#"
[index]
include = ["**/*.py", "**/*.go"]
"#,
    )
    .unwrap();
    let merged = deep_merge_toml(base, overlay);
    let config: Config = merged.try_into().unwrap();
    // Arrays are replaced wholesale, not merged
    assert_eq!(config.index.include, vec!["**/*.py", "**/*.go"]);
}
