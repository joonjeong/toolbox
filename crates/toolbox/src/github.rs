use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Args, ValueEnum};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const APP_AGENT_WORKFLOW_SKILL_NAME: &str = "github-app-agent-workflow";
const APP_AGENT_WORKFLOW_SKILL: &str =
    include_str!("../resources/github-app-agent-workflow/SKILL.md");

#[derive(Debug, Args)]
#[command(
    about = "Authenticate as a GitHub App installation",
    long_about = "Sign a GitHub App JWT, exchange it for an installation access token, and print the token to stdout.

Use this command from coding agents or automation that need temporary GitHub repository access through a GitHub App installation. Provide the app ID and exactly one private key source: --private-key-file or --private-key. Provide --installation-id, or pass --repo OWNER/REPO to discover the installation from the repository. Values can also come from the documented environment variables.",
    after_long_help = "Purpose:
  Sign a GitHub App JWT, exchange it for an installation access token, and print the token to stdout. Use this from coding agents or automation that need temporary GitHub repository access through a GitHub App installation.

Invocation forms:
  toolbox github app-auth [OPTIONS]
  toolbox github-app-auth [OPTIONS]
  github-app-auth [OPTIONS]    when symlinked to the toolbox binary

Examples:
  toolbox github app-auth \\
    --app-id \"$GITHUB_APP_ID\" \\
    --repo OWNER/REPO \\
    --private-key-file /path/to/private-key.pem

  eval \"$(toolbox github app-auth --shell \\
    --repo OWNER/REPO \\
    --app-id \"$GITHUB_APP_ID\" \\
    --export-gh-token \\
    --private-key-file /path/to/private-key.pem)\"

  toolbox github-app-auth --jwt-only \\
    --app-id \"$GITHUB_APP_ID\" \\
    --private-key-file /path/to/private-key.pem

Environment:
  GITHUB_APP_ID
  GITHUB_APP_INSTALLATION_ID
  GITHUB_APP_PRIVATE_KEY_FILE
  GITHUB_APP_PRIVATE_KEY_PATH
  GITHUB_APP_PRIVATE_KEY
  GITHUB_API_URL

Output:
  By default, prints only the installation token. With --format json, prints structured JSON. With --shell, prints a POSIX shell export statement for GITHUB_TOKEN. Add --export-gh-token to --shell to export GH_TOKEN too. With --jwt-only, prints the signed GitHub App JWT and does not call the installation token API.

Repository scoping:
  Use --repo OWNER/REPO to discover the installation ID and scope the token to that repository. Repeat --repository to limit the token to additional repositories. OWNER/REPO is accepted for user-facing clarity; only repository names are sent to GitHub's installation token API."
)]
pub struct AppAuthArgs {
    /// GitHub App ID.
    ///
    /// Can also be set with GITHUB_APP_ID.
    #[arg(long, env = "GITHUB_APP_ID")]
    app_id: u64,

    /// GitHub App installation ID.
    ///
    /// Can also be set with GITHUB_APP_INSTALLATION_ID. Not required with
    /// --jwt-only. If omitted for token exchange, pass --repo OWNER/REPO so
    /// the installation can be discovered.
    #[arg(long, env = "GITHUB_APP_INSTALLATION_ID")]
    installation_id: Option<u64>,

    /// Path to the GitHub App private key PEM file.
    ///
    /// Use this or --private-key, not both. Can also be set with
    /// GITHUB_APP_PRIVATE_KEY_FILE or GITHUB_APP_PRIVATE_KEY_PATH.
    #[arg(
        long,
        env = "GITHUB_APP_PRIVATE_KEY_FILE",
        conflicts_with = "private_key"
    )]
    private_key_file: Option<PathBuf>,

    /// Path to the GitHub App private key PEM file.
    ///
    /// Compatibility alias for Ciel/Hermes style environments. Use this or
    /// --private-key-file, not both. Can also be set with
    /// GITHUB_APP_PRIVATE_KEY_PATH.
    #[arg(
        long = "private-key-path",
        env = "GITHUB_APP_PRIVATE_KEY_PATH",
        conflicts_with_all = ["private_key_file", "private_key"]
    )]
    private_key_path: Option<PathBuf>,

    /// GitHub App private key PEM content.
    ///
    /// Use this or --private-key-file, not both. Can also be set with
    /// GITHUB_APP_PRIVATE_KEY. Prefer --private-key-file in shell history.
    #[arg(
        long,
        env = "GITHUB_APP_PRIVATE_KEY",
        allow_hyphen_values = true,
        conflicts_with = "private_key_file"
    )]
    private_key: Option<String>,

    /// GitHub API base URL.
    ///
    /// Override for GitHub Enterprise Server. Can also be set with
    /// GITHUB_API_URL.
    #[arg(long, env = "GITHUB_API_URL", default_value = "https://api.github.com")]
    api_url: String,

    /// Limit the installation token to a repository.
    ///
    /// Repeat for multiple repositories. OWNER/REPO is accepted for user-facing
    /// clarity, but only REPO is sent to GitHub's installation token API.
    #[arg(long = "repository", value_name = "OWNER/REPO")]
    repositories: Vec<String>,

    /// Repository used to discover the installation ID and scope the token.
    ///
    /// Pass OWNER/REPO. If --installation-id is omitted, this calls GitHub's
    /// repository installation API with the app JWT before creating the token.
    #[arg(long = "repo", value_name = "OWNER/REPO")]
    repo: Option<String>,

    /// Limit installation token permissions.
    ///
    /// Repeat as key=value, for example --permission contents=read. Values are
    /// sent unchanged to GitHub's installation token API.
    #[arg(long = "permission", value_name = "KEY=VALUE")]
    permissions: Vec<PermissionArg>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, conflicts_with = "shell")]
    format: OutputFormat,

    /// Emit `export GITHUB_TOKEN=...` instead of the raw token.
    ///
    /// Intended for `eval "$(toolbox github app-auth --shell ...)"`.
    #[arg(long)]
    shell: bool,

    /// With --shell, export GH_TOKEN in addition to GITHUB_TOKEN.
    #[arg(long, requires = "shell")]
    export_gh_token: bool,

    /// Print the signed GitHub App JWT and skip token exchange.
    ///
    /// Useful for debugging app authentication. The JWT is intentionally short
    /// lived and remains below GitHub's 10-minute maximum.
    #[arg(long, conflicts_with_all = ["shell", "repositories", "repo", "permissions"])]
    jwt_only: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Args)]
#[command(
    about = "Create the GitHub App agent workflow skill",
    long_about = "Create the bundled github-app-agent-workflow skill under a target skills directory.

The command writes INSTALL_PATH/github-app-agent-workflow/SKILL.md. Use it to install the agent-facing workflow guidance next to Codex, Hermes, or another agent's skill directory without copying files manually.",
    after_long_help = "Examples:
  toolbox github agent-skill --install-path ~/.codex/skills
  toolbox github-agent-skill -i ./skills --force

Output:
  Prints the created skill directory path."
)]
pub struct AppAgentWorkflowSkillArgs {
    /// Directory where the skill folder should be created.
    ///
    /// The command creates <INSTALL_PATH>/github-app-agent-workflow/SKILL.md.
    #[arg(long, short = 'i', value_name = "INSTALL_PATH")]
    install_path: PathBuf,

    /// Overwrite an existing SKILL.md.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Serialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

#[derive(Debug, Serialize)]
struct TokenRequest {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    repositories: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    permissions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstallationResponse {
    id: u64,
}

#[derive(Debug, Serialize)]
struct JsonTokenOutput<'a> {
    token: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct JsonJwtOutput<'a> {
    jwt: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PermissionArg {
    key: String,
    value: String,
}

pub fn app_auth(args: AppAuthArgs) -> Result<()> {
    let jwt = create_jwt(args.app_id, &read_private_key(&args)?)?;

    if args.jwt_only {
        print_jwt(&args, &jwt)?;
        return Ok(());
    }

    let client = github_client(&jwt)?;
    let installation_id = resolve_installation_id(&args, &client)?;
    let response = create_installation_token(&args, &client, installation_id)?;
    if args.shell {
        println!("{}", shell_exports(&response.token, args.export_gh_token));
    } else {
        print_token(&args, &response)?;
    }

    Ok(())
}

pub fn create_app_agent_workflow_skill(args: AppAgentWorkflowSkillArgs) -> Result<()> {
    let skill_dir = args.install_path.join(APP_AGENT_WORKFLOW_SKILL_NAME);
    let skill_file = skill_dir.join("SKILL.md");

    if skill_file.exists() && !args.force {
        return Err(anyhow!(
            "{} already exists; pass --force to overwrite it",
            skill_file.display()
        ));
    }

    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("failed to create {}", skill_dir.display()))?;
    fs::write(&skill_file, APP_AGENT_WORKFLOW_SKILL)
        .with_context(|| format!("failed to write {}", skill_file.display()))?;

    println!("{}", skill_dir.display());
    Ok(())
}

fn read_private_key(args: &AppAuthArgs) -> Result<String> {
    match (&args.private_key, &args.private_key_file, &args.private_key_path) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => Err(anyhow!(
            "use only one of --private-key, --private-key-file, or --private-key-path"
        )),
        (Some(key), None, None) => Ok(key.clone()),
        (None, Some(path), None) | (None, None, Some(path)) => fs::read_to_string(path)
            .with_context(|| format!("failed to read private key from {}", path.display())),
        (None, None, None) => Err(anyhow!(
            "missing private key; set --private-key-file, --private-key-path, --private-key, GITHUB_APP_PRIVATE_KEY_FILE, GITHUB_APP_PRIVATE_KEY_PATH, or GITHUB_APP_PRIVATE_KEY"
        )),
    }
}

fn create_jwt(app_id: u64, private_key: &str) -> Result<String> {
    // The JWT is valid for 8 minutes, which is less than the 10-minute maximum.
    const JWT_LIFETIME_SECONDS: i64 = 8 * 60;
    // Account for clock skew by setting the "issued at" time to 60 seconds in the past.
    const JWT_IAT_SKEW_SECONDS: i64 = 60;

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let claims = Claims {
        iat: now - JWT_IAT_SKEW_SECONDS,
        exp: now + JWT_LIFETIME_SECONDS,
        iss: app_id.to_string(),
    };

    let key = EncodingKey::from_rsa_pem(private_key.as_bytes())
        .context("private key must be an RSA PEM key")?;
    jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)
        .context("failed to create GitHub App JWT")
}

fn resolve_installation_id(args: &AppAuthArgs, jwt: &str, client: &Client) -> Result<u64> {
    if let Some(installation_id) = args.installation_id {
        return Ok(installation_id);
    }

    let repo = args.repo.as_deref().ok_or_else(|| {
        anyhow!("missing installation id; set --installation-id, GITHUB_APP_INSTALLATION_ID, or --repo OWNER/REPO")
    })?;
    discover_installation_id(args, jwt, client, repo)
}

fn discover_installation_id(
    args: &AppAuthArgs,
    _jwt: &str,
    client: &Client,
    repo: &str,
) -> Result<u64> {
    let (owner, name) = repo.split_once('/').ok_or_else(|| {
        anyhow!("--repo must be OWNER/REPO so the GitHub installation can be discovered")
    })?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(anyhow!(
            "--repo must be OWNER/REPO so the GitHub installation can be discovered"
        ));
    }

    let url = format!(
        "{}/repos/{}/{}/installation",
        args.api_url.trim_end_matches('/'),
        owner,
        name
    );
    let text = send_github_request(client.get(url), "GitHub repository installation API")?;
    let response: InstallationResponse =
        serde_json::from_str(&text).context("failed to parse GitHub installation response")?;
    Ok(response.id)
}

fn create_installation_token(
    args: &AppAuthArgs,
    client: &Client,
    installation_id: u64,
) -> Result<TokenResponse> {
    let url = format!(
        "{}/app/installations/{}/access_tokens",
        args.api_url.trim_end_matches('/'),
        installation_id
    );
    let body = TokenRequest {
        repositories: token_repository_names(args),
        permissions: permissions_map(&args.permissions),
    };

    let text = send_github_request(
        client.post(url).json(&body),
        "GitHub installation token API",
    )?;

    let response: TokenResponse =
        serde_json::from_str(&text).context("failed to parse GitHub token response")?;
    Ok(response)
}

fn github_client(jwt: &str) -> Result<Client> {
    Client::builder()
        .default_headers(default_headers(jwt)?)
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build GitHub API client")
}

fn send_github_request(
    request: reqwest::blocking::RequestBuilder,
    api_name: &str,
) -> Result<String> {
    let response = request
        .send()
        .with_context(|| format!("failed to call {api_name}"))?;

    let status = response.status();
    let text = response
        .text()
        .context("failed to read GitHub API response body")?;

    if !status.is_success() {
        return Err(anyhow!("GitHub API returned {status}: {text}"));
    }

    Ok(text)
}

fn default_headers(jwt: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("toolbox/github-app-auth"),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2022-11-28"),
    );
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {jwt}"))
            .context("failed to build authorization header")?,
    );
    Ok(headers)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_exports(token: &str, export_gh_token: bool) -> String {
    let quoted = shell_quote(token);
    if export_gh_token {
        format!("export GITHUB_TOKEN={quoted}\nexport GH_TOKEN={quoted}")
    } else {
        format!("export GITHUB_TOKEN={quoted}")
    }
}

fn print_token(args: &AppAuthArgs, response: &TokenResponse) -> Result<()> {
    match args.format {
        OutputFormat::Text => println!("{}", response.token),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string(&JsonTokenOutput {
                token: &response.token,
                expires_at: response.expires_at.as_deref(),
            })
            .context("failed to serialize token JSON")?
        ),
    }
    Ok(())
}

fn print_jwt(args: &AppAuthArgs, jwt: &str) -> Result<()> {
    match args.format {
        OutputFormat::Text => println!("{jwt}"),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string(&JsonJwtOutput { jwt })
                .context("failed to serialize JWT JSON")?
        ),
    }
    Ok(())
}

fn token_repository_names(args: &AppAuthArgs) -> Vec<String> {
    let mut repositories = args.repositories.clone();
    if let Some(repo) = &args.repo {
        repositories.push(repo.clone());
    }
    repository_names(&repositories)
}

fn repository_names(repositories: &[String]) -> Vec<String> {
    repositories
        .iter()
        .map(|repository| {
            repository
                .rsplit_once('/')
                .map_or(repository.as_str(), |(_, name)| name)
                .to_string()
        })
        .collect()
}

fn permissions_map(permissions: &[PermissionArg]) -> BTreeMap<String, String> {
    permissions
        .iter()
        .map(|permission| (permission.key.clone(), permission.value.clone()))
        .collect()
}

impl std::str::FromStr for PermissionArg {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let (key, permission_value) = value
            .split_once('=')
            .ok_or_else(|| anyhow!("--permission must be KEY=VALUE"))?;
        if key.is_empty() || permission_value.is_empty() {
            return Err(anyhow!("--permission must be KEY=VALUE"));
        }
        Ok(Self {
            key: key.to_string(),
            value: permission_value.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{permissions_map, repository_names, shell_exports, shell_quote, PermissionArg};

    #[test]
    fn quotes_token_for_posix_shell() {
        assert_eq!(shell_quote("abc'def"), "'abc'\\''def'");
    }

    #[test]
    fn exports_gh_token_when_requested() {
        assert_eq!(
            shell_exports("abc'def", true),
            "export GITHUB_TOKEN='abc'\\''def'\nexport GH_TOKEN='abc'\\''def'"
        );
    }

    #[test]
    fn extracts_repository_names_for_installation_token_request() {
        let repositories = vec![
            "joonjeong/toolbox".to_string(),
            "plain-repo".to_string(),
            "owner/nested/name".to_string(),
        ];

        assert_eq!(
            repository_names(&repositories),
            vec!["toolbox", "plain-repo", "name"]
        );
    }

    #[test]
    fn parses_permission_arguments() {
        let permissions = vec![
            "contents=read".parse::<PermissionArg>().unwrap(),
            "pull_requests=write".parse::<PermissionArg>().unwrap(),
        ];
        let mapped = permissions_map(&permissions);

        assert_eq!(mapped.get("contents").map(String::as_str), Some("read"));
        assert_eq!(
            mapped.get("pull_requests").map(String::as_str),
            Some("write")
        );
    }
}
