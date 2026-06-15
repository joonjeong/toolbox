use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn shows_top_level_help() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("github")
            .and(predicate::str::contains("github-app-auth"))
            .and(predicate::str::contains("github-app-run"))
            .and(predicate::str::contains("agent-skill"))
            .and(predicate::str::contains("toolbox github app-auth"))
            .and(predicate::str::contains("toolbox github app-run"))
            .and(predicate::str::contains("github-app-auth ...")),
    );
}

#[test]
fn shows_version() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");
    let assert = cmd.arg("--version").assert().success();

    if let Some(toolbox_version) = option_env!("TOOLBOX_VERSION") {
        assert.stdout(
            predicate::str::contains(env!("CARGO_PKG_VERSION"))
                .or(predicate::str::contains(toolbox_version)),
        );
    } else {
        assert.stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
    }
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
                    "debugging app-based authentication behavior",
                ))
                .and(predicate::str::contains("toolbox github app-run"))
                .and(predicate::str::contains("--format json"))
                .and(predicate::str::contains("GITHUB_APP_PRIVATE_KEY_FILE"))
                .and(predicate::str::contains("GITHUB_APP_PRIVATE_KEY_PATH"))
                .and(predicate::str::contains("--repo <OWNER/REPO>"))
                .and(predicate::str::contains(
                    "--installation-id <INSTALLATION_ID>",
                ))
                .and(predicate::str::contains("GITHUB_APP_INSTALLATION_ID"))
                .and(predicate::str::contains("Repeat for multiple repositories"))
                .and(predicate::str::contains("only repository names are sent"))
                .and(predicate::str::contains("--repository").not())
                .and(predicate::str::contains("--shell").not())
                .and(predicate::str::contains("--export-gh-token").not())
                .and(predicate::str::contains("--include-token").not())
                .and(predicate::str::contains("export GH_TOKEN").not()),
        );
}

#[test]
fn shows_github_app_run_agent_usage() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args(["github", "app-run", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Run a command with a GitHub App installation token")
                .and(predicate::str::contains("toolbox github app-run"))
                .and(predicate::str::contains("github-app-run"))
                .and(predicate::str::contains("GH_TOKEN"))
                .and(predicate::str::contains("GITHUB_TOKEN"))
                .and(predicate::str::contains("-- <COMMAND>"))
                .and(predicate::str::contains("--repo <OWNER/REPO>"))
                .and(predicate::str::contains("only repository names are sent")),
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

#[cfg(unix)]
#[test]
fn github_app_run_symlink_style_help_does_not_duplicate_subcommand_name() {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("toolbox"));
    cmd.arg0("github-app-run");

    let output = cmd.arg("--help").output().expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("Usage: github-app-run [OPTIONS]"));
    assert!(!stdout.contains("github-app-run github-app-run [OPTIONS]"));
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

#[cfg(unix)]
#[test]
fn github_app_run_runs_command_with_installation_token_environment() {
    let (api_url, server) = one_token_response_server();
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-run",
        "--api-url",
        &api_url,
        "--",
        "sh",
        "-c",
        "test \"$GH_TOKEN\" = test-token && \
         test \"$GITHUB_TOKEN\" = test-token && \
         test -z \"${GITHUB_APP_ID+x}\" && \
         test -z \"${GITHUB_APP_INSTALLATION_ID+x}\" && \
         test -z \"${GITHUB_APP_PRIVATE_KEY+x}\" && \
         test \"$1\" = --body && \
         test \"$2\" = Done",
        "child-command",
        "--body",
        "Done",
    ])
    .env("GITHUB_APP_ID", "1")
    .env("GITHUB_APP_INSTALLATION_ID", "42")
    .env("GITHUB_APP_PRIVATE_KEY", TEST_RSA_PRIVATE_KEY)
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
    assert!(request.contains("authorization: bearer "));
}

#[cfg(unix)]
#[test]
fn github_app_run_exits_with_child_exit_code() {
    let (api_url, server) = one_token_response_server();
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-run",
        "--app-id",
        "1",
        "--installation-id",
        "42",
        "--api-url",
        &api_url,
        "--private-key",
        TEST_RSA_PRIVATE_KEY,
        "--",
        "sh",
        "-c",
        "exit 42",
    ])
    .assert()
    .code(42);

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
}

#[cfg(unix)]
#[test]
fn github_app_run_reuses_valid_cached_installation_token() {
    let cache_dir = unique_temp_dir("toolbox-token-cache-test");
    let (api_url, server) = token_cache_response_server();

    let mut first = Command::cargo_bin("toolbox").expect("binary exists");
    first
        .args([
            "github",
            "app-run",
            "--app-id",
            "1",
            "--installation-id",
            "42",
            "--api-url",
            &api_url,
            "--private-key",
            TEST_RSA_PRIVATE_KEY,
            "--",
            "sh",
            "-c",
            "test \"$GH_TOKEN\" = cached-token",
        ])
        .env("XDG_CACHE_HOME", &cache_dir)
        .assert()
        .success();

    let mut second = Command::cargo_bin("toolbox").expect("binary exists");
    second
        .args([
            "github",
            "app-run",
            "--app-id",
            "1",
            "--installation-id",
            "42",
            "--api-url",
            &api_url,
            "--private-key",
            "not-a-key",
            "--",
            "sh",
            "-c",
            "test \"$GH_TOKEN\" = cached-token",
        ])
        .env("XDG_CACHE_HOME", &cache_dir)
        .assert()
        .success();

    let requests = server.join().expect("server thread completed");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("post /app/installations/42/access_tokens "));
    assert!(requests[1].starts_with("get /installation/repositories "));

    fs::remove_dir_all(cache_dir).expect("temporary cache directory removed");
}

#[cfg(unix)]
#[test]
fn github_app_run_mints_token_when_cache_directory_is_unavailable() {
    let (api_url, server) = one_token_response_server();
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-run",
        "--app-id",
        "1",
        "--installation-id",
        "42",
        "--api-url",
        &api_url,
        "--private-key",
        TEST_RSA_PRIVATE_KEY,
        "--",
        "sh",
        "-c",
        "test \"$GH_TOKEN\" = test-token",
    ])
    .env_remove("XDG_CACHE_HOME")
    .env_remove("HOME")
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
}

#[cfg(unix)]
#[test]
fn github_app_run_exits_with_child_signal_status() {
    let (api_url, server) = one_token_response_server();
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-run",
        "--app-id",
        "1",
        "--installation-id",
        "42",
        "--api-url",
        &api_url,
        "--private-key",
        TEST_RSA_PRIVATE_KEY,
        "--",
        "sh",
        "-c",
        "kill -TERM $$",
    ])
    .assert()
    .code(143);

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
}

#[test]
fn github_app_run_requires_command_after_separator() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-run",
        "--app-id",
        "1",
        "--repo",
        "OWNER/REPO",
        "--private-key",
        TEST_RSA_PRIVATE_KEY,
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("<COMMAND>"));
}

#[test]
fn github_app_run_accepts_command_options_after_separator() {
    let mut cmd = Command::cargo_bin("toolbox").expect("binary exists");

    cmd.args([
        "github",
        "app-run",
        "--app-id",
        "1",
        "--repo",
        "OWNER/REPO",
        "--private-key",
        "not-a-key",
        "--",
        "gh",
        "pr",
        "comment",
        "123",
        "--body",
        "Done",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "private key must be an RSA PEM key",
    ));
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
    assert!(skill.contains("toolbox github app-run"));
    assert!(skill.contains("without printing the token or exporting it"));
    assert!(skill.contains("primarily for debugging GitHub App authentication behavior"));

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

#[cfg(unix)]
fn one_token_response_server() -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server binds");
    let address = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test server accepts");
        let mut buffer = [0; 8192];
        let bytes = stream.read(&mut buffer).expect("test server reads request");
        let request = String::from_utf8_lossy(&buffer[..bytes]).to_ascii_lowercase();
        let body = r#"{"token":"test-token","expires_at":"2026-06-15T00:00:00Z","repository_selection":"selected","repositories":[],"permissions":{}}"#;
        let response = format!(
            "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("test server writes response");
        request
    });

    (format!("http://{address}"), handle)
}

#[cfg(unix)]
fn token_cache_response_server() -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server binds");
    let address = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for index in 0..2 {
            let (mut stream, _) = listener.accept().expect("test server accepts");
            let mut buffer = [0; 8192];
            let bytes = stream.read(&mut buffer).expect("test server reads request");
            requests.push(String::from_utf8_lossy(&buffer[..bytes]).to_ascii_lowercase());

            let body = if index == 0 {
                r#"{"token":"cached-token","expires_at":"2099-06-15T00:00:00Z","repository_selection":"selected","repositories":[],"permissions":{}}"#
            } else {
                r#"{"total_count":1,"repositories":[],"repository_selection":"selected"}"#
            };
            let status = if index == 0 { "201 Created" } else { "200 OK" };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("test server writes response");
        }
        requests
    });

    (format!("http://{address}"), handle)
}
