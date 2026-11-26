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

- Class scanner: finds class names in `class="…"` and `className="…"`
  attributes of HTML, JSX and similar markup. Names are borrowed from the
  input, so nothing is copied.
- Pruner: parses CSS with Lightning CSS and removes rules and single
  selectors that need a class or id that no content file uses. It follows
  conservative correctness rules:
  - never removes CSS because of tags, attributes, combinators or
    pseudo-classes and pseudo-elements such as `:hover` or `::before`;
  - never removes CSS because of anything inside `:not()`, `:has()` or
    `:nth-child(… of …)`; `:is()` and `:where()` are kept when any
    alternative can match;
  - matches escaped names such as `.md\:flex`, `.w-1\/2` and `.w-\[10px\]`;
  - prunes inside `@media`, `@supports`, `@container`, `@scope` and
    `@starting-style`, and drops those blocks once empty;
  - keeps named `@layer` blocks, even empty ones, so cascade order is kept;
  - keeps `@keyframes`, `@font-face`, `@property` and custom properties
    until usage tracking exists;
  - removes nested rules together with their parent;
  - recovers from broken rules instead of failing the whole file.
- Output is printed with media queries in the widely supported
  `min-width` form, not the newer range syntax.
- `prunecss scan <files>` command: prints every class name found.
- `prunecss prune <css> --content <files>` command, with `--output`,
  `--minify` and `--stats`.
- Structured errors in `prunecss-core`: `PruneError` with a kind, a message
  and an optional 1-based source location. No Lightning CSS types appear in
  the public API.
- Minimum supported Rust version: 1.88.

[Unreleased]: https://github.com/Anikhet/prunecss/commits/main
