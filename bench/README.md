# prunecss benchmarks

A reproducible comparison of `prunecss` with **PurgeCSS 9.1.0**: speed,
memory, output size and, most importantly, *which CSS each tool keeps*.

```bash
make -C bench          # npm ci, fetch + verify corpora, build prunecss, run everything
make -C bench quick    # smoke run: few iterations, no "huge" corpus (do not quote)
```

or step by step:

```bash
cd bench
npm ci                 # pinned tools (package-lock.json)
npm run fetch          # corpora into corpus/, checked against corpus/manifest.json
npm run bench          # = node run.mjs; see `node run.mjs --help`-style flags below
```

Results land in `results/<date>-<host>.json` (all raw data) and a generated
`results/<date>-<host>.md`. Regenerate the Markdown from the JSON with
`node lib/report.mjs results/<file>.json`.

Requirements: Node 22+, a Rust toolchain, `curl`, and ideally GNU time at
`/usr/bin/time` (`apt-get install time`). hyperfine is installed with
`cargo install hyperfine --locked` if it is missing.

`run.mjs` flags: `--quick`, `--only bootstrap,tailwind`, `--runs N`
(default 10), `--warmup N` (default 3), `--rss-runs N` (default 10),
`--skip-build`, `--no-tinybench`, `--label "text"`, `--examples N`.

## Latest results (indicative, shared cloud VM)

From `results/2026-11-26-vm.md` (prunecss commit `a66faef`, PurgeCSS 9.1.0):

| | prunecss vs PurgeCSS CLI |
|---|---|
| Wall time | **5.4–24.4× faster** (median 9.0×) across 7 corpora |
| vs PurgeCSS's JS API in a warm process | 2.3–5.7× faster (median 3.9×), for prunecss's whole CLI run |
| Peak memory | PurgeCSS used 1.5–8.1× as much |
| Possible false removals (static DOM) | **prunecss 0 on every corpus**; PurgeCSS 376 on Tailwind (default extractor), 1 with the Tailwind extractor, 5 on the edge cases (see the note below) |
| Output size | PurgeCSS is smaller, mostly because prunecss always keeps tag/attribute selectors. prunecss's extra bytes, minified / gzip / brotli: Bootstrap 0.8 / 0.2 / 0.2 kB, Bulma 65.6 / 5.7 / 0.1 kB, Tailwind 30.0 / 3.9 / 2.7 kB (2.8 / 0.7 / 0.6 kB against the Tailwind extractor), synthetic corpora up to 40.4 / 5.0 / 4.2 kB |

The committed run predates a fix to the false-removal count: `run.mjs` then
counted PurgeCSS's trimmed `:is()` list (`:is(.alert, .toast) .close` printed
as `.alert .close`, which is correct) as a removal, so the results file says 6
for the edge cases. The JSON's examples show the other 5 are escaped Tailwind
names. `run.mjs` now counts trimmed `:is()`/`:where()` lists as rewritten.

Rust micro-benchmarks are separate and need no corpus:

```bash
cargo bench -p prunecss-core              # extract_classes MB/s, prune on Bootstrap-shaped CSS
cargo bench -p prunecss-core -- --quick   # fast smoke run
```

`prune/bootstrap-5.3.8` is added automatically once `npm run fetch` has
downloaded the real Bootstrap corpus.

## What is measured, and why

Every tool gets **the same stylesheet, the same content files and the same
(empty) safelist**. prunecss cannot take globs yet, so the harness expands
each corpus's file pattern itself and passes the identical explicit list to
both CLIs (one function, `contentArgs` in `run.mjs`, to change when globs
land). Before anything is timed, each command is run once and must exit 0
and write a non-empty file; the output's SHA-256 is recorded. A tool that
fails quietly cannot look fast.

| metric | how | why |
|---|---|---|
| CLI wall time | `hyperfine -N --warmup 3 --runs 10`, raw JSON kept | What users pay. Reported as mean ± σ (plus median, min–max); never the minimum alone. |
| Node startup | `node -e ''` and `node -e "require('purgecss')"` rows | Separates Node's fixed cost from PurgeCSS's real work. |
| PurgeCSS in-process | tinybench on `new PurgeCSS().purge()` with inputs preloaded, JIT warmed | PurgeCSS's **best case** (no startup, no file reads). Compared against the *full* prunecss CLI run, which favours PurgeCSS. |
| Peak memory | `/usr/bin/time -f %M`, 10 runs, median (falls back to polling `/proc/<pid>/status` VmHWM, and says so in the results) | hyperfine's mean says nothing about RAM. |
| Output size | every output **and the input** go through the same minifier, lightningcss 1.33.0 (`minify: true, errorRecovery: true`), then raw / `gzip -9` / brotli q11 | Differences then reflect pruning, not formatting. |
| Correctness | selector sets diffed, categorised, and checked against a static-DOM oracle | Speed is meaningless if the output breaks the page. |

### Correctness, in detail

1. Each minified stylesheet is parsed with postcss and reduced to a set of
   keys: `"<at-rule context> | <selector>"` for every selector of every rule
   (selector lists are split), plus `@keyframes name`, `@font-face family`,
   `@property`, `@layer` statements. Formatting and rule order drop out.
2. **Diff**: kept by both, kept only by prunecss, kept only by PurgeCSS. Each
   difference is put into one category: `at-rule`, `rewritten` (the selector
   text is not in the input, so the tool changed it), `escaped`
   (`md\:flex`), `id`, `attribute`, `tag-only` (`table`, `kbd`), `functional`
   (`:not/:is/:where/:has`), `pseudo`, or `class` (the extractors found
   different names). For each example the report lists which required
   class/id names the *other* tool's extractor did not find.
3. **Static-DOM oracle** (`lib/oracle.mjs`): the content files are parsed as
   HTML (htmlparser2) and each selector is matched with css-select after
   removing state pseudo-classes and pseudo-elements (`:hover`, `::before`,
   `:focus-visible`… are assumed able to apply; structural ones such as
   `:not(:last-child)` are evaluated). Then:
   - **possible false removal** = in the input, missing from the output,
     *matches* an element of the content. This is the worst failure.
   - **kept but unmatched** = kept, matches nothing statically. An upper
     bound on leftover CSS: it includes CSS for classes JavaScript adds.
   The oracle is a lower bound on what a page needs. It cannot see classes
   added at runtime, Astro/Jekyll templating, or JSX expressions. It also
   reads only `class` attributes: a class set only through `className`,
   Vue `:class`, Svelte `class:` or `classList.add` counts as unused, so
   removing its rules is not flagged. A browser
   coverage run (Playwright) would be the next step; it is not here yet.

## Corpora

Third-party files are **not committed**. `corpus/manifest.json` records the
source and SHA-256 of every file, and `npm run fetch` refuses to continue on
a mismatch. Generated corpora use fixed seeds; their hashes are recorded too
(`--update-manifest` rewrites them after a deliberate change).

| corpus | stylesheet | content |
|---|---|---|
| bootstrap | `bootstrap@5.3.8` `dist/css/bootstrap.css` (280 kB) from npm | the 34 non-RTL example pages, `site/src/assets/examples/*/index.astro`, from `twbs/bootstrap@v5.3.8` (raw.githubusercontent.com) |
| bulma | `bulma@1.0.4` `css/bulma.css` (764 kB) from npm | 50 documentation pages, `docs/documentation/**/*.html` (Jekyll sources), from `jgthms/bulma@1.0.4` |
| tailwind | built with `@tailwindcss/cli@4.3.5` from `corpus/tailwind/input.css`: `@import "tailwindcss"` plus broad `@source inline()` safelists (4.5 MB, ~37 k selectors) | 40 pages from `scripts/gen-tailwind.mjs` (seed `0x7a11`): navbars, heroes, card grids, forms, tables, pricing; variants (`md:`, `hover:`, `dark:`, `group-hover:`, `peer-checked:`, `data-[…]:`), fractions (`w-1/2`), arbitrary values (`h-[10px]`, `bg-[#1da1f2]`, `grid-cols-[200px_1fr]`) |
| edge | committed, hand-written (`corpus/edge/`) | ids, tags, attribute selectors, classes added from JS, Vue `:class`, JSX `clsx()`/template literals, Svelte `class:`, escaped names, `:is/:not/:has`, keyframes, font-face, unquoted/upper-case attributes. Correctness only (too small to time) |
| synthetic-{small,medium,large,huge} | `scripts/gen-synthetic.mjs`: 1 k / 10 k / 50 k / 200 k rules | 10 / 100 / 500 / 2 000 HTML files of 150 elements; ~20% of classes used. Mix of simple, compound, descendant, `:hover`, `>` tag, `[data-state]`, `::before`, selector lists, `@media` |

Bootstrap's example pages are Astro templates. Both tools read them as plain
text, which is how they would be used in a real Astro project. Some markup
there comes from components (`<Placeholder>` renders `bd-placeholder-img`)
that neither tool can see.

PurgeCSS's default extractor, `/[A-Za-z0-9_-]+/g`, splits Tailwind names
such as `md:flex` into `md` and `flex`. On the Tailwind corpus PurgeCSS is
therefore run twice: with its default, and with the documented
Tailwind-friendly extractor in `config/purgecss-tailwind.config.mjs`.

## Reproducing on a quiet machine

The committed results come from a shared, noisy cloud VM and are
**indicative only**. For numbers worth publishing:

- Use a physical Linux machine, on AC power, with browsers/IDEs closed.
- Fix the CPU frequency: `sudo cpupower frequency-set -g performance`.
- Disable turbo: `echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo`
  (Intel) or `echo 0 | sudo tee /sys/devices/system/cpu/cpufreq/boost` (AMD).
- Pin to the same cores for every tool, e.g. `taskset -c 2,3 make -C bench`,
  and say how many. prunecss reads and scans content files on all the cores
  it is given (rayon); PurgeCSS mostly uses one, so the core count changes
  the ratio.
- Put the repo on local SSD or tmpfs, not a network/overlay filesystem.
- Run twice, on two machines if you can (x86 Linux and Apple Silicon); quote
  ranges ("X–Y× across N corpora, median Z×"), never a single best case.
- The governor, turbo state and load average are recorded in every results
  file, so a noisy run is visible after the fact.

All runs here are warm-cache (files already in the page cache). For cold
cache, add `--prepare 'sync; echo 3 | sudo tee /proc/sys/vm/drop_caches'` to
the hyperfine call in `run.mjs`. hyperfine runs the tools one after another;
for long suites, run the harness several times and compare.

## Reading a results file

- **Headline**: speedup range across timed corpora (PurgeCSS CLI mean ÷
  prunecss CLI mean), the same against PurgeCSS's in-process API, memory
  ratio, and whether prunecss removed anything the static DOM uses.
- **Speed** tables: mean ± σ per tool. "× slower" carries a propagated σ;
  if σ is a large fraction of the mean the machine was noisy.
- **Output size**: compare the *minified* columns; "tool raw" is only how the
  tool happened to format its file.
- **Correctness**: start with "Selectors removed that the content uses".
  Then the per-corpus diffs explain every size difference.
- **Where prunecss loses**: generated from the data; every loss is listed.

## Known caveats

- **Noisy container.** The committed run shares 4 vCPUs (KVM) with other
  workloads; no governor control. σ is large on small corpora.
- **Both tools run with defaults**: no safelist, explicit file lists, and
  keyframes/fonts/variables kept. prunecss's scanner reads every token of a
  file (like Tailwind's), so it sees `classList.add('show')`, `clsx()`,
  Vue `:class`, Svelte `class:`, `id=` and unquoted `class=foo`, which the
  first harness run (against an early build) flagged as gaps; those are
  fixed and kept as regression checks in `repro/false-removals.sh`.
- **Design differences, not bugs**: prunecss never removes tag, attribute
  or pseudo-only selectors (`kbd`, `[data-theme=dark]`, `::selection`);
  PurgeCSS removes them when the word never appears. On Bulma this alone is
  ~65 kB of minified CSS (two `[data-theme]` custom-property blocks), though
  it compresses to almost nothing under brotli.
- **Printer**: prunecss prints through Lightning CSS (alpha.72), tuned for
  old browsers (`rgba()` kept, legacy placeholder selectors kept as
  written), so its raw output is normalised. The shared minifier evens this
  out for size and selector comparison.
- **Static oracle, not a browser**: see above. "Matches static DOM" for
  Astro/Jekyll sources is approximate.
- PurgeCSS runs with its defaults (`fontFace`, `keyframes`, `variables` all
  off, i.e. kept), which matches prunecss's behaviour of keeping them.
- The in-process numbers are only for PurgeCSS; prunecss has no in-process
  binding yet (napi-rs is planned), so its CLI time is used.

## Files

```
bench/
  Makefile, package.json, package-lock.json   entry points and pinned tools
  run.mjs                     the harness
  lib/corpora.mjs             corpus definitions (one place to add a corpus)
  lib/selectors.mjs           selector sets, diff, categories
  lib/oracle.mjs              static-DOM matcher
  lib/sizes.mjs               shared minifier + gzip/brotli
  lib/env.mjs                 versions, hardware, OS
  lib/report.mjs              JSON -> Markdown
  scripts/fetch-corpus.mjs    fetch + generate + verify corpora
  scripts/gen-tailwind.mjs    Tailwind pages and input CSS
  scripts/gen-synthetic.mjs   scaling corpus
  scripts/tinybench-purgecss.mjs  in-process PurgeCSS timing
  config/purgecss-tailwind.config.mjs
  corpus/manifest.json        sources + SHA-256 of every corpus file
  corpus/edge/                hand-written edge cases (committed)
  repro/false-removals.sh     regression checks for prunecss bugs found here (all fixed)
  results/                    committed result files
```
