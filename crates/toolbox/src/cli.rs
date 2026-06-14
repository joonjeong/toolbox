use std::ffi::{OsStr, OsString};
use std::path::Path;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::github;

#[derive(Debug, Parser)]
#[command(name = "toolbox")]
#[command(about = "Personal general-purpose CLI/TUI toolbox")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// GitHub related tools.
    Github(GithubCommand),
    /// Authenticate as a GitHub App installation.
    GithubAppAuth(github::AppAuthArgs),
}

#[derive(Debug, Args)]
struct GithubCommand {
    #[command(subcommand)]
    command: GithubSubcommand,
}

#[derive(Debug, Subcommand)]
enum GithubSubcommand {
    /// Authenticate as a GitHub App installation.
    AppAuth(github::AppAuthArgs),
}

pub fn run<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if args.is_empty() {
        args.push(OsString::from("toolbox"));
    }

    let invoked_as = args
        .first()
        .and_then(|arg| Path::new(arg).file_name())
        .and_then(OsStr::to_str)
        .unwrap_or("toolbox");

    if matches!(invoked_as, "github-app-auth" | "toolbox-github-app-auth") {
        args.insert(1, OsString::from("github-app-auth"));
    }

    let cli = Cli::parse_from(args);
    match cli.command {
        Command::Github(github_command) => match github_command.command {
            GithubSubcommand::AppAuth(args) => github::app_auth(args),
        },
        Command::GithubAppAuth(args) => github::app_auth(args),
    }
}
