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

### Added

- **Scanner** that finds candidate class names in any text: HTML, JSX/TSX
  (`className`, `clsx(...)`, template literals, object keys), Vue (`:class`),
  Svelte (`class:active`), Astro (`class:list`), Angular (`[ngClass]`), CSS
  modules (`styles.button`) and DOM calls (`classList.add`). Tailwind-style
  names stay whole (`md:flex`, `w-1/2`, `grid-cols-[repeat(2,1fr)]`).
  Linear time; names are borrowed from the input.
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
  - recovers from broken rules the way browsers do.
- **Safelist**: standard, greedy and deep patterns, plus keyframes and
  variables; each an exact name or a `/regex/` (linear-time matching).
- **Opt-in removal** of unused `@keyframes`, `@font-face` and custom
  properties, following prefixed properties, shorthands, `var()` chains and
  names used from content files.
- **`prunecss.json`** config with PurgeCSS-style keys, discovered in the
  current directory or a parent; unknown keys are rejected. JSON Schema in
  `prunecss.schema.json` and `prunecss schema`.
- **CLI**: `prunecss prune` with globs that respect `.gitignore`, parallel
  scanning, `--output` (file or directory), stdin (`-`), `--check` for CI,
  `--json`, `--stats` and safelist/tracking flags; `prunecss scan` (with
  `--css` to filter by a stylesheet); `prunecss completions <shell>`.
- Refuses to prune when the content patterns match no files.
- Exit codes: 0 success, 1 `--check` found removable CSS, 2 error.
- Media queries are printed in the widely supported `min-width` form.
- `prunecss-core` library API: `prune`, `extract_classes`, `defined_names`,
  `PruneOptions`, `Safelist`, `Pattern`, and a structured `PruneError` with
  a 1-based source location. Public types are `#[non_exhaustive]`; no
  Lightning CSS types appear in the API.
- Minimum supported Rust version: 1.88.

[Unreleased]: https://github.com/Anikhet/prunecss/commits/main
