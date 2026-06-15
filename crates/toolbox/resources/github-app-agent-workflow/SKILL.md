---
name: github-app-agent-workflow
description: Perform GitHub agent work through toolbox github-app-run without exposing temporary installation tokens. Use when an agent or automation needs to work on issues, pull requests, releases, or repository API calls through GitHub App credentials while avoiding personal access tokens, token stdout, shell exports, persistent gh login, shell history leaks, or accidental token logging.
---

# GitHub App Agent Workflow

Use `toolbox github app-run` to run GitHub commands inside a short-lived GitHub
App installation token context without printing the token or exporting it into
the parent shell.

Prefer this skill when GitHub access should come from a GitHub App installation
rather than a personal token. The default workflow is token-non-disclosure:
`toolbox` obtains the installation token, injects it into the child process as
`GH_TOKEN` and `GITHUB_TOKEN`, removes GitHub App credential environment
variables from the child environment, and exits with the child command status.
When a matching local cache entry exists, `app-run` checks its expiration and
validates it with GitHub before reuse; rejected or expired cached tokens are
replaced automatically.

Do not ask the agent to print, copy, paste, log, persist, or manually export the
temporary installation token. Use `app-run` for each GitHub command or each
explicit shell command group.

## Command Forms

Use any supported `app-run` invocation form:

```sh
toolbox github app-run [OPTIONS] -- COMMAND [ARG]...
toolbox github-app-run [OPTIONS] -- COMMAND [ARG]...
github-app-run [OPTIONS] -- COMMAND [ARG]... # when symlinked to toolbox
```

When scripting for portability, prefer `toolbox github app-run`.

To create this skill in another agent's skills directory, run:

```sh
toolbox github agent-skill --install-path /path/to/skills
```

## Required Inputs

Provide:

- `--app-id` or `GITHUB_APP_ID`
- `--repo OWNER/REPO`
- exactly one private key source:
  - `--private-key-file` or `GITHUB_APP_PRIVATE_KEY_FILE`
  - `--private-key-path` or `GITHUB_APP_PRIVATE_KEY_PATH`
  - `--private-key` or `GITHUB_APP_PRIVATE_KEY`

Prefer `--private-key-file` or `--private-key-path` in shell commands so PEM
contents do not appear in shell history or process listings.

For Ciel/Hermes-compatible environments that provide a private key path:

```sh
toolbox github app-run \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --private-key-file "$GITHUB_APP_PRIVATE_KEY_PATH" \
  -- gh pr view 123 --repo OWNER/REPO
```

## Preferred `gh` Workflows

Run each `gh` command through `app-run`. This avoids token stdout and avoids
leaving `GH_TOKEN` in the parent shell:

```sh
toolbox github app-run \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --private-key-file /path/to/private-key.pem \
  -- gh pr view 123 --repo OWNER/REPO
```

For issue or PR triage, request only the permissions needed for that read-only
operation:

```sh
toolbox github app-run \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --permission contents=read \
  --permission pull_requests=read \
  --private-key-file /path/to/private-key.pem \
  -- gh pr checks 123 --repo OWNER/REPO
```

For a write workflow, scope the token to the smallest repository and permission
set that covers the operation:

```sh
toolbox github app-run \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --permission issues=write \
  --permission pull_requests=write \
  --private-key-file /path/to/private-key.pem \
  -- gh pr comment 123 --repo OWNER/REPO --body-file /tmp/comment.md
```

Use `gh api` the same way:

```sh
toolbox github app-run \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --permission actions=read \
  --permission contents=read \
  --private-key-file /path/to/private-key.pem \
  -- gh api repos/OWNER/REPO/actions/runs --jq '.workflow_runs[0].status'
```

Pass `OWNER/REPO` for readability. The command sends only repository names to
GitHub's installation token API, as required by GitHub.

## Grouped Commands

`app-run` executes the command after `--` directly. It does not invoke a shell.
Pipes, redirects, shell functions, aliases, variable assignments, and command
groups require an explicit shell:

```sh
toolbox github app-run \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --private-key-file /path/to/private-key.pem \
  -- sh -c 'gh pr view "$1" --repo "$2" --json title,url | jq .url' sh 123 OWNER/REPO
```

For several related `gh` commands, use one explicit shell command group. The
token remains inside that child process tree and never appears in the parent
shell:

```sh
toolbox github app-run \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --permission contents=read \
  --permission pull_requests=read \
  --private-key-file /path/to/private-key.pem \
  -- sh -c '
    set -eu
    gh pr view "$1" --repo "$2"
    gh pr checks "$1" --repo "$2"
    gh pr diff "$1" --repo "$2" --name-only
  ' sh 123 OWNER/REPO
```

Keep grouped command blocks short and task-focused. Start a new `app-run`
invocation when repository scope changes, privileges should be narrower, or a
command fails due to expiration.

## Environment Boundary

The child command receives:

- `GH_TOKEN` set to the temporary installation token
- `GITHUB_TOKEN` set to the same temporary installation token
- ordinary inherited environment such as `PATH`, locale, and working directory

The child command does not receive these GitHub App credential variables:

- `GITHUB_APP_ID`
- `GITHUB_APP_INSTALLATION_ID`
- `GITHUB_APP_PRIVATE_KEY`
- `GITHUB_APP_PRIVATE_KEY_FILE`
- `GITHUB_APP_PRIVATE_KEY_PATH`
- `GITHUB_API_URL`

Do not re-export those variables into the child command unless explicitly
debugging the authentication flow. The child should operate with the scoped
installation token only.

`app-run` may reuse a matching locally cached installation token until it is
near expiration. The cache is keyed by app, installation selector, API URL,
repository scope, and requested permissions. Reuse still keeps the token inside
the `app-run` child environment; it does not print the token or export it into
the parent shell.

## Authentication Diagnostics

Avoid direct token output during ordinary agent work. `toolbox github app-auth`
is primarily for debugging GitHub App authentication behavior: JWT signing,
installation discovery, installation token exchange, requested permissions, and
repository scoping. It prints a valid installation token to stdout by default,
so that output can leak through logs, shell tracing, command substitution,
process environments, terminal scrollback, or copy/paste.

Only use `app-auth` when diagnosing the app-based authentication flow or when an
external integration truly requires the token string and has a concrete
secret-safe destination. Prefer this command for ordinary agent GitHub work:

```sh
toolbox github app-run \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --private-key-file /path/to/private-key.pem \
  -- gh pr view 123 --repo OWNER/REPO
```

If `app-auth` is unavoidable, treat the session as a debugging or integration
boundary: disable shell tracing, never log stdout, and do not persist the token
with `gh auth login --with-token` unless persistent local authentication is
explicitly intended and cleaned up afterward.

Print only the GitHub App JWT for debugging:

```sh
toolbox github app-auth --jwt-only \
  --app-id "$GITHUB_APP_ID" \
  --private-key-file /path/to/private-key.pem
```

Do not paste JWT output into issue comments, PR comments, build logs, or chat.

For structured automation diagnostics:

```sh
toolbox github app-auth \
  --repo OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --private-key-file /path/to/private-key.pem \
  --format json
```

JSON output is diagnostic-first and never includes the installation token.

## Options To Remember

- `--api-url` or `GITHUB_API_URL`: override for GitHub Enterprise Server token
  minting. This does not configure the child command's GitHub host; for `gh`
  Enterprise usage, set the appropriate `GH_HOST` or pass an explicit
  `--repo HOST/OWNER/REPO` form when needed.
- `--repo OWNER/REPO`: scope the token to a repository. Repeat `--repo` for
  multiple repositories. Without `--installation-id`, the first `--repo` value
  is also used to discover the app installation.
- `--installation-id`: use a known installation ID and skip repository
  installation discovery.
- `--permission key=value`: repeat to request narrower token permissions.
- `--format json` and `--jwt-only` belong to `app-auth`, not `app-run`.

## Common Failure Cases

- The App is not installed on the repository or owning account. A public
  repository can still return `404` from installation discovery because public
  release/download access is unrelated to GitHub App installation access.
- Requested permissions are broader than the installation allows. Ask for equal
  or narrower permissions, or update the App installation permissions first.
- The token expired or GitHub rejects a cached token. `app-run` should
  automatically mint a fresh scoped token.
- The private key path or environment variable is missing. Check
  `--private-key-file`, `--private-key-path`, `GITHUB_APP_PRIVATE_KEY_FILE`, and
  `GITHUB_APP_PRIVATE_KEY_PATH`.
- Shell syntax was passed directly after `--`. Use `-- sh -c '...'` for pipes,
  redirects, aliases, shell functions, and grouped commands.

## Operational Notes

- Use `app-run` for ordinary agent GitHub work; do not export `GH_TOKEN`
  manually.
- Treat `app-auth` as a diagnostic command for app-based authentication behavior
  unless a non-agent integration explicitly needs token stdout.
- Scope tokens with `--repo OWNER/REPO` whenever possible.
- Request only the permissions needed by the child command.
- Do not use `--private-key` in shell commands; it can leak through shell
  history or process listings. Prefer `--private-key-file` or
  `--private-key-path`.
- Do not log stdout from `toolbox github app-auth`; it may be the token.
- Do not persist temporary app tokens with `gh auth login` unless explicitly
  required and cleaned up afterward.
- Treat the GitHub App private key as a high-value secret. A short-lived
  installation token does not make the private key safe to expose.
- Review the GitHub App installation permissions and repository access. A token
  inherits those permissions, so a compromised token can still perform any
  allowed write actions during its lifetime.
- `--jwt-only` is for debugging app authentication only. A JWT can mint
  installation tokens while valid.
- The HTTP client uses a finite timeout, so automation should fail instead of
  hanging indefinitely.
- The JWT is intentionally short-lived and remains below GitHub's 10-minute
  maximum lifetime.
- Public release downloads can be tested without authentication. GitHub App auth
  can only be fully tested against a repository where the App is installed.
- Run `toolbox github app-run --help` before changing scripts; the help output
  is the command contract for agent usage.
