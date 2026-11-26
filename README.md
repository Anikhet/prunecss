# prunecss

Remove unused CSS, fast. Written in Rust.

> **Status: early development.** Only the class scanner exists so far.

Websites often ship a large stylesheet (all of Bootstrap, an old design system)
while each page uses a fraction of it. `prunecss` scans your HTML, JSX and
templates for the class names they actually use, then removes the CSS rules
that can never match. The goal is to be a much faster drop-in alternative to
[PurgeCSS](https://github.com/FullHuman/purgecss).

## Try it

```bash
cargo run -- scan path/to/page.html
```

prints every class name used in the file.

## Roadmap

- [x] Scan `class="…"` / `className="…"` attributes
- [ ] Parse CSS (via [Lightning CSS](https://lightningcss.dev)) and match selectors
- [ ] Remove unused rules and write the result
- [ ] Safelist and PurgeCSS-compatible config
- [ ] Parallel scanning of whole projects (rayon)
- [ ] Benchmarks against PurgeCSS
- [ ] npm package, Vite and PostCSS plugins
- [ ] Language server and VS Code extension

## License

MIT or Apache-2.0, at your option.
