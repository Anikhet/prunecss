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
- `@keyframes`, `@font-face` or custom properties, unless you opt in (see
  below), and never `@property`;
- named `@layer` blocks, even empty ones, because they fix the cascade order.

It does:

- remove single selectors from lists, so `.used, .unused {}` becomes `.used {}`;
- match escaped names such as `.md\:flex`, `.w-1\/2` and `.w-\[10px\]`;
- prune inside `@media`, `@supports`, `@container`, `@scope` and
  `@starting-style`, and drop those blocks once they are empty;
- remove nested rules together with their parent.

### Finding class names

prunecss does not parse your templates. It splits every content file into
candidate names, the approach Tailwind's own scanner uses, so it finds names
in HTML, JSX/TSX (`className`, `clsx(...)`, template literals, object keys),
Vue (`:class`), Svelte (`class:active`), Astro (`class:list`), Angular
(`[ngClass]`), CSS modules (`styles.button`) and DOM calls such as
`classList.add("open")`. Tailwind-style names stay whole: `md:flex`, `w-1/2`,
`!mt-0`, `grid-cols-[repeat(auto-fill,minmax(0,1fr))]`.

Over-collecting is deliberate. A candidate that isn't a real class keeps no
CSS, but a missed one would delete CSS the page needs.

### Safelist

Class names assembled at runtime, such as `` `btn-${size}` ``, cannot be
found by any static scanner. Safelist them, using an exact name or a
`/regex/`:

```bash
prunecss prune styles.css --content src/App.jsx \
  --safelist /^btn-/ \
  --safelist-greedy /^swiper/ \
  --safelist-deep prose
```

- `--safelist` treats a matching class or id as used.
- `--safelist-greedy` keeps any selector that contains a match, such as
  `.swiper-slide .unused`.
- `--safelist-deep` also keeps every rule nested inside a matching selector.

### Unused keyframes, fonts and variables (opt-in)

Like PurgeCSS, these are kept unless you ask:

- `--keyframes` removes `@keyframes` that no kept rule names, including
  through `-webkit-animation`, the `animation` shorthand or a variable.
- `--font-face` removes `@font-face` rules whose family no kept rule names,
  including through the `font` shorthand.
- `--variables` removes custom properties that nothing reads through
  `var()`, following chains such as `--brand: var(--base)`.

Content files count as references for all three, so an animation, font or
variable used from JavaScript is kept.

### Known limits
- Output is reprinted by Lightning CSS, so some values change to an equivalent
  form (`white` becomes `#fff`, `::before` becomes `:before`). Media queries
  keep the widely supported `min-width` form.

## Roadmap

- [x] Scan `class="…"` / `className="…"` attributes
- [x] Parse CSS with [Lightning CSS](https://lightningcss.dev) and match selectors
- [x] Remove unused rules and selectors and write the result
- [x] Detect class names in JS, Vue, Svelte, Astro and Angular templates
- [x] Safelist (standard, greedy, deep; exact or `/regex/`)
- [x] Opt-in removal of unused `@keyframes`, `@font-face` and custom properties
- [ ] PurgeCSS-compatible config file
- [ ] Parallel scanning of whole projects (`ignore` + rayon)
- [ ] Benchmarks against PurgeCSS
- [ ] npm package, Vite and PostCSS plugins
- [ ] Language server and VS Code extension

## Requirements

Rust 1.88 or newer to build from source.

## License

MIT or Apache-2.0, at your option.
