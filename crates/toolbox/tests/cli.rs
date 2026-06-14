use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn shows_top_level_help() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("github")
            .and(predicate::str::contains("github-app-auth"))
            .and(predicate::str::contains("agent-skill"))
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

#[test]
fn creates_github_app_agent_workflow_skill() {
    let skills_dir = unique_temp_dir("toolbox-skill-test");
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "agent-skill",
        "--install-path",
        skills_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("github-app-agent-workflow"));

    let skill_file = skills_dir
        .join("github-app-agent-workflow")
        .join("SKILL.md");
    let skill = fs::read_to_string(&skill_file).expect("skill file exists");
    assert!(skill.contains("name: github-app-agent-workflow"));
    assert!(skill.contains("toolbox github app-auth"));

    fs::remove_dir_all(skills_dir).expect("temporary skill directory removed");
}

#[test]
fn refuses_to_overwrite_existing_skill_without_force() {
    let skills_dir = unique_temp_dir("toolbox-skill-test");

    let mut create = Command::cargo_bin("toolbox").expect("binary exists");
    create
        .args([
            "github-agent-skill",
            "-i",
            skills_dir.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success();

    assert!(skills_dir
        .join("github-app-agent-workflow")
        .join("SKILL.md")
        .exists());

    let mut overwrite = Command::cargo_bin("toolbox").expect("binary exists");
    overwrite
        .args([
            "github-agent-skill",
            "-i",
            skills_dir.to_str().expect("utf-8 path"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    fs::remove_dir_all(skills_dir).expect("temporary skill directory removed");
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{unique}-{counter}",
        std::process::id()
    ))
}
