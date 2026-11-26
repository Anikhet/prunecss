# 2. Use Lightning CSS to parse and print CSS, with our own prune pass

- Status: accepted
- Date: 2026-11-26

## Context

prunecss needs to parse real-world CSS, understand selectors (including
escaped names like `.md\:flex`, nesting, `:is()`, `:not()` and modern
at-rules such as `@layer`, `@container` and `@scope`), change the rule tree,
and print the result.

Writing a correct CSS parser is a large project on its own. The Rust
options are:

- **Lightning CSS**: a fast, widely used parser, transformer and printer. It
  builds a typed tree of rules and selectors that borrows from the input, and
  it stores class names already unescaped. It has never left
  `1.0.0-alpha`, so any release may break its API.
- Lower-level crates such as `cssparser` alone: stable, but we would have to
  build selector parsing and printing ourselves.

Lightning CSS also has a built-in `unused_symbols` option. It removes classes
and ids that you list, but only removes a rule when *all* of its selectors
are unused. It never trims `.unused` out of `.used, .unused {}`. Its
`minify()` step also merges rules and rewrites values, which is more than
pruning.

## Decision

- Parse and print with **Lightning CSS**, pinned to exactly
  `=1.0.0-alpha.72` (with default features off). Upgrades are deliberate,
  one at a time, and checked against the snapshot tests.
- Parse with **`error_recovery: true`**. Real stylesheets often contain a few
  broken rules; we skip those with a warning instead of failing the whole
  file.
- Write **our own prune pass** over the rule tree instead of using
  `unused_symbols`. It removes single selectors from lists and decides each
  selector component by component (see
  [ADR 0003](0003-conservative-bag-of-names-matching.md)).
- **Print only**, without calling `StyleSheet::minify`, so we don't rewrite
  values or merge rules. We set the printer targets so media queries are
  lowered to the widely supported `(min-width: …)` form, instead of the newer
  `(width >= …)` range syntax, which older browsers ignore.
- **Never expose Lightning CSS types** in the `prunecss-core` public API.
  Its errors are converted at the boundary into our own `PruneError`, with
  1-based line and column numbers.

## Consequences

- We get a mature, fast parser and correct printing of modern CSS for free.
- Upgrading Lightning CSS can break our code. Because it is pinned and hidden
  behind our own types, an upgrade touches only `pruner.rs` and never breaks
  our users.
- Output is reprinted, so some values change to an equivalent form (`white`
  becomes `#fff`, `::before` becomes `:before`). This is documented in the
  README.
- Our prune pass has to handle every rule type Lightning CSS knows about.
  Rule types we don't understand are kept, which is safe.
