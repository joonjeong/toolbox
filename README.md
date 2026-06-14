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
toolbox github-app-auth ...
github-app-auth ... # when symlinked to the toolbox binary
```

## GitHub App authentication

`github app-auth` creates a GitHub App JWT and exchanges it for an installation
access token. It is intended for coding agents that need to work on GitHub
issues or pull requests as a GitHub App installation.

```sh
toolbox github app-auth \
  --app-id "$GITHUB_APP_ID" \
  --installation-id "$GITHUB_APP_INSTALLATION_ID" \
  --private-key-file /path/to/private-key.pem
```

The command prints the installation token to stdout. For shell setup:

```sh
eval "$(toolbox github app-auth --shell \
  --app-id "$GITHUB_APP_ID" \
  --installation-id "$GITHUB_APP_INSTALLATION_ID" \
  --private-key-file /path/to/private-key.pem)"
```

Supported environment variables:

- `GITHUB_APP_ID`
- `GITHUB_APP_INSTALLATION_ID`
- `GITHUB_APP_PRIVATE_KEY_FILE`
- `GITHUB_APP_PRIVATE_KEY`
- `GITHUB_API_URL`

Useful options:

- `--repository OWNER/REPO` limits the token to one or more repositories.
- `--jwt-only` prints the signed GitHub App JWT without exchanging it.

## Releases

Pushing a `v*` tag runs the release workflow and uploads `toolbox` binaries for
Linux x86_64, macOS x86_64, and macOS aarch64 to the matching GitHub release.

The weekly release workflow runs every Sunday at 10:00 KST and creates a
headver-style release from the default branch. Until the project is ready for a
stable major version, automated releases use major version `0` in the form
`v0.<commit-count>.0`.
