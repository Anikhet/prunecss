//! Remove CSS rules whose selectors can never match the scanned content.
//!
//! Matching is a "bag of names" check, the same conservative model PurgeCSS
//! uses: a selector is kept when every class and id it *requires* appears in
//! the set of used names. Page structure (combinators like `>`), tags,
//! attributes and pseudo-classes are not checked, so they never cause a
//! removal. That keeps more CSS than strictly needed, but never deletes CSS a
//! page uses.

use std::collections::{BTreeSet, HashSet};
use std::hash::BuildHasher;

use lightningcss::rules::style::StyleRule;
use lightningcss::rules::{CssRule, CssRuleList};
use lightningcss::selector::{Component, Selector};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::targets::{Features, Targets};

/// A set of class and id names known to be used by the content.
///
/// Implemented for the standard sets so callers can pick whichever their
/// scanner produces.
pub trait UsedNames {
    /// Returns `true` if `name` (unescaped, without `.` or `#`) is used.
    fn contains_name(&self, name: &str) -> bool;
}

impl UsedNames for BTreeSet<&str> {
    fn contains_name(&self, name: &str) -> bool {
        self.contains(name)
    }
}

impl<S: BuildHasher> UsedNames for HashSet<&str, S> {
    fn contains_name(&self, name: &str) -> bool {
        self.contains(name)
    }
}

/// Options for [`prune`].
///
/// Construct with [`PruneOptions::default`] and the builder methods, so new
/// options can be added without breaking your code:
///
/// ```
/// let options = prunecss_core::PruneOptions::default().minify(true);
/// assert!(options.minify);
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct PruneOptions {
    /// Print without whitespace.
    ///
    /// Rule order and selectors are never changed. Lightning CSS's printer
    /// does write some values in an equivalent shorter form either way, for
    /// example `blue` as `#00f` and `::before` as `:before`.
    pub minify: bool,
}

impl PruneOptions {
    /// Sets [`PruneOptions::minify`].
    #[must_use]
    pub fn minify(mut self, minify: bool) -> Self {
        self.minify = minify;
        self
    }
}

/// The result of pruning one stylesheet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PruneOutput {
    /// The stylesheet with unused rules removed.
    pub css: String,
    /// How many selectors were removed, counting each selector in a list
    /// such as `.a, .b` separately.
    pub removed_selectors: usize,
}

/// What went wrong in [`prune`]. See [`PruneError::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PruneErrorKind {
    /// The stylesheet could not be parsed, even with error recovery.
    Parse,
    /// The pruned stylesheet could not be printed.
    Print,
}

/// A position in the input stylesheet. Both numbers start at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceLocation {
    /// Line number, starting at 1.
    pub line: u32,
    /// Column number, starting at 1.
    pub column: u32,
}

/// An error returned by [`prune`].
///
/// It carries the input position when one is known, so callers can point at
/// the problem. No Lightning CSS types appear here: that crate is still an
/// alpha, and its types would tie our public API to its releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneError {
    kind: PruneErrorKind,
    message: String,
    location: Option<SourceLocation>,
}

impl PruneError {
    fn from_lightningcss<T: std::fmt::Display>(
        kind: PruneErrorKind,
        err: &lightningcss::error::Error<T>,
    ) -> Self {
        Self {
            kind,
            message: err.kind.to_string(),
            location: err.loc.as_ref().map(|loc| SourceLocation {
                // Lightning CSS counts lines from 0 and columns from 1.
                line: loc.line.saturating_add(1),
                column: loc.column,
            }),
        }
    }

    /// What kind of failure this was.
    #[must_use]
    pub fn kind(&self) -> PruneErrorKind {
        self.kind
    }

    /// A description of the problem, without the location.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Where in the input stylesheet the problem is, if known.
    #[must_use]
    pub fn location(&self) -> Option<SourceLocation> {
        self.location
    }
}

impl std::fmt::Display for PruneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let action = match self.kind {
            PruneErrorKind::Parse => "could not parse CSS",
            PruneErrorKind::Print => "could not print CSS",
        };
        match self.location {
            Some(loc) => write!(
                f,
                "{action} at line {}, column {}: {}",
                loc.line, loc.column, self.message
            ),
            None => write!(f, "{action}: {}", self.message),
        }
    }
}

impl std::error::Error for PruneError {}

/// Removes every rule and selector in `css` that can never match `used`.
///
/// # Errors
///
/// Returns a [`PruneErrorKind::Parse`] error if the stylesheet is too broken
/// to recover, and a [`PruneErrorKind::Print`] error if printing fails.
///
/// ```
/// use std::collections::BTreeSet;
/// use prunecss_core::{prune, PruneOptions};
///
/// let used = BTreeSet::from(["btn"]);
/// let css = ".btn { color: red } .unused { color: blue }";
/// let output = prune(css, &used, &PruneOptions::default().minify(true))?;
/// assert_eq!(output.css, ".btn{color:red}");
/// # Ok::<(), prunecss_core::PruneError>(())
/// ```
pub fn prune(
    css: &str,
    used: &impl UsedNames,
    options: &PruneOptions,
) -> Result<PruneOutput, PruneError> {
    let parser_options = ParserOptions {
        // Skip a broken rule with a warning instead of failing the whole
        // file; real-world stylesheets often contain a few.
        error_recovery: true,
        ..ParserOptions::default()
    };
    let mut sheet = StyleSheet::parse(css, parser_options)
        .map_err(|err| PruneError::from_lightningcss(PruneErrorKind::Parse, &err))?;

    let mut removed_selectors = 0;
    prune_rules(&mut sheet.rules, used, &mut removed_selectors);

    // Printing only; `StyleSheet::minify` is deliberately not called because
    // it also merges rules and rewrites values, which is not our job.
    let printer_options = PrinterOptions {
        minify: options.minify,
        targets: Targets {
            // Lightning CSS normalises `(min-width: 768px)` to the newer
            // `(width >= 768px)` range syntax, which older browsers ignore.
            // Always print the widely supported form instead.
            include: Features::MediaQueries,
            ..Targets::default()
        },
        ..PrinterOptions::default()
    };
    let printed = sheet
        .to_css(printer_options)
        .map_err(|err| PruneError::from_lightningcss(PruneErrorKind::Print, &err))?;

    Ok(PruneOutput {
        css: printed.code,
        removed_selectors,
    })
}

fn prune_rules<R>(rules: &mut CssRuleList<'_, R>, used: &impl UsedNames, removed: &mut usize) {
    rules.0.retain_mut(|rule| keep_rule(rule, used, removed));
}

/// Prunes inside `rule` and returns whether the rule itself should stay.
fn keep_rule<R>(rule: &mut CssRule<'_, R>, used: &impl UsedNames, removed: &mut usize) -> bool {
    match rule {
        CssRule::Style(style) => keep_style_rule(style, used, removed),
        CssRule::Nesting(nesting) => keep_style_rule(&mut nesting.style, used, removed),
        CssRule::Media(group) => keep_group(&mut group.rules, used, removed),
        CssRule::Supports(group) => keep_group(&mut group.rules, used, removed),
        CssRule::Container(group) => keep_group(&mut group.rules, used, removed),
        CssRule::Scope(group) => keep_group(&mut group.rules, used, removed),
        CssRule::StartingStyle(group) => keep_group(&mut group.rules, used, removed),
        CssRule::MozDocument(group) => keep_group(&mut group.rules, used, removed),
        CssRule::LayerBlock(layer) => {
            prune_rules(&mut layer.rules, used, removed);
            // A named layer fixes its position in the cascade order even
            // when empty, so removing it could change which styles win.
            layer.name.is_some() || !layer.rules.0.is_empty()
        }
        // Keyframes, fonts, @property, @import and the rest have no selectors.
        // They need usage tracking before they can be removed safely.
        _ => true,
    }
}

/// Prunes a grouping rule such as `@media`, keeping it unless pruning
/// emptied it.
fn keep_group<R>(
    rules: &mut CssRuleList<'_, R>,
    used: &impl UsedNames,
    removed: &mut usize,
) -> bool {
    let was_empty = rules.0.is_empty();
    prune_rules(rules, used, removed);
    was_empty || !rules.0.is_empty()
}

fn keep_style_rule<R>(
    style: &mut StyleRule<'_, R>,
    used: &impl UsedNames,
    removed: &mut usize,
) -> bool {
    let before = style.selectors.0.len();
    style
        .selectors
        .0
        .retain(|selector| selector_can_match(selector, used));
    *removed += before - style.selectors.0.len();

    if style.selectors.0.is_empty() {
        // Nested rules are relative to this one, so they go with it.
        *removed += count_selectors(&style.rules);
        return false;
    }
    prune_rules(&mut style.rules, used, removed);
    true
}

fn selector_can_match(selector: &Selector<'_>, used: &impl UsedNames) -> bool {
    selector
        .iter_raw_match_order()
        .all(|component| component_can_match(component, used))
}

fn component_can_match(component: &Component<'_>, used: &impl UsedNames) -> bool {
    match component {
        // Lightning CSS stores names unescaped, so `.md\:flex` arrives here
        // as `md:flex`, the same text the scanner finds in markup.
        Component::Class(name) | Component::ID(name) => used.contains_name(&name.0),
        // One matching alternative is enough.
        Component::Is(list) | Component::Where(list) | Component::Any(_, list) => list
            .iter()
            .any(|selector| selector_can_match(selector, used)),
        // `:not()`, `:has()`, `:nth-child(… of …)`, tags, attributes and
        // pseudo-classes never cause a removal: matching them needs the
        // rendered page, and guessing wrong would delete CSS that is used.
        // `&` always matches because nested rules are only visited when
        // their parent was kept.
        _ => true,
    }
}

fn count_selectors<R>(rules: &CssRuleList<'_, R>) -> usize {
    rules
        .0
        .iter()
        .map(|rule| match rule {
            CssRule::Style(style) => style.selectors.0.len() + count_selectors(&style.rules),
            CssRule::Nesting(nesting) => {
                nesting.style.selectors.0.len() + count_selectors(&nesting.style.rules)
            }
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn prune_min(css: &str, used: &[&str]) -> String {
        let used: BTreeSet<&str> = used.iter().copied().collect();
        match prune(css, &used, &PruneOptions::default().minify(true)) {
            Ok(output) => output.css,
            Err(err) => panic!("prune failed: {err}"),
        }
    }

    #[rstest]
    #[case::used_class(".a{color:red}", &["a"], ".a{color:red}")]
    #[case::unused_class(".b{color:red}", &["a"], "")]
    #[case::used_id("#nav{color:red}", &["nav"], "#nav{color:red}")]
    #[case::unused_id("#nav{color:red}", &[], "")]
    #[case::element_only("p{color:red}", &[], "p{color:red}")]
    #[case::universal("*{margin:0}", &[], "*{margin:0}")]
    #[case::root(":root{--x:1px}", &[], ":root{--x:1px}")]
    #[case::attribute_only("[hidden]{display:none}", &[], "[hidden]{display:none}")]
    fn prune_should_decide_simple_selectors(
        #[case] css: &str,
        #[case] used: &[&str],
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, used), expected);
    }

    #[rstest]
    #[case::keeps_used_selector_in_list(".a,.b{color:red}", &["a"], ".a{color:red}")]
    #[case::compound_needs_every_class(".a.b{color:red}", &["a"], "")]
    #[case::compound_with_every_class(".a.b{color:red}", &["a", "b"], ".a.b{color:red}")]
    #[case::descendant_needs_both(".a .b{color:red}", &["a"], "")]
    #[case::child_with_both(".a>.b{color:red}", &["a", "b"], ".a>.b{color:red}")]
    #[case::tag_with_class("a.btn{color:red}", &["btn"], "a.btn{color:red}")]
    fn prune_should_handle_selector_lists_and_combinators(
        #[case] css: &str,
        #[case] used: &[&str],
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, used), expected);
    }

    // Rule 1: names are compared unescaped, so Tailwind-style classes survive.
    #[rstest]
    #[case::escaped_colon(r".md\:flex{display:flex}", "md:flex", r".md\:flex{display:flex}")]
    #[case::escaped_slash(r".w-1\/2{width:50%}", "w-1/2", r".w-1\/2{width:50%}")]
    #[case::escaped_brackets(r".w-\[10px\]{width:10px}", "w-[10px]", r".w-\[10px\]{width:10px}")]
    #[case::hex_escaped_digit(r".\31 0{width:10px}", "10", r".\31 0{width:10px}")]
    fn prune_should_match_escaped_class_names(
        #[case] css: &str,
        #[case] used: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, &[used]), expected);
    }

    // Rule 4: transient states and pseudo-elements never cause a removal.
    #[rstest]
    #[case::hover(".a:hover{color:red}", ".a:hover{color:red}")]
    #[case::focus_visible(".a:focus-visible{color:red}", ".a:focus-visible{color:red}")]
    // The printer writes the equivalent legacy `:before` form.
    #[case::before(r#".a::before{content:"x"}"#, r#".a:before{content:"x"}"#)]
    fn prune_should_ignore_pseudo_classes_and_elements(#[case] css: &str, #[case] expected: &str) {
        assert_eq!(prune_min(css, &["a"]), expected);
    }

    // Rule 3: functional pseudo-classes.
    #[rstest]
    #[case::not_keeps_inner(".a:not(.zzz){color:red}", &["a"], ".a:not(.zzz){color:red}")]
    #[case::has_keeps_inner(".a:has(.zzz){color:red}", &["a"], ".a:has(.zzz){color:red}")]
    #[case::nth_of_keeps_inner(
        ".a:nth-child(2 of .zzz){color:red}",
        &["a"],
        ".a:nth-child(2 of .zzz){color:red}"
    )]
    #[case::is_with_one_used(":is(.a,.b) p{color:red}", &["a"], ":is(.a,.b) p{color:red}")]
    #[case::is_with_none_used(":is(.a,.b) p{color:red}", &[], "")]
    #[case::where_with_none_used(":where(.a,.b){color:red}", &[], "")]
    fn prune_should_handle_functional_pseudo_classes(
        #[case] css: &str,
        #[case] used: &[&str],
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, used), expected);
    }

    #[rstest]
    #[case::media_kept("@media (min-width:1px){.a{color:red}}", &["a"], "@media (min-width:1px){.a{color:red}}")]
    #[case::media_emptied("@media (min-width:1px){.b{color:red}}", &[], "")]
    #[case::media_partly(
        "@media print{.a{color:red}.b{color:blue}}",
        &["a"],
        "@media print{.a{color:red}}"
    )]
    #[case::supports_emptied("@supports (display:grid){.b{display:grid}}", &[], "")]
    #[case::container_emptied("@container (width>1px){.b{color:red}}", &[], "")]
    #[case::scope_emptied("@scope (.card){.b{color:red}}", &[], "")]
    #[case::starting_style_emptied("@starting-style{.b{opacity:0}}", &[], "")]
    #[case::moz_document_emptied("@-moz-document url-prefix(){.b{color:red}}", &[], "")]
    #[case::scope_kept("@scope (.card){.a{color:red}}", &["a"], "@scope(.card){.a{color:red}}")]
    fn prune_should_prune_inside_grouping_rules(
        #[case] css: &str,
        #[case] used: &[&str],
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, used), expected);
    }

    // Printing must not upgrade media queries to the newer range syntax:
    // `(width >= 768px)` breaks in Safari before 16.4.
    #[rstest]
    #[case::min_width(
        "@media (min-width:768px){.a{color:red}}",
        "@media (min-width:768px){.a{color:red}}"
    )]
    #[case::max_width(
        "@media (max-width:767.98px){.a{color:red}}",
        "@media (max-width:767.98px){.a{color:red}}"
    )]
    fn prune_should_keep_media_queries_compatible(#[case] css: &str, #[case] expected: &str) {
        assert_eq!(prune_min(css, &["a"]), expected);
    }

    // Rule 7: layer order affects the cascade, so named layers always stay.
    #[rstest]
    #[case::named_layer_emptied("@layer base{.b{color:red}}", "@layer base{}")]
    #[case::layer_statement("@layer a,b;", "@layer a,b;")]
    #[case::anonymous_layer_emptied("@layer{.b{color:red}}", "")]
    fn prune_should_keep_layer_order(#[case] css: &str, #[case] expected: &str) {
        assert_eq!(prune_min(css, &[]), expected);
    }

    // Rule 8: nested rules follow their parent.
    #[rstest]
    #[case::nested_hover_kept(
        ".a{color:red;&:hover{color:red}}",
        &["a"],
        ".a{color:red;&:hover{color:red}}"
    )]
    #[case::nested_removed_with_parent(".b{color:red;&:hover{color:blue}}", &[], "")]
    #[case::nested_unused_child(".a{color:red;& .c{color:blue}}", &["a"], ".a{color:red}")]
    fn prune_should_resolve_nesting(
        #[case] css: &str,
        #[case] used: &[&str],
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, used), expected);
    }

    // Keyframes, fonts and variables need usage tracking (rules 5 and 6).
    // Until that exists they are always kept, never guessed at.
    #[rstest]
    #[case::keyframes("@keyframes spin{0%{opacity:0}}")]
    #[case::font_face("@font-face{font-family:X;src:url(x.woff2)}")]
    #[case::property("@property --x{syntax:\"<length>\";inherits:false;initial-value:0}")]
    fn prune_should_keep_named_at_rules_for_now(#[case] css: &str) {
        assert!(!prune_min(css, &[]).is_empty());
    }

    #[test]
    fn prune_should_accept_a_hash_set_of_used_names() {
        let used: HashSet<&str> = HashSet::from(["a"]);
        let output = prune(
            ".a{color:red}.b{color:red}",
            &used,
            &PruneOptions::default().minify(true),
        )
        .map(|output| output.css)
        .ok();
        assert_eq!(output.as_deref(), Some(".a{color:red}"));
    }

    #[test]
    fn prune_should_count_nested_selectors_removed_with_their_parent() {
        let used = BTreeSet::from(["a"]);
        let css = ".b{color:red;&:hover{color:red}& .c,& .d{color:red}@media print{color:red}}";
        let removed = prune(css, &used, &PruneOptions::default())
            .map(|output| output.removed_selectors)
            .ok();
        assert_eq!(removed, Some(4));
    }

    // The older `@nest` spelling of nesting.
    #[test]
    fn prune_should_remove_unused_legacy_nest_rules() {
        let css = ".a{color:red;@nest .x &{color:red}}";
        assert_eq!(prune_min(css, &["a"]), ".a{color:red}");
    }

    #[test]
    fn prune_should_keep_used_legacy_nest_rules() {
        let css = ".a{color:red;@nest .x &{color:red}}";
        assert!(prune_min(css, &["a", "x"]).contains(".x"));
    }

    #[test]
    fn prune_should_count_legacy_nest_selectors_removed_with_their_parent() {
        let used = BTreeSet::from(["a"]);
        let css = ".b{color:red;@nest .x &,.y &{color:red}}";
        let removed = prune(css, &used, &PruneOptions::default())
            .map(|output| output.removed_selectors)
            .ok();
        assert_eq!(removed, Some(3));
    }

    #[test]
    fn prune_should_count_removed_selectors_individually() {
        let used = BTreeSet::from(["a"]);
        let css = ".a,.b,.c{color:red}.d{color:blue}";
        let removed = prune(css, &used, &PruneOptions::default())
            .map(|output| output.removed_selectors)
            .ok();
        assert_eq!(removed, Some(3));
    }

    // Browsers skip a rule with an invalid selector and carry on; so do we.
    #[test]
    fn prune_should_skip_rules_with_invalid_selectors() {
        let css = ".a{color:red} .b..x{color:red} .c{color:red}";
        assert_eq!(
            prune_min(css, &["a", "b", "c"]),
            ".a{color:red}.c{color:red}"
        );
    }

    // Fixing broken declarations is not our job; leave them as written.
    #[test]
    fn prune_should_leave_invalid_declarations_untouched() {
        let css = ".a{color:;width:1px}";
        assert_eq!(prune_min(css, &["a"]), css);
    }

    #[test]
    fn prune_should_print_readable_css_when_not_minifying() {
        let used = BTreeSet::from(["a"]);
        let css = ".a{color:red}";
        let output = prune(css, &used, &PruneOptions::default())
            .map(|o| o.css)
            .ok();
        assert_eq!(output.as_deref(), Some(".a {\n  color: red;\n}\n"));
    }

    fn lightningcss_error(loc: Option<(u32, u32)>) -> lightningcss::error::Error<&'static str> {
        lightningcss::error::Error {
            kind: "unexpected token",
            loc: loc.map(|(line, column)| lightningcss::error::ErrorLocation {
                filename: String::new(),
                line,
                column,
            }),
        }
    }

    #[test]
    fn prune_error_should_convert_lines_to_start_at_one() {
        let err =
            PruneError::from_lightningcss(PruneErrorKind::Parse, &lightningcss_error(Some((0, 7))));
        assert_eq!(err.location(), Some(SourceLocation { line: 1, column: 7 }));
    }

    #[test]
    fn prune_error_should_display_its_location() {
        let err =
            PruneError::from_lightningcss(PruneErrorKind::Parse, &lightningcss_error(Some((2, 4))));
        assert_eq!(
            err.to_string(),
            "could not parse CSS at line 3, column 4: unexpected token"
        );
    }

    #[test]
    fn prune_error_should_display_without_a_location() {
        let err = PruneError::from_lightningcss(PruneErrorKind::Print, &lightningcss_error(None));
        assert_eq!(err.to_string(), "could not print CSS: unexpected token");
    }

    #[test]
    fn prune_error_should_expose_kind_and_message() {
        let err = PruneError::from_lightningcss(PruneErrorKind::Print, &lightningcss_error(None));
        assert_eq!(
            (err.kind(), err.message()),
            (PruneErrorKind::Print, "unexpected token")
        );
    }

    // With error recovery on, broken input is skipped the way browsers skip
    // it instead of failing the whole file.
    #[rstest]
    #[case::unclosed_block(".a{color:red", ".a{color:red}")]
    #[case::stray_brace(".a{color:red}}", ".a{color:red}")]
    #[case::incomplete_at_rule("@media (", "")]
    #[case::import_after_rules(".a{color:red}@import url(x.css);", ".a{color:red}")]
    #[case::nul_byte("\u{0}", "")]
    fn prune_should_recover_from_broken_stylesheets(#[case] css: &str, #[case] expected: &str) {
        assert_eq!(prune_min(css, &["a"]), expected);
    }

    mod properties {
        use super::*;
        use proptest::prelude::*;

        /// Small stylesheets built from a fixed alphabet of class names.
        fn stylesheet() -> impl Strategy<Value = String> {
            let selector = prop::sample::select(vec![".a", ".b", ".c", ".a .b", ".b:hover", "p"]);
            prop::collection::vec(selector, 0..8).prop_map(|selectors| {
                selectors
                    .iter()
                    .map(|s| [*s, "{color:red}"].concat())
                    .collect::<String>()
            })
        }

        fn used_names() -> impl Strategy<Value = Vec<&'static str>> {
            prop::sample::subsequence(vec!["a", "b", "c"], 0..=3)
        }

        proptest! {
            #[test]
            fn prune_never_panics_on_arbitrary_text(css in ".{0,200}") {
                let used = BTreeSet::from(["a"]);
                let _ = prune(&css, &used, &PruneOptions::default());
            }

            #[test]
            fn prune_is_idempotent(css in stylesheet(), used in used_names()) {
                let once = prune_min(&css, &used);
                prop_assert_eq!(prune_min(&once, &used), once);
            }

            #[test]
            fn prune_keeps_everything_when_every_name_is_used(css in stylesheet()) {
                let kept = prune_min(&css, &["a", "b", "c"]);
                let original = prune_min(&css, &["a", "b", "c", "unrelated"]);
                prop_assert_eq!(kept.matches('{').count(), css.matches('{').count());
                prop_assert_eq!(kept, original);
            }
        }
    }
}
