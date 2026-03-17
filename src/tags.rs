use anyhow::{bail, Context, Result};
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

use crate::types::{classify_file, FileCategory};

/// Maximum tag name length
const TAG_MAX_LEN: usize = 32;

/// Default frontmatter field names to check for tags
const FRONTMATTER_FALLBACK_FIELDS: &[&str] = &["bobbin-tags", "labels"];

/// Tags configuration loaded from .bobbin/tags.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TagsConfig {
    /// Pattern-based tag rules
    pub rules: Vec<TagRule>,
    /// Global tag effects on scoring (`[effects.<tag>]`)
    pub effects: std::collections::HashMap<String, TagEffect>,
    /// Role-scoped tag effects (`[[effects_scoped]]`)
    pub effects_scoped: Vec<ScopedEffect>,
    /// Frontmatter tag extraction config
    pub frontmatter: FrontmatterConfig,
    /// Code comment tag extraction config
    pub comments: CommentsConfig,
}

/// Configuration for extracting tags from markdown YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FrontmatterConfig {
    /// Whether frontmatter extraction is enabled
    pub enabled: bool,
    /// Primary YAML field name to extract tags from
    pub field: String,
}

impl Default for FrontmatterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            field: "tags".to_string(),
        }
    }
}

/// Configuration for extracting tags from code comments.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommentsConfig {
    /// Whether comment extraction is enabled
    pub enabled: bool,
    /// Comment directive prefix (e.g. "bobbin:tag")
    pub prefix: String,
}

impl Default for CommentsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prefix: "bobbin:tag".to_string(),
        }
    }
}

/// A single pattern → tags rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagRule {
    /// Glob pattern to match file paths
    pub pattern: String,
    /// Tags to apply when pattern matches
    pub tags: Vec<String>,
    /// Optional repo scope (only apply when indexing this repo)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
}

/// Effect applied when a tag is present on a chunk.
/// Global effects live in `[effects.<tag>]` in tags.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TagEffect {
    /// Score multiplier: positive = boost, negative = demote
    #[serde(default)]
    pub boost: f32,
    /// When true, chunks with this tag are excluded from results entirely
    #[serde(default)]
    pub exclude: bool,
    /// When true, chunks with this tag bypass relevance threshold and are injected first
    #[serde(default)]
    pub pin: bool,
    /// Lines of budget reserved for pinned chunks (only meaningful when pin=true)
    #[serde(default)]
    pub budget_reserve: usize,
}

/// Role-scoped tag effect from `[[effects_scoped]]` in tags.toml.
/// Overrides the global effect for matching roles.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScopedEffect {
    /// Tag this effect applies to
    pub tag: String,
    /// Role glob pattern (e.g. "aegis/*", "aegis/crew/sentinel")
    pub role: String,
    /// Score multiplier (optional — omitted means no boost change)
    #[serde(default)]
    pub boost: f32,
    /// When true, chunks with this tag are excluded for matching roles
    #[serde(default)]
    pub exclude: bool,
    /// When true, chunks with this tag are pinned for matching roles
    #[serde(default)]
    pub pin: bool,
    /// Lines of budget reserved for pinned chunks (only meaningful when pin=true)
    #[serde(default)]
    pub budget_reserve: usize,
}

/// The resolved effect for a specific tag + role combination.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEffect {
    pub boost: f32,
    pub exclude: bool,
    pub pin: bool,
    pub budget_reserve: usize,
}

impl TagsConfig {
    /// Load tags config from a TOML file
    pub fn load(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))
    }

    /// Load tags config, returning default if file doesn't exist
    pub fn load_or_default(path: &Path) -> Self {
        if path.exists() {
            match Self::load(path) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("warning: failed to parse {}: {e:#}; using defaults", path.display());
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }

    /// Save tags config to a TOML file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content =
            toml::to_string_pretty(self).context("serializing tags config")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(path, content)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// Path to tags.toml relative to a bobbin data root
    pub fn tags_path(repo_root: &Path) -> std::path::PathBuf {
        repo_root.join(".bobbin").join("tags.toml")
    }

    /// Resolve the effective tag effect for a given tag and role.
    ///
    /// Resolution order:
    /// 1. Check `effects_scoped` for entries matching this tag AND role glob.
    ///    Most specific role glob wins (counted by non-wildcard path segments).
    /// 2. Fall back to global `effects[tag]`.
    /// 3. Return None if neither exists.
    pub fn resolve_effect(&self, tag: &str, role: Option<&str>) -> Option<ResolvedEffect> {
        if let Some(role) = role {
            let mut best: Option<(&ScopedEffect, usize)> = None;
            for scoped in &self.effects_scoped {
                if scoped.tag != tag {
                    continue;
                }
                if let Ok(pat) = Pattern::new(&scoped.role) {
                    if pat.matches(role) {
                        let specificity = role_specificity(&scoped.role);
                        if best.map_or(true, |(_, s)| specificity > s) {
                            best = Some((scoped, specificity));
                        }
                    }
                }
            }
            if let Some((scoped, _)) = best {
                return Some(ResolvedEffect {
                    boost: scoped.boost,
                    exclude: scoped.exclude,
                    pin: scoped.pin,
                    budget_reserve: scoped.budget_reserve,
                });
            }
        }
        // Fall back to global effect
        self.effects.get(tag).map(|e| ResolvedEffect {
            boost: e.boost,
            exclude: e.exclude,
            pin: e.pin,
            budget_reserve: e.budget_reserve,
        })
    }

    /// Check if a chunk's tags result in a pin effect for the given role.
    /// Returns the maximum budget_reserve across all matching pin effects.
    pub fn resolve_pin(&self, tags_str: &str, role: Option<&str>) -> Option<usize> {
        if tags_str.is_empty() {
            return None;
        }
        let mut max_reserve: usize = 0;
        let mut is_pinned = false;
        for tag in tags_str.split(',') {
            if let Some(effect) = self.resolve_effect(tag, role) {
                if effect.pin {
                    is_pinned = true;
                    max_reserve = max_reserve.max(effect.budget_reserve);
                }
            }
        }
        if is_pinned { Some(max_reserve) } else { None }
    }
}

/// Count specificity of a role pattern: number of path segments that don't contain wildcards.
/// More literal segments = more specific.
fn role_specificity(pattern: &str) -> usize {
    pattern
        .split('/')
        .filter(|seg| !seg.contains('*') && !seg.contains('?'))
        .count()
}

/// Build a SQL filter clause that includes only chunks having at least one of the given tags.
/// Returns a WHERE-compatible expression using LIKE on the comma-separated tags column.
pub fn build_tag_include_filter(tags: &[String]) -> String {
    let clauses: Vec<String> = tags.iter().map(|t| tag_match_clause(t)).collect();
    if clauses.len() == 1 {
        clauses.into_iter().next().unwrap()
    } else {
        format!("({})", clauses.join(" OR "))
    }
}

/// Build a SQL filter clause that excludes chunks having any of the given tags.
/// Returns a WHERE-compatible expression that rejects any match.
pub fn build_tag_exclude_filter(tags: &[String]) -> String {
    let clauses: Vec<String> = tags
        .iter()
        .map(|t| format!("NOT ({})", tag_match_clause(t)))
        .collect();
    if clauses.len() == 1 {
        clauses.into_iter().next().unwrap()
    } else {
        format!("({})", clauses.join(" AND "))
    }
}

/// Build a SQL filter clause that excludes chunks whose tags have an exclude effect
/// for the given role. Checks both global effects (exclude=true) and scoped effects.
/// Returns None if no excludes are active.
pub fn build_effect_exclude_filter(config: &TagsConfig, role: Option<&str>) -> Option<String> {
    let mut excluded_tags: Vec<String> = Vec::new();

    // Collect globally excluded tags
    for (tag, effect) in &config.effects {
        if effect.exclude {
            // Check if a scoped override for this role un-excludes it
            if let Some(r) = role {
                let scoped_override = config
                    .effects_scoped
                    .iter()
                    .filter(|s| s.tag == *tag)
                    .filter(|s| {
                        Pattern::new(&s.role)
                            .map(|p| p.matches(r))
                            .unwrap_or(false)
                    })
                    .max_by_key(|s| role_specificity(&s.role));
                if let Some(s) = scoped_override {
                    if !s.exclude {
                        continue; // scoped override says don't exclude
                    }
                }
            }
            excluded_tags.push(tag.clone());
        }
    }

    // Collect role-scoped excludes (tags excluded for this role but not globally)
    if let Some(role) = role {
        for scoped in &config.effects_scoped {
            if !scoped.exclude {
                continue;
            }
            if excluded_tags.contains(&scoped.tag) {
                continue; // already excluded globally
            }
            if let Ok(pat) = Pattern::new(&scoped.role) {
                if pat.matches(role) {
                    excluded_tags.push(scoped.tag.clone());
                }
            }
        }
    }

    if excluded_tags.is_empty() {
        None
    } else {
        Some(build_tag_exclude_filter(&excluded_tags))
    }
}

/// Build a SQL LIKE clause matching a single tag in the comma-separated tags column.
fn tag_match_clause(tag: &str) -> String {
    let escaped = tag.replace('\'', "''");
    format!(
        "(tags = '{e}' OR tags LIKE '{e},%' OR tags LIKE '%,{e}' OR tags LIKE '%,{e},%')",
        e = escaped
    )
}

/// Validate a tag name: lowercase alphanumeric, hyphens, colons; max 32 chars.
pub fn validate_tag(tag: &str) -> Result<()> {
    if tag.is_empty() {
        bail!("tag name cannot be empty");
    }
    if tag.len() > TAG_MAX_LEN {
        bail!("tag '{}' exceeds {} character limit", tag, TAG_MAX_LEN);
    }
    if !tag
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == ':')
    {
        bail!(
            "tag '{}' contains invalid characters (allowed: a-z, 0-9, -, :)",
            tag
        );
    }
    Ok(())
}

/// Get convention tags for a file path based on classify_file() heuristics.
pub fn convention_tags(file_path: &str) -> Vec<String> {
    let mut tags = Vec::new();

    match classify_file(file_path) {
        FileCategory::Test => tags.push("auto:test".to_string()),
        FileCategory::Documentation => tags.push("auto:docs".to_string()),
        FileCategory::Config => tags.push("auto:config".to_string()),
        FileCategory::Custom(ref name) => tags.push(format!("auto:{}", name)),
        FileCategory::Source => {}
    }

    // Generated file detection
    let lower = file_path.to_lowercase();
    if lower.ends_with(".min.js")
        || lower.ends_with(".min.css")
        || lower.ends_with(".gen.go")
        || lower.ends_with(".generated.ts")
        || lower.contains("/generated/")
        || lower.ends_with("_generated.rs")
    {
        tags.push("auto:generated".to_string());
    }

    tags
}

/// Resolve all tags for a file path: convention tags + pattern rules from config.
/// Returns a comma-separated sorted string (empty string if no tags).
pub fn resolve_tags(config: &TagsConfig, file_path: &str, repo: Option<&str>) -> String {
    let mut tags: BTreeSet<String> = BTreeSet::new();

    // Convention tags
    tags.extend(convention_tags(file_path));

    // Pattern rules
    for rule in &config.rules {
        // Skip if rule is repo-scoped and doesn't match current repo
        if let Some(ref rule_repo) = rule.repo {
            if repo.map(|r| r != rule_repo).unwrap_or(true) {
                continue;
            }
        }
        match Pattern::new(&rule.pattern) {
            Ok(pat) => {
                // Try matching the path directly, and also with a dummy prefix
                // so that `**/foo` patterns match root-relative paths like `foo/bar.md`.
                // Without this, `**/snapshots/**/*.md` fails on `snapshots/ian/2026.md`
                // because `**/` requires at least one path component before the match.
                let prefixed = format!("_/{}", file_path);
                if pat.matches(file_path) || pat.matches(&prefixed) {
                    tags.extend(rule.tags.iter().cloned());
                }
            }
            Err(_) => {
                // Log once per bad pattern would be ideal, but for now just skip.
                // Invalid patterns are caught at config validation time.
            }
        }
    }

    // BTreeSet is already sorted
    let tag_vec: Vec<String> = tags.into_iter().collect();
    tag_vec.join(",")
}

/// Resolve tags for each chunk in a file, merging all sources:
/// convention + pattern rules + frontmatter (markdown) + code comments.
///
/// Modifies chunks in-place, setting their `tags` field.
pub fn resolve_tags_for_chunks(
    config: &TagsConfig,
    file_path: &str,
    repo: Option<&str>,
    content: &str,
    chunks: &mut [crate::types::Chunk],
) {
    // 1. File-level tags: convention + pattern rules
    let file_tags_str = resolve_tags(config, file_path, repo);
    let mut file_tags: BTreeSet<String> = if file_tags_str.is_empty() {
        BTreeSet::new()
    } else {
        file_tags_str.split(',').map(|s| s.to_string()).collect()
    };

    // 2. Frontmatter tags (apply to all chunks in this file)
    if config.frontmatter.enabled {
        let fm_tags = extract_frontmatter_tags(content, &config.frontmatter);
        file_tags.extend(fm_tags);
    }

    // 3. Code comment tags (per-chunk, keyed by line number)
    let comment_tags = if config.comments.enabled {
        extract_comment_tags(content, &config.comments)
    } else {
        std::collections::HashMap::new()
    };

    // 4. Merge and assign per-chunk
    for chunk in chunks.iter_mut() {
        let mut tags = file_tags.clone();

        // Auto-tag Go init() functions (boilerplate, usually noise for search)
        if chunk.name.as_deref() == Some("init")
            && matches!(chunk.chunk_type, crate::types::ChunkType::Function)
            && file_path.ends_with(".go")
        {
            tags.insert("auto:init".to_string());
        }

        // Check for comment tags on the line immediately before this chunk
        if chunk.start_line > 1 {
            if let Some(ct) = comment_tags.get(&(chunk.start_line - 1)) {
                tags.extend(ct.iter().cloned());
            }
        }
        // Also check the chunk's own first line (comment may be part of the chunk)
        if let Some(ct) = comment_tags.get(&chunk.start_line) {
            tags.extend(ct.iter().cloned());
        }

        if !tags.is_empty() {
            chunk.tags = tags.into_iter().collect::<Vec<_>>().join(",");
        }
    }
}

/// Extract tags from YAML frontmatter content.
///
/// Looks for the configured field (default: `tags`) plus fallback fields
/// (`bobbin-tags`, `labels`). Supports:
/// - Inline YAML list: `tags: [canonical, architecture]`
/// - Single value: `tags: canonical`
/// - Block list: `tags:\n  - canonical\n  - architecture`
///
/// Tags without a namespace prefix get `user:` prepended.
pub fn extract_frontmatter_tags(content: &str, config: &FrontmatterConfig) -> Vec<String> {
    // Quick check: does this look like it has frontmatter?
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return vec![];
    }

    // Extract frontmatter text between --- fences
    let after_fence = &trimmed[3..];
    let Some(close_pos) = after_fence.find("\n---") else {
        return vec![];
    };
    let fm_text = &after_fence[..close_pos];

    // Try primary field, then fallback fields
    let fields_to_check: Vec<&str> = std::iter::once(config.field.as_str())
        .chain(FRONTMATTER_FALLBACK_FIELDS.iter().copied())
        .collect();

    for field in fields_to_check {
        let tags = parse_yaml_field_tags(fm_text, field);
        if !tags.is_empty() {
            return tags;
        }
    }

    vec![]
}

/// Parse tags from a specific YAML field in frontmatter text.
fn parse_yaml_field_tags(fm_text: &str, field: &str) -> Vec<String> {
    let lines: Vec<&str> = fm_text.lines().collect();
    let field_prefix = format!("{}:", field);

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with(&field_prefix) {
            continue;
        }

        let after_key = trimmed[field_prefix.len()..].trim();

        if after_key.is_empty() {
            // Block list format:
            //   tags:
            //     - canonical
            //     - architecture
            return parse_yaml_block_list(&lines[i + 1..]);
        } else if after_key.starts_with('[') && after_key.ends_with(']') {
            // Inline list: tags: [canonical, architecture]
            let inner = &after_key[1..after_key.len() - 1];
            return parse_tag_values(inner.split(','));
        } else {
            // Single value: tags: canonical
            return parse_tag_values(std::iter::once(after_key));
        }
    }

    vec![]
}

/// Parse a YAML block list (lines starting with `- `).
fn parse_yaml_block_list(lines: &[&str]) -> Vec<String> {
    let mut tags = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("- ") {
            let val = trimmed[2..].trim().trim_matches('"').trim_matches('\'');
            if let Some(tag) = normalize_content_tag(val) {
                tags.push(tag);
            }
        } else if !trimmed.is_empty() {
            // Non-list-item, non-empty line means end of block list
            break;
        }
    }
    tags
}

/// Parse comma-separated or individual tag values, normalizing each.
fn parse_tag_values<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    values
        .map(|v| v.trim().trim_matches('"').trim_matches('\''))
        .filter(|v| !v.is_empty())
        .filter_map(normalize_content_tag)
        .collect()
}

/// Normalize a tag from content extraction:
/// - Lowercase it
/// - If no namespace prefix, add `user:`
/// - Validate the result
/// Returns None if the tag is invalid.
fn normalize_content_tag(raw: &str) -> Option<String> {
    let tag = raw.trim().to_lowercase();
    if tag.is_empty() {
        return None;
    }

    let namespaced = if tag.contains(':') {
        tag
    } else {
        format!("user:{}", tag)
    };

    if validate_tag(&namespaced).is_ok() {
        Some(namespaced)
    } else {
        None
    }
}

/// Extract `bobbin:tag` directives from code comments.
///
/// Scans each line for comment patterns like:
/// - `// bobbin:tag security critical`
/// - `# bobbin:tag deprecated`
/// - `/* bobbin:tag internal */`
///
/// Returns a map from 1-based line number to tags found on that line.
pub fn extract_comment_tags(
    content: &str,
    config: &CommentsConfig,
) -> std::collections::HashMap<u32, Vec<String>> {
    let mut result = std::collections::HashMap::new();
    let prefix = &config.prefix;

    for (idx, line) in content.lines().enumerate() {
        let line_num = idx as u32 + 1;
        let trimmed = line.trim();

        // Try each comment style
        let directive = None
            .or_else(|| extract_after_comment_prefix(trimmed, "//", prefix))
            .or_else(|| extract_after_comment_prefix(trimmed, "#", prefix))
            .or_else(|| extract_block_comment_directive(trimmed, prefix));

        if let Some(tag_str) = directive {
            let tags = parse_tag_values(tag_str.split_whitespace());
            if !tags.is_empty() {
                result.insert(line_num, tags);
            }
        }
    }

    result
}

/// Try to extract a bobbin:tag directive after a line comment prefix (// or #).
fn extract_after_comment_prefix<'a>(
    line: &'a str,
    comment_prefix: &str,
    directive_prefix: &str,
) -> Option<&'a str> {
    let stripped = line.strip_prefix(comment_prefix)?.trim_start();
    let after = stripped.strip_prefix(directive_prefix)?;
    // Must be followed by whitespace or end of line
    if after.is_empty() {
        return None; // No tags specified
    }
    if !after.starts_with(char::is_whitespace) {
        return None; // e.g. "bobbin:tagging" — not our directive
    }
    Some(after.trim())
}

/// Try to extract a bobbin:tag directive from a block comment (/* ... */).
fn extract_block_comment_directive<'a>(line: &'a str, directive_prefix: &str) -> Option<&'a str> {
    let inner = line.strip_prefix("/*")?.strip_suffix("*/")?.trim();
    let after = inner.strip_prefix(directive_prefix)?;
    if after.is_empty() {
        return None;
    }
    if !after.starts_with(char::is_whitespace) {
        return None;
    }
    Some(after.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_tag_valid() {
        assert!(validate_tag("deprecated").is_ok());
        assert!(validate_tag("auto:test").is_ok());
        assert!(validate_tag("my-tag-123").is_ok());
        assert!(validate_tag("a").is_ok());
    }

    #[test]
    fn test_validate_tag_invalid() {
        assert!(validate_tag("").is_err());
        assert!(validate_tag("UPPERCASE").is_err());
        assert!(validate_tag("has space").is_err());
        assert!(validate_tag("has.dot").is_err());
        assert!(validate_tag(&"a".repeat(33)).is_err());
    }

    #[test]
    fn test_convention_tags() {
        assert_eq!(convention_tags("tests/test_foo.py"), vec!["auto:test"]);
        assert_eq!(convention_tags("docs/guide.md"), vec!["auto:docs"]);
        assert_eq!(convention_tags("Cargo.toml"), vec!["auto:config"]);
        assert!(convention_tags("src/main.rs").is_empty());
    }

    #[test]
    fn test_convention_tags_generated() {
        assert!(convention_tags("dist/bundle.min.js").contains(&"auto:generated".to_string()));
    }

    #[test]
    fn test_resolve_tags_empty_config() {
        let config = TagsConfig::default();
        // Source file gets no tags
        assert_eq!(resolve_tags(&config, "src/main.rs", None), "");
        // Test file gets convention tag
        assert_eq!(resolve_tags(&config, "tests/test_foo.py", None), "auto:test");
    }

    #[test]
    fn test_resolve_tags_with_rules() {
        let config = TagsConfig {
            rules: vec![
                TagRule {
                    pattern: "docs/deprecated/**".to_string(),
                    tags: vec!["deprecated".to_string()],
                    repo: None,
                },
                TagRule {
                    pattern: "src/auth/**".to_string(),
                    tags: vec!["security".to_string(), "critical".to_string()],
                    repo: None,
                },
            ],
            ..Default::default()
        };

        // Convention + pattern tags merge
        assert_eq!(
            resolve_tags(&config, "docs/deprecated/old.md", None),
            "auto:docs,deprecated"
        );
        assert_eq!(
            resolve_tags(&config, "src/auth/login.rs", None),
            "critical,security"
        );
        // No match
        assert_eq!(resolve_tags(&config, "src/utils.rs", None), "");
    }

    #[test]
    fn test_resolve_tags_repo_scoped() {
        let config = TagsConfig {
            rules: vec![TagRule {
                pattern: "*.md".to_string(),
                tags: vec!["ops-docs".to_string()],
                repo: Some("aegis".to_string()),
            }],
            ..Default::default()
        };

        // Matches repo
        assert_eq!(
            resolve_tags(&config, "guide.md", Some("aegis")),
            "auto:docs,ops-docs"
        );
        // Wrong repo
        assert_eq!(
            resolve_tags(&config, "guide.md", Some("bobbin")),
            "auto:docs"
        );
        // No repo
        assert_eq!(resolve_tags(&config, "guide.md", None), "auto:docs");
    }

    #[test]
    fn test_resolve_tags_globstar_root_relative() {
        // Regression test: `**/snapshots/**/*.md` must match `snapshots/ian/2026.md`
        // even though the path has no leading directory component.
        let config = TagsConfig {
            rules: vec![
                TagRule {
                    pattern: "**/snapshots/**/*.md".to_string(),
                    tags: vec!["type:memory".to_string()],
                    repo: None,
                },
                TagRule {
                    pattern: "**/CHANGELOG.md".to_string(),
                    tags: vec!["type:changelog".to_string()],
                    repo: None,
                },
            ],
            ..Default::default()
        };

        // Root-relative path (no leading dir) — must match via prefixed fallback
        assert_eq!(
            resolve_tags(&config, "snapshots/ian/2026-03-12.md", None),
            "auto:docs,type:memory"
        );
        // Root-level file
        assert_eq!(
            resolve_tags(&config, "CHANGELOG.md", None),
            "auto:docs,type:changelog"
        );
        // Nested path — should also match (direct match)
        assert_eq!(
            resolve_tags(&config, "foo/snapshots/ian/2026-03-12.md", None),
            "auto:docs,type:memory"
        );
        // Non-matching path
        assert_eq!(resolve_tags(&config, "src/main.rs", None), "");
    }

    #[test]
    fn test_tags_config_roundtrip() {
        let config = TagsConfig {
            rules: vec![TagRule {
                pattern: "**/*_test.go".to_string(),
                tags: vec!["test".to_string()],
                repo: None,
            }],
            effects: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "deprecated".to_string(),
                    TagEffect {
                        boost: -0.8,
                        exclude: false,
                        ..Default::default()
                    },
                );
                m.insert(
                    "noise".to_string(),
                    TagEffect {
                        boost: 0.0,
                        exclude: true,
                        ..Default::default()
                    },
                );
                m
            },
            effects_scoped: vec![ScopedEffect {
                tag: "internal".to_string(),
                role: "external/*".to_string(),
                boost: 0.0,
                exclude: true,
                ..Default::default()
            }],
            ..Default::default()
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: TagsConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.rules.len(), 1);
        assert_eq!(parsed.rules[0].pattern, "**/*_test.go");
        assert!((parsed.effects["deprecated"].boost - (-0.8)).abs() < f32::EPSILON);
        assert!(!parsed.effects["deprecated"].exclude);
        assert!(parsed.effects["noise"].exclude);
        assert_eq!(parsed.effects_scoped.len(), 1);
        assert_eq!(parsed.effects_scoped[0].tag, "internal");
        assert_eq!(parsed.effects_scoped[0].role, "external/*");
        assert!(parsed.effects_scoped[0].exclude);
        // Phase 3 defaults
        assert!(parsed.frontmatter.enabled);
        assert_eq!(parsed.frontmatter.field, "tags");
        assert!(parsed.comments.enabled);
        assert_eq!(parsed.comments.prefix, "bobbin:tag");
    }

    #[test]
    fn test_tags_config_backward_compat() {
        // Old-style config without exclude or effects_scoped should parse fine
        let toml_str = r#"
[[rules]]
pattern = "*.md"
tags = ["docs"]

[effects.deprecated]
boost = -0.8
"#;
        let config: TagsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules.len(), 1);
        assert!(!config.effects["deprecated"].exclude); // default false
        assert!(config.effects_scoped.is_empty()); // default empty
    }

    #[test]
    fn test_resolve_effect_global_only() {
        let config = TagsConfig {
            effects: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "deprecated".to_string(),
                    TagEffect {
                        boost: -0.8,
                        exclude: false,
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let resolved = config.resolve_effect("deprecated", None);
        assert_eq!(
            resolved,
            Some(ResolvedEffect {
                boost: -0.8,
                exclude: false,
                pin: false,
                budget_reserve: 0,
            })
        );

        // No effect for unknown tag
        assert_eq!(config.resolve_effect("unknown", None), None);
    }

    #[test]
    fn test_resolve_effect_scoped_overrides_global() {
        let config = TagsConfig {
            effects: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "internal".to_string(),
                    TagEffect {
                        boost: 0.0,
                        exclude: false,
                        ..Default::default()
                    },
                );
                m
            },
            effects_scoped: vec![ScopedEffect {
                tag: "internal".to_string(),
                role: "external/*".to_string(),
                boost: 0.0,
                exclude: true,
                ..Default::default()
            }],
            ..Default::default()
        };

        // External role: scoped effect wins (exclude)
        let resolved = config.resolve_effect("internal", Some("external/user1"));
        assert_eq!(
            resolved,
            Some(ResolvedEffect {
                boost: 0.0,
                exclude: true,
                pin: false,
                budget_reserve: 0,
            })
        );

        // Aegis role: falls back to global (no exclude)
        let resolved = config.resolve_effect("internal", Some("aegis/crew/ellie"));
        assert_eq!(
            resolved,
            Some(ResolvedEffect {
                boost: 0.0,
                exclude: false,
                pin: false,
                budget_reserve: 0,
            })
        );
    }

    #[test]
    fn test_resolve_effect_most_specific_role_wins() {
        let config = TagsConfig {
            effects_scoped: vec![
                ScopedEffect {
                    tag: "test".to_string(),
                    role: "aegis/*".to_string(),
                    boost: -0.3,
                    exclude: false,
                    ..Default::default()
                },
                ScopedEffect {
                    tag: "test".to_string(),
                    role: "aegis/crew/sentinel".to_string(),
                    boost: 0.3,
                    exclude: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        // sentinel gets the specific override (boost +0.3)
        let resolved = config.resolve_effect("test", Some("aegis/crew/sentinel"));
        assert_eq!(
            resolved,
            Some(ResolvedEffect {
                boost: 0.3,
                exclude: false,
                pin: false,
                budget_reserve: 0,
            })
        );

        // other aegis crew gets the broad match (boost -0.3)
        let resolved = config.resolve_effect("test", Some("aegis/crew/ellie"));
        assert_eq!(
            resolved,
            Some(ResolvedEffect {
                boost: -0.3,
                exclude: false,
                pin: false,
                budget_reserve: 0,
            })
        );
    }

    #[test]
    fn test_build_tag_include_filter_single() {
        let filter = build_tag_include_filter(&["user:canonical".to_string()]);
        assert!(filter.contains("tags = 'user:canonical'"));
        assert!(filter.contains("tags LIKE 'user:canonical,%'"));
        assert!(filter.contains("tags LIKE '%,user:canonical'"));
        assert!(filter.contains("tags LIKE '%,user:canonical,%'"));
    }

    #[test]
    fn test_build_tag_include_filter_multiple() {
        let filter = build_tag_include_filter(&[
            "user:canonical".to_string(),
            "user:architecture".to_string(),
        ]);
        // Multiple tags are OR'd
        assert!(filter.starts_with('('));
        assert!(filter.contains(" OR "));
    }

    #[test]
    fn test_build_tag_exclude_filter_single() {
        let filter = build_tag_exclude_filter(&["auto:test".to_string()]);
        assert!(filter.starts_with("NOT ("));
    }

    #[test]
    fn test_build_tag_exclude_filter_multiple() {
        let filter = build_tag_exclude_filter(&[
            "auto:test".to_string(),
            "user:deprecated".to_string(),
        ]);
        // Multiple excludes are AND'd
        assert!(filter.starts_with('('));
        assert!(filter.contains(" AND "));
    }

    #[test]
    fn test_build_effect_exclude_filter_global_only() {
        let config = TagsConfig {
            effects: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "noise".to_string(),
                    TagEffect {
                        boost: 0.0,
                        exclude: true,
                        ..Default::default()
                    },
                );
                m.insert(
                    "deprecated".to_string(),
                    TagEffect {
                        boost: -0.8,
                        exclude: false,
                        ..Default::default()
                    },
                );
                m
            },
            ..Default::default()
        };

        let filter = build_effect_exclude_filter(&config, None);
        assert!(filter.is_some());
        let f = filter.unwrap();
        assert!(f.contains("noise"));
        assert!(!f.contains("deprecated")); // not excluded, only demoted
    }

    #[test]
    fn test_build_effect_exclude_filter_scoped_adds_exclude() {
        let config = TagsConfig {
            effects_scoped: vec![ScopedEffect {
                tag: "internal".to_string(),
                role: "external/*".to_string(),
                boost: 0.0,
                exclude: true,
                ..Default::default()
            }],
            ..Default::default()
        };

        // External role: internal tag is excluded
        let filter = build_effect_exclude_filter(&config, Some("external/user1"));
        assert!(filter.is_some());
        assert!(filter.unwrap().contains("internal"));

        // Aegis role: no excludes
        let filter = build_effect_exclude_filter(&config, Some("aegis/crew/ellie"));
        assert!(filter.is_none());
    }

    #[test]
    fn test_build_effect_exclude_filter_scoped_overrides_global() {
        let config = TagsConfig {
            effects: {
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "internal".to_string(),
                    TagEffect {
                        boost: 0.0,
                        exclude: true, // globally excluded
                        ..Default::default()
                    },
                );
                m
            },
            effects_scoped: vec![ScopedEffect {
                tag: "internal".to_string(),
                role: "aegis/*".to_string(),
                boost: 0.0,
                exclude: false, // but NOT excluded for aegis
                ..Default::default()
            }],
            ..Default::default()
        };

        // Aegis role: scoped override un-excludes
        let filter = build_effect_exclude_filter(&config, Some("aegis/crew/ellie"));
        assert!(filter.is_none());

        // External role: global exclude applies
        let filter = build_effect_exclude_filter(&config, Some("external/user1"));
        assert!(filter.is_some());
        assert!(filter.unwrap().contains("internal"));
    }

    #[test]
    fn test_build_effect_exclude_filter_none_when_empty() {
        let config = TagsConfig::default();
        assert!(build_effect_exclude_filter(&config, None).is_none());
    }

    #[test]
    fn test_role_specificity() {
        assert_eq!(role_specificity("*"), 0);
        assert_eq!(role_specificity("aegis/*"), 1);
        assert_eq!(role_specificity("aegis/crew/*"), 2);
        assert_eq!(role_specificity("aegis/crew/sentinel"), 3);
    }

    #[test]
    fn test_toml_with_effects_scoped() {
        let toml_str = r#"
[effects.noise]
exclude = true

[[effects_scoped]]
tag = "canonical"
role = "aegis/*"
boost = 0.5

[[effects_scoped]]
tag = "internal"
role = "external/*"
exclude = true
"#;
        let config: TagsConfig = toml::from_str(toml_str).unwrap();
        assert!(config.effects["noise"].exclude);
        assert_eq!(config.effects_scoped.len(), 2);
        assert_eq!(config.effects_scoped[0].tag, "canonical");
        assert!((config.effects_scoped[0].boost - 0.5).abs() < f32::EPSILON);
        assert!(config.effects_scoped[1].exclude);
    }

    // ===== Phase 3: Content Extraction Tests =====

    #[test]
    fn test_frontmatter_tags_inline_list() {
        let content = "---\ntitle: Auth Architecture\ntags: [canonical, architecture, security]\n---\n\n# Auth\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert_eq!(tags, vec!["user:canonical", "user:architecture", "user:security"]);
    }

    #[test]
    fn test_frontmatter_tags_single_value() {
        let content = "---\ntags: canonical\n---\n\n# Doc\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert_eq!(tags, vec!["user:canonical"]);
    }

    #[test]
    fn test_frontmatter_tags_block_list() {
        let content = "---\ntags:\n  - canonical\n  - architecture\n---\n\n# Doc\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert_eq!(tags, vec!["user:canonical", "user:architecture"]);
    }

    #[test]
    fn test_frontmatter_tags_with_namespace() {
        let content = "---\ntags: [auto:test, security]\n---\n\n# Doc\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert_eq!(tags, vec!["auto:test", "user:security"]);
    }

    #[test]
    fn test_frontmatter_tags_quoted() {
        let content = "---\ntags: [\"canonical\", 'architecture']\n---\n\n# Doc\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert_eq!(tags, vec!["user:canonical", "user:architecture"]);
    }

    #[test]
    fn test_frontmatter_fallback_field() {
        let content = "---\nbobbin-tags: [security]\n---\n\n# Doc\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert_eq!(tags, vec!["user:security"]);
    }

    #[test]
    fn test_frontmatter_labels_fallback() {
        let content = "---\nlabels: [internal, deprecated]\n---\n\n# Doc\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert_eq!(tags, vec!["user:internal", "user:deprecated"]);
    }

    #[test]
    fn test_frontmatter_no_frontmatter() {
        let content = "# Just a heading\n\nSome content\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_frontmatter_no_tags_field() {
        let content = "---\ntitle: Something\nauthor: Someone\n---\n\n# Doc\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_frontmatter_custom_field() {
        let content = "---\ncategories: [security, ops]\n---\n\n# Doc\n";
        let config = FrontmatterConfig {
            enabled: true,
            field: "categories".to_string(),
        };
        let tags = extract_frontmatter_tags(content, &config);
        assert_eq!(tags, vec!["user:security", "user:ops"]);
    }

    #[test]
    fn test_frontmatter_invalid_tags_skipped() {
        let content = "---\ntags: [valid, INVALID, also-valid]\n---\n\n# Doc\n";
        let config = FrontmatterConfig::default();
        let tags = extract_frontmatter_tags(content, &config);
        // INVALID gets lowercased to "invalid" which is valid
        assert_eq!(tags, vec!["user:valid", "user:invalid", "user:also-valid"]);
    }

    #[test]
    fn test_comment_tags_rust() {
        let content = "use std::io;\n\n// bobbin:tag security critical\nfn authenticate() {\n}\n";
        let config = CommentsConfig::default();
        let tags = extract_comment_tags(content, &config);
        assert_eq!(
            tags.get(&3),
            Some(&vec!["user:security".to_string(), "user:critical".to_string()])
        );
        assert_eq!(tags.len(), 1);
    }

    #[test]
    fn test_comment_tags_python() {
        let content = "import os\n\n# bobbin:tag deprecated\ndef old_handler():\n    pass\n";
        let config = CommentsConfig::default();
        let tags = extract_comment_tags(content, &config);
        assert_eq!(
            tags.get(&3),
            Some(&vec!["user:deprecated".to_string()])
        );
    }

    #[test]
    fn test_comment_tags_block_comment() {
        let content = "/* bobbin:tag internal */\npub(crate) fn helper() {}\n";
        let config = CommentsConfig::default();
        let tags = extract_comment_tags(content, &config);
        assert_eq!(
            tags.get(&1),
            Some(&vec!["user:internal".to_string()])
        );
    }

    #[test]
    fn test_comment_tags_no_false_positive() {
        let content = "// bobbin:tagging system\n// just a comment\nfn foo() {}\n";
        let config = CommentsConfig::default();
        let tags = extract_comment_tags(content, &config);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_comment_tags_empty_directive() {
        let content = "// bobbin:tag\nfn foo() {}\n";
        let config = CommentsConfig::default();
        let tags = extract_comment_tags(content, &config);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_comment_tags_with_namespace() {
        let content = "// bobbin:tag auto:test user:critical\nfn test_auth() {}\n";
        let config = CommentsConfig::default();
        let tags = extract_comment_tags(content, &config);
        assert_eq!(
            tags.get(&1),
            Some(&vec!["auto:test".to_string(), "user:critical".to_string()])
        );
    }

    #[test]
    fn test_comment_tags_disabled() {
        let content = "// bobbin:tag security\nfn auth() {}\n";
        let config = CommentsConfig {
            enabled: false,
            prefix: "bobbin:tag".to_string(),
        };
        // When disabled, the caller should not call extract_comment_tags,
        // but the function itself still works — it's the caller's responsibility
        let tags = extract_comment_tags(content, &config);
        assert!(!tags.is_empty()); // function doesn't check enabled flag
    }

    #[test]
    fn test_normalize_content_tag() {
        assert_eq!(normalize_content_tag("canonical"), Some("user:canonical".to_string()));
        assert_eq!(normalize_content_tag("auto:test"), Some("auto:test".to_string()));
        assert_eq!(normalize_content_tag("SECURITY"), Some("user:security".to_string()));
        assert_eq!(normalize_content_tag(""), None);
        assert_eq!(normalize_content_tag("  "), None);
    }

    #[test]
    fn test_toml_with_frontmatter_and_comments() {
        let toml_str = r#"
[frontmatter]
enabled = true
field = "categories"

[comments]
enabled = true
prefix = "tag:"
"#;
        let config: TagsConfig = toml::from_str(toml_str).unwrap();
        assert!(config.frontmatter.enabled);
        assert_eq!(config.frontmatter.field, "categories");
        assert!(config.comments.enabled);
        assert_eq!(config.comments.prefix, "tag:");
    }

    #[test]
    fn test_toml_backward_compat_no_frontmatter_comments() {
        let toml_str = r#"
[[rules]]
pattern = "*.md"
tags = ["docs"]
"#;
        let config: TagsConfig = toml::from_str(toml_str).unwrap();
        // Defaults should apply
        assert!(config.frontmatter.enabled);
        assert_eq!(config.frontmatter.field, "tags");
        assert!(config.comments.enabled);
        assert_eq!(config.comments.prefix, "bobbin:tag");
    }

    #[test]
    fn test_auto_init_tag_go_init_function() {
        use crate::types::{Chunk, ChunkType};
        let config = TagsConfig::default();
        let mut chunks = vec![
            Chunk {
                id: "1".to_string(),
                file_path: "cmd/main.go".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("init".to_string()),
                start_line: 10,
                end_line: 20,
                content: "func init() { cobra.AddCommand(fooCmd) }".to_string(),
                language: "go".to_string(),
                tags: String::new(),
            },
            Chunk {
                id: "2".to_string(),
                file_path: "cmd/main.go".to_string(),
                chunk_type: ChunkType::Function,
                name: Some("main".to_string()),
                start_line: 25,
                end_line: 35,
                content: "func main() { Execute() }".to_string(),
                language: "go".to_string(),
                tags: String::new(),
            },
        ];
        resolve_tags_for_chunks(&config, "cmd/main.go", None, "", &mut chunks);
        assert!(chunks[0].tags.contains("auto:init"), "Go init() should get auto:init tag");
        assert!(!chunks[1].tags.contains("auto:init"), "Go main() should NOT get auto:init tag");
    }

    #[test]
    fn test_auto_init_tag_not_applied_to_rust() {
        use crate::types::{Chunk, ChunkType};
        let config = TagsConfig::default();
        let mut chunks = vec![Chunk {
            id: "1".to_string(),
            file_path: "src/lib.rs".to_string(),
            chunk_type: ChunkType::Function,
            name: Some("init".to_string()),
            start_line: 1,
            end_line: 5,
            content: "fn init() {}".to_string(),
            language: "rust".to_string(),
            tags: String::new(),
        }];
        resolve_tags_for_chunks(&config, "src/lib.rs", None, "", &mut chunks);
        assert!(!chunks[0].tags.contains("auto:init"), "Rust init() should NOT get auto:init tag");
    }

    #[test]
    fn test_resolve_pin_basic() {
        let config = TagsConfig {
            effects: {
                let mut m = std::collections::HashMap::new();
                m.insert("user:critical".to_string(), TagEffect {
                    pin: true,
                    budget_reserve: 50,
                    ..Default::default()
                });
                m.insert("user:boost-only".to_string(), TagEffect {
                    boost: 0.3,
                    ..Default::default()
                });
                m
            },
            ..Default::default()
        };

        // Pinned tag → returns budget_reserve
        assert_eq!(config.resolve_pin("user:critical", None), Some(50));

        // Non-pin tag → None
        assert_eq!(config.resolve_pin("user:boost-only", None), None);

        // Empty tags → None
        assert_eq!(config.resolve_pin("", None), None);

        // Unknown tag → None
        assert_eq!(config.resolve_pin("user:unknown", None), None);

        // Mixed tags with one pin → returns max reserve
        assert_eq!(config.resolve_pin("user:boost-only,user:critical", None), Some(50));
    }

    #[test]
    fn test_resolve_pin_scoped() {
        let config = TagsConfig {
            effects: {
                let mut m = std::collections::HashMap::new();
                m.insert("user:guardrails".to_string(), TagEffect {
                    pin: true,
                    budget_reserve: 30,
                    ..Default::default()
                });
                m
            },
            effects_scoped: vec![ScopedEffect {
                tag: "user:guardrails".to_string(),
                role: "external/*".to_string(),
                pin: false, // not pinned for external roles
                ..Default::default()
            }],
            ..Default::default()
        };

        // Aegis role: falls back to global → pinned
        assert_eq!(config.resolve_pin("user:guardrails", Some("aegis/crew/ellie")), Some(30));

        // External role: scoped override → not pinned
        assert_eq!(config.resolve_pin("user:guardrails", Some("external/user1")), None);
    }
}
