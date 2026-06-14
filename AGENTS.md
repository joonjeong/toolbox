# AGENTS.md

## Project Intent

`toolbox` is a personal collection of general-purpose tools that should be easy
to distribute as Rust binaries attached to GitHub releases.

The repository is intentionally a Rust monorepo from the beginning. Most tools
are expected to be CLI or TUI programs. Keep new work compatible with both:

- busybox-style invocation, for example `toolbox github app-auth`
- direct subcommand invocation, for example `toolbox github-app-auth`
- symlink shim invocation, for example `github-app-auth` pointing at `toolbox`

Prefer small, focused crates under `crates/` over large unrelated modules in one
place. The current entrypoint crate is `crates/toolbox`.

## Current Tool Surface

The first supported command is GitHub App authentication for coding agents that
need to work on issues or pull requests:

```sh
toolbox github app-auth ...
toolbox github-app-auth ...
github-app-auth ...
```

The command signs a GitHub App JWT, exchanges it for an installation token, and
prints the token or a shell export statement. Preserve these behaviors when
refactoring.

Important details:

- `--repository OWNER/REPO` is user-facing, but GitHub's installation token API
  expects only repository names in the `repositories` field.
- The blocking HTTP client should have a finite timeout so automation does not
  hang indefinitely.
- GitHub App JWTs must stay below GitHub's 10-minute maximum lifetime. Account
  for clock skew when changing `iat` and `exp`.
- CLI parsing should defensively handle unusual argv shapes, including an empty
  argv passed by tests or low-level process execution.

## Release Policy

Release distribution is handled by GitHub Actions:

- Release workflow runs must use LINE HeadVer tags only. Do not add alternate
  tag/version calculation paths for releases.
- The weekly release workflow runs every Sunday at 10:00 KST using cron
  `0 1 * * 0`.
- Weekly automated releases use LINE HeadVer: `{head}.{yearweek}.{build}`.
  Keep the head value at `0` until the user explicitly changes it. Calculate
  `{yearweek}` with ISO week-year/week in KST, and use the GitHub Actions run
  number as `{build}`. Do not reinterpret HeadVer as semantic compatibility
  versioning or commit-count versioning.
- Use `scripts/headver` for HeadVer calculation instead of duplicating date
  logic in workflow YAML.
- Use `scripts/release-metadata` to prepare release tags, titles, notes, target
  commits, and asset suffixes instead of building those values inline in
  workflow YAML.
- Keep release build/upload logic in `.github/workflows/release.yml`. If
  `.github/workflows/weekly-release.yml` exists, it should pass the HeadVer
  inputs to `release.yml` instead of duplicating metadata generation or the
  release build matrix.

When changing release workflows, keep Linux x86_64, macOS x86_64, and macOS
aarch64 assets working unless the user explicitly changes the support matrix.

## Development Commands

Run these before publishing code changes:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For workflow-only changes, also parse or otherwise validate the edited YAML when
possible.

## Engineering Preferences

- Follow existing Rust workspace conventions and keep changes scoped.
- Prefer explicit, testable helper functions for behavior that can regress, such
  as command dispatch or API request normalization.
- Keep comments sparse and useful. Comments should explain protocol constraints
  or non-obvious edge cases, not restate straightforward code.
- Keep `Cargo.lock` committed; this repository builds distributable binaries.
- Do not silently revert user-authored commits or GitHub web edits. Fetch and
  fast-forward before adding new work to a PR branch when the remote moved.

## PR Workflow

When updating a PR:

- Inspect the current branch and remote branch first.
- Stage only files that are part of the requested scope.
- Use concise commits that describe the behavior changed.
- If review comments were accepted and fixed, resolve the corresponding GitHub
  review threads after verifying the fix.
- Update the PR title and body when the scope changes materially.
