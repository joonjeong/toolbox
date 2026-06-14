use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn shows_top_level_help() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("github").and(predicate::str::contains("github-app-auth")),
    );
}

#[test]
fn github_app_auth_requires_private_key() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-auth",
        "--app-id",
        "1",
        "--installation-id",
        "2",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("missing private key"));
}
