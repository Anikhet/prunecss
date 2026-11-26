//! Shared input handling and invariant checks for the prunecss fuzz targets.
//!
//! Each target in `fuzz_targets/` is a thin wrapper that turns libFuzzer's
//! bytes into inputs and calls one or more `check_*` functions here. The
//! checks panic when an invariant breaks, which is how libFuzzer learns it
//! found a bug, so this crate uses `panic!`/`assert!` freely: it is test code.

pub mod grammar;

use std::collections::BTreeSet;

use lightningcss::rules::{CssRule, CssRuleList};
use lightningcss::selector::{Component, Selector};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use prunecss_core::{PruneOptions, extract_classes, prune};

/// Separates the stylesheet from the content in a raw fuzz input.
///
/// `0xFF` never appears in valid UTF-8, so splitting on it before the lossy
/// conversion loses nothing a real stylesheet could contain.
pub const CONTENT_SEPARATOR: u8 = 0xFF;

/// Splits a raw fuzz input into `(css, content)`, both lossily decoded.
///
/// Without a separator the whole input is CSS and the content is empty.
#[must_use]
pub fn split_input(data: &[u8]) -> (String, String) {
    let (css, content) = match data.iter().position(|&b| b == CONTENT_SEPARATOR) {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (data, &[][..]),
    };
    (
        String::from_utf8_lossy(css).into_owned(),
        String::from_utf8_lossy(content).into_owned(),
    )
}

/// Names the content uses: what [`extract_classes`] finds, plus every
/// whitespace-separated word.
///
/// The extra words let the fuzzer mark names as used without first having
/// to discover `class="…"` syntax, which reaches the keep paths much sooner.
#[must_use]
pub fn used_names(content: &str) -> BTreeSet<&str> {
    let mut used = extract_classes(content);
    used.extend(content.split_ascii_whitespace());
    used
}

fn minified() -> PruneOptions {
    PruneOptions::default().minify(true)
}

/// Invariant 1: `prune` never panics, whatever the input. Errors are fine.
pub fn check_no_panic(css: &str, used: &BTreeSet<&str>) {
    for minify in [false, true] {
        let _ = prune(css, used, &PruneOptions::default().minify(minify));
    }
}

/// Invariant 2: pruning is idempotent.
///
/// Pruning the minified output again with the same used names must give
/// byte-identical output and remove nothing.
pub fn check_idempotent(css: &str, used: &BTreeSet<&str>) {
    if has_known_issue(css) {
        return;
    }
    let Ok(once) = prune(css, used, &minified()) else {
        return;
    };
    let twice = prune(&once.css, used, &minified()).unwrap_or_else(|err| {
        panic!(
            "pruned output no longer parses: {err}\noutput: {:?}",
            once.css
        )
    });
    assert!(
        same_output(css, &twice.css, &once.css),
        "prune is not idempotent\ninput: {css:?}\nonce:  {:?}\ntwice: {:?}",
        once.css,
        twice.css
    );
    assert_eq!(
        twice.removed_selectors, 0,
        "second prune removed selectors but printed the same CSS\nonce: {:?}",
        once.css
    );
    #[cfg(feature = "pretty")]
    check_pretty_printer(css, used, &once.css);
}

/// Stricter checks on the pretty (non-minified) printer, behind the `pretty`
/// feature because Lightning CSS currently fails them (bugs 3 and 4 in
/// `regressions/README.md`):
///
/// - pretty output is idempotent, like minified output;
/// - pruning the pretty output with `minify` gives the same CSS as
///   minifying directly, so the two printers agree.
#[cfg(feature = "pretty")]
fn check_pretty_printer(css: &str, used: &BTreeSet<&str>, minified_once: &str) {
    let pretty_options = PruneOptions::default();
    let Ok(pretty) = prune(css, used, &pretty_options) else {
        panic!("prune succeeded minified but failed pretty-printed\ninput: {css:?}");
    };
    let pretty_twice = prune(&pretty.css, used, &pretty_options).unwrap_or_else(|err| {
        panic!(
            "pretty output no longer parses: {err}\npretty: {:?}",
            pretty.css
        )
    });
    assert!(
        same_output(css, &pretty_twice.css, &pretty.css),
        "pretty-printed prune is not idempotent\ninput: {css:?}\nonce:  {:?}\ntwice: {:?}",
        pretty.css,
        pretty_twice.css
    );
    let from_pretty = prune(&pretty.css, used, &minified()).unwrap_or_else(|err| {
        panic!(
            "pretty output no longer parses: {err}\npretty: {:?}",
            pretty.css
        )
    });
    assert!(
        same_output(css, &from_pretty.css, minified_once),
        "pretty and minified output disagree\ninput:  {css:?}\npretty: {:?}\nvia pretty: {:?}\nminified:   {minified_once:?}",
        pretty.css,
        from_pretty.css
    );
}

/// Invariant 3: the output re-parses, and removal is stable.
///
/// Every class and id name left in the output is collected by parsing the
/// output with Lightning CSS (so escapes such as `.md\:flex` are handled
/// exactly as the pruner sees them). Pruning the output again with *all* of
/// those names marked used must keep everything: if a selector survived the
/// first pass, a used set that is a superset of everything it requires can
/// never remove it.
pub fn check_removal_is_stable(css: &str, used: &BTreeSet<&str>) {
    if has_known_issue(css) {
        return;
    }
    let Ok(once) = prune(css, used, &minified()) else {
        return;
    };
    let names = names_in(&once.css);
    let all: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    let again = prune(&once.css, &all, &minified()).unwrap_or_else(|err| {
        panic!(
            "pruned output no longer parses: {err}\noutput: {:?}",
            once.css
        )
    });
    assert_eq!(
        again.removed_selectors, 0,
        "re-pruning with every name in the output removed selectors\ninput:  {css:?}\nonce:   {:?}\nagain:  {:?}\nnames:  {all:?}",
        once.css, again.css
    );
    assert!(
        same_output(css, &again.css, &once.css),
        "re-pruning with every name in the output changed it\ninput: {css:?}\nonce:  {:?}\nagain: {:?}\nnames: {all:?}",
        once.css,
        again.css
    );
}

/// Inputs that hit an already-reported bug, skipped by the invariant checks
/// so fuzzing keeps finding new bugs instead of the same one. Panics are
/// never skipped: `check_no_panic` does not call this.
///
/// Each entry has a reproducer in `regressions/`. Delete the entry when the
/// bug is fixed, so the reproducer is checked again. The `no-skips`
/// feature turns every skip off.
#[must_use]
pub fn has_known_issue(css: &str) -> bool {
    if cfg!(feature = "no-skips") {
        return false;
    }
    // Bug 1 (regressions/README.md): Lightning CSS prints some identifiers
    // without re-escaping them: unknown at-rule names (`@a\.b` becomes
    // `@a.b`), unknown pseudo-class names (`:\/` becomes `:/`) and media
    // types (`sbre\/` becomes `sbre/`). The output then re-parses as
    // something else, or the rule is dropped. Class and id names are
    // escaped correctly, so escapes there are still fuzzed.
    escaped_identifier_outside_class_or_id(css)
        // Bug 7: numbers beyond f32/i32 range are printed clamped, and not
        // to the same value twice: `1e20px` prints as `2147483647px`, and
        // `99999999999999999999999999999999999999999px` prints as
        // `3.40282e38px`, which the next pass turns into `2147483647px`.
        || has_huge_number(css)
        // Bug 8: in unparsed values (custom properties, unknown properties)
        // Lightning CSS prints adjacent tokens without a separator and drops
        // a leading `+`, so a single pass can change a value:
        // `--x:1+2px` -> `--x:12px`, `x:a.0rem` -> `x:a0rem`,
        // `--y:a/**/b` -> `--y:ab`. Reported in regressions/README.md.
        || has_token_merge_risk(css)
        // Bug 9: control characters (NUL, DEL, …) inside unknown at-rules
        // and invalid declarations are printed raw, so the next parse reads
        // something else: `a{@media{x:}@\0` -> `a{x:@\u{fffd};}` ->
        // `a{x:@\u{fffd}}`. Real stylesheets don't contain them.
        || css.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\u{c}'))
}

/// Whether `css` has a token boundary that Lightning CSS may print without
/// a separator (bug 8). Narrow on purpose: `w-1.5` (digit, dot, digit) and
/// `.a.b` (letter, dot, letter) in selectors are still fuzzed.
fn has_token_merge_risk(css: &str) -> bool {
    let bytes = css.as_bytes();
    let adjacent = bytes.windows(3).any(|w| match w {
        // `a.0rem`: an identifier directly followed by a number.
        [l, b'.', d] if l.is_ascii_alphabetic() && d.is_ascii_digit() => true,
        // `..0rem`
        [b'.', b'.', d] if d.is_ascii_digit() => true,
        // `1+2px` / `1+.5px`: a number directly followed by a signed one.
        [d, b'+', n] if d.is_ascii_digit() && (n.is_ascii_digit() || *n == b'.') => true,
        _ => false,
    });
    // `a/**/b`: a comment used as the only separator between two tokens.
    let comment_separator = css.match_indices("/*").any(|(start, _)| {
        let before = bytes[..start].last().is_some_and(|c| !c.is_ascii_whitespace());
        let after = css[start + 2..]
            .find("*/")
            .and_then(|end| bytes.get(start + 2 + end + 2))
            .is_some_and(|c| !c.is_ascii_whitespace());
        before && after
    });
    adjacent || comment_separator
}

/// Whether `css` has a number of ten or more digits, or one with an
/// exponent, either of which can overflow Lightning CSS's `i32`/`f32`.
fn has_huge_number(css: &str) -> bool {
    let bytes = css.as_bytes();
    let long_run = bytes
        .split(|c| !c.is_ascii_digit())
        .any(|digits| digits.len() >= 10);
    let exponent = bytes.windows(3).any(|w| {
        w[0].is_ascii_digit()
            && matches!(w[1], b'e' | b'E')
            && (w[2].is_ascii_digit() || matches!(w[2], b'+' | b'-'))
    });
    long_run || exponent
}

/// Whether two outputs of the same input count as the same.
///
/// With the `no-skips` feature: byte equality. By default the
/// comparison ignores whitespace bugs already reported (regressions/README.md),
/// so fuzzing can look for real changes:
///
/// - Bug 2: a comment inside unparsed tokens (custom property, unknown
///   property, unknown at-rule prelude or block) is dropped but the
///   whitespace around it is kept, so `--x: a /**/ b` minifies to
///   `--x:a  b`, and a second pass collapses or trims it. Inputs with a
///   comment are compared with all whitespace removed.
/// - Bugs 5 and 6: `@container name{…}` (no query) and `@media ,screen`
///   (an empty query in the list) gain a space on the next pass. So the
///   length of a whitespace run is ignored, but a space appearing or
///   vanishing between two tokens still counts.
fn same_output(input: &str, a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if cfg!(feature = "no-skips") {
        return false;
    }
    if input.contains("/*") {
        let squeeze = |s: &str| {
            s.bytes()
                .filter(|c| !c.is_ascii_whitespace())
                .collect::<Vec<_>>()
        };
        return squeeze(a) == squeeze(b);
    }
    a.split_ascii_whitespace().eq(b.split_ascii_whitespace())
}

/// Whether `css` has a backslash escape in an identifier that is not
/// directly after `.` or `#` (so not a class or id name).
///
/// A rough tokenizer: it also flags escapes in strings and values, which
/// only means those inputs are skipped, never that a bug is hidden.
fn escaped_identifier_outside_class_or_id(css: &str) -> bool {
    let bytes = css.as_bytes();
    let is_ident = |c: u8| c == b'-' || c == b'_' || c.is_ascii_alphanumeric() || c >= 0x80;
    let mut i = 0;
    while i < bytes.len() {
        if !(is_ident(bytes[i]) || bytes[i] == b'\\') {
            i += 1;
            continue;
        }
        let start = i;
        let mut escaped = false;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                escaped = true;
                i += 2;
            } else if is_ident(bytes[i]) {
                i += 1;
            } else {
                break;
            }
        }
        let before = start.checked_sub(1).map(|p| bytes[p]);
        if escaped && !matches!(before, Some(b'.' | b'#')) {
            return true;
        }
    }
    false
}

/// Every class and id name in `css`, unescaped, including names inside
/// functional pseudo-classes and nested rules.
///
/// Panics if `css` (which is always prunecss output here) fails to parse.
#[must_use]
pub fn names_in(css: &str) -> BTreeSet<String> {
    let options = ParserOptions {
        error_recovery: true,
        ..ParserOptions::default()
    };
    let sheet = StyleSheet::parse(css, options)
        .unwrap_or_else(|err| panic!("pruned output does not parse: {err}\noutput: {css:?}"));
    let mut names = BTreeSet::new();
    collect_rules(&sheet.rules, &mut names);
    names
}

fn collect_rules<R>(rules: &CssRuleList<'_, R>, names: &mut BTreeSet<String>) {
    for rule in &rules.0 {
        match rule {
            CssRule::Style(style) => {
                collect_selectors(&style.selectors.0, names);
                collect_rules(&style.rules, names);
            }
            CssRule::Nesting(nesting) => {
                collect_selectors(&nesting.style.selectors.0, names);
                collect_rules(&nesting.style.rules, names);
            }
            CssRule::Media(group) => collect_rules(&group.rules, names),
            CssRule::Supports(group) => collect_rules(&group.rules, names),
            CssRule::Container(group) => collect_rules(&group.rules, names),
            CssRule::Scope(group) => collect_rules(&group.rules, names),
            CssRule::StartingStyle(group) => collect_rules(&group.rules, names),
            CssRule::MozDocument(group) => collect_rules(&group.rules, names),
            CssRule::LayerBlock(layer) => collect_rules(&layer.rules, names),
            // The pruner never looks inside other rules, so their names
            // cannot affect what it keeps.
            _ => {}
        }
    }
}

fn collect_selectors(selectors: &[Selector<'_>], names: &mut BTreeSet<String>) {
    for selector in selectors {
        for component in selector.iter_raw_match_order() {
            collect_component(component, names);
        }
    }
}

// Collecting more names than the pruner checks is harmless (the used set
// only grows), so this recurses into every selector-carrying component.
fn collect_component(component: &Component<'_>, names: &mut BTreeSet<String>) {
    match component {
        Component::Class(name) | Component::ID(name) => {
            names.insert(name.0.to_string());
        }
        Component::Is(list)
        | Component::Where(list)
        | Component::Any(_, list)
        | Component::Negation(list)
        | Component::Has(list) => collect_selectors(list, names),
        Component::NthOf(data) => collect_selectors(data.selectors(), names),
        Component::Slotted(selector) | Component::Host(Some(selector)) => {
            collect_selectors(std::slice::from_ref(selector), names);
        }
        _ => {}
    }
}
