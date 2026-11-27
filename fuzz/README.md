# Fuzzing prunecss

## What fuzzing is

A fuzzer is a program that writes test inputs for you. It starts from a few
example inputs (the *seed corpus*), changes them in small random ways (flips
bytes, splices two inputs together, inserts words from a *dictionary*), and
runs each one through a small function called a *fuzz target*. It watches
which lines of code each input reaches. Inputs that reach new code are kept
and changed further. Over millions of runs it finds inputs no person would
think to write.

A fuzz target fails by panicking. A target can check two kinds of thing:

- **Crashes**: the code must never panic, whatever the input.
- **Invariants**: rules that must hold for every input, for example "pruning
  the output a second time changes nothing". The target checks the rule
  with `assert!`, so a broken rule becomes a panic the fuzzer can find.

We use [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz), which drives
LLVM's libFuzzer. The layout copies ruff and Biome: `fuzz/` is its own Cargo
workspace (see the empty `[workspace]` table in `Cargo.toml`), so its
nightly-only build never touches the main workspace.

prunecss parses CSS with Lightning CSS, which has no fuzzing of its own, so
these targets fuzz it as well. Several bugs found so far are in its printer;
see [`regressions/README.md`](regressions/README.md).

## Targets

| Target | Input | What must hold |
|---|---|---|
| `prune_no_panic` | bytes, `css 0xFF content` | `prune(css, extract_classes(content))` never panics, minified or not. Errors are fine. |
| `prune_idempotent` | bytes, `css 0xFF content` | Pruning the minified output again with the same names gives the same output and removes nothing. |
| `prune_output_reparses` | bytes, `css 0xFF content` | The output parses again, and pruning it with *every* class and id name it contains removes nothing. |
| `extract_classes_no_panic` | text | `extract_classes` never panics; every name is non-empty and has no ASCII whitespace; a borrowed name is a slice of the input, and an owned (decoded) name appears only when the input contains `&` (an HTML entity) or `\` (a JavaScript escape). |
| `prune_structured` | a CSS grammar (`src/grammar.rs`) | All of the above `prune` checks, on well-formed stylesheets that reach the pruning logic on almost every run. |

Notes:

- The byte targets split the input at the first `0xFF` byte, which can never
  appear in UTF-8 text. Before the split is CSS, after it is content. With no
  `0xFF`, the content is empty.
- `prune_idempotent` and `prune_output_reparses` treat every word of the
  content as a used name, as well as what `extract_classes` finds. That way
  the fuzzer does not first have to invent `class="…"` to keep a rule.
- `prune_output_reparses` collects names by parsing the output with
  Lightning CSS, so escaped names such as `.md\:flex` are handled exactly as
  the pruner sees them. Using a superset of the names a kept selector needs
  can never remove it, so any removal is a bug.
- The checks live in `src/lib.rs` so every target shares them.

### Known bugs are skipped

The invariant checks skip inputs that hit a bug already reported, so
fuzzing finds new bugs instead of the same one again and again. Panics are
never skipped. The skips are `has_known_issue` and `same_output` in
`src/lib.rs`. Each skip names its bug in `regressions/README.md`. When a bug
is fixed, delete its skip. `cargo test` in `fuzz/` checks that every file in
`regressions/` is still skipped and that ordinary CSS (hex colours, Tailwind
escapes such as `.\32 xl\:inset-0`, escapes in strings) is not.

Two Cargo features turn on stricter checks that currently fail:

- `pretty`: the non-minified printer must be idempotent too, and must agree
  with the minified one.
- `no-skips`: turn every known-bug skip off and compare outputs byte for
  byte. Use it to replay `regressions/` and see which bugs are still open.

## Running locally

You need nightly Rust and cargo-fuzz:

```bash
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
```

From the repository root:

```bash
# Build every target.
cargo +nightly fuzz build -s none

# Fuzz one target for 3 minutes.
mkdir -p fuzz/corpus-work/prune_idempotent
cargo +nightly fuzz run prune_idempotent -s none \
  fuzz/corpus-work/prune_idempotent fuzz/corpus/prune_idempotent -- \
  -max_total_time=180 -timeout=1 -dict=fuzz/dict/css.dict

# Keep going after a crash, saving every one (useful while known bugs are open).
cargo +nightly fuzz run prune_idempotent -s none \
  fuzz/corpus-work/prune_idempotent fuzz/corpus/prune_idempotent -- \
  -max_total_time=180 -timeout=1 -dict=fuzz/dict/css.dict -fork=1 -ignore_crashes=1

# Turn on the stricter checks.
cargo +nightly fuzz run prune_idempotent -s none --features pretty,no-skips
```

What the flags do:

- `-s none` turns off the address sanitizer. prunecss has no `unsafe` code,
  so it only slows things down. (On Apple silicon you may need to drop it.)
- `-timeout=1` reports any input that takes more than a second as a hang.
- `-dict=fuzz/dict/css.dict` gives the fuzzer CSS words to insert, such as
  `@media`, `:is(` and `class="`.
- The first directory, `fuzz/corpus-work/<target>`, gets the new inputs the
  fuzzer finds. It is ignored by git. The second, `fuzz/corpus/<target>`,
  holds the seeds, which are committed. This keeps the committed corpus small.

Replace `prune_idempotent` with any target name from the table above.

## When the fuzzer finds a bug

libFuzzer stops, prints the panic message, and saves the input to
`fuzz/artifacts/<target>/crash-<hash>`. Then:

1. Reproduce it:
   `cargo +nightly fuzz run <target> -s none fuzz/artifacts/<target>/crash-<hash>`
2. Make it as small as possible:
   `cargo +nightly fuzz tmin <target> -s none fuzz/artifacts/<target>/crash-<hash>`
   This writes `minimized-from-<hash>` next to it.
3. Keep the minimized input as a regression: copy it to
   `fuzz/regressions/<target>/<short_descriptive_name>` and describe the bug
   in `fuzz/regressions/README.md`.
4. Fix it test-first: write a failing unit test in `crates/prunecss-core`
   from the minimized input, then fix the code.
5. Check the reproducer now passes:
   `cargo +nightly fuzz run <target> -s none fuzz/regressions/<target> -- -runs=0`
   (`-runs=0` runs only the given inputs, without fuzzing.)

To view a reproducer, which may contain binary bytes:
`python3 -c "import sys; print(repr(open(sys.argv[1], 'rb').read()))" <file>`.

## Adding seeds

Seeds are small example inputs in `fuzz/corpus/<target>/`, one file each.
Byte targets expect `css`, a `0xFF` byte, then content. Keep each directory
under about 100 files. To add only the inputs from a run that reach new
code, let libFuzzer merge them, then review what it added before committing:

```bash
cargo +nightly fuzz run prune_idempotent -s none \
  fuzz/corpus/prune_idempotent fuzz/corpus-work/prune_idempotent -- -merge=1
```

## In CI

`.github/workflows/fuzz.yml` builds every target, runs `cargo test` in
`fuzz/` and replays the seeds and regressions on pull requests that touch
`crates/prunecss-core/`, `fuzz/` or the workspace manifests. Once a day, and on manual dispatch, it
fuzzes each target for 5 minutes and uploads `fuzz/artifacts/` if one fails.
