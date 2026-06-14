use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::Args;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const APP_AGENT_WORKFLOW_SKILL_NAME: &str = "github-app-agent-workflow";
const APP_AGENT_WORKFLOW_SKILL: &str =
    include_str!("../../../skills/github-app-agent-workflow/SKILL.md");

#[derive(Debug, Args)]
#[command(
    about = "Authenticate as a GitHub App installation",
    long_about = "Sign a GitHub App JWT, exchange it for an installation access token, and print the token to stdout.

Use this command from coding agents or automation that need temporary GitHub repository access through a GitHub App installation. Provide the app ID, installation ID, and exactly one private key source: --private-key-file or --private-key. Values can also come from the documented environment variables.",
    after_long_help = "Purpose:
  Sign a GitHub App JWT, exchange it for an installation access token, and print the token to stdout. Use this from coding agents or automation that need temporary GitHub repository access through a GitHub App installation.

Invocation forms:
  toolbox github app-auth [OPTIONS]
  toolbox github-app-auth [OPTIONS]
  github-app-auth [OPTIONS]    when symlinked to the toolbox binary

Examples:
  toolbox github app-auth \\
    --app-id \"$GITHUB_APP_ID\" \\
    --installation-id \"$GITHUB_APP_INSTALLATION_ID\" \\
    --private-key-file /path/to/private-key.pem

  eval \"$(toolbox github app-auth --shell \\
    --repository OWNER/REPO \\
    --app-id \"$GITHUB_APP_ID\" \\
    --installation-id \"$GITHUB_APP_INSTALLATION_ID\" \\
    --private-key-file /path/to/private-key.pem)\"

  toolbox github-app-auth --jwt-only \\
    --app-id \"$GITHUB_APP_ID\" \\
    --private-key-file /path/to/private-key.pem

Environment:
  GITHUB_APP_ID
  GITHUB_APP_INSTALLATION_ID
  GITHUB_APP_PRIVATE_KEY_FILE
  GITHUB_APP_PRIVATE_KEY
  GITHUB_API_URL

Output:
  By default, prints only the installation token. With --shell, prints a POSIX shell export statement for GITHUB_TOKEN. With --jwt-only, prints the signed GitHub App JWT and does not call the installation token API.

Repository scoping:
  Repeat --repository to limit the token to specific repositories. Pass OWNER/REPO for user-facing clarity; only the repository name is sent to GitHub's installation token API."
)]
pub struct AppAuthArgs {
    /// GitHub App ID.
    ///
    /// Can also be set with GITHUB_APP_ID.
    #[arg(long, env = "GITHUB_APP_ID")]
    app_id: u64,

    /// GitHub App installation ID.
    ///
    /// Can also be set with GITHUB_APP_INSTALLATION_ID.
    #[arg(long, env = "GITHUB_APP_INSTALLATION_ID")]
    installation_id: u64,

    /// Path to the GitHub App private key PEM file.
    ///
    /// Use this or --private-key, not both. Can also be set with
    /// GITHUB_APP_PRIVATE_KEY_FILE.
    #[arg(
        long,
        env = "GITHUB_APP_PRIVATE_KEY_FILE",
        conflicts_with = "private_key"
    )]
    private_key_file: Option<PathBuf>,

    /// GitHub App private key PEM content.
    ///
    /// Use this or --private-key-file, not both. Can also be set with
    /// GITHUB_APP_PRIVATE_KEY. Prefer --private-key-file in shell history.
    #[arg(
        long,
        env = "GITHUB_APP_PRIVATE_KEY",
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

    /// Emit `export GITHUB_TOKEN=...` instead of the raw token.
    ///
    /// Intended for `eval "$(toolbox github app-auth --shell ...)"`.
    #[arg(long)]
    shell: bool,

    /// Print the signed GitHub App JWT and skip token exchange.
    ///
    /// Useful for debugging app authentication. The JWT is intentionally short
    /// lived and remains below GitHub's 10-minute maximum.
    #[arg(long, conflicts_with_all = ["shell", "repositories"])]
    jwt_only: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Create the GitHub App agent workflow skill in a directory",
    long_about = "Create the bundled github-app-agent-workflow skill under a target skills directory.

The command writes DIR/github-app-agent-workflow/SKILL.md. Use it to install the agent-facing workflow guidance next to Codex, Hermes, or another agent's skill directory without copying files manually.",
    after_long_help = "Examples:
  toolbox github app-agent-workflow-skill --directory ~/.codex/skills
  toolbox github-app-agent-workflow-skill --directory ./skills --force

Output:
  Prints the created skill directory path."
)]
pub struct AppAgentWorkflowSkillArgs {
    /// Directory where the skill folder should be created.
    ///
    /// The command creates <DIRECTORY>/github-app-agent-workflow/SKILL.md.
    #[arg(long, short = 'd', value_name = "DIRECTORY")]
    directory: PathBuf,

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
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
}

pub fn app_auth(args: AppAuthArgs) -> Result<()> {
    let jwt = create_jwt(args.app_id, &read_private_key(&args)?)?;

    if args.jwt_only {
        println!("{jwt}");
        return Ok(());
    }

    let token = create_installation_token(&args, &jwt)?;
    if args.shell {
        println!("export GITHUB_TOKEN={}", shell_quote(&token));
    } else {
        println!("{token}");
    }

    Ok(())
}

pub fn create_app_agent_workflow_skill(args: AppAgentWorkflowSkillArgs) -> Result<()> {
    let skill_dir = args.directory.join(APP_AGENT_WORKFLOW_SKILL_NAME);
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
    match (&args.private_key, &args.private_key_file) {
        (Some(_), Some(_)) => Err(anyhow!(
            "use only one of --private-key or --private-key-file"
        )),
        (Some(key), None) => Ok(key.clone()),
        (None, Some(path)) => fs::read_to_string(path)
            .with_context(|| format!("failed to read private key from {}", path.display())),
        (None, None) => Err(anyhow!(
            "missing private key; set --private-key-file, --private-key, GITHUB_APP_PRIVATE_KEY_FILE, or GITHUB_APP_PRIVATE_KEY"
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

fn create_installation_token(args: &AppAuthArgs, jwt: &str) -> Result<String> {
    let url = format!(
        "{}/app/installations/{}/access_tokens",
        args.api_url.trim_end_matches('/'),
        args.installation_id
    );
    let body = TokenRequest {
        repositories: repository_names(&args.repositories),
    };

    let client = Client::builder()
        .default_headers(default_headers(jwt)?)
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build GitHub API client")?;

    let response = client
        .post(url)
        .json(&body)
        .send()
        .context("failed to call GitHub installation token API")?;

    let status = response.status();
    let text = response
        .text()
        .context("failed to read GitHub API response body")?;

    if !status.is_success() {
        return Err(anyhow!("GitHub API returned {status}: {text}"));
    }

    let response: TokenResponse =
        serde_json::from_str(&text).context("failed to parse GitHub token response")?;
    Ok(response.token)
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

#[cfg(test)]
mod tests {
    use super::{repository_names, shell_quote};

    #[test]
    fn quotes_token_for_posix_shell() {
        assert_eq!(shell_quote("abc'def"), "'abc'\\''def'");
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
}
