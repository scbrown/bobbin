//! Tests for `webhook`.
//!
//! Split out of `webhook.rs` so that file clears the 500-line error limit
//! (bobbin-aoz). `scripts/check-file-size.sh` exempts `*tests.rs` by
//! design, and the alternative — an allowlist entry — is the exit that
//! makes the ratchet meaningless.

use super::*;

#[test]
fn test_webhook_signature_accepts_only_the_exact_body() {
    // RFC 4231 test case 2: pins the HMAC construction independently of our
    // request handler and prevents a plain SHA-256(secret || body) substitute.
    let body = b"what do ya want for nothing?";
    let signature = "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
    assert!(valid_signature("Jefe", Some(signature), body));
    assert!(!valid_signature("Jefe", Some(signature), b"changed"));
}

#[test]
fn test_webhook_signature_fails_closed() {
    let body = br#"{"ref":"refs/heads/main"}"#;
    let valid = hex::encode(hmac_sha256(b"configured", body));
    assert!(!valid_signature("configured", None, body));
    assert!(!valid_signature("configured", Some("not-hex"), body));
    assert!(!valid_signature("", Some(&valid), body));
    assert!(valid_signature("configured", Some(&valid), body));
}

#[test]
fn test_web_base_from_remote_ssh() {
    assert_eq!(
        web_base_from_remote("git@github.com:owner/repo.git"),
        Some("https://github.com/owner/repo".to_string())
    );
}

#[test]
fn test_web_base_from_remote_https() {
    assert_eq!(
        web_base_from_remote("https://github.com/owner/repo.git"),
        Some("https://github.com/owner/repo".to_string())
    );
}

#[test]
fn test_web_base_from_remote_http() {
    assert_eq!(
        web_base_from_remote("http://git.example:3000/stiwi/bobbin"),
        Some("http://git.example:3000/stiwi/bobbin".to_string())
    );
}

#[test]
fn test_detect_forge_github() {
    assert_eq!(detect_forge("github.com"), ForgeType::GitHub);
}

#[test]
fn test_detect_forge_gitlab() {
    assert_eq!(detect_forge("gitlab.com"), ForgeType::GitLab);
    assert_eq!(detect_forge("gitlab.internal.corp"), ForgeType::GitLab);
}

#[test]
fn test_detect_forge_bitbucket() {
    assert_eq!(detect_forge("bitbucket.org"), ForgeType::Bitbucket);
}

#[test]
fn test_detect_forge_selfhosted_default() {
    // Unknown self-hosted → Forgejo
    assert_eq!(detect_forge("git.example"), ForgeType::Forgejo);
    assert_eq!(detect_forge("code.internal.com"), ForgeType::Forgejo);
}

#[test]
fn test_build_source_url_auto_github() {
    let overrides = std::collections::HashMap::new();
    let url = build_source_url("https://github.com/owner/repo", "", "repo", &overrides);
    assert_eq!(
        url,
        "https://github.com/owner/repo/blob/main/{path}#L{line}"
    );
}

#[test]
fn test_build_source_url_auto_forgejo() {
    let overrides = std::collections::HashMap::new();
    let url = build_source_url(
        "http://git.example:3000/stiwi/bobbin",
        "",
        "bobbin",
        &overrides,
    );
    assert_eq!(
        url,
        "http://git.example:3000/stiwi/bobbin/src/branch/main/{path}#L{line}"
    );
}

#[test]
fn test_build_source_url_with_template_override() {
    let overrides = std::collections::HashMap::new();
    let url = build_source_url(
        "https://github.com/owner/repo",
        "{remote_base}/tree/develop/{path}#L{line}",
        "repo",
        &overrides,
    );
    assert_eq!(
        url,
        "https://github.com/owner/repo/tree/develop/{path}#L{line}"
    );
}

#[test]
fn test_build_source_url_with_forge_override() {
    let mut overrides = std::collections::HashMap::new();
    overrides.insert("git.example".to_string(), "gitlab".to_string());
    let url = build_source_url(
        "http://git.example:3000/stiwi/bobbin",
        "",
        "bobbin",
        &overrides,
    );
    // git.example overridden to gitlab
    assert_eq!(
        url,
        "http://git.example:3000/stiwi/bobbin/-/blob/main/{path}#L{line}"
    );
}

#[test]
fn test_host_from_url() {
    assert_eq!(
        host_from_url("https://github.com/owner/repo"),
        Some("github.com")
    );
    assert_eq!(
        host_from_url("http://git.example:3000/stiwi/bobbin"),
        Some("git.example")
    );
    assert_eq!(host_from_url("not-a-url"), None);
}
