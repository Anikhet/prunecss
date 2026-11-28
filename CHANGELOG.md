# Changelog

All notable changes to prunecss are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). While the version
is 0.x, a minor version bump (0.1 to 0.2) may contain breaking changes.

From the first release on, this file is updated by
[release-plz](https://release-plz.dev/) from
[Conventional Commits](https://www.conventionalcommits.org/) messages, then
edited by hand where needed so that each entry reads as a summary for users,
not a raw commit log. See [CONTRIBUTING.md](CONTRIBUTING.md#commit-messages).

## [Unreleased]

## [0.1.0] - 2026-11-27

First release.

### Added

- **Scanner** that finds candidate class names in any text: HTML, JSX/TSX
  (`className`, `clsx(...)`, template literals, object keys), Vue (`:class`),
  Svelte (`class:active`), Astro (`class:list`), Angular (`[ngClass]`), CSS
  modules (`styles.button`) and DOM calls (`classList.add`,
  `querySelector(".a+.b")`). Tailwind-style names stay whole (`md:flex`,
  `w-1/2`, `grid-cols-[repeat(2,1fr)]`, `bg-[url('…')]`, `bg-(--brand)`).
  Names written with HTML entities (`[&amp;_p]:mt-2`, `class=&quot;x&quot;`,
  also double-escaped and named entities) or JavaScript escapes
  (`"btn\x2dlg"`) are decoded. Linear time; names are borrowed from the
  input except the few that only exist after decoding.
- **Pruner** that parses CSS with Lightning CSS and removes rules and single
  selectors needing a class or id no content file uses. Conservative rules:
  - never removes CSS because of tags, attributes, combinators,
    pseudo-classes or pseudo-elements, or anything inside `:not()`,
    `:has()` or `:nth-child(… of …)`; `:is()`/`:where()` are kept when any
    alternative can match;
  - matches escaped names such as `.md\:flex`, `.w-1\/2` and `.w-\[10px\]`;
  - prunes inside `@media`, `@supports`, `@container`, `@scope` and
    `@starting-style`, and drops those blocks once empty;
  - keeps named `@layer` blocks, even empty ones;
  - removes nested rules together with their parent;
  - recovers from broken rules the way browsers do;
  - refuses stylesheets nested more than 40 levels deep with an error, so
    `prune` stays within a 2 MiB thread stack even in debug builds.
- **Safelist**: standard, greedy and deep patterns, plus keyframes and
  variables; each an exact name or a `/regex/` (linear-time matching,
  `/regex/i` flags supported).
- **Opt-in removal** of unused `@keyframes`, `@font-face` and custom
  properties, following prefixed properties, shorthands, `var()` chains and
  names used from content files. Readers in `@container style()` queries,
  `@page`, `@position-try`, `@font-feature-values` and unknown at-rules
  count; quoted and escaped keyframe and custom-property names are matched,
  and font families are matched in any letter case.
- **`prunecss.json`** config with PurgeCSS-style keys, discovered in the
  current directory or a parent; unknown keys are rejected. JSON Schema in
  `prunecss.schema.json` and `prunecss schema`.
- **CLI**: `prunecss prune` with globs that respect `.gitignore`, parallel
  scanning, `--output` (file or directory), stdin (`-`), `--check` for CI,
  `--json`, `--stats`, `--safelist`, `--safelist-greedy`, `--safelist-deep`,
  `--safelist-keyframes`, `--safelist-variables`, `--keyframes`,
  `--font-face` and `--variables`; `--no-minify`, `--no-keyframes`,
  `--no-font-face` and `--no-variables` turn off a setting from
  `prunecss.json`. `prunecss scan` (with `--css` to filter by a
  stylesheet); `prunecss completions <shell>`.
- Exit codes: 0 success, 1 `--check` found removable CSS, 2 error.
- **Safe output**: prunecss never overwrites an input stylesheet or content
  file (hard links and symlinks included) and never writes outside the
  output directory. With several stylesheets, `-o DIR` mirrors their paths
  from the inputs' common parent and refuses two inputs that would share an
  output path. When stylesheets are named on the command line, `output`
  from `prunecss.json` is ignored; only `-o` decides. `"output": "-"` means
  standard output. A broken pipe ends the run quietly.
- **Refuses to prune** when the content patterns match no files or yield no
  names at all, since every class would look unused. A single pattern that
  matches nothing prints a warning, as does a stylesheet none of whose
  classes are used.
- **Warnings** for rules and declarations the parser can't read and drops,
  including at-rules with an empty prelude such as `@supports{…}`
  (`ParseWarning`, printed as `warning: file:line:col: …`, listed in
  `--json`, counted by `--check`). Problems the parser recovers from while
  keeping the rule are not reported.
- **Output that stays compatible**: printing targets old browsers (colours
  stay `rgba()` rather than 8-digit hex, media queries use the `min-width`
  form, nesting is never flattened); legacy `:-moz-placeholder` and
  `:-ms-input-placeholder` are printed as written, also inside
  `:nth-child(… of …)` and `@scope`; `@charset "UTF-8"` is added when
  needed; nesting parents keep the selector that sets `&`'s specificity.
- `prunecss-core` library API: `prune`, `extract_classes`, `defined_names`,
  `PruneOptions` (builder methods), `Safelist`, `Pattern` and
  `PatternError` (`kind()`, `message()`, `pattern()`), the sealed
  `UsedNames` trait, and a structured `PruneError` with a 1-based source
  location. Public types are `#[non_exhaustive]`; no Lightning CSS or
  `regex` types appear in the API.
- Minimum supported Rust version: 1.88.
- **Fuzzing** (`fuzz/`, cargo-fuzz) and property tests for monotonicity,
  no invented rules and safelist equivalence.
- Benchmarks against PurgeCSS 9.1.0 (`make -C bench`) and criterion
  micro-benchmarks.

[Unreleased]: https://github.com/Anikhet/prunecss/compare/prunecss-v0.1.0...HEAD
[0.1.0]: https://github.com/Anikhet/prunecss/releases/tag/prunecss-v0.1.0
