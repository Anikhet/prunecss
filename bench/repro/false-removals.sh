#!/usr/bin/env bash
# Minimal reproducers for CSS that prunecss removes or changes although the
# page uses it. Found by the benchmark's correctness diff (see bench/README.md).
#
#   cargo build --release -p prunecss && bench/repro/false-removals.sh
#
# Each case prints the input, the content, and prunecss's output. The
# expected output is stated in the case's comment.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bin="${PRUNECSS:-$here/../../target/release/prunecss}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

case_() {
  local name="$1" css="$2" html="$3" expected="$4"
  printf '%s\n' "$css" >"$tmp/in.css"
  printf '%s\n' "$html" >"$tmp/page.html"
  echo "=== $name"
  echo "css:      $css"
  echo "content:  $html"
  echo "expected: $expected"
  printf 'got:      '
  "$bin" prune "$tmp/in.css" --content "$tmp/page.html" --minify
  echo
}

# 1. id selectors are removed even when the element has that id: the scanner
#    collects class names only, and #id is matched against that set.
case_ "id selector" '#site-nav{position:sticky}#site-nav .link{color:navy}' \
  '<nav id="site-nav"><a class="link">x</a></nav>' \
  '#site-nav{position:sticky}#site-nav .link{color:navy}'

# 2. Unquoted attribute values (valid HTML) are not scanned.
case_ "unquoted class attribute" '.unquoted{color:orange}' \
  '<p class=unquoted>x</p>' \
  '.unquoted{color:orange}'

# 3. The printer renames legacy placeholder pseudo-classes, so the kept rule
#    no longer matches in the browsers it was written for:
#    :-moz-placeholder -> :-moz-placeholder-shown (old Firefox) and
#    :-ms-input-placeholder -> :-ms-placeholder-shown (IE 10-11, legacy
#    Edge). Neither printed selector is implemented by any browser, so the
#    rule becomes dead. Lightning CSS appears to parse both as a prefixed
#    :placeholder-shown and print the prefix back. Bulma 1.0.4 has 4 such
#    rules (selector lists).
case_ "legacy placeholder pseudo-classes rewritten" \
  '.input:-moz-placeholder{color:gray}.input:-ms-input-placeholder{color:gray}' \
  '<input class="input" placeholder="Name">' \
  '.input:-moz-placeholder{color:gray}.input:-ms-input-placeholder{color:gray}'

# 4. Classes that only appear in JavaScript or framework syntax (not a quoted
#    class/className attribute) are removed. PurgeCSS keeps them because its
#    extractor takes every word in the file. This is a known scanner gap, not
#    a matching bug; a safelist would be the usual workaround once it exists.
case_ "class added from JavaScript / framework syntax" \
  '.is-open{display:block}.vue-active{color:green}.clsx-on{color:red}.svelte-dir{color:purple}' \
  "<script>el.classList.add('is-open')</script><span :class=\"{ 'vue-active': on }\"></span><b className={clsx('btn', on && 'clsx-on')}></b><i class:svelte-dir={on}></i>" \
  '.is-open{display:block}.vue-active{color:green}.clsx-on{color:red}.svelte-dir{color:purple}'
