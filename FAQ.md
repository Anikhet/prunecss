# Frequently asked questions

- [Why another unused-CSS remover?](#why-another-unused-css-remover)
- [Is it a drop-in replacement for PurgeCSS?](#is-it-a-drop-in-replacement-for-purgecss)
- [Why is my output bigger than PurgeCSS's?](#why-is-my-output-bigger-than-purgecsss)
- [A class I use was removed. What do I do?](#a-class-i-use-was-removed-what-do-i-do)
- [How do I keep classes built at runtime?](#how-do-i-keep-classes-built-at-runtime)
- [Do I need it with Tailwind CSS?](#do-i-need-it-with-tailwind-css)
- [Why does my CSS look slightly different after pruning?](#why-does-my-css-look-slightly-different-after-pruning)
- [Can I use it from Vite, PostCSS, webpack or npm?](#can-i-use-it-from-vite-postcss-webpack-or-npm)
- [Can I use it as a Rust library?](#can-i-use-it-as-a-rust-library)
- [How do I use it in CI?](#how-do-i-use-it-in-ci)
- [Does it send anything over the network?](#does-it-send-anything-over-the-network)

## Why another unused-CSS remover?

The popular tools are written in JavaScript and run through Node. On large
projects or in CI, startup and single-threaded parsing add up. prunecss is a
native binary that scans content files in parallel and parses CSS with
[Lightning CSS](https://lightningcss.dev). In the
[benchmarks](BENCHMARKS.md) it is 5–24× faster than the PurgeCSS CLI and
uses less memory, and it removed nothing a static page needs.

## Is it a drop-in replacement for PurgeCSS?

Mostly. It uses the same "bag of names" model, `prunecss.json` uses
PurgeCSS's option names (`content`, `css`, `output`, `safelist` with
`standard`, `deep`, `greedy`, `keyframes` and `variables`, and the
`keyframes`, `fontFace` and `variables` switches), and safelist regexes are
written as `"/^btn-/"`. The differences:

- prunecss never removes CSS because of tags, attributes or pseudo-classes,
  only because of classes and ids (see the next question).
- Custom extractors are not supported. The built-in scanner already reads
  Tailwind-style names, JSX, Vue, Svelte, Astro and Angular.
- There is no JavaScript API yet; see [plugins](#can-i-use-it-from-vite-postcss-webpack-or-npm).

## Why is my output bigger than PurgeCSS's?

PurgeCSS also removes rules for tags and attributes it doesn't find, such as
`table`, `kbd` or `[type="search"]`. Those are often created at runtime by
JavaScript, a CMS or a Markdown renderer, so prunecss always keeps them.
After compression the difference is usually a few kilobytes. This is a
deliberate trade: a few extra bytes cost much less than a broken page.

## A class I use was removed. What do I do?

First check that the file using it is matched by your `content` patterns:
`prunecss scan 'src/**/*' --css dist/app.css` lists the names prunecss
finds. If the name is built at runtime, [safelist it](#how-do-i-keep-classes-built-at-runtime). Otherwise
it's a bug; please
[report a false removal](https://github.com/Anikhet/prunecss/issues/new?template=false_removal.yml)
with the smallest content file and CSS that show it. These reports are
fixed first.

## How do I keep classes built at runtime?

No static scanner can see `` `btn-${size}` `` or names that come from an
API. Either write the full names somewhere in your source (a comment
listing `btn-sm btn-lg` is enough) or safelist them:

```bash
prunecss prune app.css -c src --safelist /^btn-/ --safelist-deep prose
```

## Do I need it with Tailwind CSS?

Not for Tailwind's own utilities: Tailwind already generates only the
classes it finds. prunecss is useful for the rest of your CSS: a component
library, Bootstrap or Bulma, an old design system, or third-party
stylesheets. It understands Tailwind-style names (`md:flex`, `w-1/2`,
`w-[10px]`) in your content, so mixing the two is safe.

## Why does my CSS look slightly different after pruning?

prunecss parses the stylesheet and prints the kept rules again, so
formatting changes and some values take an equivalent shorter form
(`white` becomes `#fff`). Printing targets old browsers so nothing gets
less compatible. The README's
[known limits](README.md#known-limits) list the few changes that can
matter. A future version will copy kept rules byte for byte.

## Can I use it from Vite, PostCSS, webpack or npm?

Not yet. An npm package with Vite and PostCSS plugins is next on the
[roadmap](README.md#roadmap). Until then, run the CLI after your build,
for example in an npm `postbuild` script.

## Can I use it as a Rust library?

Yes. `prunecss-core` contains all the logic; the CLI is a thin wrapper
around it. It is not on crates.io yet: until 0.1.0 is published, depend on
it from git (`prunecss-core = { git = "https://github.com/Anikhet/prunecss" }`)
and read its documentation with `cargo doc -p prunecss-core --open`.

```rust
use std::collections::BTreeSet;
use prunecss_core::{extract_classes, prune, PruneOptions};

let html = r#"<div class="card">"#;
let used: BTreeSet<_> = extract_classes(html);
let out = prune(".card{} .unused{}", &used, &PruneOptions::default())?;
```

The API is still 0.x and may change between minor versions.

## How do I use it in CI?

`prunecss prune --check` exits with `1` when it would remove anything and
`0` when nothing is unused, without writing files. Add `--json` for a
machine-readable report. Any error exits with `2`, including content
patterns that together match no files at all. A single pattern that matches
nothing, next to others that do, only prints a warning.

## Does it send anything over the network?

No. prunecss reads only the files you give it and writes only the outputs
you ask for. It has no telemetry.
