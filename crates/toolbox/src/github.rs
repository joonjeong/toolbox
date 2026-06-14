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

#[derive(Debug, Args)]
pub struct AppAuthArgs {
    /// GitHub App ID.
    #[arg(long, env = "GITHUB_APP_ID")]
    app_id: u64,

    /// GitHub App installation ID.
    #[arg(long, env = "GITHUB_APP_INSTALLATION_ID")]
    installation_id: u64,

    /// Path to the GitHub App private key PEM file.
    #[arg(long, env = "GITHUB_APP_PRIVATE_KEY_FILE")]
    private_key_file: Option<PathBuf>,

    /// GitHub App private key PEM content.
    #[arg(long, env = "GITHUB_APP_PRIVATE_KEY")]
    private_key: Option<String>,

    /// GitHub API base URL.
    #[arg(long, env = "GITHUB_API_URL", default_value = "https://api.github.com")]
    api_url: String,

    /// Limit the installation token to specific repositories.
    #[arg(long = "repository", value_name = "OWNER/REPO")]
    repositories: Vec<String>,

    /// Emit a shell export statement instead of the raw token.
    #[arg(long)]
    shell: bool,

    /// Print the JWT instead of exchanging it for an installation token.
    #[arg(long)]
    jwt_only: bool,
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
