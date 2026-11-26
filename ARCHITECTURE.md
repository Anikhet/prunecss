# Architecture

This document describes the high-level shape of prunecss. Read it before
making a larger change. It describes things that should rarely change; for
details, read the code and its doc comments. Use your editor's symbol search
to find the names mentioned here.

## Bird's-eye view

prunecss takes a stylesheet and a set of content files (HTML, JSX,
templates). It answers one question for every selector in the stylesheet:
*could this ever match something in the content?* If the answer is a clear
no, the selector is removed. If the answer is "maybe", it stays.

It never renders pages or runs scripts. Instead it collects a flat set (a
"bag") of class names from the content and checks each selector against that
set. See [ADR 0003](docs/adr/0003-conservative-bag-of-names-matching.md) for
why.

The data flow is a straight line:

```text
content files ──read──▶ text ──extract_classes──▶ set of &str names ─┐
                                                                      ▼
stylesheet ──read──▶ text ──Lightning CSS parse──▶ rules ──prune──▶ rules ──print──▶ CSS
                          (error recovery on)                          (media queries lowered)
```

1. **Read.** The CLI reads every file into a `String`.
2. **Scan.** `extract_classes` finds class names in each content file. The
   names are `&str` slices borrowed from the file text, so no strings are
   copied. The CLI merges them into one ordered set.
3. **Parse.** `prune` parses the stylesheet with Lightning CSS, with error
   recovery on, so one broken rule does not fail the whole file.
4. **Prune.** A custom pass walks the rules. It removes single selectors that
   need an unused class or id, then removes rules with no selectors left, and
   then grouping rules (`@media`, `@supports`, …) that pruning emptied.
5. **Print.** Lightning CSS prints the result. Media queries are always
   printed in the widely supported `min-width` form.

## Codemap

### `crates/prunecss-core`

The library. All logic lives here. It is published to crates.io and will be
wrapped by every future front end.

- `src/lib.rs`: the public API. Modules are private and every public item is
  re-exported at the crate root, so each item has exactly one path.
- `src/scanner.rs`: `extract_classes`, a small byte scanner that finds
  `class` and `className` attribute values.
- `src/pruner.rs`: `prune`, `PruneOptions`, `PruneOutput`, `PruneError`,
  `PruneErrorKind`, `SourceLocation` and the `UsedNames` trait. This is where
  the decision "keep or remove" is made. The key functions are
  `keep_rule`, `keep_style_rule` and `component_can_match`.
- `proptest-regressions/`: inputs that once broke a property test. They are
  replayed on every test run.

### `crates/prunecss-cli`

The `prunecss` binary. A thin wrapper: it parses arguments with clap, reads
and writes files, calls the core and prints results. Errors use `anyhow`.

- `src/main.rs`: the `scan` and `prune` subcommands.
- `tests/cli.rs`: integration tests that run the real binary.
- `tests/fixtures/`: small sample projects (HTML, JSX, CSS) the tests run on.
- `tests/snapshots/`: `insta` snapshots of whole CSS outputs. Review changes
  with `cargo insta review`.

### Coming next

Modules for project discovery (walking a directory with `.gitignore`
support), a more general scanner and a PurgeCSS-compatible config are being
added. They will live in `prunecss-core`, with rayon for parallel file
reading. This document will describe them once they settle.

## Invariants

These rules hold everywhere. Several are about things that must **not**
happen.

- **When in doubt, keep.** A selector is removed only when it needs a class
  or id that is certainly not used. Keeping too much is a missed
  optimisation; removing too much breaks someone's site.
- **Never remove CSS because of** tags, attributes, combinators (`>`, `+`,
  `~`, descendant), pseudo-classes or pseudo-elements, or anything inside
  `:not()`, `:has()` or `:nth-child(… of …)`. Matching those needs the
  rendered page.
- **`:is()` and `:where()` are kept if any one alternative can match.**
- **Never remove what we can't track yet:** `@keyframes`, `@font-face`,
  `@property`, custom properties, `@import` and other at-rules without
  selectors.
- **Never remove a named `@layer`**, even an empty one. It fixes cascade
  order.
- **Never invent rules.** The output contains only rules and selectors from
  the input. Pruning is idempotent: pruning the output again changes nothing.
- **Never panic on any input.** Broken CSS is skipped or reported as a
  `PruneError`. Property tests check this.
- **Output is deterministic.** Nothing iterates a `HashSet` or `HashMap` into
  the output. Same input, same bytes out, on every run and machine.
- **The core does no IO of its own.** It never prints, never reads files and
  never writes files beyond what its API is given. Text goes in, text comes
  out. The CLI (and later the npm package or editor) does the IO.
- **No Lightning CSS types in the public API.** Lightning CSS is a pinned
  alpha. Its errors are converted at the boundary into `PruneError`. See
  [ADR 0002](docs/adr/0002-lightning-css-for-parsing.md).
- **No async in the core.** The work is CPU-bound; parallelism uses rayon.
  See [ADR 0004](docs/adr/0004-rayon-not-async.md).
- **No `unwrap`, `expect` or `panic!` outside tests.** Clippy enforces this.

## Layers and boundaries

```text
┌───────────────┐ ┌────────────────────────┐ ┌──────────────────────────┐
│ prunecss CLI  │ │ npm package (napi-rs), │ │ language server,         │
│ (now)         │ │ Vite/PostCSS plugins   │ │ VS Code extension        │
│               │ │ (planned)              │ │ (planned, may use async) │
└───────┬───────┘ └───────────┬────────────┘ └────────────┬─────────────┘
        └─────────────────────┼───────────────────────────┘
                              ▼
                ┌───────────────────────────┐
                │ prunecss-core             │
                │ pure functions over text  │
                └───────────────────────────┘
```

- **Core vs front ends.** Every front end calls the same public functions of
  `prunecss-core`. Front ends own IO, argument parsing, printing and
  user-facing error messages. If a front end needs logic, that logic belongs
  in the core.
- **Public API stability.** The core's public structs and enums are
  `#[non_exhaustive]`, and options grow through builder methods. This lets us
  add features without breaking users.
- **Async only at the edge.** Only the future language server waits on
  editor messages, so only it may use an async runtime.

## Cross-cutting concerns

- **Testing.** Unit tests sit next to the code, integration tests run the
  real binary on fixtures, `proptest` checks invariants, and `insta`
  snapshots pin whole outputs. Pruning logic aims for 100% coverage.
- **Errors.** The core uses its own `PruneError` type with a 1-based
  `SourceLocation`; the CLI wraps errors with `anyhow` context.
- **Performance.** Borrow, don't copy: class names stay `&str` slices of the
  input. Measure with `--release` builds only.
