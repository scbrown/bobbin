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
// Session-scoped dedup
// ---------------------------------------------------------------------------

/// Dedup key: (rule_name, key_args_hash).
/// Two firings with the same rule name and same key args are duplicates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DedupKey {
    pub rule_name: String,
    pub args_hash: String,
}

/// Session dedup tracker. Stores fired (rule, args) combinations in a
/// JSONL file at `.bobbin/session/<session_id>/reactions.jsonl`.
pub struct DedupTracker {
    fired: std::collections::HashSet<DedupKey>,
    path: Option<std::path::PathBuf>,
}

impl DedupTracker {
    /// Load dedup state for a session. Creates the file if needed.
    pub fn load(repo_root: &Path, session_id: &str) -> Self {
        if session_id.is_empty() {
            return Self { fired: std::collections::HashSet::new(), path: None };
        }
        let dir = repo_root.join(".bobbin").join("session").join(session_id);
        let path = dir.join("reactions.jsonl");

        let mut fired = std::collections::HashSet::new();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                for line in content.lines() {
                    if let Ok(key) = serde_json::from_str::<DedupKey>(line) {
                        fired.insert(key);
                    }
                }
            }
        }

        Self { fired, path: Some(path) }
    }

    /// Check if a rule+args combination has already fired.
    pub fn has_fired(&self, key: &DedupKey) -> bool {
        self.fired.contains(key)
    }

    /// Record that a rule+args combination has fired.
    pub fn record(&mut self, key: DedupKey) {
        if self.fired.insert(key.clone()) {
            // Append to file
            if let Some(ref path) = self.path {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(line) = serde_json::to_string(&key) {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        let _ = writeln!(f, "{}", line);
                    }
                }
            }
        }
    }

    /// Compute a dedup key for a rule match.
    /// Uses the tool_input values that the rule's match conditions target,
    /// or the full tool_name for rules without match conditions.
    pub fn make_key(rule: &ReactionRule, event: &ToolEvent) -> DedupKey {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(event.tool_name.as_bytes());
        if rule.match_conditions.is_empty() {
            // For rules without match conditions, hash key tool args
            // Use file_path for Edit/Write, command for Bash, etc.
            for key in &["file_path", "command", "container", "service", "pattern"] {
                if let Some(val) = event.arg(key) {
                    hasher.update(key.as_bytes());
                    hasher.update(val.as_bytes());
                }
            }
        } else {
            for param in rule.match_conditions.keys() {
                if let Some(val) = event.arg(param) {
                    hasher.update(param.as_bytes());
                    hasher.update(val.as_bytes());
                }
            }
        }
        let hash = hex::encode(hasher.finalize());
        DedupKey {
            rule_name: rule.name.clone(),
            args_hash: hash[..16].to_string(), // Truncate for readability
        }
    }
}

// ---------------------------------------------------------------------------
// Reaction evaluation engine
// ---------------------------------------------------------------------------

/// Result of evaluating all reactions for a tool event.
pub struct EvaluationResult {
    /// Formatted reaction output to inject.
    pub output: String,
    /// Number of reactions that fired.
    pub reactions_fired: usize,
    /// Rule names that fired.
    pub rules_fired: Vec<String>,
    /// Rules that matched but were deduped.
    pub rules_deduped: usize,
}

/// Evaluate reaction rules for a tool event.
/// Handles matching, dedup, coupling, and output formatting.
/// Does NOT handle ContextAssembler searches — returns pending search
/// queries for the caller to execute (since ContextAssembler is async and
/// requires Embedder/VectorStore that live in hook.rs).
pub fn evaluate_reactions(
    event: &ToolEvent,
    rules: &[CompiledRule],
    dedup: &mut DedupTracker,
    metadata_store: Option<&MetadataStore>,
    global_budget: usize,
) -> EvaluationResult {
    let matches = match_rules(event, rules);
    let mut output = String::new();
    let mut lines_used = 0usize;
    let mut reactions_fired = 0;
    let mut rules_fired = Vec::new();
    let mut rules_deduped = 0;

    for (compiled, match_result) in &matches {
        // Budget check
        if lines_used >= global_budget {
            break;
        }

        // Dedup check
        let dedup_key = DedupTracker::make_key(&compiled.rule, event);
        if dedup.has_fired(&dedup_key) {
            rules_deduped += 1;
            continue;
        }

        // Render guidance
        let guidance = render_template(&compiled.rule.guidance, event, &match_result.captures);

        // Handle coupling-based reactions
        let coupled_files = if compiled.rule.use_coupling {
            if let Some(store) = metadata_store {
                let file_path = event.arg("file_path").unwrap_or("");
                match query_coupling(store, file_path, compiled.rule.coupling_threshold, 10) {
                    Ok(result) if !result.coupled_files.is_empty() => {
                        Some(result.coupled_files)
                    }
                    Ok(_) => Some(vec![]), // Empty coupling — still fire with guidance
                    Err(_) => Some(vec![]), // Error querying — still fire with guidance
                }
            } else {
                Some(vec![]) // No store available
            }
        } else {
            None // Not a coupling rule
        };

        // Format the reaction output
        let reaction_text = format_reaction(
            &compiled.rule,
            &guidance,
            coupled_files.as_deref(),
        );

        // Count lines and enforce per-reaction budget
        let reaction_lines: Vec<&str> = reaction_text.lines().collect();
        let max_lines = compiled.rule.max_context_lines.min(global_budget - lines_used);
        let lines_to_add = reaction_lines.len().min(max_lines);

        if !output.is_empty() {
            output.push('\n');
            lines_used += 1;
        }

        for line in &reaction_lines[..lines_to_add] {
            output.push_str(line);
            output.push('\n');
        }
        lines_used += lines_to_add;

        // If we truncated, add an indicator
        if lines_to_add < reaction_lines.len() {
            output.push_str("... (truncated by budget)\n");
            lines_used += 1;
        }

        // Record dedup and stats
        dedup.record(dedup_key);
        reactions_fired += 1;
        rules_fired.push(compiled.rule.name.clone());
    }

    EvaluationResult {
        output,
        reactions_fired,
        rules_fired,
        rules_deduped,
    }
}

/// Return pending search queries from matching rules (for search-based reactions).
/// The caller executes these via ContextAssembler and formats the results.
pub fn pending_searches(
    event: &ToolEvent,
    rules: &[CompiledRule],
    dedup: &DedupTracker,
) -> Vec<PendingSearch> {
    let matches = match_rules(event, rules);
    let mut searches = Vec::new();

    for (compiled, match_result) in &matches {
        if compiled.rule.use_coupling || compiled.rule.search_query.is_empty() {
            continue;
        }
        let dedup_key = DedupTracker::make_key(&compiled.rule, event);
        if dedup.has_fired(&dedup_key) {
            continue;
        }
        let query = render_template(&compiled.rule.search_query, event, &match_result.captures);
        if query.trim().is_empty() {
            continue;
        }
        searches.push(PendingSearch {
            rule_name: compiled.rule.name.clone(),
            query,
            group: if compiled.rule.search_group.is_empty() {
                None
            } else {
                Some(compiled.rule.search_group.clone())
            },
            tags: compiled.rule.search_tags.clone(),
            max_lines: compiled.rule.max_context_lines,
        });
    }

    searches
}

/// A search that needs to be executed by the caller via ContextAssembler.
#[derive(Debug, Clone)]
pub struct PendingSearch {
    pub rule_name: String,
    pub query: String,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub max_lines: usize,
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

    // -- Dedup tests --

    #[test]
    fn test_dedup_key_same_rule_same_args() {
        let rule = ReactionRule {
            name: "test".into(),
            tool: "Edit".into(),
            match_conditions: HashMap::new(),
            guidance: String::new(),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: false,
            coupling_threshold: 0.3,
        };
        let event1 = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/a.rs"}),
        };
        let event2 = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/a.rs"}),
        };
        let key1 = DedupTracker::make_key(&rule, &event1);
        let key2 = DedupTracker::make_key(&rule, &event2);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_dedup_key_same_rule_different_args() {
        let rule = ReactionRule {
            name: "test".into(),
            tool: "Edit".into(),
            match_conditions: HashMap::new(),
            guidance: String::new(),
            search_query: String::new(),
            search_group: String::new(),
            search_tags: vec![],
            max_context_lines: 50,
            use_coupling: false,
            coupling_threshold: 0.3,
        };
        let event1 = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/a.rs"}),
        };
        let event2 = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/b.rs"}),
        };
        let key1 = DedupTracker::make_key(&rule, &event1);
        let key2 = DedupTracker::make_key(&rule, &event2);
        assert_ne!(key1, key2); // Different files = different keys
    }

    #[test]
    fn test_dedup_tracker_in_memory() {
        let mut tracker = DedupTracker {
            fired: std::collections::HashSet::new(),
            path: None,
        };
        let key = DedupKey {
            rule_name: "test".into(),
            args_hash: "abc123".into(),
        };
        assert!(!tracker.has_fired(&key));
        tracker.record(key.clone());
        assert!(tracker.has_fired(&key));
    }

    #[test]
    fn test_dedup_tracker_persistence() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(".bobbin").join("session").join("test-session");
        std::fs::create_dir_all(&session_dir).unwrap();

        // Create tracker, record a key
        {
            let mut tracker = DedupTracker::load(tmp.path(), "test-session");
            let key = DedupKey {
                rule_name: "rule1".into(),
                args_hash: "hash1".into(),
            };
            assert!(!tracker.has_fired(&key));
            tracker.record(key);
        }

        // Reload tracker — key should persist
        {
            let tracker = DedupTracker::load(tmp.path(), "test-session");
            let key = DedupKey {
                rule_name: "rule1".into(),
                args_hash: "hash1".into(),
            };
            assert!(tracker.has_fired(&key));

            // Different key should not be fired
            let key2 = DedupKey {
                rule_name: "rule2".into(),
                args_hash: "hash2".into(),
            };
            assert!(!tracker.has_fired(&key2));
        }
    }

    // -- Evaluation tests --

    #[test]
    fn test_evaluate_reactions_basic() {
        let rules = vec![make_rule("r1", "Edit", HashMap::new())];
        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/test.rs"}),
        };
        let mut dedup = DedupTracker {
            fired: std::collections::HashSet::new(),
            path: None,
        };
        let result = evaluate_reactions(&event, &rules, &mut dedup, None, 100);
        assert_eq!(result.reactions_fired, 1);
        assert_eq!(result.rules_fired, vec!["r1"]);
        assert!(result.output.contains("=== Reaction: r1 ==="));
        assert!(result.output.contains("Guidance for r1"));
    }

    #[test]
    fn test_evaluate_reactions_dedup() {
        let rules = vec![make_rule("r1", "Edit", HashMap::new())];
        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/test.rs"}),
        };
        let mut dedup = DedupTracker {
            fired: std::collections::HashSet::new(),
            path: None,
        };

        // First evaluation fires
        let result1 = evaluate_reactions(&event, &rules, &mut dedup, None, 100);
        assert_eq!(result1.reactions_fired, 1);

        // Second evaluation with same args is deduped
        let result2 = evaluate_reactions(&event, &rules, &mut dedup, None, 100);
        assert_eq!(result2.reactions_fired, 0);
        assert_eq!(result2.rules_deduped, 1);
        assert!(result2.output.is_empty());
    }

    #[test]
    fn test_evaluate_reactions_dedup_different_args() {
        let rules = vec![make_rule("r1", "Edit", HashMap::new())];
        let mut dedup = DedupTracker {
            fired: std::collections::HashSet::new(),
            path: None,
        };

        let event1 = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/a.rs"}),
        };
        let result1 = evaluate_reactions(&event1, &rules, &mut dedup, None, 100);
        assert_eq!(result1.reactions_fired, 1);

        // Different file = different args = fires again
        let event2 = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/b.rs"}),
        };
        let result2 = evaluate_reactions(&event2, &rules, &mut dedup, None, 100);
        assert_eq!(result2.reactions_fired, 1);
    }

    #[test]
    fn test_evaluate_reactions_budget_limit() {
        // Create rules with large guidance that exceeds budget
        let rules: Vec<CompiledRule> = (0..5)
            .map(|i| {
                CompiledRule::compile(ReactionRule {
                    name: format!("rule{}", i),
                    tool: "Edit".into(),
                    match_conditions: HashMap::new(),
                    guidance: "Line1\nLine2\nLine3\nLine4\nLine5".into(),
                    search_query: String::new(),
                    search_group: String::new(),
                    search_tags: vec![],
                    max_context_lines: 50,
                    use_coupling: false,
                    coupling_threshold: 0.3,
                })
                .unwrap()
            })
            .collect();

        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/tmp/test.rs"}),
        };
        let mut dedup = DedupTracker {
            fired: std::collections::HashSet::new(),
            path: None,
        };

        // Budget of 20 lines — not all 5 rules will fit
        let result = evaluate_reactions(&event, &rules, &mut dedup, None, 20);
        assert!(result.reactions_fired > 0);
        assert!(result.reactions_fired < 5);
    }

    #[test]
    fn test_evaluate_no_match() {
        let rules = vec![make_rule("r1", "Edit", HashMap::new())];
        let event = ToolEvent {
            tool_name: "Bash".into(),
            tool_input: json!({"command": "ls"}),
        };
        let mut dedup = DedupTracker {
            fired: std::collections::HashSet::new(),
            path: None,
        };
        let result = evaluate_reactions(&event, &rules, &mut dedup, None, 100);
        assert_eq!(result.reactions_fired, 0);
        assert!(result.output.is_empty());
    }

    // -- Pending searches tests --

    #[test]
    fn test_pending_searches() {
        let rules: Vec<CompiledRule> = vec![
            CompiledRule::compile(ReactionRule {
                name: "search-rule".into(),
                tool: "Edit".into(),
                match_conditions: HashMap::new(),
                guidance: "Check related".into(),
                search_query: "related to {file_stem}".into(),
                search_group: "infra".into(),
                search_tags: vec!["auto:config".into()],
                max_context_lines: 50,
                use_coupling: false,
                coupling_threshold: 0.3,
            })
            .unwrap(),
            CompiledRule::compile(ReactionRule {
                name: "coupling-rule".into(),
                tool: "Edit".into(),
                match_conditions: HashMap::new(),
                guidance: "Coupling check".into(),
                search_query: String::new(),
                search_group: String::new(),
                search_tags: vec![],
                max_context_lines: 50,
                use_coupling: true,
                coupling_threshold: 0.3,
            })
            .unwrap(),
        ];

        let event = ToolEvent {
            tool_name: "Edit".into(),
            tool_input: json!({"file_path": "/home/user/main.tf"}),
        };
        let dedup = DedupTracker {
            fired: std::collections::HashSet::new(),
            path: None,
        };

        let searches = pending_searches(&event, &rules, &dedup);
        assert_eq!(searches.len(), 1); // Only search-rule, not coupling-rule
        assert_eq!(searches[0].rule_name, "search-rule");
        assert_eq!(searches[0].query, "related to main");
        assert_eq!(searches[0].group, Some("infra".into()));
        assert_eq!(searches[0].tags, vec!["auto:config"]);
    }
}
