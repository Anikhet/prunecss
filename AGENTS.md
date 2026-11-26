# prunecss — guidance for AI coding agents

A fast unused-CSS remover written in Rust: scan content files for the class
names they use, then remove CSS rules that can never match.

## Layout

- `crates/prunecss-core` — library, published to crates.io. All logic lives here.
- `crates/prunecss-cli` — the `prunecss` binary. Thin wrapper; errors use `anyhow`.
- Planned later: npm package via napi-rs, Vite/PostCSS plugins, then a language
  server + VS Code extension, all wrapping the same core.

## Public API rules (prunecss-core)

- Everything public is re-exported at the crate root (`prunecss_core::prune`);
  modules stay private so each item has exactly one path.
- Public structs and enums are `#[non_exhaustive]`. Options grow through
  builder methods (`PruneOptions::default().minify(true)`), never struct
  literals, so adding a field is not a breaking change.
- Never expose Lightning CSS (or other dependency) types. It is a pinned
  alpha; leaking its types would tie our semver to its releases.
- Errors are a struct with a private `kind`, a message and an optional
  1-based `SourceLocation`; convert dependency errors at the boundary.
- `extract_classes` returns `BTreeSet<Cow<str>>`: names are borrowed from the
  input except the few that only exist after decoding HTML entities.
- Output fidelity rules live in `prune`: printer targets old browsers with
  `Features::Nesting` excluded, legacy placeholder pseudo-classes are
  repaired, `@charset` is added when needed, nesting depth is capped
  (`MAX_NESTING`). Keep them when touching printing.
- Minimum supported Rust version is 1.88 (`rust-version`), checked in CI.
  Raising it is a minor-version change and needs a changelog entry.

This file is read by AI coding agents (`CLAUDE.md` just includes it). Humans
should start with CONTRIBUTING.md and ARCHITECTURE.md; the project's AI
policy is in AI_POLICY.md.

## Commands (all must pass before committing)

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test
typos
```

`just check` runs all four; `just ci` adds docs, MSRV, coverage, cargo-deny
and packaging. The toolchain is pinned in `rust-toolchain.toml`.

Benchmarks: always `--release`.

## Rust style (based on Apollo's Rust Best Practices handbook)

- Borrow instead of cloning. Take `&str` / `&[T]` in parameters; return borrowed
  data (`&str` slices of the input) where lifetimes allow. Use `Cow` when
  ownership is ambiguous.
- No `unwrap()` / `expect()` / `panic!` outside tests. Return `Result` and use `?`.
  Clippy enforces this (`clippy.toml` allows them inside tests).
- The library never prints (`print_stdout`/`print_stderr` lints); only the CLI
  does, via a crate-level `#[expect]`.
- Never iterate a `HashSet`/`HashMap` where order reaches output
  (`iter_over_hash_type`): CSS output and snapshots must be deterministic.
- Prefer iterators over manual loops; avoid needless intermediate `.collect()`.
- Prefer generics (static dispatch); use `dyn Trait` only for heterogeneous
  collections, boxed at API boundaries.
- Use `#[expect(clippy::lint, reason = "...")]` instead of `#[allow(...)]`.
- `//` comments explain *why*; `///` docs explain *what/how* for every public item.
  Every `TODO` links an issue: `// TODO(#42): ...`.
- Consider the type-state pattern for the scan → match → prune pipeline once it
  stabilises.

## Concurrency

The work is CPU-bound (reading and parsing files), so use **rayon** for
parallelism. Do **not** add Tokio/async to the core. Async belongs only in the
future language-server layer, which waits on editor messages.

## Testing (TDD)

- Red → green → refactor: write the failing test first (`todo!()` placeholder),
  confirm it fails, then implement.
- Unit tests in `#[cfg(test)] mod tests` next to the code; integration tests in
  `tests/` run the real CLI against sample projects in `fixtures/`.
- Name tests by behaviour: `extract_classes_should_ignore_data_attributes`.
- One behaviour per test (a couple of `assert_eq!`s is fine). Prefer
  `assert_eq!` over `assert!`.
- `rstest` for table-style cases; `proptest` for invariants that must hold for
  any input (never panics, never removes safelisted classes, never invents rules);
  `cargo insta` snapshots for whole CSS outputs.
- No `mockall`: the core is pure functions over text. Use real fixture files.
- Test error paths with `Result`/`matches!`, not `#[should_panic]`.
- Coverage (`cargo llvm-cov`): 100% on matching/pruning logic (it decides what
  gets deleted), 80%+ elsewhere.
- Speed vs PurgeCSS is measured end-to-end with `hyperfine`; internal hot paths
  with `criterion`.
