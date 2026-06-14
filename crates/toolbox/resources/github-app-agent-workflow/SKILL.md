---
name: github-app-agent-workflow
description: Perform GitHub agent work through toolbox GitHub App authentication, using short-lived installation token sessions with gh safely. Use when an agent or automation needs to work on issues, pull requests, releases, or repository API calls through GitHub App credentials while avoiding personal access tokens, unnecessary token minting, persistent gh login, shell history leaks, or accidental token logging.
---

# GitHub App Agent Workflow

Use the `toolbox` binary to sign a GitHub App JWT and exchange it for an
installation access token, then pass that token to `gh` through a bounded
environment session. Prefer this skill when GitHub access should come from a
GitHub App installation rather than a personal token.

Treat the auth flow as sensitive. The command can print valid temporary tokens
or JWTs, and those values can leak through logs, shell tracing, process
environments, persistent `gh` auth state, or careless copy/paste.

Also avoid minting tokens more often than necessary. Create one installation
token per coherent task or repository scope, reuse it for the related `gh`
commands, then let the subshell or CI step end so the token leaves the
environment.

## Command Forms

Use any supported invocation form:

```sh
toolbox github app-auth [OPTIONS]
toolbox github-app-auth [OPTIONS]
github-app-auth [OPTIONS] # when this name is symlinked to the toolbox binary
```

When scripting for portability, prefer `toolbox github app-auth`.

To create this skill in another agent's skills directory, run:

```sh
toolbox github agent-skill --output-path /path/to/skills
```

## Required Inputs

Provide:

- `--app-id` or `GITHUB_APP_ID`
- `--installation-id` or `GITHUB_APP_INSTALLATION_ID`
- exactly one private key source:
  - `--private-key-file` or `GITHUB_APP_PRIVATE_KEY_FILE`
  - `--private-key` or `GITHUB_APP_PRIVATE_KEY`

Prefer `--private-key-file` in shell commands so PEM contents do not appear in
shell history or process listings.

## Preferred `gh` Workflows

Prefer a bounded token session for a coherent task. Mint once, run the related
`gh` commands, then leave the subshell:

```sh
(
  set +x
  export GH_TOKEN="$(
    toolbox github app-auth \
      --repository OWNER/REPO \
      --app-id "$GITHUB_APP_ID" \
      --installation-id "$GITHUB_APP_INSTALLATION_ID" \
      --private-key-file /path/to/private-key.pem
  )"
  gh pr view 123 --repo OWNER/REPO
  gh pr checks 123 --repo OWNER/REPO
  gh pr diff 123 --repo OWNER/REPO --name-only
)
```

For issue or PR triage, reuse the same token for the whole read-only pass:

```sh
(
  set +x
  export GH_TOKEN="$(
    toolbox github app-auth \
      --repository OWNER/REPO \
      --app-id "$GITHUB_APP_ID" \
      --installation-id "$GITHUB_APP_INSTALLATION_ID" \
      --private-key-file /path/to/private-key.pem
  )"
  gh issue list --repo OWNER/REPO
  gh pr list --repo OWNER/REPO
  gh pr checks 123 --repo OWNER/REPO
)
```

For a write workflow, mint once for the smallest repository scope that covers
the operation:

```sh
(
  set +x
  export GH_TOKEN="$(
    toolbox github app-auth \
      --repository OWNER/REPO \
      --app-id "$GITHUB_APP_ID" \
      --installation-id "$GITHUB_APP_INSTALLATION_ID" \
      --private-key-file /path/to/private-key.pem
  )"
  gh pr comment 123 --repo OWNER/REPO --body-file /tmp/comment.md
  gh pr edit 123 --repo OWNER/REPO --add-label automation
)
```

Use the token for direct GitHub API calls through `gh api` in the same session:

```sh
(
  set +x
  export GH_TOKEN="$(
    toolbox github app-auth \
      --repository OWNER/REPO \
      --app-id "$GITHUB_APP_ID" \
      --installation-id "$GITHUB_APP_INSTALLATION_ID" \
      --private-key-file /path/to/private-key.pem
  )"
  gh api repos/OWNER/REPO/actions/runs --jq '.workflow_runs[0].status'
  gh api repos/OWNER/REPO/releases/latest --jq '.tag_name'
)
```

Use `GITHUB_TOKEN` instead of `GH_TOKEN` only when a tool specifically requires
that name. Keep the same bounded-session pattern:

```sh
(
  set +x
  export GITHUB_TOKEN="$(
    toolbox github app-auth \
      --repository OWNER/REPO \
      --app-id "$GITHUB_APP_ID" \
      --installation-id "$GITHUB_APP_INSTALLATION_ID" \
      --private-key-file /path/to/private-key.pem
  )"
  gh release view --repo OWNER/REPO
)
```

Pass `OWNER/REPO` for readability. The command sends only repository names to
GitHub's installation token API, as required by GitHub.

## Session Reuse Rules

- Mint once per coherent task, not once per `gh` command.
- Reuse the token inside a single subshell, CI step, or tightly scoped process
  environment.
- Keep the session no broader than the repository set and operation class
  required for the task.
- Start a new token session when repository scope changes, privileges need to be
  narrower, the token may have leaked, or a command fails due to expiration.
- Do not cache installation tokens on disk. If reuse must cross process
  boundaries, prefer a secret manager or CI secret-masking mechanism with a
  clear cleanup path.
- Do not keep one long-lived interactive shell loaded with `GH_TOKEN` just for
  convenience. The goal is fewer token exchanges without widening exposure.

## Direct Token Output

Avoid direct token output unless the caller has a concrete reason and a
secret-safe destination. This prints a valid installation token to stdout:

```sh
toolbox github app-auth \
  --repository OWNER/REPO \
  --app-id "$GITHUB_APP_ID" \
  --installation-id "$GITHUB_APP_INSTALLATION_ID" \
  --private-key-file /path/to/private-key.pem
```

`--shell` prints an export statement. Use it only inside a controlled subshell or
CI step where logs are masked and shell tracing is disabled, then reuse that
session for the related `gh` commands:

```sh
(
  set +x
  eval "$(toolbox github app-auth --shell \
    --repository OWNER/REPO \
    --app-id "$GITHUB_APP_ID" \
    --installation-id "$GITHUB_APP_INSTALLATION_ID" \
    --private-key-file /path/to/private-key.pem)"
  gh pr view 123 --repo OWNER/REPO
  gh pr checks 123 --repo OWNER/REPO
)
```

Avoid `gh auth login --with-token` for temporary GitHub App tokens unless the
goal is intentionally persistent local `gh` authentication. Prefer per-command
or per-subshell `GH_TOKEN`.

Print only the GitHub App JWT for debugging:

```sh
toolbox github app-auth --jwt-only \
  --app-id "$GITHUB_APP_ID" \
  --private-key-file /path/to/private-key.pem
```

Do not paste JWT output into issue comments, PR comments, build logs, or chat.

## Options To Remember

- `--api-url` or `GITHUB_API_URL`: override for GitHub Enterprise Server.
- `--shell`: print `export GITHUB_TOKEN=...` instead of the raw token.
- `--jwt-only`: do not call the installation token API.
- `--repository OWNER/REPO`: repeat to scope a token to selected repositories.

## Operational Notes

- Disable shell tracing (`set +x`) before command substitution or `eval`.
- Scope tokens with `--repository OWNER/REPO` whenever possible.
- Reuse one scoped token session for related `gh` commands instead of minting a
  new token for every API call.
- Do not use `--private-key` in shell commands; it can leak through shell
  history or process listings. Prefer `--private-key-file`.
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
- Run `toolbox github app-auth --help` before changing scripts; the help output
  is the command contract for agent usage.
