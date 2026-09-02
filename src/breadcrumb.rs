//! Durable, repository-local context breadcrumbs.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breadcrumb {
    pub name: String,
    pub description: String,
    pub query: String,
    #[serde(default)]
    pub pinned_files: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_recalled: Option<DateTime<Utc>>,
    #[serde(default)]
    pub recall_count: u64,
    #[serde(default)]
    pub ttl_days: u32,
}

pub type Breadcrumbs = BTreeMap<String, Breadcrumb>;

#[derive(Debug, Clone)]
pub struct BreadcrumbStore {
    path: PathBuf,
}

impl BreadcrumbStore {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            path: repo_root.join(".bobbin").join("breadcrumbs.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A missing store is an empty collection. Every other read or parse error
    /// is explicit so a mutation can never erase an unreadable store.
    pub fn load(&self) -> Result<Breadcrumbs> {
        if !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let bytes = fs::read(&self.path)
            .with_context(|| format!("Failed to read breadcrumb store: {}", self.path.display()))?;
        serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "Failed to parse breadcrumb store: {} (file left unchanged)",
                self.path.display()
            )
        })
    }

    pub fn save(&self, breadcrumbs: &Breadcrumbs) -> Result<()> {
        let parent = self
            .path
            .parent()
            .context("breadcrumb store path has no parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;

        let bytes = serde_json::to_vec_pretty(breadcrumbs)
            .context("Failed to serialize breadcrumb store")?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp_path = parent.join(format!(
            ".breadcrumbs.json.{}.{}.tmp",
            std::process::id(),
            nonce
        ));
        let result = (|| -> Result<()> {
            let mut temp = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)
                .with_context(|| {
                    format!("Failed to create temporary store: {}", temp_path.display())
                })?;
            temp.write_all(&bytes).with_context(|| {
                format!("Failed to write temporary store: {}", temp_path.display())
            })?;
            temp.write_all(b"\n")?;
            temp.sync_all().with_context(|| {
                format!("Failed to sync temporary store: {}", temp_path.display())
            })?;
            fs::rename(&temp_path, &self.path).with_context(|| {
                format!(
                    "Failed to replace breadcrumb store: {}",
                    self.path.display()
                )
            })?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }

    pub fn create(&self, breadcrumb: Breadcrumb) -> Result<()> {
        validate_name(&breadcrumb.name)?;
        if breadcrumb.query.trim().is_empty() {
            bail!("Breadcrumb query must not be empty");
        }
        if breadcrumb.description.trim().is_empty() {
            bail!("Breadcrumb description must not be empty");
        }
        let mut breadcrumbs = self.load()?;
        if breadcrumbs.contains_key(&breadcrumb.name) {
            bail!("Breadcrumb '{}' already exists", breadcrumb.name);
        }
        breadcrumbs.insert(breadcrumb.name.clone(), breadcrumb);
        self.save(&breadcrumbs)
    }

    pub fn recall(&self, name: &str, recalled_at: DateTime<Utc>) -> Result<Breadcrumb> {
        let mut breadcrumbs = self.load()?;
        let breadcrumb = breadcrumbs
            .get_mut(name)
            .with_context(|| format!("Breadcrumb '{}' not found", name))?;
        breadcrumb.recall_count = breadcrumb.recall_count.saturating_add(1);
        breadcrumb.last_recalled = Some(recalled_at);
        let recalled = breadcrumb.clone();
        self.save(&breadcrumbs)?;
        Ok(recalled)
    }

    pub fn delete(&self, name: &str) -> Result<Breadcrumb> {
        let mut breadcrumbs = self.load()?;
        let removed = breadcrumbs
            .remove(name)
            .with_context(|| format!("Breadcrumb '{}' not found", name))?;
        self.save(&breadcrumbs)?;
        Ok(removed)
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!(
            "Invalid breadcrumb name '{}': use lowercase letters, digits, and hyphens",
            name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> Breadcrumb {
        Breadcrumb {
            name: name.into(),
            description: "Where authentication refresh is implemented".into(),
            query: "token refresh flow".into(),
            pinned_files: vec!["src/auth.rs".into()],
            tags: vec!["auth".into()],
            keywords: vec!["refresh_token".into()],
            created_by: "test-agent".into(),
            created_at: Utc::now(),
            last_recalled: None,
            recall_count: 0,
            ttl_days: 0,
        }
    }

    #[test]
    fn missing_store_is_empty() {
        let temp = tempfile::tempdir().unwrap();
        assert!(BreadcrumbStore::new(temp.path()).load().unwrap().is_empty());
    }

    #[test]
    fn crud_round_trip_updates_recall_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let store = BreadcrumbStore::new(temp.path());
        store.create(sample("auth-refresh")).unwrap();
        let when = Utc::now();
        let recalled = store.recall("auth-refresh", when).unwrap();
        assert_eq!(recalled.recall_count, 1);
        assert_eq!(recalled.last_recalled, Some(when));
        assert_eq!(store.load().unwrap()["auth-refresh"], recalled);
        assert_eq!(store.delete("auth-refresh").unwrap(), recalled);
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn corrupt_store_is_reported_and_not_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let store = BreadcrumbStore::new(temp.path());
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), b"not json\n").unwrap();
        assert!(store.create(sample("auth-refresh")).is_err());
        assert_eq!(fs::read(store.path()).unwrap(), b"not json\n");
    }

    #[test]
    fn duplicate_and_invalid_names_are_explicit_errors() {
        let temp = tempfile::tempdir().unwrap();
        let store = BreadcrumbStore::new(temp.path());
        store.create(sample("auth-refresh")).unwrap();
        assert!(store.create(sample("auth-refresh")).is_err());
        assert!(store.create(sample("Auth Refresh")).is_err());
    }
}
