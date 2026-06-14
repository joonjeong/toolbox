use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn shows_top_level_help() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("github")
            .and(predicate::str::contains("github-app-auth"))
            .and(predicate::str::contains("toolbox github app-auth"))
            .and(predicate::str::contains("github-app-auth ...")),
    );
}

#[test]
fn shows_github_app_auth_agent_usage() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args(["github", "app-auth", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Sign a GitHub App JWT")
                .and(predicate::str::contains(
                    "eval \"$(toolbox github app-auth --shell",
                ))
                .and(predicate::str::contains("GITHUB_APP_PRIVATE_KEY_FILE"))
                .and(predicate::str::contains("--repository <OWNER/REPO>"))
                .and(predicate::str::contains("only the repository name is sent")),
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
