//! Tool-aware context reactions: rule engine for PostToolUse hook.
//!
//! Reactions fire immediately after tool calls, providing targeted guidance
//! and contextual file injection. Rules live in `.bobbin/reactions.toml`.

use anyhow::{Context, Result};
use glob::Pattern;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::storage::MetadataStore;
use crate::types::FileCoupling;

// ---------------------------------------------------------------------------
// Schema: reactions.toml
// ---------------------------------------------------------------------------

/// Top-level config: a list of reaction rules.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReactionConfig {
    /// Reaction rules
    #[serde(default)]
    pub reactions: Vec<ReactionRule>,
}

/// A single reaction rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionRule {
    /// Rule name (used for dedup and metrics)
    pub name: String,

    /// Tool name pattern — glob (e.g., "Edit", "mcp__homelab__*", "Bash")
    pub tool: String,

    /// Optional parameter match conditions.
    /// Keys are parameter names, values are regex patterns.
    #[serde(default, rename = "match")]
    pub match_conditions: HashMap<String, String>,

    /// Guidance text shown to the agent. Supports `{args.X}` templating.
    #[serde(default)]
    pub guidance: String,

    /// Search query template. Supports `{args.X}`, `{file_stem}`.
    #[serde(default)]
    pub search_query: String,

    /// Which index group to search (e.g., "goldblum").
    #[serde(default)]
    pub search_group: String,

    /// Tag filters for scoped search.
    #[serde(default)]
    pub search_tags: Vec<String>,

    /// Max lines of injected context for this reaction.
    #[serde(default = "default_max_context_lines")]
    pub max_context_lines: usize,

    /// Use temporal coupling instead of search.
    #[serde(default)]
    pub use_coupling: bool,

    /// Minimum coupling score (when use_coupling = true).
    #[serde(default = "default_coupling_threshold")]
    pub coupling_threshold: f32,
}

fn default_max_context_lines() -> usize {
    50
}

fn default_coupling_threshold() -> f32 {
    0.3
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

impl ReactionConfig {
    /// Load reactions from a TOML file (typically `.bobbin/reactions.toml`).
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read reactions file: {}", path.display()))?;
        Self::parse(&content)
    }

    /// Parse reactions from a TOML string.
    pub fn parse(toml_str: &str) -> Result<Self> {
        toml::from_str(toml_str).context("Failed to parse reactions TOML")
    }

    /// Load from `.bobbin/reactions.toml` relative to a repo root.
    /// Returns an empty config if the file doesn't exist.
    pub fn load_for_repo(repo_root: &Path) -> Self {
        let path = repo_root.join(".bobbin").join("reactions.toml");
        if path.exists() {
            Self::load(&path).unwrap_or_default()
        } else {
            Self::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Compiled rule (pre-compiled patterns for fast matching)
// ---------------------------------------------------------------------------

/// A compiled reaction rule with pre-compiled glob and regex patterns.
pub struct CompiledRule {
    pub rule: ReactionRule,
    tool_pattern: Pattern,
    match_regexes: Vec<(String, Regex)>,
}

impl CompiledRule {
    /// Compile a ReactionRule into match-ready form.
    pub fn compile(rule: ReactionRule) -> Result<Self> {
        let tool_pattern = Pattern::new(&rule.tool)
            .with_context(|| format!("Invalid tool glob pattern '{}' in rule '{}'", rule.tool, rule.name))?;

        let mut match_regexes = Vec::new();
        for (param, pattern) in &rule.match_conditions {
            let regex = Regex::new(pattern)
                .with_context(|| format!("Invalid regex '{}' for param '{}' in rule '{}'", pattern, param, rule.name))?;
            match_regexes.push((param.clone(), regex));
        }

        Ok(Self {
            rule,
            tool_pattern,
            match_regexes,
        })
    }
}

// ---------------------------------------------------------------------------
// Rule matching
// ---------------------------------------------------------------------------

/// Result of matching a rule against a tool call.
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// The rule name
    pub rule_name: String,
    /// Captured groups from regex matches (param_name -> captures)
    pub captures: HashMap<String, String>,
}

/// A tool call event to match against rules.
#[derive(Debug, Clone)]
pub struct ToolEvent {
    /// Tool name (e.g., "Edit", "Bash", "mcp__homelab__batch_probe")
    pub tool_name: String,
    /// Tool input parameters as JSON
    pub tool_input: serde_json::Value,
}

impl ToolEvent {
    /// Get a tool argument by name, as a string.
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.tool_input.get(name).and_then(|v| v.as_str())
    }
}

/// Match a tool event against compiled rules. Returns all matching rules.
pub fn match_rules<'a>(event: &ToolEvent, rules: &'a [CompiledRule]) -> Vec<(&'a CompiledRule, MatchResult)> {
    let mut matches = Vec::new();

    for compiled in rules {
        // Step 1: tool name glob match
        if !compiled.tool_pattern.matches(&event.tool_name) {
            continue;
        }

        // Step 2: parameter match conditions (all must match)
        let mut all_match = true;
        let mut captures = HashMap::new();

        for (param, regex) in &compiled.match_regexes {
            let value = event.arg(param).unwrap_or("");
            if let Some(caps) = regex.captures(value) {
                // Store the full match
                captures.insert(param.clone(), caps.get(0).map_or("", |m| m.as_str()).to_string());
                // Store named captures
                for name in regex.capture_names().flatten() {
                    if let Some(m) = caps.name(name) {
                        captures.insert(format!("matched.{}", name), m.as_str().to_string());
                    }
                }
            } else {
                all_match = false;
                break;
            }
        }

        if all_match {
            matches.push((
                compiled,
                MatchResult {
                    rule_name: compiled.rule.name.clone(),
                    captures,
                },
            ));
        }
    }

    matches
}

// ---------------------------------------------------------------------------
// Template rendering
// ---------------------------------------------------------------------------

/// Render a template string, substituting `{args.X}`, `{file_stem}`,
/// and `{matched.X}` placeholders.
pub fn render_template(template: &str, event: &ToolEvent, captures: &HashMap<String, String>) -> String {
    let mut result = template.to_string();

    // Replace {args.X} with tool input parameters
    let args_re = Regex::new(r"\{args\.([a-zA-Z_][a-zA-Z0-9_]*)\}").unwrap();
    result = args_re
        .replace_all(&result, |caps: &regex::Captures| {
            let param_name = &caps[1];
            event.arg(param_name).unwrap_or("").to_string()
        })
        .into_owned();

    // Replace {matched.X} with regex capture groups
    let matched_re = Regex::new(r"\{matched\.([a-zA-Z_][a-zA-Z0-9_]*)\}").unwrap();
    result = matched_re
        .replace_all(&result, |caps: &regex::Captures| {
            let capture_name = &caps[1];
            captures
                .get(&format!("matched.{}", capture_name))
                .map_or("", |v| v.as_str())
                .to_string()
        })
        .into_owned();

    // Replace {file_stem} — derived from args.file_path
    if result.contains("{file_stem}") {
        let file_stem = event
            .arg("file_path")
            .and_then(|p| Path::new(p).file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("");
        result = result.replace("{file_stem}", file_stem);
    }

    result
}

// ---------------------------------------------------------------------------
// Coupling query
// ---------------------------------------------------------------------------

/// Result of a coupling-based reaction.
#[derive(Debug, Clone)]
pub struct CouplingResult {
    /// Files coupled to the edited file, above the threshold.
    pub coupled_files: Vec<CoupledFile>,
}

/// A file coupled to the edited file.
#[derive(Debug, Clone)]
pub struct CoupledFile {
    pub path: String,
    pub score: f32,
    pub co_changes: u32,
}

/// Query temporal coupling for a file path.
/// Returns coupled files above the given threshold.
pub fn query_coupling(
    store: &MetadataStore,
    file_path: &str,
    threshold: f32,
    limit: usize,
) -> Result<CouplingResult> {
    let couplings = store.get_coupling(file_path, limit)?;

    let coupled_files: Vec<CoupledFile> = couplings
        .into_iter()
        .filter(|c| c.score >= threshold)
        .map(|c: FileCoupling| {
            // Return the "other" file (not the one we queried)
            let path = if c.file_a == file_path {
                c.file_b
            } else {
                c.file_a
            };
            CoupledFile {
                path,
                score: c.score,
                co_changes: c.co_changes,
            }
        })
        .collect();

    Ok(CouplingResult { coupled_files })
}

// ---------------------------------------------------------------------------
// Reaction output formatting
// ---------------------------------------------------------------------------

/// Format a reaction for injection into agent context.
pub fn format_reaction(
    rule: &ReactionRule,
    guidance: &str,
    coupled_files: Option<&[CoupledFile]>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== Reaction: {} ===\n\n", rule.name));
    out.push_str(guidance.trim());
    out.push('\n');

    if let Some(files) = coupled_files {
        if files.is_empty() {
            // No coupled files found
        } else {
            out.push_str(&format!("\n--- Coupled files ({} results) ---\n\n", files.len()));
            for f in files {
                out.push_str(&format!(
                    "  {} (coupling: {:.2}, co-changes: {})\n",
                    f.path, f.score, f.co_changes
                ));
            }
        }
    }

    out.push_str(&format!("\n=== End Reaction ==="));
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- TOML parsing tests --

    #[test]
    fn test_parse_empty_config() {
        let config = ReactionConfig::parse("").unwrap();
        assert!(config.reactions.is_empty());
    }

    #[test]
    fn test_parse_single_rule() {
        let toml = r#"
[[reactions]]
name = "iac-drift-check"
tool = "mcp__homelab__batch_probe"
guidance = "Check IaC for drift"
search_query = "terraform container"
max_context_lines = 30
"#;
        let config = ReactionConfig::parse(toml).unwrap();
        assert_eq!(config.reactions.len(), 1);
        let rule = &config.reactions[0];
        assert_eq!(rule.name, "iac-drift-check");
        assert_eq!(rule.tool, "mcp__homelab__batch_probe");
        assert_eq!(rule.guidance, "Check IaC for drift");
        assert_eq!(rule.search_query, "terraform container");
        assert_eq!(rule.max_context_lines, 30);
        assert!(!rule.use_coupling);
    }

    #[test]
    fn test_parse_rule_with_match_conditions() {
        let toml = r#"
[[reactions]]
name = "apt-iac"
tool = "Bash"
guidance = "Package installed directly"
search_query = "ansible package"

[reactions.match]
command = "apt install .*"
"#;
        let config = ReactionConfig::parse(toml).unwrap();
        assert_eq!(config.reactions.len(), 1);
        let rule = &config.reactions[0];
        assert_eq!(rule.match_conditions.get("command").unwrap(), "apt install .*");
    }

    #[test]
    fn test_parse_coupling_rule() {
        let toml = r#"
[[reactions]]
name = "coupled-files"
tool = "Edit"
use_coupling = true
coupling_threshold = 0.4
guidance = "These files historically change together"
"#;
        let config = ReactionConfig::parse(toml).unwrap();
        let rule = &config.reactions[0];
        assert!(rule.use_coupling);
        assert!((rule.coupling_threshold - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_multiple_rules() {
        let toml = r#"
[[reactions]]
name = "rule1"
tool = "Edit"
guidance = "First rule"

[[reactions]]
name = "rule2"
tool = "Bash"
guidance = "Second rule"
search_tags = ["auto:config", "user:ops"]
"#;
        let config = ReactionConfig::parse(toml).unwrap();
        assert_eq!(config.reactions.len(), 2);
        assert_eq!(config.reactions[1].search_tags, vec!["auto:config", "user:ops"]);
    }

    #[test]
    fn test_parse_default_values() {
        let toml = r#"
[[reactions]]
name = "minimal"
tool = "Edit"
"#;
        let config = ReactionConfig::parse(toml).unwrap();
        let rule = &config.reactions[0];
        assert_eq!(rule.max_context_lines, 50); // default
        assert!((rule.coupling_threshold - 0.3).abs() < f32::EPSILON); // default
        assert!(rule.guidance.is_empty());
        assert!(rule.search_query.is_empty());
        assert!(rule.search_group.is_empty());
        assert!(rule.search_tags.is_empty());
        assert!(!rule.use_coupling);
    }

    #[test]
    fn test_parse_invalid_toml() {
        let result = ReactionConfig::parse("not valid toml [[[");
        assert!(result.is_err());
    }

    // -- Rule compilation tests --

    #[test]
    fn test_compile_valid_rule() {
        let rule = ReactionRule {
            name: "test".into(),
            tool: "Edit".into(),
            match_conditions: HashMap::new(),
            guidance: "test".into(),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: false,
            coupling_threshold: 0.3,
        };
        assert!(CompiledRule::compile(rule).is_ok());
    }

    #[test]
    fn test_compile_glob_pattern() {
        let rule = ReactionRule {
            name: "test".into(),
            tool: "mcp__homelab__*".into(),
            match_conditions: HashMap::new(),
            guidance: String::new(),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: false,
            coupling_threshold: 0.3,
        };
        let compiled = CompiledRule::compile(rule).unwrap();
        assert!(compiled.tool_pattern.matches("mcp__homelab__batch_probe"));
        assert!(compiled.tool_pattern.matches("mcp__homelab__service_restart"));
        assert!(!compiled.tool_pattern.matches("Edit"));
    }

    #[test]
    fn test_compile_invalid_glob() {
        let rule = ReactionRule {
            name: "test".into(),
            tool: "[invalid".into(),
            match_conditions: HashMap::new(),
            guidance: String::new(),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: false,
            coupling_threshold: 0.3,
        };
        assert!(CompiledRule::compile(rule).is_err());
    }

    #[test]
    fn test_compile_invalid_regex() {
        let mut conditions = HashMap::new();
        conditions.insert("command".into(), "[invalid regex".into());
        let rule = ReactionRule {
            name: "test".into(),
            tool: "Bash".into(),
            match_conditions: conditions,
            guidance: String::new(),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: false,
            coupling_threshold: 0.3,
        };
        assert!(CompiledRule::compile(rule).is_err());
    }

    // -- Matching tests --

    fn make_rule(name: &str, tool: &str, conditions: HashMap<String, String>) -> CompiledRule {
        CompiledRule::compile(ReactionRule {
            name: name.into(),
            tool: tool.into(),
            match_conditions: conditions,
            guidance: format!("Guidance for {}", name),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: false,
            coupling_threshold: 0.3,
        })
        .unwrap()
    }

    #[test]
    fn test_match_exact_tool() {
        let rules = vec![make_rule("r1", "Edit", HashMap::new())];
        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/test.rs"}),
        };
        let matches = match_rules(&event, &rules);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].1.rule_name, "r1");
    }

    #[test]
    fn test_match_glob_tool() {
        let rules = vec![make_rule("r1", "mcp__homelab__*", HashMap::new())];
        let event = ToolEvent {
            tool_name: "mcp__homelab__batch_probe".into(),
            tool_input: json!({"command": "uptime", "container": "monitoring"}),
        };
        let matches = match_rules(&event, &rules);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_no_match_wrong_tool() {
        let rules = vec![make_rule("r1", "Edit", HashMap::new())];
        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "ls"}),
        };
        let matches = match_rules(&event, &rules);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_with_param_regex() {
        let mut conditions = HashMap::new();
        conditions.insert("command".into(), "apt install .*".into());
        let rules = vec![make_rule("apt", "Bash", conditions)];

        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "apt install nginx"}),
        };
        let matches = match_rules(&event, &rules);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_no_match_param_regex_fails() {
        let mut conditions = HashMap::new();
        conditions.insert("command".into(), "^apt install .*".into());
        let rules = vec![make_rule("apt", "Bash", conditions)];

        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "cargo build"}),
        };
        let matches = match_rules(&event, &rules);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_file_path_glob() {
        let mut conditions = HashMap::new();
        conditions.insert("file_path".into(), r".*\.tf$".into());
        let rules = vec![make_rule("tf", "Edit", conditions)];

        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/home/user/infra/main.tf"}),
        };
        let matches = match_rules(&event, &rules);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_multiple_rules_match() {
        let rules = vec![
            make_rule("r1", "Edit", HashMap::new()),
            make_rule("r2", "Edit", HashMap::new()),
            make_rule("r3", "Bash", HashMap::new()),
        ];
        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/test.rs"}),
        };
        let matches = match_rules(&event, &rules);
        assert_eq!(matches.len(), 2); // r1 and r2 match, r3 doesn't
    }

    #[test]
    fn test_match_missing_param_treated_as_empty() {
        let mut conditions = HashMap::new();
        conditions.insert("nonexistent".into(), "^$".into()); // matches empty string
        let rules = vec![make_rule("r1", "Bash", conditions)];
        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "ls"}),
        };
        let matches = match_rules(&event, &rules);
        assert_eq!(matches.len(), 1); // empty string matches ^$
    }

    #[test]
    fn test_match_named_captures() {
        let mut conditions = HashMap::new();
        conditions.insert("command".into(), r"apt install (?P<package>\S+)".into());
        let rules = vec![make_rule("apt", "Bash", conditions)];
        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "apt install nginx"}),
        };
        let matches = match_rules(&event, &rules);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].1.captures.get("matched.package").unwrap(), "nginx");
    }

    // -- Template rendering tests --

    #[test]
    fn test_render_args_substitution() {
        let event = ToolEvent {
            tool_name: "mcp__homelab__batch_probe".into(),
            tool_input: json!({"container": "monitoring", "command": "uptime"}),
        };
        let result = render_template(
            "Container {args.container} ran {args.command}",
            &event,
            &HashMap::new(),
        );
        assert_eq!(result, "Container monitoring ran uptime");
    }

    #[test]
    fn test_render_missing_arg() {
        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "ls"}),
        };
        let result = render_template("Value: {args.missing}", &event, &HashMap::new());
        assert_eq!(result, "Value: ");
    }

    #[test]
    fn test_render_file_stem() {
        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/home/user/infra/main.tf"}),
        };
        let result = render_template("Resource: {file_stem}", &event, &HashMap::new());
        assert_eq!(result, "Resource: main");
    }

    #[test]
    fn test_render_file_stem_no_path() {
        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "ls"}),
        };
        let result = render_template("Resource: {file_stem}", &event, &HashMap::new());
        assert_eq!(result, "Resource: ");
    }

    #[test]
    fn test_render_matched_captures() {
        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "apt install nginx"}),
        };
        let mut captures = HashMap::new();
        captures.insert("matched.package".into(), "nginx".into());
        let result = render_template("Package: {matched.package}", &event, &captures);
        assert_eq!(result, "Package: nginx");
    }

    #[test]
    fn test_render_mixed_templates() {
        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/home/user/terraform/monitoring.tf", "old_string": "x", "new_string": "y"}),
        };
        let result = render_template(
            "terraform {file_stem} resource {args.file_path}",
            &event,
            &HashMap::new(),
        );
        assert_eq!(
            result,
            "terraform monitoring resource /home/user/terraform/monitoring.tf"
        );
    }

    #[test]
    fn test_render_no_templates() {
        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({}),
        };
        let result = render_template("No templates here", &event, &HashMap::new());
        assert_eq!(result, "No templates here");
    }

    // -- Coupling tests (unit-level, no DB) --

    #[test]
    fn test_coupled_file_filtering() {
        // Test the filtering logic without a real MetadataStore
        let couplings = vec![
            FileCoupling {
                file_a: "src/main.rs".into(),
                file_b: "src/lib.rs".into(),
                score: 0.8,
                co_changes: 15,
                last_co_change: 1000,
            },
            FileCoupling {
                file_a: "src/main.rs".into(),
                file_b: "src/test.rs".into(),
                score: 0.2, // below threshold
                co_changes: 2,
                last_co_change: 500,
            },
            FileCoupling {
                file_a: "src/utils.rs".into(),
                file_b: "src/main.rs".into(),
                score: 0.5,
                co_changes: 8,
                last_co_change: 900,
            },
        ];

        let file_path = "src/main.rs";
        let threshold = 0.3;

        let coupled: Vec<CoupledFile> = couplings
            .into_iter()
            .filter(|c| c.score >= threshold)
            .map(|c| {
                let path = if c.file_a == file_path {
                    c.file_b
                } else {
                    c.file_a
                };
                CoupledFile {
                    path,
                    score: c.score,
                    co_changes: c.co_changes,
                }
            })
            .collect();

        assert_eq!(coupled.len(), 2);
        assert_eq!(coupled[0].path, "src/lib.rs");
        assert_eq!(coupled[0].co_changes, 15);
        assert_eq!(coupled[1].path, "src/utils.rs");
    }

    // -- Format output tests --

    #[test]
    fn test_format_reaction_with_coupled_files() {
        let rule = ReactionRule {
            name: "coupled-files".into(),
            tool: "Edit".into(),
            match_conditions: HashMap::new(),
            guidance: "These files change together.".into(),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: true,
            coupling_threshold: 0.3,
        };
        let files = vec![
            CoupledFile {
                path: "src/lib.rs".into(),
                score: 0.85,
                co_changes: 12,
            },
            CoupledFile {
                path: "src/config.rs".into(),
                score: 0.42,
                co_changes: 5,
            },
        ];
        let output = format_reaction(&rule, "These files change together.", Some(&files));
        assert!(output.contains("=== Reaction: coupled-files ==="));
        assert!(output.contains("These files change together."));
        assert!(output.contains("src/lib.rs (coupling: 0.85, co-changes: 12)"));
        assert!(output.contains("src/config.rs (coupling: 0.42, co-changes: 5)"));
        assert!(output.contains("=== End Reaction ==="));
    }

    #[test]
    fn test_format_reaction_guidance_only() {
        let rule = ReactionRule {
            name: "simple".into(),
            tool: "Bash".into(),
            match_conditions: HashMap::new(),
            guidance: "Remember to check IaC".into(),
            search_query: "terraform".into(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: false,
            coupling_threshold: 0.3,
        };
        let output = format_reaction(&rule, "Remember to check IaC", None);
        assert!(output.contains("=== Reaction: simple ==="));
        assert!(output.contains("Remember to check IaC"));
        assert!(!output.contains("Coupled files"));
        assert!(output.contains("=== End Reaction ==="));
    }

    #[test]
    fn test_format_reaction_empty_coupled_files() {
        let rule = ReactionRule {
            name: "empty".into(),
            tool: "Edit".into(),
            match_conditions: HashMap::new(),
            guidance: "Checking coupling...".into(),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: true,
            coupling_threshold: 0.3,
        };
        let output = format_reaction(&rule, "Checking coupling...", Some(&[]));
        assert!(output.contains("Checking coupling..."));
        assert!(!output.contains("Coupled files"));
    }

    // -- Integration-style test: full TOML → match → render --

    #[test]
    fn test_end_to_end_match_and_render() {
        let toml = r#"
[[reactions]]
name = "iac-drift-check"
tool = "mcp__homelab__batch_probe"
guidance = """
You modified container {args.container} via batch_probe.
Ensure changes are reflected in IaC.
"""
search_query = "terraform container {args.container}"
search_group = "goldblum"
search_tags = ["auto:config"]
max_context_lines = 50

[[reactions]]
name = "apt-iac"
tool = "Bash"
guidance = "Package {matched.package} installed. Add to IaC."
search_query = "ansible package {matched.package}"

[reactions.match]
command = "apt install (?P<package>\\S+)"

[[reactions]]
name = "coupled-files"
tool = "Edit"
use_coupling = true
coupling_threshold = 0.3
guidance = "Review coupled files for {file_stem}"
"#;

        let config = ReactionConfig::parse(toml).unwrap();
        assert_eq!(config.reactions.len(), 3);

        // Compile all rules
        let compiled: Vec<CompiledRule> = config
            .reactions
            .into_iter()
            .map(|r| CompiledRule::compile(r).unwrap())
            .collect();

        // Test 1: batch_probe event
        let event1 = ToolEvent {
            tool_name: "mcp__homelab__batch_probe".into(),
            tool_input: json!({"command": "uptime", "container": "monitoring"}),
        };
        let matches1 = match_rules(&event1, &compiled);
        assert_eq!(matches1.len(), 1);
        assert_eq!(matches1[0].1.rule_name, "iac-drift-check");

        let guidance = render_template(&matches1[0].0.rule.guidance, &event1, &matches1[0].1.captures);
        assert!(guidance.contains("container monitoring"));

        let query = render_template(&matches1[0].0.rule.search_query, &event1, &matches1[0].1.captures);
        assert_eq!(query, "terraform container monitoring");

        // Test 2: apt install event
        let event2 = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "apt install nginx"}),
        };
        let matches2 = match_rules(&event2, &compiled);
        assert_eq!(matches2.len(), 1);
        assert_eq!(matches2[0].1.rule_name, "apt-iac");

        let guidance2 = render_template(&matches2[0].0.rule.guidance, &event2, &matches2[0].1.captures);
        assert_eq!(guidance2, "Package nginx installed. Add to IaC.");

        // Test 3: Edit event (should match coupled-files rule)
        let event3 = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/home/user/src/handler.go", "old_string": "x", "new_string": "y"}),
        };
        let matches3 = match_rules(&event3, &compiled);
        assert_eq!(matches3.len(), 1);
        assert_eq!(matches3[0].1.rule_name, "coupled-files");

        let guidance3 = render_template(&matches3[0].0.rule.guidance, &event3, &matches3[0].1.captures);
        assert_eq!(guidance3, "Review coupled files for handler");

        // Test 4: unrelated tool — no matches
        let event4 = ToolEvent {
            tool_name: "Read".into(),
            tool_input: json!({"file_path": "/tmp/test"}),
        };
        let matches4 = match_rules(&event4, &compiled);
        assert!(matches4.is_empty());
    }

    #[test]
    fn test_wildcard_all_tools() {
        let rules = vec![make_rule("catch-all", "*", HashMap::new())];
        let event = ToolEvent {
            tool_name: "AnythingAtAll".into(),
            tool_input: json!({}),
        };
        let matches = match_rules(&event, &rules);
        assert_eq!(matches.len(), 1);
    }

    // -- TOML roundtrip test --

    #[test]
    fn test_serialize_roundtrip() {
        let config = ReactionConfig {
            reactions: vec![ReactionRule {
                name: "test".into(),
                tool: "Edit".into(),
                match_conditions: HashMap::new(),
                guidance: "Test guidance".into(),
                search_query: "test query".into(),
                search_group: "default".into(),
                search_tags: vec!["auto:config".into()],
                max_context_lines: 30,
                use_coupling: false,
                coupling_threshold: 0.3,
            }],
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: ReactionConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.reactions.len(), 1);
        assert_eq!(deserialized.reactions[0].name, "test");
        assert_eq!(deserialized.reactions[0].max_context_lines, 30);
    }
}
