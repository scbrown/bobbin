//! Shared remote credential discovery and per-process write refusal circuit.
use anyhow::{bail, Result};
use std::{
    collections::HashSet,
    path::Path,
    sync::{Mutex, OnceLock},
};

const FIX: &str = "checked QUIPU_AUTH_TOKEN > QUIPU_AUTH_TOKEN_FILE > ~/.config/quipu/token; install the issued token at ~/.config/quipu/token; run caboodle doctor";
static DISABLED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn key(endpoint: &str) -> String {
    match reqwest::Url::parse(endpoint) {
        Ok(url) => url.origin().ascii_serialization(),
        Err(_) => endpoint.trim_end_matches('/').to_owned(),
    }
}

pub(crate) fn check(endpoint: &str) -> Result<()> {
    let disabled = DISABLED
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| anyhow::anyhow!("Quipu auth circuit unavailable; writes skipped"))?;
    if disabled.contains(&key(endpoint)) {
        bail!("Quipu writes disabled for this process session; run caboodle doctor and restart the client after repair");
    }
    Ok(())
}

fn disable(endpoint: &str, why: &str) -> anyhow::Error {
    if let Ok(mut disabled) = DISABLED.get_or_init(Default::default).lock() {
        if disabled.insert(key(endpoint)) {
            eprintln!("Quipu credential {why}; {FIX}; writes disabled for this process session");
        }
    }
    anyhow::anyhow!("Quipu writes disabled: credential {why}; run caboodle doctor")
}

pub(crate) fn rejected(endpoint: &str) -> anyhow::Error {
    disable(endpoint, "rejected (HTTP 401)")
}

pub(crate) fn require(endpoint: &str) -> Result<String> {
    check(endpoint)?;
    match resolve(
        std::env::var("QUIPU_AUTH_TOKEN").ok().as_deref(),
        std::env::var_os("QUIPU_AUTH_TOKEN_FILE")
            .as_deref()
            .map(Path::new),
        directories::BaseDirs::new()
            .as_ref()
            .map(|dirs| dirs.home_dir()),
    ) {
        Ok(Some(token)) => Ok(token),
        Ok(None) => Err(disable(endpoint, "missing")),
        Err(_) => Err(disable(endpoint, "unreadable")),
    }
}

fn resolve(
    inline: Option<&str>,
    explicit: Option<&Path>,
    home: Option<&Path>,
) -> Result<Option<String>> {
    if let Some(value) = inline.filter(|s| !s.is_empty()) {
        return Ok(Some(value.to_owned()));
    }
    let default = home.map(|home| home.join(".config/quipu/token"));
    let Some(path) = explicit
        .filter(|p| !p.as_os_str().is_empty())
        .or(default.as_deref())
    else {
        return Ok(None);
    };
    match std::fs::read_to_string(path) {
        Ok(value) => Ok((!value.trim().is_empty()).then(|| value.trim().to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => bail!("Quipu credential file unreadable; {FIX}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_and_explicit_precedence_never_fall_back_to_legacy() {
        let home = tempfile::tempdir().unwrap();
        let canonical = home.path().join(".config/quipu/token");
        let legacy = home.path().join(".config/aegis/quipu_token");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(legacy, "legacy").unwrap();
        assert_eq!(resolve(None, None, Some(home.path())).unwrap(), None);
        std::fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        std::fs::write(canonical, "canonical\n").unwrap();
        assert_eq!(
            resolve(None, None, Some(home.path())).unwrap().as_deref(),
            Some("canonical")
        );
        let explicit = home.path().join("override");
        assert_eq!(
            resolve(None, Some(&explicit), Some(home.path())).unwrap(),
            None
        );
        std::fs::write(&explicit, "override\n").unwrap();
        assert_eq!(
            resolve(Some("inline"), Some(&explicit), Some(home.path()))
                .unwrap()
                .as_deref(),
            Some("inline")
        );
        assert_eq!(
            resolve(Some(""), Some(&explicit), Some(home.path()))
                .unwrap()
                .as_deref(),
            Some("override")
        );
        std::fs::write(&explicit, "\n").unwrap();
        assert_eq!(
            resolve(None, Some(&explicit), Some(home.path())).unwrap(),
            None
        );
        std::fs::write(&explicit, [0xff]).unwrap();
        assert!(resolve(None, Some(&explicit), Some(home.path())).is_err());
    }
    #[test]
    fn rejection_disables_other_paths_on_same_server_but_not_other_servers() {
        let endpoint = "http://auth-circuit.test:23456/knot/stage"; // gitleaks:allow synthetic URL, no credential
        check(endpoint).unwrap();
        assert!(rejected(endpoint).to_string().contains("rejected"));
        assert!(check("http://auth-circuit.test:23456/import").is_err());
        assert!(check("http://auth-circuit-control.test:23456/import").is_ok());
    }
}
