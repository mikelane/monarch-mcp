# Releasing monarch-mcp

## Overview

Releases are fully automated via [release-plz](https://release-plz.dev) +
[cargo-dist](https://opensource.axo.dev/cargo-dist/). The only human action
required is **merging the Release PR** that release-plz opens.

```
merge a feat:/fix:/etc. commit to main
          │
          ▼
  release-plz detects unreleased changes
          │
          ▼
  release-plz opens (or updates) a Release PR:
    • bumps version in Cargo.toml
    • updates CHANGELOG.md from Conventional Commits
          │
          ▼
  you review + merge the Release PR
          │
          ▼
  release-plz (on merge):
    • publishes the crate to crates.io
    • pushes a git tag  (e.g. v0.2.0)
          │
          ▼
  cargo-dist's release.yml triggers on the new tag:
    • cross-compiles for all 5 platforms
    • produces shell + PowerShell one-line installers
    • creates the GitHub Release with binaries + checksums
```

## Required repository secrets

Set these in **Settings → Secrets and variables → Actions**:

### `CARGO_REGISTRY_TOKEN`

A crates.io API token with the **publish-new** and **publish-update** scopes.

1. Log in to [crates.io](https://crates.io) → Account Settings → API Tokens.
2. Create a token scoped to `publish-new` + `publish-update`.
3. Add it as `CARGO_REGISTRY_TOKEN` in GitHub Actions secrets.

### `RELEASE_PLZ_TOKEN`

A GitHub **Personal Access Token (classic)** — or a fine-grained PAT — with at
minimum:

| Scope | Why |
|---|---|
| `repo` (or `contents: write` + `pull-requests: write` for fine-grained) | Open/update the Release PR, push the git tag |

> **Why not `GITHUB_TOKEN`?**
> GitHub explicitly prevents `GITHUB_TOKEN`-opened pull requests from triggering
> other workflow runs (e.g. CI) to avoid infinite loops. A Release PR opened with
> `GITHUB_TOKEN` would sit with no CI checks — and merge-guardian would block it.
> Using a PAT makes the PR appear as if a human opened it, so CI fires normally.

To create a classic PAT:

1. GitHub → Settings → Developer settings → Personal access tokens → Tokens (classic).
2. Tick the `repo` scope.
3. Add it as `RELEASE_PLZ_TOKEN` in GitHub Actions secrets.

## How release-plz and cargo-dist divide responsibilities

| Tool | Owns |
|---|---|
| release-plz | Version bump · CHANGELOG · crates.io publish · git tag |
| cargo-dist | Binary builds · shell/PowerShell installers · GitHub Release |

The key configuration that keeps them from fighting:

- `release-plz.toml` sets `git_release_enable = false` — release-plz pushes the
  tag but does **not** create the GitHub Release.
- `dist-workspace.toml` sets `ci = "github"` — cargo-dist's generated
  `.github/workflows/release.yml` fires on `push: tags: '**[0-9]+.[0-9]+.[0-9]+*'`,
  which is exactly the tag release-plz pushes.

## Install methods after a release

```bash
# One-line shell installer (macOS / Linux)
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/mikelane/monarch-mcp/releases/latest/download/monarch-mcp-installer.sh | sh

# PowerShell (Windows)
irm https://github.com/mikelane/monarch-mcp/releases/latest/download/monarch-mcp-installer.ps1 | iex

# Cargo (from crates.io)
cargo install monarch-mcp
```

## Homebrew

There is intentionally **no Homebrew tap**. Tap-free Homebrew (`brew install monarch-mcp`
without a tap) requires submission to [homebrew-core](https://github.com/Homebrew/homebrew-core),
which has a 30-day release history and 75-star notability threshold. That submission is a
future manual step once the repo meets the criteria — it is out of scope for this automation.

## Troubleshooting

**Release PR has no CI checks.**
The `RELEASE_PLZ_TOKEN` secret is probably set to a `GITHUB_TOKEN` value or is missing.
Replace it with a PAT (see above).

**crates.io publish fails with "invalid token".**
Regenerate the crates.io token and update `CARGO_REGISTRY_TOKEN`.

**cargo-dist release.yml didn't fire after the Release PR merged.**
Check that release-plz pushed a tag (`git tag -l`). If the tag is missing, release-plz's
publish step may have failed — check the release-plz job logs on the merge commit.

**"package `monarch-mcp` cannot be published to crates.io" error.**
Confirm `publish` is not set to `false` in `Cargo.toml`. It is currently unset (defaults to
allowed).
