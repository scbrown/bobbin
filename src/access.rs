//! Role-based repository access filtering (§69).
//!
//! Provides [`RepoFilter`] which determines which repos a caller can see
//! based on their role and the `[access]` config section.

use std::collections::HashSet;

use crate::config::{AccessConfig, RoleConfig};

/// Determines which repositories are visible to a given role.
#[derive(Debug, Clone)]
pub struct RepoFilter {
    /// Repos explicitly allowed (None = allow all per default_allow)
    allowed: Option<HashSet<String>>,
    /// Repos explicitly denied (always takes precedence)
    denied: HashSet<String>,
    /// Whether unlisted repos are visible
    default_allow: bool,
}

impl RepoFilter {
    /// Build a filter from config and the caller's role string.
    ///
    /// If no `[access]` section is configured (empty roles list), returns
    /// a permissive filter that allows everything.
    pub fn from_config(config: &AccessConfig, role: &str) -> Self {
        if config.roles.is_empty() {
            return Self::allow_all();
        }

        // Find matching role config (most specific wins)
        let matched = Self::find_matching_role(&config.roles, role);

        match matched {
            Some(role_config) => {
                let allowed = if role_config.allow.is_empty() {
                    None // No allow list = use default_allow
                } else {
                    Some(role_config.allow.iter().cloned().collect())
                };
                let denied = role_config.deny.iter().cloned().collect();
                Self {
                    allowed,
                    denied,
                    default_allow: config.default_allow,
                }
            }
            None => {
                // No matching role — use default_allow with no explicit grants
                Self {
                    allowed: None,
                    denied: HashSet::new(),
                    default_allow: config.default_allow,
                }
            }
        }
    }

    /// Create a filter that allows everything (for backward compat / no config).
    pub fn allow_all() -> Self {
        Self {
            allowed: None,
            denied: HashSet::new(),
            default_allow: true,
        }
    }

    /// Check if a repository name is visible under this filter.
    pub fn is_allowed(&self, repo_name: &str) -> bool {
        // Deny always wins
        if self.matches_any(&self.denied, repo_name) {
            return false;
        }

        // If there's an explicit allow list, repo must match it
        if let Some(ref allowed) = self.allowed {
            return self.matches_any(allowed, repo_name);
        }

        // No explicit allow list — use default_allow
        self.default_allow
    }

    /// Filter a list of results, keeping only those from allowed repos.
    /// The `repo_extractor` function extracts the repo name from each item.
    pub fn filter_vec<T, F>(&self, items: Vec<T>, repo_extractor: F) -> Vec<T>
    where
        F: Fn(&T) -> &str,
    {
        items
            .into_iter()
            .filter(|item| self.is_allowed(repo_extractor(item)))
            .collect()
    }

    /// Extract repo name from a file path like `/var/lib/bobbin/repos/aegis/src/main.rs`.
    /// Returns the segment after "repos/" or the first path component.
    pub fn repo_from_path(path: &str) -> &str {
        // Look for "repos/<name>/" pattern
        if let Some(idx) = path.find("/repos/") {
            let after = &path[idx + 7..]; // skip "/repos/"
            if let Some(slash) = after.find('/') {
                return &after[..slash];
            }
            return after;
        }
        // Fallback: first path component (for relative paths like "aegis/src/main.rs")
        path.split('/').next().unwrap_or(path)
    }

    /// Resolve the effective role from explicit value, env vars, or default.
    ///
    /// Priority: explicit > BOBBIN_ROLE > GT_ROLE > BD_ACTOR > "default"
    pub fn resolve_role(explicit: Option<&str>) -> String {
        if let Some(role) = explicit {
            if !role.is_empty() {
                return role.to_string();
            }
        }
        if let Ok(role) = std::env::var("BOBBIN_ROLE") {
            if !role.is_empty() {
                return role;
            }
        }
        if let Ok(role) = std::env::var("GT_ROLE") {
            if !role.is_empty() {
                return role;
            }
        }
        if let Ok(role) = std::env::var("BD_ACTOR") {
            if !role.is_empty() {
                return role;
            }
        }
        "default".to_string()
    }

    /// Find the most specific matching role config for a given role string.
    ///
    /// Matching priority:
    /// 1. Exact match (e.g. "aegis/crew/ian" matches "aegis/crew/ian")
    /// 2. Wildcard match, most specific first (e.g. "aegis/crew/*" before "aegis/*")
    /// 3. "default" role as fallback
    fn find_matching_role<'a>(roles: &'a [RoleConfig], role: &str) -> Option<&'a RoleConfig> {
        // 1. Exact match
        if let Some(r) = roles.iter().find(|r| r.name == role) {
            return Some(r);
        }

        // 2. Wildcard match — find all that match, pick most specific (longest prefix)
        let mut best_match: Option<&RoleConfig> = None;
        let mut best_len = 0;
        for r in roles {
            if let Some(prefix) = r.name.strip_suffix("/*") {
                if role.starts_with(prefix) && (role.len() == prefix.len() || role.as_bytes().get(prefix.len()) == Some(&b'/')) {
                    if prefix.len() > best_len {
                        best_len = prefix.len();
                        best_match = Some(r);
                    }
                }
            }
        }
        if best_match.is_some() {
            return best_match;
        }

        // 3. "default" fallback
        roles.iter().find(|r| r.name == "default")
    }

    /// Check if a repo name matches any pattern in the set.
    /// Supports "*" (match all) and literal names.
    fn matches_any(&self, patterns: &HashSet<String>, repo_name: &str) -> bool {
        if patterns.contains("*") {
            return true;
        }
        patterns.contains(repo_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RoleConfig;

    fn make_config(default_allow: bool, roles: Vec<RoleConfig>) -> AccessConfig {
        AccessConfig {
            default_allow,
            roles,
        }
    }

    fn role(name: &str, allow: &[&str], deny: &[&str]) -> RoleConfig {
        RoleConfig {
            name: name.to_string(),
            allow: allow.iter().map(|s| s.to_string()).collect(),
            deny: deny.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn test_no_config_allows_everything() {
        let config = make_config(true, vec![]);
        let filter = RepoFilter::from_config(&config, "anything");
        assert!(filter.is_allowed("aegis"));
        assert!(filter.is_allowed("personal-planning"));
        assert!(filter.is_allowed("cv"));
    }

    #[test]
    fn test_default_role_deny() {
        let config = make_config(true, vec![
            role("default", &[], &["personal-planning", "cv", "resume"]),
        ]);
        let filter = RepoFilter::from_config(&config, "unknown-agent");
        assert!(filter.is_allowed("aegis"));
        assert!(filter.is_allowed("bobbin"));
        assert!(!filter.is_allowed("personal-planning"));
        assert!(!filter.is_allowed("cv"));
        assert!(!filter.is_allowed("resume"));
    }

    #[test]
    fn test_human_sees_everything() {
        let config = make_config(true, vec![
            role("default", &[], &["personal-planning", "cv"]),
            role("human", &["*"], &[]),
        ]);
        let filter = RepoFilter::from_config(&config, "human");
        assert!(filter.is_allowed("personal-planning"));
        assert!(filter.is_allowed("cv"));
        assert!(filter.is_allowed("aegis"));
    }

    #[test]
    fn test_deny_overrides_allow() {
        let config = make_config(true, vec![
            role("test", &["*"], &["secret-repo"]),
        ]);
        let filter = RepoFilter::from_config(&config, "test");
        assert!(filter.is_allowed("aegis"));
        assert!(!filter.is_allowed("secret-repo"));
    }

    #[test]
    fn test_wildcard_role_matching() {
        let config = make_config(false, vec![
            role("aegis/*", &["aegis", "bobbin", "gastown"], &[]),
        ]);
        let filter = RepoFilter::from_config(&config, "aegis/crew/ian");
        assert!(filter.is_allowed("aegis"));
        assert!(filter.is_allowed("bobbin"));
        assert!(!filter.is_allowed("personal-planning"));
    }

    #[test]
    fn test_most_specific_wildcard_wins() {
        let config = make_config(false, vec![
            role("aegis/*", &["aegis", "bobbin"], &[]),
            role("aegis/crew/*", &["aegis", "bobbin", "gastown", "homelab-mcp"], &[]),
        ]);
        // aegis/crew/ian should match aegis/crew/* (more specific) over aegis/*
        let filter = RepoFilter::from_config(&config, "aegis/crew/ian");
        assert!(filter.is_allowed("homelab-mcp"));

        // aegis/polecats/alpha should match aegis/* (less specific)
        let filter2 = RepoFilter::from_config(&config, "aegis/polecats/alpha");
        assert!(!filter2.is_allowed("homelab-mcp"));
        assert!(filter2.is_allowed("bobbin"));
    }

    #[test]
    fn test_exact_match_over_wildcard() {
        let config = make_config(false, vec![
            role("aegis/*", &["aegis"], &[]),
            role("aegis/crew/ian", &["aegis", "bobbin", "personal-planning"], &[]),
        ]);
        let filter = RepoFilter::from_config(&config, "aegis/crew/ian");
        assert!(filter.is_allowed("personal-planning"));
    }

    #[test]
    fn test_default_allow_false_no_match() {
        let config = make_config(false, vec![
            role("human", &["*"], &[]),
        ]);
        // Unknown role, no default role defined, default_allow=false
        let filter = RepoFilter::from_config(&config, "random-agent");
        assert!(!filter.is_allowed("aegis"));
    }

    #[test]
    fn test_default_allow_true_no_match() {
        let config = make_config(true, vec![
            role("human", &["*"], &[]),
        ]);
        // Unknown role, no default role defined, default_allow=true
        let filter = RepoFilter::from_config(&config, "random-agent");
        assert!(filter.is_allowed("aegis"));
    }

    #[test]
    fn test_filter_vec() {
        let config = make_config(true, vec![
            role("default", &[], &["secret"]),
        ]);
        let filter = RepoFilter::from_config(&config, "default");

        let items = vec!["aegis/main.rs", "secret/passwords.txt", "bobbin/lib.rs"];
        let filtered = filter.filter_vec(items, |item| item.split('/').next().unwrap());
        assert_eq!(filtered, vec!["aegis/main.rs", "bobbin/lib.rs"]);
    }

    #[test]
    fn test_repo_from_path() {
        assert_eq!(
            RepoFilter::repo_from_path("/var/lib/bobbin/repos/aegis/src/main.rs"),
            "aegis"
        );
        assert_eq!(
            RepoFilter::repo_from_path("/var/lib/bobbin/repos/homelab-mcp/tools.go"),
            "homelab-mcp"
        );
        assert_eq!(RepoFilter::repo_from_path("bobbin/src/lib.rs"), "bobbin");
    }

    #[test]
    fn test_resolve_role_explicit() {
        assert_eq!(RepoFilter::resolve_role(Some("human")), "human");
    }

    #[test]
    fn test_resolve_role_default() {
        // Clear env vars to test default
        std::env::remove_var("BOBBIN_ROLE");
        std::env::remove_var("GT_ROLE");
        std::env::remove_var("BD_ACTOR");
        assert_eq!(RepoFilter::resolve_role(None), "default");
    }

    #[test]
    fn test_config_toml_roundtrip() {
        let config = AccessConfig {
            default_allow: true,
            roles: vec![
                RoleConfig {
                    name: "human".to_string(),
                    allow: vec!["*".to_string()],
                    deny: vec![],
                },
                RoleConfig {
                    name: "default".to_string(),
                    allow: vec![],
                    deny: vec!["cv".to_string(), "resume".to_string()],
                },
            ],
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: AccessConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.default_allow, true);
        assert_eq!(deserialized.roles.len(), 2);
        assert_eq!(deserialized.roles[0].name, "human");
        assert_eq!(deserialized.roles[1].deny, vec!["cv", "resume"]);
    }

    #[test]
    fn test_backward_compat_no_access_section() {
        // Config TOML without [access] section should deserialize with defaults
        let toml_str = r#"
[embedding]
model = "all-MiniLM-L6-v2"
"#;
        let config: crate::config::Config = toml::from_str(toml_str).unwrap();
        assert!(config.access.default_allow);
        assert!(config.access.roles.is_empty());
        let filter = RepoFilter::from_config(&config.access, "anything");
        assert!(filter.is_allowed("any-repo"));
    }
}
