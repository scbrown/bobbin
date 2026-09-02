use assert_cmd::Command;
use predicates::prelude::*;

fn bobbin() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("bobbin"))
}

#[test]
fn breadcrumb_cli_crud_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_str().unwrap();

    bobbin()
        .args([
            "bc",
            "create",
            "auth-refresh",
            "token refresh flow",
            "Authentication refresh context",
            "--pin",
            "src/auth.rs,src/token.rs",
            "--tag",
            "auth,security",
            "--on",
            "refresh_token,token_expiry",
            "--path",
            root,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Created breadcrumb 'auth-refresh'",
        ));

    bobbin()
        .args(["--json", "bc", "list", "--path", root])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\": \"auth-refresh\""));

    bobbin()
        .args(["bc", "recall", "auth-refresh", "--path", root])
        .assert()
        .success()
        .stdout(predicate::str::contains("Query: token refresh flow"))
        .stdout(predicate::str::contains("src/auth.rs"));

    bobbin()
        .args(["bc", "delete", "auth-refresh", "--path", root])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Deleted breadcrumb 'auth-refresh'",
        ));

    bobbin()
        .args(["bc", "recall", "auth-refresh", "--path", root])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Breadcrumb 'auth-refresh' not found",
        ));
}

#[test]
fn aliases_are_visible_and_share_handlers() {
    bobbin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("mark"))
        .stdout(predicate::str::contains("recall"));

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_str().unwrap();
    bobbin()
        .args([
            "mark",
            "auth-refresh",
            "token refresh flow",
            "Authentication refresh context",
            "--path",
            root,
        ])
        .assert()
        .success();
    bobbin()
        .args(["recall", "auth-refresh", "--path", root])
        .assert()
        .success()
        .stdout(predicate::str::contains("token refresh flow"));
}

#[test]
fn corrupt_store_fails_without_erasing_it() {
    let temp = tempfile::tempdir().unwrap();
    let store_dir = temp.path().join(".bobbin");
    std::fs::create_dir_all(&store_dir).unwrap();
    let store = store_dir.join("breadcrumbs.json");
    std::fs::write(&store, "{broken").unwrap();

    bobbin()
        .args([
            "mark",
            "auth-refresh",
            "token refresh flow",
            "Authentication refresh context",
            "--path",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("file left unchanged"));
    assert_eq!(std::fs::read_to_string(store).unwrap(), "{broken");
}
