use std::ffi::{OsStr, OsString};
use std::path::Path;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::github;

#[derive(Debug, Parser)]
#[command(name = "toolbox")]
#[command(version)]
#[command(about = "Personal general-purpose CLI/TUI toolbox")]
#[command(after_long_help = "Invocation forms:
  toolbox github app-auth ...
  toolbox github-app-auth ...
  github-app-auth ...        when symlinked to the toolbox binary
  toolbox github agent-skill --install-path DIR ...

Run `toolbox github app-auth --help` for GitHub App authentication options and examples.")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
#[command(name = "github-app-auth")]
#[command(version)]
struct GithubAppAuthCli {
    #[command(flatten)]
    args: github::AppAuthArgs,
}

#[derive(Debug, Parser)]
#[command(name = "github-agent-skill")]
#[command(version)]
struct GithubAgentSkillCli {
    #[command(flatten)]
    args: github::AppAgentWorkflowSkillArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// GitHub related tools.
    Github(GithubCommand),
    /// Authenticate as a GitHub App installation.
    GithubAppAuth(github::AppAuthArgs),
    /// Create the GitHub App agent workflow skill.
    GithubAgentSkill(github::AppAgentWorkflowSkillArgs),
}

#[derive(Debug, Args)]
#[command(about = "GitHub related tools")]
struct GithubCommand {
    #[command(subcommand)]
    command: GithubSubcommand,
}

#[derive(Debug, Subcommand)]
enum GithubSubcommand {
    /// Authenticate as a GitHub App installation.
    AppAuth(github::AppAuthArgs),
    /// Create the GitHub App agent workflow skill.
    AgentSkill(github::AppAgentWorkflowSkillArgs),
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
        .unwrap_or("toolbox")
        .to_string();

    if matches!(
        invoked_as.as_str(),
        "github-app-auth" | "toolbox-github-app-auth"
    ) {
        let cli = GithubAppAuthCli::parse_from(args);
        return github::app_auth(cli.args);
    }
    if matches!(
        invoked_as.as_str(),
        "github-agent-skill" | "toolbox-github-agent-skill"
    ) {
        let cli = GithubAgentSkillCli::parse_from(args);
        return github::create_app_agent_workflow_skill(cli.args);
    }

    let cli = Cli::parse_from(args);
    match cli.command {
        Command::Github(github_command) => match github_command.command {
            GithubSubcommand::AppAuth(args) => github::app_auth(args),
            GithubSubcommand::AgentSkill(args) => github::create_app_agent_workflow_skill(args),
        },
        Command::GithubAppAuth(args) => github::app_auth(args),
        Command::GithubAgentSkill(args) => github::create_app_agent_workflow_skill(args),
    }
}
