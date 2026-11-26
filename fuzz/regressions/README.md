# Fuzz regressions

Minimized inputs that broke an invariant. Each file is a raw input for the
target named by its directory. Byte targets read `css`, an optional `0xFF`
byte, then content. `manual/` holds cases found by hand while studying a
fuzzer crash. Those outputs are stable but wrong, so no invariant catches
them yet.

Replay a directory (with `no-skips`, every file below fails until fixed):

```bash
cargo +nightly fuzz run prune_idempotent -s none --features pretty,no-skips \
  fuzz/regressions/prune_idempotent -- -runs=0
```

All bugs found so far are in Lightning CSS's **printer**
(`lightningcss =1.0.0-alpha.72`), not in prunecss's matching logic: prunecss
prints what it parsed, and printing then parsing again does not always give
back the same stylesheet. prunecss is affected because it re-prints the whole
stylesheet, and users may run it twice (for example once per build step).
None of them panics.

| # | Bug | Reproducer → first output → second output | Skipped by | Severity |
|---|---|---|---|---|
| 1 | Identifiers outside class/id selectors are printed **without re-escaping**: unknown at-rule names, unknown pseudo-class names, media types. | `@a\.b;` → `@a.b;` → `@a .b;`<br>`@\d` → `@` + U+000D + `;` → *(rule gone)*<br>`:\/{color:red}` → `:/{color:red}` → *(rule gone)*<br>`@media sbre\/ and (max-width:1px){p{x:y}}` → `@media sbre/ and …` → *(rule gone)* | `has_known_issue` | Medium: changes meaning or drops rules on the next parse. Rare in real CSS. |
| 2 | A **comment inside unparsed tokens** (custom property, unknown property, unknown at-rule prelude or block) is removed, but the whitespace on both sides is kept. | `.a{--x: a /**/ b}` → `.a{--x:a  b}` → `.a{--x:a b}`<br>`@o: /*` → `@o : ;` → `@o :;`<br>`@x{/* c */ @y}` → leading space kept, then trimmed | `same_output` (inputs with `/*`) | Low: whitespace only. |
| 3 | **Pretty printer**: an empty declaration value prints as `o: ;`, and minifying that keeps the space. | `c{o:` → pretty `c {\n  o: ;\n}` → minified `c{o: }`, but direct minify gives `c{o:}` | `pretty` feature off | Low. |
| 4 | **Pretty printer**: an unknown at-rule's block gains an indented blank line on every pass. | `@foo{}` → `@foo {\n  \n}` → `@foo {\n  \n  \n\n}` → … | `pretty` feature off | Low, but unbounded growth. |
| 5 | `@container name{…}` (a name, no query) gains a space on the next pass. | `@container foo{p{x:y}}` → `@container foo {p{x:y}}` → `@container foo  {…}` | `same_output` (whitespace runs) | Low. |
| 6 | `@media` with an empty first query gains a space on the next pass. | `@media , screen{…}` → `@media ,screen{…}` → `@media  ,screen{…}` | `same_output` (whitespace runs) | Low. |
| 7 | **Out-of-range numbers** are clamped, and not to the same value each time. | `width:99999999999999999999999999999999999999999px` → `3.40282e38px` → `2147483647px`<br>`height:1e20px` → `2147483647px` | `has_known_issue` (≥10 digits or an exponent) | Low: absurd values. |
| 8 | **Token lists are printed without separators**, so adjacent tokens merge, and `+` on a number is dropped. | `.a{x:..0rem}` → `.a{x:.0rem}` → `.a{x:0rem}`<br>`manual/`: `.a{x:a.0rem}` → `.a{x:a0rem}` (an ident!)<br>`manual/`: `.a{--y:a/**/b}` → `.a{--y:ab}`<br>`manual/`: `.a{--x:1+2px}` → `.a{--x:12px}` | **not skipped** | **High**: silently changes values in custom properties and unparsed declarations in a single pass. |

Bug 8 is not skipped because it is the most serious, and because no cheap
input check can tell which inputs reach it. Until it is fixed,
`prune_idempotent` and `prune_output_reparses` will keep finding it. Run them
with `-fork=1 -ignore_crashes=1` to collect other crashes as well.

## Where the bugs live

- Bug 1: confirmed in source for at-rules: `lightningcss/src/rules/unknown.rs`
  writes the name with `dest.write_str(&self.name)`, not
  `serialize_identifier`. Unknown pseudo-classes and media types behave the
  same way; their printers were not read.
- Bug 4: in the same file, the block is printed between `indent()` and
  `newline()` calls around the raw tokens, which keep their own whitespace.
- Bugs 2 and 8 (inferred from behaviour, source not read): unparsed token
  lists keep whitespace tokens beside removed comments (2), and no `/**/`
  or space is written between tokens that would merge when re-tokenized
  (8), which the CSS Syntax serialization rules require.
- Bug 7 (inferred): numbers pass through `f32`, and integer dimensions
  through `i32`, on parse and print.

## Fixing them

prunecss cannot fix the printer itself, but each bug can be covered from
our side, test-first:

1. Copy the reproducer into a unit test in
   `crates/prunecss-core/src/pruner.rs` that asserts the output is stable
   (prune twice, compare).
2. Fix it upstream in Lightning CSS, or work around it in prunecss.
3. Delete the bug's skip in `fuzz/src/lib.rs` and check the reproducer
   passes without `no-skips`.
