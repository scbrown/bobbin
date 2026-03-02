use anyhow::{bail, Context, Result};
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

use crate::types::{classify_file, FileCategory};

/// Maximum tag name length
const TAG_MAX_LEN: usize = 32;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagEffect {
    /// Score multiplier: positive = boost, negative = demote
    #[serde(default)]
    pub boost: f32,
    /// When true, chunks with this tag are excluded from results entirely
    #[serde(default)]
    pub exclude: bool,
}

/// Role-scoped tag effect from `[[effects_scoped]]` in tags.toml.
/// Overrides the global effect for matching roles.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// The resolved effect for a specific tag + role combination.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEffect {
    pub boost: f32,
    pub exclude: bool,
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
            Self::load(path).unwrap_or_default()
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
                });
            }
        }
        // Fall back to global effect
        self.effects.get(tag).map(|e| ResolvedEffect {
            boost: e.boost,
            exclude: e.exclude,
        })
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
        if let Ok(pat) = Pattern::new(&rule.pattern) {
            if pat.matches(file_path) {
                tags.extend(rule.tags.iter().cloned());
            }
        }
    }

    // BTreeSet is already sorted
    let tag_vec: Vec<String> = tags.into_iter().collect();
    tag_vec.join(",")
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
                    },
                );
                m.insert(
                    "noise".to_string(),
                    TagEffect {
                        boost: 0.0,
                        exclude: true,
                    },
                );
                m
            },
            effects_scoped: vec![ScopedEffect {
                tag: "internal".to_string(),
                role: "external/*".to_string(),
                boost: 0.0,
                exclude: true,
            }],
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
                exclude: false
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
                    },
                );
                m
            },
            effects_scoped: vec![ScopedEffect {
                tag: "internal".to_string(),
                role: "external/*".to_string(),
                boost: 0.0,
                exclude: true,
            }],
            ..Default::default()
        };

        // External role: scoped effect wins (exclude)
        let resolved = config.resolve_effect("internal", Some("external/user1"));
        assert_eq!(
            resolved,
            Some(ResolvedEffect {
                boost: 0.0,
                exclude: true
            })
        );

        // Aegis role: falls back to global (no exclude)
        let resolved = config.resolve_effect("internal", Some("aegis/crew/ellie"));
        assert_eq!(
            resolved,
            Some(ResolvedEffect {
                boost: 0.0,
                exclude: false
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
                },
                ScopedEffect {
                    tag: "test".to_string(),
                    role: "aegis/crew/sentinel".to_string(),
                    boost: 0.3,
                    exclude: false,
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
                exclude: false
            })
        );

        // other aegis crew gets the broad match (boost -0.3)
        let resolved = config.resolve_effect("test", Some("aegis/crew/ellie"));
        assert_eq!(
            resolved,
            Some(ResolvedEffect {
                boost: -0.3,
                exclude: false
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
                    },
                );
                m.insert(
                    "deprecated".to_string(),
                    TagEffect {
                        boost: -0.8,
                        exclude: false,
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
                    },
                );
                m
            },
            effects_scoped: vec![ScopedEffect {
                tag: "internal".to_string(),
                role: "aegis/*".to_string(),
                boost: 0.0,
                exclude: false, // but NOT excluded for aegis
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
}
