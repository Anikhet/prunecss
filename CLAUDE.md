# prunecss — guidance for Claude

A fast unused-CSS remover written in Rust: scan content files for the class
names they use, then remove CSS rules that can never match.

## Layout

- `crates/prunecss-core` — library. All logic lives here. Errors use `thiserror`.
- `crates/prunecss-cli` — the `prunecss` binary. Thin wrapper; errors use `anyhow`.
- Planned later: npm package via napi-rs, Vite/PostCSS plugins, then a language
  server + VS Code extension, all wrapping the same core.

## Commands (all must pass before committing)

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test
```

Benchmarks: always `--release`.

## Rust style (based on Apollo's Rust Best Practices handbook)

- Borrow instead of cloning. Take `&str` / `&[T]` in parameters; return borrowed
  data (`&str` slices of the input) where lifetimes allow. Use `Cow` when
  ownership is ambiguous.
- No `unwrap()` / `expect()` / `panic!` outside tests. Return `Result` and use `?`.
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
