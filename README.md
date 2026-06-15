# toolbox

Personal general-purpose tools packaged as Rust binaries.

## Shape

This repository starts as a Cargo workspace so CLI/TUI tools can grow without
having to reshape the repository later.

```text
crates/
  toolbox/   busybox-style entrypoint and shared command dispatcher
```

The primary binary is `toolbox`. Commands can be used in three forms:

```sh
toolbox github app-auth ...
toolbox github app-run ... -- COMMAND [ARG]...
toolbox github-app-auth ...
toolbox github-app-run ... -- COMMAND [ARG]...
github-app-auth ... # when symlinked to the toolbox binary
github-app-run ... # when symlinked to the toolbox binary
```

## GitHub App authentication

`github app-auth` creates a GitHub App JWT and exchanges it for an installation
access token. It is intended for coding agents that need to work on GitHub
issues or pull requests as a GitHub App installation.

```sh
toolbox github app-auth \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  --private-key-file /path/to/private-key.pem
```

The command prints the installation token to stdout. For `gh`, assign that
token with shell-native command substitution:

```sh
export GH_TOKEN="$(toolbox github app-auth \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  --private-key-file /path/to/private-key.pem)"
```

Supported environment variables:

- `GITHUB_APP_ID`
- `GITHUB_APP_INSTALLATION_ID`
- `GITHUB_APP_PRIVATE_KEY_FILE`
- `GITHUB_APP_PRIVATE_KEY_PATH`
- `GITHUB_APP_PRIVATE_KEY`
- `GITHUB_API_URL`

Useful options:

- `--repo OWNER/REPO` scopes the token to a repository. Repeat `--repo` for
  multiple repositories. When `--installation-id` is omitted, the first `--repo`
  value is also used to discover the app installation.
- `--installation-id ID` skips repository installation discovery when the
  installation ID is already known.
- `--permission key=value` limits token permissions, for example
  `--permission contents=read`.
- `--format json` prints diagnostic metadata without the installation token.
- `--jwt-only` prints the signed GitHub App JWT without exchanging it.

Public release downloads can be tested without authentication. GitHub App auth
must be tested against a repository where the App is installed, even if the
repository itself is public.

`github app-run` uses the same token minting inputs as `app-auth`, but runs a
command with the temporary installation token set as both `GH_TOKEN` and
`GITHUB_TOKEN`:

```sh
toolbox github app-run \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  --private-key-file /path/to/private-key.pem \
  -- gh pr comment 123 --body "Done"
```

The command after `--` inherits stdin, stdout, stderr, the current working
directory, `PATH`, and ordinary environment variables. GitHub App credential
environment variables are removed from the child environment, so the child only
receives the scoped installation token. Shell syntax such as pipes, redirects,
aliases, and shell functions requires an explicit shell command:

```sh
toolbox github app-run \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  --private-key-file /path/to/private-key.pem \
  -- sh -c 'gh issue view 123 | jq .url'
```

`toolbox` exits with the child process exit code, so it can be used directly in
automation.

## Agent skill

The `toolbox` binary bundles a `github-app-agent-workflow` skill. It describes
how an agent can use `toolbox github app-run` with `gh` without printing,
exporting, or persisting temporary GitHub App installation tokens.

Create the bundled skill in another agent's skills directory:

```sh
toolbox github agent-skill --install-path ~/.codex/skills
```

The command writes `github-app-agent-workflow/SKILL.md` under the install path.
Use `--force` to overwrite an existing copy.

## Releases

The release workflow creates HeadVer-tagged GitHub releases and uploads
`toolbox` binaries for `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`, and `aarch64-apple-darwin`. Linux assets are
statically linked musl binaries so they do not depend on the host system's
glibc version.

The weekly release workflow runs every Sunday at 10:00 KST and creates a
[HeadVer](https://github.com/line/headver) release from the default branch. Until
the project is ready for a stable head value, automated releases use head `0` in
the form `v0.<yearweek>.<build>`. The weekly workflow only calculates the
HeadVer tag and delegates release creation, builds, and asset uploads to the
release workflow. If there are no commits after the latest merged `v*` release
tag, the weekly workflow skips the release.

HeadVer values are calculated by `scripts/headver`:

```sh
scripts/headver --head 0 --build 123 --timezone Asia/Seoul
```

The script emits `key=value` lines, including `version`, `tag`, and
`asset_suffix`, so CI can append it directly to `$GITHUB_OUTPUT`.

Release metadata is calculated by `scripts/release-metadata`, which wraps
`scripts/headver` and emits the release tag, title, notes, target commit, and
asset suffix used by `.github/workflows/release.yml`.

Weekly release change detection is calculated by `scripts/weekly-release-changes`.
It emits `should_release=false` when the target commit has no commits after the
latest merged `v*` release tag.
