# prunecss

Remove unused CSS, fast. Written in Rust.

> **Status: early development.** Scanning and pruning work on individual files.
> Not yet ready for production stylesheets.

Websites often ship a large stylesheet (all of Bootstrap, an old design system)
while each page uses a fraction of it. `prunecss` scans your HTML, JSX and
templates for the class names they actually use, then removes the CSS rules
that can never match. The goal is to be a much faster drop-in alternative to
[PurgeCSS](https://github.com/FullHuman/purgecss).

## Try it

```bash
cargo run -- prune styles.css --content index.html App.jsx --stats > pruned.css
```

Other options: `--output pruned.css` writes to a file, and `--minify` removes
whitespace. To see which class names were found:

```bash
cargo run -- scan index.html App.jsx
```

## How it decides what to remove

A selector is kept when every class and id it needs appears somewhere in the
content files. This is the same conservative "bag of names" model PurgeCSS
uses. To avoid breaking pages, prunecss **never** removes CSS because of:

- tags, attributes, combinators (`>`, `+`, `~`) or pseudo-classes such as
  `:hover`, `:focus` and `::before`;
- anything inside `:not()`, `:has()` or `:nth-child(… of …)`;
- `@keyframes`, `@font-face`, `@property` or custom properties, which need
  usage tracking that doesn't exist yet;
- named `@layer` blocks, even empty ones, because they fix the cascade order.

It does:

- remove single selectors from lists, so `.used, .unused {}` becomes `.used {}`;
- match escaped names such as `.md\:flex`, `.w-1\/2` and `.w-\[10px\]`;
- prune inside `@media`, `@supports`, `@container`, `@scope` and
  `@starting-style`, and drop those blocks once they are empty;
- remove nested rules together with their parent.

### Known limits

- Class names built at runtime, such as `` `btn-${size}` `` or `clsx(...)`
  calls, are not detected yet, so their CSS is removed. Don't use prunecss on
  such projects yet.
- Output is reprinted by Lightning CSS, so some values change to an equivalent
  form (`white` becomes `#fff`, `::before` becomes `:before`). Media queries
  keep the widely supported `min-width` form.

## Roadmap

- [x] Scan `class="…"` / `className="…"` attributes
- [x] Parse CSS with [Lightning CSS](https://lightningcss.dev) and match selectors
- [x] Remove unused rules and selectors and write the result
- [ ] Detect class names in JS expressions (`clsx`, template literals)
- [ ] Safelist and PurgeCSS-compatible config
- [ ] Track `@keyframes`, fonts and custom properties
- [ ] Parallel scanning of whole projects (`ignore` + rayon)
- [ ] Benchmarks against PurgeCSS
- [ ] npm package, Vite and PostCSS plugins
- [ ] Language server and VS Code extension

## License

MIT or Apache-2.0, at your option.
