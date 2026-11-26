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
content globs ──expand──▶ files ──read (parallel)──▶ text ──extract_classes──▶ set of &str names ─┐
                                                                                                    ▼
stylesheet ──read──▶ text ──Lightning CSS parse──▶ rules ──prune selectors──▶ ──track at-rules──▶ ──print──▶ CSS
                          (error recovery on)          (+ safelist)          (opt-in)        (media queries lowered)
```

1. **Find and read.** The CLI expands paths and globs (respecting
   `.gitignore`) and reads every content file into a `String`, in parallel.
   If no content file matches, it stops: every class would look unused.
2. **Scan.** `extract_classes` splits each file into candidate names, the
   approach Tailwind's scanner uses, so it works on HTML, JSX, Vue, Svelte,
   Astro and plain JavaScript alike. Candidates are `&str` slices borrowed
   from the file text, so no strings are copied. The CLI merges them, in
   parallel, into one hash set.
3. **Parse.** `prune` parses the stylesheet with Lightning CSS, with error
   recovery on, so one broken rule does not fail the whole file.
4. **Prune.** A custom pass walks the rules. It removes single selectors that
   need an unused class or id (unless the safelist keeps them), then rules
   with no selectors left, and then grouping rules (`@media`, `@supports`,
   …) that pruning emptied.
5. **Track (opt-in).** If asked, unused `@keyframes`, `@font-face` rules and
   custom properties are removed, judged against what survived step 4.
6. **Print.** Lightning CSS prints the result. Media queries are always
   printed in the widely supported `min-width` form.

## Codemap

### `crates/prunecss-core`

The library. All logic lives here. It is published to crates.io and will be
wrapped by every future front end.

- `src/lib.rs`: the public API. Modules are private and every public item is
  re-exported at the crate root, so each item has exactly one path.
- `src/scanner.rs`: `extract_classes`, a linear-time tokenizer that splits
  text into candidate names. It reads the text twice, keeping `[…]` whole
  (Tailwind arbitrary values) and splitting on it (JavaScript arrays), and
  returns the union.
- `src/pruner.rs`: `prune`, `defined_names`, `PruneOptions`, `PruneOutput`,
  `PruneError`, `PruneErrorKind`, `SourceLocation` and the `UsedNames`
  trait. This is where the decision "keep or remove" is made for selectors.
  The key functions are `keep_rule`, `keep_style_rule`,
  `component_can_match` and `mentions` (safelist).
- `src/safelist.rs`: `Pattern` (exact name or linear-time regex) and
  `Safelist` (standard, greedy, deep, keyframes, variables).
- `src/tracking.rs`: the opt-in removal of unused `@keyframes`, `@font-face`
  and custom properties. References are read from the parsed values of kept
  declarations, so prefixes and shorthands count; variables follow `var()`
  chains to a fixed point.
- `src/rules.rs`: the one list of conditional grouping rules, shared by every
  pass that walks the rule tree.
- `proptest-regressions/`: inputs that once broke a property test. They are
  replayed on every test run.

### `crates/prunecss-cli`

The `prunecss` binary. A thin wrapper: it parses arguments with clap, reads
and writes files, calls the core and prints results. Errors use `anyhow`.

- `src/main.rs`: argument parsing, merging flags with the config file, and
  exit codes (0 success, 1 `--check` found removable CSS, 2 error).
- `src/config.rs`: `prunecss.json` (PurgeCSS-style keys, unknown keys
  rejected) and its JSON Schema, committed as `prunecss.schema.json`.
- `src/files.rs`: expanding paths and globs with the `ignore` crate.
- `src/run.rs`: the `prune` and `scan` flows, parallel reading with rayon,
  output files and the text/JSON report.
- `tests/cli.rs`: integration tests that run the real binary.
- `tests/fixtures/`: small sample projects (HTML, JSX, CSS) the tests run on.
- `tests/snapshots/`: `insta` snapshots of whole CSS outputs. Review changes
  with `cargo insta review`.

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
- **At-rules without selectors are kept** unless their tracking is switched
  on: `@keyframes`, `@font-face` and custom properties are removed only on
  request, and only when nothing kept and no content file mentions them.
  `@property`, `@import` and other at-rules are always kept. If a value
  can't be inspected, tracking removes nothing.
- **A safelisted name always survives.** Names inside `:not()`/`:has()`
  never trigger the safelist.
- **No content, no pruning.** If the content patterns match no files, the
  CLI refuses to run.
- **Never remove a named `@layer`**, even an empty one. It fixes cascade
  order.
- **Never invent rules.** The output contains only rules and selectors from
  the input. Pruning is idempotent: pruning the output again changes nothing.
- **Never panic on any input.** Broken CSS is skipped or reported as a
  `PruneError`. Property tests check this.
- **Output is deterministic.** Nothing iterates a `HashSet` or `HashMap` into
  the output. Same input, same bytes out, on every run and machine.
- **The core does no IO of its own.** It never prints, reads or writes
  files. Text goes in, text comes out. File discovery, config files and
  output live in the CLI (and later the npm package or editor).
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
