use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
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
fn shows_version() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
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
                .and(predicate::str::contains("GITHUB_APP_PRIVATE_KEY_PATH"))
                .and(predicate::str::contains("--repo <OWNER/REPO>"))
                .and(predicate::str::contains("--repository <OWNER/REPO>"))
                .and(predicate::str::contains("only repository names are sent"))
                .and(predicate::str::contains("--installation-id").not())
                .and(predicate::str::contains("GITHUB_APP_INSTALLATION_ID").not()),
        );
}

#[cfg(unix)]
#[test]
fn symlink_style_help_does_not_duplicate_subcommand_name() {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("toolbox"));
    cmd.arg0("github-app-auth");

    let output = cmd.arg("--help").output().expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("Usage: github-app-auth [OPTIONS]"));
    assert!(!stdout.contains("github-app-auth github-app-auth [OPTIONS]"));
}

#[test]
fn github_app_auth_requires_private_key() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-auth",
        "--app-id",
        "1",
        "--repo",
        "OWNER/REPO",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("missing private key"));
}

#[test]
fn github_app_auth_requires_repo_for_token_exchange() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-auth",
        "--app-id",
        "1",
        "--private-key",
        TEST_RSA_PRIVATE_KEY,
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("--repo <OWNER/REPO>"));
}

#[test]
fn github_app_auth_jwt_only_does_not_require_installation_id() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-auth",
        "--jwt-only",
        "--app-id",
        "1",
        "--private-key",
        TEST_RSA_PRIVATE_KEY,
    ])
    .assert()
    .success()
    .stdout(predicate::str::starts_with("eyJ"));
}

#[test]
fn github_app_auth_jwt_only_can_print_json() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github-app-auth",
        "--jwt-only",
        "--format",
        "json",
        "--app-id",
        "1",
        "--private-key",
        TEST_RSA_PRIVATE_KEY,
    ])
    .assert()
    .success()
    .stdout(predicate::str::starts_with("{\"jwt\":\"eyJ").and(predicate::str::contains("\"}")));
}

#[test]
fn github_app_auth_shell_can_export_gh_token_too() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-auth",
        "--shell",
        "--export-gh-token",
        "--app-id",
        "1",
        "--repo",
        "OWNER/REPO",
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

const TEST_RSA_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC56nCrsvN8UqK+\n\
yKdhY9ecfqRnqzphLzOKtyxhWz28W0xLE2HbzRkSz6IoQbO71QVLrs2IpsHxtJnW\n\
05xBQiR8YAUI0w3K0lVIAbM/OewkqgiLAvZvX/iocT1URy2ixJs6f1eodkJ0Os0n\n\
CU0DZRX5vUe6ic90OIGyftgcJpYutpq1oSEdQLCHwjGIIitpRd6ztAdMXtUuZ2wZ\n\
R0o4UaCF/Ptyt+CpZW/jRQoBoDiXetnPc7GGu9JoAZU0tpt3sdqvnrNdafyd7FMO\n\
bKyxfb7uNHGqqy36dWGQcpBqsw6WZZJVTZPKPBwngXGg8hM6wGoHIfDCePvkhsCi\n\
5WqOP/qvAgMBAAECggEAFHjIxFdVsWRmEE0HBVXZqZ1WXCYCLS5l5gnqhKPn5eRF\n\
v+SX+3yXnLcpW3Z0pKO9zAopDrmSFJv27q1pgNQYMWvfUgvvclx70IyDYNxvcNAa\n\
VbhTS4tNVbr2bl/SGiC9GRFppR60jZjl+zzucofAhjn9+n/vTJRmT7Hg+SSUl/sK\n\
AshWb6F6TyHnw/gdqysq9qS+kSvmRywxEv21Vu8EgZGm/bys/Zu53XKrwOpw974p\n\
rsn/4b+vk6oiZJOB9nPbrBcch3duCdtkbVij9dYz1MVbPpke8uLb9iUF9d+q5wJu\n\
BcToL/8ErYmkPctlt19vl896S6oe4z9a5xIx7nwo4QKBgQD7OPHbKoJNyhq/Gml6\n\
sn+WIpEG7AKHRsJK3K0XyEs2l78ekCsYJlbWU/ymP8dZfr2DoQKeUk43/yoiLWAO\n\
c6eW9Pq2oJNRzhQdls45hdgjm6iKQpFRTdVoK7mK1ZFjnl8dr0e6NyEfdSV7IIP4\n\
oxRmhe2BVduBLIejxp2erXX4IQKBgQC9c44vjB6lsdDZXJLnwopBnuSzgbUiNzuM\n\
t532mu+tSw3vWm3lE43CHkvFM7HjrxxAK90EYq8l5k57LiaEbrMnMR1SnjnIOkUR\n\
vHjaocnES9FF9wOMCoOrz6SIHR4Xx6Tvj03YsPMgJrihJyLu/CpEjkZK86IW0/EP\n\
QjriOHFYzwKBgQCkE44KmVnfWndbhvGLHFeuA8d6oNwJ5BHzeOtoE/3jmvpNCNXM\n\
gQXIF7R0FEWr0tYNyTP/mTvS4Mlw5vfMmIbFVh0E+B0fmZuTs7He6ea/YuOR4WYt\n\
lsshrSUSYugBCyeOKLONEIKGnCktoI/w7Pne9+ulxCCH3kB8m7TINPxOYQKBgGeQ\n\
bd/MJ0zI4bSRCLWtAUtSAw+mDlDABMut7KpMlE0VRG7d7klV4R6G1UDeO5aNuVHT\n\
KKUnFTwQpEJuPhwTL9hy3ua1HD06rVs+voo1+0hVcfdfSw8ZCFW50uWdlT/GoYFb\n\
w2B7isy+nhtqe4xNSQXlCMQcXzU/cv22ZN4ZoMy9AoGBAJA6iJSdEz3wQkqPVLvQ\n\
YasrScJznIf2ZoWawMx2GDY1jrCEFLa2j+2jAFmXHJhCXnk63XUATNVDtHHB/T9A\n\
VnX0GJ36qCIZLrq+r2IYUHJNpRCbMDxBpPHeGybT/7c648FzahrdHfF2ygyKO4PW\n\
MzBXuiFERcpCt4YM/pVtnc99\n\
-----END PRIVATE KEY-----";

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
