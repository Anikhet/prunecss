# Releasing prunecss

This guide is for maintainers. It explains how a release happens, what you
must set up once, and what to do when something goes wrong.

## How it works

```text
PR merged to main ──► release-plz opens/updates a "release" PR
                      (version bump + CHANGELOG.md)
                                │
               you review, edit the changelog, merge
                                │
                                ▼
        release-plz publishes prunecss-core, then prunecss, to crates.io
        (Trusted Publishing: no API token stored anywhere)
        and creates tags prunecss-core-vX.Y.Z / prunecss-vX.Y.Z
        plus a GitHub release for each
                                │
                                ▼
        release-binaries.yml builds the CLI for 6 platforms and attaches
        archives, SHA256SUMS and signed build provenance to the
        prunecss-vX.Y.Z GitHub release
```

The pieces:

| File | What it does |
| --- | --- |
| `.github/workflows/release-plz.yml` | Opens the release PR; publishes when it is merged. |
| `release-plz.toml` | release-plz settings: tags, changelog format, version group. |
| `.github/workflows/release-binaries.yml` | Builds, checksums, attests and uploads the binaries. |
| `CHANGELOG.md` | One changelog for both crates, in [Keep a Changelog] format. |

Both crates always share one version number (`version.workspace = true` in
`Cargo.toml`, and a `version_group` in `release-plz.toml`).

## One-time setup

Do these steps once, in order. Steps 1 to 4 publish the first version by
hand; crates.io only lets Trusted Publishing update crates that already
exist.

### 1. Check that the crates are ready

From an up-to-date `main` with a clean working tree:

```bash
cargo publish --workspace --dry-run --locked
```

This builds both `.crate` files exactly as crates.io will receive them. It
must finish without errors. (CI runs the same check as the `Package` job.)

Published versions are permanent. You can *yank* a broken version, which
stops new projects from picking it, but you can never delete it or reuse the
number.

### 2. Get a short-lived crates.io token

1. Sign in to <https://crates.io> with GitHub and verify your email address
   (Account Settings).
2. Go to Account Settings, API Tokens, New Token. Give it the
   `publish-new` and `publish-update` scopes and the shortest expiry offered.
3. Run `cargo login` and paste the token.

### 3. Publish 0.1.0 by hand

First finish the 0.1.0 notes in `CHANGELOG.md`, in a PR merged to `main`:

- Move everything under `## [Unreleased]` into the `## [0.1.0]` section.
- Replace `Unreleased` in the `## [0.1.0] - Unreleased` heading with the
  publish date, for example `## [0.1.0] - 2026-12-01`.
- Leave the `## [Unreleased]` heading above it, empty.

release-plz inserts each new release right below `## [Unreleased]`, so
anything left under that heading would end up inside the next generated
release section.

The library has to exist before the CLI that depends on it:

```bash
cargo publish -p prunecss-core --locked
cargo publish -p prunecss --locked
```

(`cargo publish --workspace --locked` does the same in the right order.)

Then tag the release so release-plz and the changelog know where 0.1.0 is:

```bash
git tag prunecss-core-v0.1.0
git tag prunecss-v0.1.0
git push origin prunecss-core-v0.1.0 prunecss-v0.1.0
```

Pushing `prunecss-v0.1.0` starts the **Release binaries** workflow. Because
no GitHub release exists yet, it creates a *draft* release with the binaries
attached. Open it under Releases, write a short description and publish it.

### 4. Throw the token away

Run `cargo logout`, then delete the token on crates.io (Account Settings, API
Tokens, Revoke). From now on nothing needs a crates.io token.

### 5. Create the `release` environment on GitHub

Repository Settings, Environments, New environment, name it exactly
`release`.

- Under **Deployment branches and tags**, choose "Selected branches and
  tags" and add `main`. Only workflows running on `main` can then use it.
- Optional but recommended: add yourself under **Required reviewers**. Every
  publish then waits for you to click "Approve" in the Actions run, which is a
  last chance to stop a release. Ordinary merges do not ask: a first job,
  **Release-plz check**, runs without the environment and starts the gated
  **Release-plz release** job only when the workspace version has no
  `prunecss-core-vX.Y.Z` or `prunecss-vX.Y.Z` tag yet, that is after a
  release PR is merged or after a publish that stopped halfway.

### 6. Turn on Trusted Publishing on crates.io

Do this for **both** crates, `prunecss-core` and `prunecss`:

1. Open the crate on crates.io, then Settings, Trusted Publishing, Add.
2. Choose GitHub and fill in:
   - Repository owner: `Anikhet`
   - Repository name: `prunecss`
   - Workflow filename: `release-plz.yml`
   - Environment: `release`
3. Save.

Then, on the same page, turn on the setting that **requires** Trusted
Publishing (crates.io added this in January 2026; the exact label may
differ). After that, even a leaked token cannot publish these crates.

How it works: when the release job runs, GitHub gives it a signed identity
token (that is what `id-token: write` in the workflow allows). release-plz
swaps it with crates.io for a publish token that lasts a few minutes. The
environment check means no other workflow or job in the repository can do
this, even though they can also ask GitHub for an identity token.

### 7. Allow release-plz to open pull requests

Repository Settings, Actions, General, **Workflow permissions**:

- Leave "Read repository contents and packages permissions" selected. Each
  workflow asks for the extra permissions it needs, job by job.
- Tick **Allow GitHub Actions to create and approve pull requests**.

Also create two labels (Issues, Labels, New label): `release` (used on the
release PR) and `dependencies` (used by Dependabot).

### 8. Protect `main` with a ruleset

Repository Settings, Rules, Rulesets, New ruleset, New branch ruleset:

- Name: `main`. Enforcement status: Active.
- Target branches: Add target, "Include default branch".
- Tick:
  - **Restrict deletions**
  - **Block force pushes**
  - **Require a pull request before merging**
  - **Require status checks to pass**, and add the check named
    **`CI success`**. That one job waits for every other CI job, so you
    never have to update the ruleset when CI changes.
- Create.

Optionally, under Settings, General, Pull Requests, allow only **squash
merging**. The PR title then becomes the commit message, which is what the
changelog is built from (see below).

## The normal release flow

### Write good PR titles

The changelog is generated from commit messages, using [Conventional
Commits] prefixes:

| Prefix | Changelog section |
| --- | --- |
| `feat: ...` | Added |
| `perf: ...`, `refactor: ...` | Changed |
| `deprecate: ...` | Deprecated |
| `remove: ...` | Removed |
| `fix: ...` | Fixed |
| `security: ...` | Security |
| `chore(deps): ...`, `build(deps): ...` | Dependencies |
| `ci:`, `test:`, `docs:`, `chore:`, `build:`, `style:` | not listed |
| anything else | Changed |

A commit whose body mentions "security" is listed under Security whatever
its type. Dependabot's cargo PRs use `chore(deps)`, so a Lightning CSS
upgrade always shows up in the release PR's changelog.

Add `!` for a breaking change (`feat!: rename --out to --output`). It is
always listed and marked **breaking**. While the version is `0.x`, a breaking
change bumps the minor version (0.2.3 to 0.3.0); anything else bumps the patch
version.

Raising the minimum supported Rust version (`rust-version`) counts as a
breaking change here: use `feat!:` or `fix!:`, so it gets a minor bump and a
changelog entry.

### 1. Review the release PR

After every merge to `main`, release-plz opens or updates a PR called
something like `chore: release v0.2.0`. It contains the version bump and the
new `CHANGELOG.md` section. It also runs `cargo-semver-checks` on
`prunecss-core` and warns in the PR if the public API broke without a
matching version bump.

Before merging:

- **Edit the changelog.** The generated text is a draft, not release notes.
  Merge duplicate lines, reword for users, and drop noise. Push your edits
  as a commit to the release PR branch, and do it last, right before merging:
  if anything else is merged to `main` afterwards, release-plz closes a
  release PR that has your commits in it and opens a fresh one, and you would
  have to copy your edits over from the closed PR.
- **Run CI on it.** Pull requests opened by a workflow do not start other
  workflows (a GitHub rule that stops endless loops), so CI does not run on
  the release PR by itself and the `CI success` check stays pending. Close
  the PR and reopen it: that counts as your action and starts CI. (To make
  this automatic later, give release-plz a GitHub App token; see
  <https://release-plz.dev/docs/github/token>.)

### 2. Merge it

When `CI success` is green, merge the release PR. Then watch the
**Release-plz** run under Actions:

1. **Release-plz check** sees that the new version is not tagged yet.
   **Release-plz release** then waits for your approval if you added a
   required reviewer to the `release` environment, publishes
   `prunecss-core` and `prunecss`, pushes the two tags and creates the two
   GitHub releases.
2. **Release binaries** then builds the CLI for:

   | Target | Built on |
   | --- | --- |
   | `x86_64-unknown-linux-gnu` | Ubuntu 24.04 (x86_64) |
   | `x86_64-unknown-linux-musl` (static, any Linux) | Ubuntu 24.04 (x86_64) |
   | `aarch64-unknown-linux-gnu` | Ubuntu 24.04 (Arm) |
   | `x86_64-apple-darwin` | macOS (Apple silicon, cross-compiled) |
   | `aarch64-apple-darwin` | macOS (Apple silicon) |
   | `x86_64-pc-windows-msvc` | Windows |

   It checks that each binary it can run reports the tagged version, packs
   it with the licences, README and changelog (`.tar.gz`, or `.zip` on
   Windows), writes `SHA256SUMS`, signs build provenance, and uploads
   everything to the `prunecss-vX.Y.Z` release.

### 3. Check the result

- The new versions appear on crates.io and docs.rs.
- The GitHub release has six archives and `SHA256SUMS`.
- Provenance verifies:

  ```bash
  gh release download prunecss-v0.2.0 --repo Anikhet/prunecss
  sha256sum -c SHA256SUMS
  gh attestation verify prunecss-0.2.0-x86_64-unknown-linux-gnu.tar.gz --repo Anikhet/prunecss
  ```

- Each binary carries its dependency list (built with `cargo auditable`), so
  anyone can scan the exact binary for known vulnerabilities:

  ```bash
  cargo audit bin prunecss
  ```

## When something goes wrong

**Publishing stopped halfway** (for example `prunecss-core` went out but
`prunecss` failed). Fix the cause, then re-run the failed Release-plz job.
release-plz skips versions that are already on crates.io and publishes the
rest.

**The binaries failed or are missing.** Actions, Release binaries, Run
workflow, and enter the tag (for example `prunecss-v0.2.0`). It rebuilds from
that tag and replaces the assets on the release.

**A release is broken.** Yank it so new users do not get it, then release a
fix:

```bash
cargo yank --version 0.2.0 prunecss
```

(Yanking needs a token with the `yank` scope: create a short-lived one, use
it, revoke it.) Mark the GitHub release as a pre-release or edit its notes to
point to the fix.

**"Trusted Publishing" errors in the release job.** The crates.io settings
must match exactly: owner `Anikhet`, repository `prunecss`, workflow
`release-plz.yml`, environment `release`. Renaming the workflow file or the
environment breaks publishing until crates.io is updated too.

## Related checks

- `.github/workflows/ci.yml` runs on every PR: formatting, Clippy, tests on
  Linux, macOS and Windows (plus beta, allowed to fail), the 1.88 MSRV build,
  docs, snapshots, coverage gates, `cargo deny`, `cargo package` and a lint of
  the workflow files themselves (zizmor and actionlint).
- `.github/workflows/audit.yml` checks the locked dependencies against the
  RustSec advisory database every day. If it fails, read the advisory and
  update the affected crate, or add a justified `ignore` to `deny.toml`.
- `.github/dependabot.yml` proposes grouped dependency and action updates
  every Monday. Lightning CSS always gets its own PR: it is pinned to an exact
  alpha version, and an upgrade can change CSS output or raise the MSRV, so
  review its changelog and the snapshot diffs before merging. The same PR
  updates the pin in `fuzz/Cargo.toml`, which has its own lockfile.

[Keep a Changelog]: https://keepachangelog.com/en/1.1.0/
[Conventional Commits]: https://www.conventionalcommits.org/en/v1.0.0/
