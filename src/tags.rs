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
    /// Tag effects on scoring (parsed now, applied in Phase 2)
    pub effects: std::collections::HashMap<String, TagEffect>,
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

/// Effect applied when a tag is present on a chunk (Phase 2)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagEffect {
    /// Score multiplier: positive = boost, negative = demote
    #[serde(default)]
    pub boost: f32,
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
            effects: Default::default(),
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
            effects: Default::default(),
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
                    TagEffect { boost: -0.8 },
                );
                m
            },
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: TagsConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.rules.len(), 1);
        assert_eq!(parsed.rules[0].pattern, "**/*_test.go");
        assert!((parsed.effects["deprecated"].boost - (-0.8)).abs() < f32::EPSILON);
    }
}
