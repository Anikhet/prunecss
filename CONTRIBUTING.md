# Contributing to prunecss

Thanks for your interest in prunecss. This guide explains what the project is
for, how to set up a development environment, and what a good issue or pull
request looks like.

prunecss is maintained by one person in their spare time. I aim to reply to
issues and pull requests within one to two weeks. Please be patient, and
please read this guide first: it saves us both time.

Everyone taking part must follow the [Code of Conduct](CODE_OF_CONDUCT.md).
For security problems, see [SECURITY.md](SECURITY.md) and **do not** open a
public issue. For questions, see [SUPPORT.md](SUPPORT.md).

## Scope and non-goals

prunecss removes CSS rules that can never match the class names used in your
content files. It aims to be a much faster, drop-in alternative to PurgeCSS.

It **will**:

- scan HTML, JSX and template files for class names, without running them;
- remove rules and single selectors that need a class or id nobody uses;
- stay conservative: when it is not sure a rule is unused, it keeps it;
- grow a PurgeCSS-compatible config and safelist;
- later ship as an npm package, Vite and PostCSS plugins, and a language
  server for editors. All of them will wrap the same Rust core.

It **will not**:

- run your JavaScript or render your pages in a browser;
- minify, merge or rewrite CSS beyond what pruning needs (use a minifier such
  as Lightning CSS after prunecss);
- lint CSS or check that it is valid;
- add a plugin system for arbitrary user code;
- use async runtimes in the core library. See
  [ADR 0004](docs/adr/0004-rayon-not-async.md).

If you are not sure whether an idea fits, open an issue and ask.

## Before you start

- **Bugs:** open an issue using one of the forms. Small, obvious fixes (typos,
  broken links, a clear one-line bug fix) can go straight to a pull request.
- **Features and larger changes: open an issue first** and wait for a reply
  before you write the code. A new feature is something I will maintain for
  years, so I may say no, or suggest a different design. Pull requests for
  features that were not discussed may be closed.
- Issues labelled `good first issue` or `help wanted` are open to anyone.
  Leave a comment saying you are working on one, so two people don't do the
  same work.
- Issues labelled `needs-decision` are not ready for code yet.

[ARCHITECTURE.md](ARCHITECTURE.md) gives a short tour of the code. Past design
decisions are recorded in [docs/adr/](docs/adr/).

## Development setup

Install Rust with [rustup](https://rustup.rs/). The repository pins the
toolchain it is developed with in `rust-toolchain.toml`, so rustup downloads
the right version on the first `cargo` command. The code itself must keep
building on **Rust 1.88** (the minimum supported Rust version); CI checks
that.

```bash
git clone https://github.com/Anikhet/prunecss
cd prunecss
cargo test
```

Run the tool from source:

```bash
cargo run -- scan index.html App.jsx
cargo run -- prune styles.css --content index.html App.jsx --stats
```

### Checks that must pass

CI runs these on every pull request. Run them locally before you push:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test
typos
```

With [just](https://github.com/casey/just) installed, `just check` runs all
four, and `just` lists the other recipes (`just ci` runs nearly everything CI
does). [typos](https://github.com/crate-ci/typos) is a spell checker:
`cargo install typos-cli`. A word it flags wrongly goes in `_typos.toml`.

Optional tools:

```bash
cargo install cargo-insta cargo-llvm-cov

cargo insta review   # accept or reject changed snapshots
cargo llvm-cov       # measure test coverage
```

Benchmarks and timing must always use `--release`.

## Code style

The full rules are in [CLAUDE.md](CLAUDE.md). The most important ones:

- Borrow instead of cloning. Take `&str` and `&[T]` in parameters.
- No `unwrap()`, `expect()` or `panic!` outside tests. Return `Result`.
- The core library never prints and never iterates a `HashSet` or `HashMap`
  where the order reaches the output. Output must be the same on every run.
- Every public item has a `///` doc comment. `//` comments explain *why*.
- Every `TODO` links an issue: `// TODO(#42): ...`.
- Use `#[expect(clippy::lint, reason = "...")]`, never `#[allow(...)]`.

Clippy enforces most of these, so `cargo clippy` will tell you.

## Tests

prunecss deletes code from people's stylesheets, so tests matter more than
usual. We work test-first:

1. **Red:** write a test for the behaviour you want, and run it to see it
   fail. A `todo!()` body is fine as a placeholder.
2. **Green:** write the simplest code that makes it pass.
3. **Refactor:** tidy up while the tests stay green.

Test style:

- Unit tests live in a `#[cfg(test)] mod tests` block next to the code.
  Integration tests in `crates/prunecss-cli/tests/` run the real binary
  against sample files in `crates/prunecss-cli/tests/fixtures/`.
- Name tests by behaviour: `extract_classes_should_ignore_data_attributes`.
- One behaviour per test. Prefer `assert_eq!` over `assert!`.
- Use `rstest` for table-style cases, `proptest` for rules that must hold for
  any input (never panics, never invents rules), and `insta` snapshots for
  whole CSS outputs.
- No mocking libraries. Use real CSS and HTML text.
- Test error paths by checking the `Result` (for example with `matches!`),
  not with `#[should_panic]`.
- Coverage: CI fails below 95% line coverage for `prunecss-core` and 80%
  for the whole workspace (`just coverage`). Aim for 100% on the matching and
  pruning logic, because it decides what gets deleted.

### Snapshots

Snapshot files live in `crates/prunecss-cli/tests/snapshots/`. When a change
alters output on purpose, run `cargo insta review`, look at every diff, and
commit the updated `.snap` files. If a snapshot changed and you did not
expect it to, that is a bug: don't accept it.

## Reporting CSS that was wrongly removed

A "false removal" is the worst kind of prunecss bug: your page needed a rule
and prunecss deleted it. Please report it with the
["CSS was wrongly removed" form](https://github.com/Anikhet/prunecss/issues/new?template=false_removal.yml).

The most useful report is a **minimal reproduction**:

1. Start from the files that broke.
2. Delete content and CSS rules until the problem only just still happens.
   Aim for a few lines of HTML or JS and a few lines of CSS.
3. Paste both, plus the exact command, the output you got and the output you
   expected.

A report could look like this (the exact case is made up):

```html
<!-- index.html -->
<div class="md:flex"></div>
```

```css
/* styles.css */
.md\:flex { display: flex }
```

```bash
prunecss prune styles.css --content index.html
```

Expected: the rule is kept. Actual: it was removed.

A small case like this usually becomes a regression test directly.

## Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/). The
changelog and version numbers are generated from commit messages by
release-plz, so the format matters.

```text
<type>: <short summary in the imperative>

<optional body: what changed and why>
```

Types, and the changelog section each one lands in (this mirrors
`commit_parsers` in [`release-plz.toml`](release-plz.toml)):

| Type | Use for | Changelog section |
| --- | --- | --- |
| `feat` | a new feature | Added |
| `perf` | a speed or memory improvement | Changed |
| `refactor` | a code change with no behaviour change | Changed |
| `deprecate` | marking something for later removal | Deprecated |
| `remove` | removing a feature or option | Removed |
| `fix` | a bug fix | Fixed |
| `security` | a security fix | Security |
| `chore(deps)`, `build(deps)` | dependency updates (Dependabot) | Dependencies |
| `docs` | documentation only | not listed |
| `ci`, `test`, `style`, `chore`, `build` | CI, tests, formatting, tooling | not listed |
| anything else | | Changed |

A commit whose body mentions "security" is listed under Security whatever
its type.

Add `!` after the type for a breaking change, and explain it in the body:

```text
feat!: rename `PruneOutput::css` to `PruneOutput::code`
```

An optional scope is welcome: `fix(scanner): keep slashes inside class names`.

Write the summary for a user reading the changelog: "fix: keep rules for
escaped Tailwind class names" is better than "fix: bug in pruner".

## Pull request checklist

Before you open a pull request:

- [ ] There is an issue for it, unless it is a small fix.
- [ ] Tests were written first and cover the change.
- [ ] `cargo fmt --check`, `cargo clippy ... -D warnings`, `cargo test` and
      `typos` pass (`just check`).
- [ ] Changed snapshots were reviewed with `cargo insta review`.
- [ ] Public items have doc comments.
- [ ] Commit messages follow Conventional Commits.
- [ ] Any AI assistance is disclosed (see below).

Keep pull requests small and focused on one thing. They are much easier to
review.

## AI-assisted contributions

**The maintainer builds prunecss with AI assistance.** Much of the code is
written together with Claude Code, an AI coding tool. The project's
instructions for AI agents are in [AGENTS.md](AGENTS.md), and commits it helped with
carry a `Co-Authored-By` line. I review every change, and every change must
pass the same tests, lints and checks described above.

So AI tools are welcome here. These rules apply to everyone:

- **Disclose it.** Say in the pull request which tool you used and how much:
  for example "Tests written by hand; implementation drafted with an AI tool
  and then edited", or "Mostly written by an AI coding agent". The pull
  request template has a line for this.
- **You own every line.** You must understand, test and be able to explain
  every change you submit. "The AI wrote it" is not an answer to a review
  question.
- **Write your own words.** Write issues, pull request descriptions and
  review replies yourself. If English is not your first language, using a
  translation tool is fine.
- **No autonomous agents.** Do not let an agent or bot open issues or pull
  requests, or post comments, on its own. A human must review and submit
  each one.
- **Check AI-found bugs.** If a tool found a bug or security issue, reproduce
  it yourself and include a minimal case before you report it.
- **Low-effort work is closed.** Pull requests or issues that look generated
  and unreviewed, or that ignore this guide, may be closed without a detailed
  review.

This policy is based on those of Astral, Biome, oxc, Ghostty and curl.
[AI_POLICY.md](AI_POLICY.md) has a short summary.

## License

prunecss is dual-licensed under MIT or Apache-2.0, at your option. Unless you
say otherwise, any contribution you submit is licensed the same way, as
defined in the Apache-2.0 license, with no additional terms or conditions.
