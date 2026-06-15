use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use clap::{Args, ValueEnum};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const APP_AGENT_WORKFLOW_SKILL_NAME: &str = "github-app-agent-workflow";
const APP_AGENT_WORKFLOW_SKILL: &str =
    include_str!("../resources/github-app-agent-workflow/SKILL.md");
const TOKEN_CACHE_EXPIRY_GRACE_SECONDS: i64 = 60;

#[derive(Debug, Args)]
#[command(
    about = "Authenticate as a GitHub App installation",
    long_about = "Sign a GitHub App JWT, exchange it for an installation access token, and print the token to stdout.

Use this command to debug app-based authentication behavior or to support integrations that explicitly need token stdout. For ordinary agent GitHub work, prefer toolbox github app-run. Provide the app ID, --repo OWNER/REPO, and exactly one private key source: --private-key-file or --private-key. Values can also come from the documented environment variables.",
    after_long_help = "Purpose:
  Sign a GitHub App JWT, exchange it for an installation access token, and print the token to stdout. This is primarily for debugging app-based authentication behavior or integrations that explicitly need token stdout. For ordinary agent GitHub work, prefer toolbox github app-run.

Invocation forms:
  toolbox github app-auth [OPTIONS]
  toolbox github-app-auth [OPTIONS]
  github-app-auth [OPTIONS]    when symlinked to the toolbox binary

Examples:
  toolbox github app-auth \\
    --app-id \"$GITHUB_APP_ID\" \\
    --repo OWNER/REPO \\
    --private-key-file /path/to/private-key.pem \\
    --format json

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
  By default, prints only the installation token. Treat that mode as an integration escape hatch with a secret-safe destination, not the ordinary agent workflow. With --format json, prints structured diagnostic JSON without the token. With --jwt-only, prints the signed GitHub App JWT and does not call the installation token API. Use toolbox github app-run for normal gh commands.

Repository scoping:
  Use --repo OWNER/REPO to scope the token to one or more repositories. Repeat --repo for multiple repositories. When --installation-id is omitted, the first --repo value is also used to discover the installation. OWNER/REPO is accepted for user-facing clarity; only repository names are sent to GitHub's installation token API."
)]
pub struct AppAuthArgs {
    /// GitHub App ID.
    ///
    /// Can also be set with GITHUB_APP_ID.
    #[arg(long, env = "GITHUB_APP_ID")]
    app_id: u64,

    /// GitHub App installation ID.
    ///
    /// Can also be set with GITHUB_APP_INSTALLATION_ID. Prefer --repo OWNER/REPO
    /// unless the installation ID is already known.
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

    /// Scope the token to a repository.
    ///
    /// Repeat for multiple repositories. Without --installation-id, the first
    /// --repo value is also used to discover the installation. Public repository
    /// access alone is not enough; the GitHub App must be installed on the repo
    /// or owner.
    #[arg(
        long = "repo",
        value_name = "OWNER/REPO",
        required_unless_present_any = ["jwt_only", "installation_id"]
    )]
    repos: Vec<String>,

    /// Limit installation token permissions.
    ///
    /// Repeat as key=value, for example --permission contents=read. Values are
    /// sent unchanged to GitHub's installation token API.
    #[arg(long = "permission", value_name = "KEY=VALUE")]
    permissions: Vec<PermissionArg>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Print the signed GitHub App JWT and skip token exchange.
    ///
    /// Useful for debugging app authentication. The JWT is intentionally short
    /// lived and remains below GitHub's 10-minute maximum.
    #[arg(long, conflicts_with_all = ["repos", "permissions"])]
    jwt_only: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Run a command with a GitHub App installation token",
    long_about = "Sign a GitHub App JWT, exchange it for an installation access token, and run a command with GH_TOKEN and GITHUB_TOKEN set for that process.

Use this command from coding agents or automation that need temporary GitHub repository access through a GitHub App installation without exporting a token into the parent shell.",
    after_long_help = "Purpose:
  Sign a GitHub App JWT, exchange it for an installation access token, and run a command with GH_TOKEN and GITHUB_TOKEN set for that process.

Invocation forms:
  toolbox github app-run [OPTIONS] -- COMMAND [ARG]...
  toolbox github-app-run [OPTIONS] -- COMMAND [ARG]...
  github-app-run [OPTIONS] -- COMMAND [ARG]...    when symlinked to the toolbox binary

Examples:
  toolbox github app-run \\
    --app-id \"$GITHUB_APP_ID\" \\
    --repo OWNER/REPO \\
    --private-key-file /path/to/private-key.pem \\
    -- gh pr comment 123 --body \"Done\"

Environment:
  GITHUB_APP_ID
  GITHUB_APP_INSTALLATION_ID
  GITHUB_APP_PRIVATE_KEY_FILE
  GITHUB_APP_PRIVATE_KEY_PATH
  GITHUB_APP_PRIVATE_KEY
  GITHUB_API_URL

Repository scoping:
  Use --repo OWNER/REPO to scope the token to one or more repositories. Repeat --repo for multiple repositories. When --installation-id is omitted, the first --repo value is also used to discover the installation. OWNER/REPO is accepted for user-facing clarity; only repository names are sent to GitHub's installation token API.

Execution:
  The command after -- is run directly with GH_TOKEN and GITHUB_TOKEN set to the temporary installation token. GitHub App credential environment variables are removed from the child environment. The child process inherits stdin, stdout, stderr, working directory, PATH, and other ordinary environment variables. Shell syntax such as pipes, redirects, aliases, and shell functions requires an explicit shell command, for example -- sh -c 'gh issue view 123 | jq .url'.

Git HTTPS authentication:
  Git does not automatically use GH_TOKEN or GITHUB_TOKEN as HTTPS credentials. Pass --git-credentials to install a child-only Git credential helper that answers HTTPS credential requests for the GitHub host with username x-access-token and the temporary installation token."
)]
pub struct AppRunArgs {
    /// GitHub App ID.
    ///
    /// Can also be set with GITHUB_APP_ID.
    #[arg(long, env = "GITHUB_APP_ID")]
    app_id: u64,

    /// GitHub App installation ID.
    ///
    /// Can also be set with GITHUB_APP_INSTALLATION_ID. Prefer --repo OWNER/REPO
    /// unless the installation ID is already known.
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

    /// Scope the token to a repository.
    ///
    /// Repeat for multiple repositories. Without --installation-id, the first
    /// --repo value is also used to discover the installation. Public repository
    /// access alone is not enough; the GitHub App must be installed on the repo
    /// or owner.
    #[arg(
        long = "repo",
        value_name = "OWNER/REPO",
        required_unless_present = "installation_id"
    )]
    repos: Vec<String>,

    /// Limit installation token permissions.
    ///
    /// Repeat as key=value, for example --permission contents=read. Values are
    /// sent unchanged to GitHub's installation token API.
    #[arg(long = "permission", value_name = "KEY=VALUE")]
    permissions: Vec<PermissionArg>,

    /// Configure a child-only Git credential helper for HTTPS GitHub remotes.
    ///
    /// The helper responds only for the GitHub host derived from --api-url and
    /// reads the temporary installation token from GITHUB_TOKEN in the child
    /// environment.
    #[arg(long)]
    git_credentials: bool,

    /// Command to run with GH_TOKEN and GITHUB_TOKEN set.
    #[arg(
        value_name = "COMMAND",
        required = true,
        num_args = 1..,
        last = true,
        allow_hyphen_values = true
    )]
    command: Vec<OsString>,
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

#[derive(Debug, Serialize)]
struct TokenCacheKey {
    app_id: u64,
    installation_id: u64,
    api_url: String,
    repositories: Vec<String>,
    permissions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CachedToken {
    token: String,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
    expires_at: Option<String>,
    repository_selection: Option<String>,
    #[serde(default)]
    repositories: Vec<TokenRepository>,
    #[serde(default)]
    permissions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct TokenRepository {
    name: Option<String>,
    full_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstallationResponse {
    id: u64,
}

#[derive(Debug, Serialize)]
struct JsonTokenOutput<'a> {
    installation_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository_selection: Option<&'a str>,
    repositories: Vec<String>,
    permissions: &'a BTreeMap<String, String>,
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

trait AppTokenConfig {
    fn app_id(&self) -> u64;
    fn installation_id(&self) -> Option<u64>;
    fn private_key_file(&self) -> Option<&PathBuf>;
    fn private_key_path(&self) -> Option<&PathBuf>;
    fn private_key(&self) -> Option<&str>;
    fn api_url(&self) -> &str;
    fn repos(&self) -> &[String];
    fn permissions(&self) -> &[PermissionArg];
}

impl AppTokenConfig for AppAuthArgs {
    fn app_id(&self) -> u64 {
        self.app_id
    }

    fn installation_id(&self) -> Option<u64> {
        self.installation_id
    }

    fn private_key_file(&self) -> Option<&PathBuf> {
        self.private_key_file.as_ref()
    }

    fn private_key_path(&self) -> Option<&PathBuf> {
        self.private_key_path.as_ref()
    }

    fn private_key(&self) -> Option<&str> {
        self.private_key.as_deref()
    }

    fn api_url(&self) -> &str {
        &self.api_url
    }

    fn repos(&self) -> &[String] {
        &self.repos
    }

    fn permissions(&self) -> &[PermissionArg] {
        &self.permissions
    }
}

impl AppTokenConfig for AppRunArgs {
    fn app_id(&self) -> u64 {
        self.app_id
    }

    fn installation_id(&self) -> Option<u64> {
        self.installation_id
    }

    fn private_key_file(&self) -> Option<&PathBuf> {
        self.private_key_file.as_ref()
    }

    fn private_key_path(&self) -> Option<&PathBuf> {
        self.private_key_path.as_ref()
    }

    fn private_key(&self) -> Option<&str> {
        self.private_key.as_deref()
    }

    fn api_url(&self) -> &str {
        &self.api_url
    }

    fn repos(&self) -> &[String] {
        &self.repos
    }

    fn permissions(&self) -> &[PermissionArg] {
        &self.permissions
    }
}

pub fn app_auth(args: AppAuthArgs) -> Result<()> {
    let jwt = create_jwt(args.app_id(), &read_private_key(&args)?)?;

    if args.jwt_only {
        print_jwt(&args, &jwt)?;
        return Ok(());
    }

    let client = github_client(&jwt)?;
    let installation_id = resolve_installation_id(&args, &client)?;
    let response = create_installation_token(&args, &client, installation_id)?;
    print_token(&args, installation_id, &response)?;

    Ok(())
}

pub fn app_run(args: AppRunArgs) -> Result<()> {
    let token = cached_or_fresh_installation_token(&args)?.token;
    run_with_installation_token(&args.command, &token, args.git_credentials, &args.api_url)
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

fn read_private_key(args: &impl AppTokenConfig) -> Result<String> {
    match (
        args.private_key(),
        args.private_key_file(),
        args.private_key_path(),
    ) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => Err(anyhow!(
            "use only one of --private-key, --private-key-file, or --private-key-path"
        )),
        (Some(key), None, None) => Ok(key.to_string()),
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

fn installation_token(args: &impl AppTokenConfig) -> Result<TokenResponse> {
    let jwt = create_jwt(args.app_id(), &read_private_key(args)?)?;
    let client = github_client(&jwt)?;
    let installation_id = resolve_installation_id(args, &client)?;
    create_installation_token(args, &client, installation_id)
}

fn cached_or_fresh_installation_token(args: &impl AppTokenConfig) -> Result<TokenResponse> {
    let cache_path = token_cache_path(args).ok();

    if let Some(cache_path) = cache_path.as_ref() {
        if let Ok(Some(cached)) = read_cached_token(cache_path) {
            if cached_token_is_fresh(&cached)
                && validate_cached_token(args.api_url(), &cached.token).is_ok()
            {
                return Ok(TokenResponse {
                    token: cached.token,
                    expires_at: cached.expires_at,
                    repository_selection: None,
                    repositories: Vec::new(),
                    permissions: BTreeMap::new(),
                });
            }
        }
    }

    let response = installation_token(args)?;
    if let Some(cache_path) = cache_path.as_ref() {
        let _ = write_cached_token(cache_path, &response);
    }
    Ok(response)
}

fn token_cache_key(args: &impl AppTokenConfig) -> Result<TokenCacheKey> {
    let mut repositories = args.repos().to_vec();
    repositories.sort();

    Ok(TokenCacheKey {
        app_id: args.app_id(),
        installation_id: resolve_cache_installation_id(args)?,
        api_url: args.api_url().trim_end_matches('/').to_string(),
        repositories,
        permissions: permissions_map(args.permissions()),
    })
}

fn resolve_cache_installation_id(args: &impl AppTokenConfig) -> Result<u64> {
    if let Some(installation_id) = args.installation_id() {
        return Ok(installation_id);
    }

    let jwt = create_jwt(args.app_id(), &read_private_key(args)?)?;
    let client = github_client(&jwt)?;
    resolve_installation_id(args, &client)
}

fn token_cache_path(args: &impl AppTokenConfig) -> Result<PathBuf> {
    let cache_key = token_cache_key(args)?;
    let encoded = serde_json::to_vec(&cache_key).context("failed to serialize token cache key")?;
    let digest = Sha256::digest(encoded);
    let file_name = format!(
        "{}.json",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
    );
    Ok(token_cache_dir()?.join(file_name))
}

fn token_cache_dir() -> Result<PathBuf> {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(cache_home)
            .join("toolbox")
            .join("github-app-run"));
    }

    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".cache")
            .join("toolbox")
            .join("github-app-run"));
    }

    Err(anyhow!(
        "cannot determine token cache directory; set XDG_CACHE_HOME or HOME"
    ))
}

fn read_cached_token(path: &PathBuf) -> Result<Option<CachedToken>> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let token = serde_json::from_str(&contents).ok();
            Ok(token)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read token cache {}", path.display()))
        }
    }
}

fn write_cached_token(path: &PathBuf, response: &TokenResponse) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(anyhow!("token cache path has no parent directory"));
    };
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create token cache directory {}",
            parent.display()
        )
    })?;

    let contents = serde_json::to_vec(&CachedToken {
        token: response.token.clone(),
        expires_at: response.expires_at.clone(),
    })
    .context("failed to serialize token cache")?;

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(path)
        .with_context(|| format!("failed to write token cache {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set token cache permissions {}", path.display()))?;
    }
    file.write_all(&contents)
        .with_context(|| format!("failed to write token cache {}", path.display()))?;
    Ok(())
}

fn cached_token_is_fresh(cached: &CachedToken) -> bool {
    let Some(expires_at) = cached.expires_at.as_deref() else {
        return false;
    };
    let Ok(expires_at) = OffsetDateTime::parse(expires_at, &Rfc3339) else {
        return false;
    };
    let refresh_after = expires_at - time::Duration::seconds(TOKEN_CACHE_EXPIRY_GRACE_SECONDS);
    OffsetDateTime::now_utc() < refresh_after
}

fn validate_cached_token(api_url: &str, token: &str) -> Result<()> {
    let client = installation_token_client(token)?;
    let url = format!(
        "{}/installation/repositories",
        api_url.trim_end_matches('/')
    );
    send_github_request(
        client.get(url),
        "GitHub cached installation token validation",
    )?;
    Ok(())
}

fn resolve_installation_id(args: &impl AppTokenConfig, client: &Client) -> Result<u64> {
    if let Some(installation_id) = args.installation_id() {
        return Ok(installation_id);
    }

    let repo = args
        .repos()
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("missing repository; set --repo OWNER/REPO"))?;
    discover_installation_id(args, client, repo)
}

fn discover_installation_id(
    args: &impl AppTokenConfig,
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
        args.api_url().trim_end_matches('/'),
        owner,
        name
    );
    let text = send_github_request(client.get(url), "GitHub repository installation API")
        .with_context(|| {
            format!(
                "failed to discover GitHub App installation for {repo}; public repository access is not enough. The GitHub App must be installed on the repository or owner before an installation token can be minted"
            )
        })?;
    let response: InstallationResponse =
        serde_json::from_str(&text).context("failed to parse GitHub installation response")?;
    Ok(response.id)
}

fn create_installation_token(
    args: &impl AppTokenConfig,
    client: &Client,
    installation_id: u64,
) -> Result<TokenResponse> {
    let url = format!(
        "{}/app/installations/{}/access_tokens",
        args.api_url().trim_end_matches('/'),
        installation_id
    );
    let body = TokenRequest {
        repositories: token_repository_names(args),
        permissions: permissions_map(args.permissions()),
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

fn installation_token_client(token: &str) -> Result<Client> {
    Client::builder()
        .default_headers(default_token_headers(token)?)
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
    auth_headers(&format!("Bearer {jwt}"))
}

fn default_token_headers(token: &str) -> Result<HeaderMap> {
    auth_headers(&format!("Bearer {token}"))
}

fn auth_headers(authorization: &str) -> Result<HeaderMap> {
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
        HeaderValue::from_str(authorization).context("failed to build authorization header")?,
    );
    Ok(headers)
}

fn print_token(args: &AppAuthArgs, installation_id: u64, response: &TokenResponse) -> Result<()> {
    match args.format {
        OutputFormat::Text => println!("{}", response.token),
        OutputFormat::Json => {
            let repositories = json_repository_names(response, args);
            println!(
                "{}",
                serde_json::to_string(&JsonTokenOutput {
                    installation_id,
                    repository_selection: response.repository_selection.as_deref(),
                    repositories,
                    permissions: &response.permissions,
                    expires_at: response.expires_at.as_deref(),
                })
                .context("failed to serialize token JSON")?
            );
        }
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

fn run_with_installation_token(
    command: &[OsString],
    token: &str,
    git_credentials: bool,
    api_url: &str,
) -> Result<()> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("missing command after --"))?;

    let git_credential_environment = if git_credentials {
        Some(GitCredentialEnvironment::create(api_url)?)
    } else {
        None
    };

    let mut child = Command::new(program);
    child
        .args(args)
        .env_remove("GITHUB_APP_ID")
        .env_remove("GITHUB_APP_INSTALLATION_ID")
        .env_remove("GITHUB_APP_PRIVATE_KEY")
        .env_remove("GITHUB_APP_PRIVATE_KEY_FILE")
        .env_remove("GITHUB_APP_PRIVATE_KEY_PATH")
        .env_remove("GITHUB_API_URL")
        .env("GH_TOKEN", token)
        .env("GITHUB_TOKEN", token);

    if let Some(environment) = &git_credential_environment {
        environment.configure_child(&mut child);
    }

    let status = child
        .status()
        .with_context(|| format!("failed to run {}", program.to_string_lossy()))?;
    drop(git_credential_environment);

    if status.success() {
        return Ok(());
    }

    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            std::process::exit(128 + signal);
        }
    }

    Err(anyhow!("command terminated before exiting"))
}

struct GitCredentialEnvironment {
    temp_dir: PathBuf,
    helper_path: PathBuf,
}

impl GitCredentialEnvironment {
    fn create(api_url: &str) -> Result<Self> {
        let host = git_credential_host(api_url)?;
        let temp_dir = unique_temp_dir("toolbox-git-credentials");
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&temp_dir)
            .with_context(|| format!("failed to create {}", temp_dir.display()))?;

        let helper_path = temp_dir.join("git-credential-toolbox");
        fs::write(&helper_path, git_credential_helper_script(&host))
            .with_context(|| format!("failed to write {}", helper_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&helper_path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("failed to make {} executable", helper_path.display()))?;
        }

        Ok(Self {
            temp_dir,
            helper_path,
        })
    }

    fn configure_child(&self, child: &mut Command) {
        let config_index = std::env::var("GIT_CONFIG_COUNT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);

        child
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_COUNT", (config_index + 2).to_string())
            .env(
                format!("GIT_CONFIG_KEY_{config_index}"),
                "credential.helper",
            )
            .env(format!("GIT_CONFIG_VALUE_{config_index}"), "")
            .env(
                format!("GIT_CONFIG_KEY_{}", config_index + 1),
                "credential.helper",
            )
            .env(
                format!("GIT_CONFIG_VALUE_{}", config_index + 1),
                &self.helper_path,
            );
    }
}

impl Drop for GitCredentialEnvironment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()))
}

fn git_credential_host(api_url: &str) -> Result<String> {
    let url = Url::parse(api_url).with_context(|| format!("invalid GitHub API URL: {api_url}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("GitHub API URL must include a host"))?;

    if host.eq_ignore_ascii_case("api.github.com") {
        Ok("github.com".to_string())
    } else {
        Ok(host.to_ascii_lowercase())
    }
}

fn git_credential_helper_script(host: &str) -> String {
    format!(
        r#"#!/bin/sh
test "$1" = get || exit 0

protocol=
host=
while IFS= read -r line; do
    test -n "$line" || break
    case "$line" in
        protocol=*) protocol=${{line#protocol=}} ;;
        host=*) host=${{line#host=}} ;;
    esac
done

test "$protocol" = https || exit 0
host_no_port=${{host%%:*}}
test "$host_no_port" = {quoted_host} || exit 0
test -n "$GITHUB_TOKEN" || exit 0

printf '%s\n' username=x-access-token
printf '%s\n' "password=$GITHUB_TOKEN"
"#,
        quoted_host = shell_single_quote(host)
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn token_repository_names(args: &impl AppTokenConfig) -> Vec<String> {
    repository_names(args.repos())
}

fn json_repository_names(response: &TokenResponse, args: &AppAuthArgs) -> Vec<String> {
    let repositories: Vec<String> = response
        .repositories
        .iter()
        .filter_map(|repository| {
            repository
                .full_name
                .as_deref()
                .or(repository.name.as_deref())
                .map(str::to_string)
        })
        .collect();

    if repositories.is_empty() {
        token_repository_names(args)
    } else {
        repositories
    }
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
    use super::{
        git_credential_host, json_repository_names, permissions_map, repository_names,
        token_cache_key, AppAuthArgs, OutputFormat, PermissionArg, TokenRepository, TokenResponse,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;

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

    #[test]
    fn maps_github_api_host_to_git_credential_host() {
        assert_eq!(
            git_credential_host("https://api.github.com").expect("host parses"),
            "github.com"
        );
        assert_eq!(
            git_credential_host("https://ghe.example.com/api/v3").expect("host parses"),
            "ghe.example.com"
        );
    }

    #[test]
    fn json_repository_names_prefer_github_response_metadata() {
        let response = TokenResponse {
            token: "token".to_string(),
            expires_at: Some("2026-06-14T01:23:45Z".to_string()),
            repository_selection: Some("selected".to_string()),
            repositories: vec![
                TokenRepository {
                    name: Some("toolbox".to_string()),
                    full_name: Some("joonjeong/toolbox".to_string()),
                },
                TokenRepository {
                    name: Some("other".to_string()),
                    full_name: None,
                },
            ],
            permissions: BTreeMap::new(),
        };
        let args = test_args();

        assert_eq!(
            json_repository_names(&response, &args),
            vec!["joonjeong/toolbox", "other"]
        );
    }

    #[test]
    fn json_repository_names_fall_back_to_requested_scope() {
        let response = TokenResponse {
            token: "token".to_string(),
            expires_at: None,
            repository_selection: None,
            repositories: Vec::new(),
            permissions: BTreeMap::new(),
        };
        let args = test_args();

        assert_eq!(json_repository_names(&response, &args), vec!["toolbox"]);
    }

    #[test]
    fn json_token_output_never_includes_token() {
        let mut permissions = BTreeMap::new();
        permissions.insert("contents".to_string(), "read".to_string());
        let output = super::JsonTokenOutput {
            installation_id: 123,
            repository_selection: Some("selected"),
            repositories: vec!["joonjeong/toolbox".to_string()],
            permissions: &permissions,
            expires_at: Some("2026-06-14T01:23:45Z"),
        };

        let value = serde_json::to_value(&output).expect("serializes");
        assert_eq!(value["installation_id"], 123);
        assert_eq!(value["repository_selection"], "selected");
        assert_eq!(value["repositories"][0], "joonjeong/toolbox");
        assert_eq!(value["permissions"]["contents"], "read");
        assert_eq!(value["expires_at"], "2026-06-14T01:23:45Z");
        assert!(value.get("token").is_none());
    }

    #[test]
    fn token_cache_key_sorts_repository_scope() {
        let mut first = test_args();
        first.repos = vec![
            "joonjeong/toolbox".to_string(),
            "joonjeong/other".to_string(),
        ];
        let mut second = test_args();
        second.repos = vec![
            "joonjeong/other".to_string(),
            "joonjeong/toolbox".to_string(),
        ];

        let first_key = token_cache_key(&first).expect("cache key");
        let second_key = token_cache_key(&second).expect("cache key");

        assert_eq!(first_key.repositories, second_key.repositories);
    }

    fn test_args() -> AppAuthArgs {
        AppAuthArgs {
            app_id: 1,
            installation_id: Some(2),
            private_key_file: Some(PathBuf::from("private-key.pem")),
            private_key_path: None,
            private_key: None,
            api_url: "https://api.github.com".to_string(),
            repos: vec!["joonjeong/toolbox".to_string()],
            permissions: Vec::new(),
            format: OutputFormat::Json,
            jwt_only: false,
        }
    }
}
