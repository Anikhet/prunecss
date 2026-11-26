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

use crate::safelist::{Pattern, Safelist, any_matches};
use crate::{rules, tracking};

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
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent on/off switches, mirroring PurgeCSS's options"
)]
pub struct PruneOptions {
    /// Print without whitespace.
    ///
    /// Rule order and selectors are never changed. Lightning CSS's printer
    /// does write some values in an equivalent shorter form either way, for
    /// example `blue` as `#00f` and `::before` as `:before`.
    pub minify: bool,
    /// Names to keep whatever the content says.
    pub safelist: Safelist,
    /// Remove `@keyframes` that no kept rule and no content file mentions.
    ///
    /// Off by default, like PurgeCSS. Animations started from JavaScript by
    /// name stay safe because content files are checked too.
    pub keyframes: bool,
    /// Remove `@font-face` rules whose family no kept rule and no content
    /// file mentions. Off by default.
    pub font_face: bool,
    /// Remove custom property declarations (`--name: …`) that nothing reads
    /// through `var()`, following chains of variables. Content files count
    /// as readers, so variables read from JavaScript are kept. Off by default.
    pub variables: bool,
}

impl PruneOptions {
    /// Sets [`PruneOptions::minify`].
    #[must_use]
    pub fn minify(mut self, minify: bool) -> Self {
        self.minify = minify;
        self
    }

    /// Sets [`PruneOptions::safelist`].
    #[must_use]
    pub fn safelist(mut self, safelist: Safelist) -> Self {
        self.safelist = safelist;
        self
    }

    /// Sets [`PruneOptions::keyframes`].
    #[must_use]
    pub fn keyframes(mut self, enabled: bool) -> Self {
        self.keyframes = enabled;
        self
    }

    /// Sets [`PruneOptions::font_face`].
    #[must_use]
    pub fn font_face(mut self, enabled: bool) -> Self {
        self.font_face = enabled;
        self
    }

    /// Sets [`PruneOptions::variables`].
    #[must_use]
    pub fn variables(mut self, enabled: bool) -> Self {
        self.variables = enabled;
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
    /// How many `@keyframes` rules were removed.
    pub removed_keyframes: usize,
    /// How many `@font-face` rules were removed.
    pub removed_font_faces: usize,
    /// How many custom property declarations were removed.
    pub removed_variables: usize,
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

    let matcher = Matcher {
        used,
        safelist: &options.safelist,
    };
    let mut removed_selectors = 0;
    prune_rules(&mut sheet.rules, &matcher, &mut removed_selectors);

    // Keyframes, fonts and variables are judged against what survived, so
    // this runs after selector pruning: a keyframe used only by a removed
    // rule is unused.
    let tracked = tracking::remove_unused(
        &mut sheet.rules,
        used,
        &options.safelist,
        tracking::Enabled {
            keyframes: options.keyframes,
            font_face: options.font_face,
            variables: options.variables,
        },
    );

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
        removed_keyframes: tracked.keyframes,
        removed_font_faces: tracked.font_faces,
        removed_variables: tracked.variables,
    })
}

/// Returns every class and id name that selectors in `css` mention,
/// unescaped and without `.` or `#`, in sorted order.
///
/// Useful for reports such as "which of my classes does the content use".
///
/// ```
/// let names = prunecss_core::defined_names(".btn, #nav .md\\:flex:not(.x) {}")?;
/// assert_eq!(names.into_iter().collect::<Vec<_>>(), ["btn", "md:flex", "nav", "x"]);
/// # Ok::<(), prunecss_core::PruneError>(())
/// ```
///
/// # Errors
///
/// Returns a [`PruneErrorKind::Parse`] error if the stylesheet cannot be
/// parsed even with error recovery.
pub fn defined_names(css: &str) -> Result<BTreeSet<String>, PruneError> {
    let parser_options = ParserOptions {
        error_recovery: true,
        ..ParserOptions::default()
    };
    let sheet = StyleSheet::parse(css, parser_options)
        .map_err(|err| PruneError::from_lightningcss(PruneErrorKind::Parse, &err))?;
    let mut names = BTreeSet::new();
    collect_rule_names(&sheet.rules, &mut names);
    Ok(names)
}

fn collect_rule_names<R>(rules: &CssRuleList<'_, R>, names: &mut BTreeSet<String>) {
    for rule in &rules.0 {
        if let Some(group) = rules::group_rules(rule) {
            collect_rule_names(group, names);
            continue;
        }
        let style = match rule {
            CssRule::Style(style) => style,
            CssRule::Nesting(nesting) => &nesting.style,
            CssRule::LayerBlock(layer) => {
                collect_rule_names(&layer.rules, names);
                continue;
            }
            _ => continue,
        };
        for selector in &style.selectors.0 {
            collect_selector_names(selector, names);
        }
        collect_rule_names(&style.rules, names);
    }
}

fn collect_selector_names(selector: &Selector<'_>, names: &mut BTreeSet<String>) {
    for component in selector.iter_raw_match_order() {
        match component {
            Component::Class(name) | Component::ID(name) => {
                names.insert(name.0.to_string());
            }
            Component::Is(list)
            | Component::Where(list)
            | Component::Any(_, list)
            | Component::Negation(list)
            | Component::Has(list) => {
                for inner in list {
                    collect_selector_names(inner, names);
                }
            }
            Component::NthOf(nth) => {
                for inner in nth.selectors() {
                    collect_selector_names(inner, names);
                }
            }
            Component::Slotted(inner) | Component::Host(Some(inner)) => {
                collect_selector_names(inner, names);
            }
            _ => {}
        }
    }
}

/// Decides whether a class or id name keeps a selector alive.
struct Matcher<'a, U> {
    used: &'a U,
    safelist: &'a Safelist,
}

impl<U: UsedNames> Matcher<'_, U> {
    fn is_used(&self, name: &str) -> bool {
        self.used.contains_name(name) || any_matches(&self.safelist.standard, name)
    }
}

fn prune_rules<R, U: UsedNames>(
    rules: &mut CssRuleList<'_, R>,
    matcher: &Matcher<'_, U>,
    removed: &mut usize,
) {
    rules.0.retain_mut(|rule| keep_rule(rule, matcher, removed));
}

/// Prunes inside `rule` and returns whether the rule itself should stay.
fn keep_rule<R, U: UsedNames>(
    rule: &mut CssRule<'_, R>,
    matcher: &Matcher<'_, U>,
    removed: &mut usize,
) -> bool {
    if let Some(group) = rules::group_rules_mut(rule) {
        return rules::keep_group(group, |group| prune_rules(group, matcher, removed));
    }
    match rule {
        CssRule::Style(style) => keep_style_rule(style, matcher, removed),
        CssRule::Nesting(nesting) => keep_style_rule(&mut nesting.style, matcher, removed),
        CssRule::LayerBlock(layer) => {
            prune_rules(&mut layer.rules, matcher, removed);
            // A named layer fixes its position in the cascade order even
            // when empty, so removing it could change which styles win.
            layer.name.is_some() || !layer.rules.0.is_empty()
        }
        // Keyframes, fonts, @property, @import and the rest have no
        // selectors; `tracking` handles the ones that can be removed.
        _ => true,
    }
}

fn keep_style_rule<R, U: UsedNames>(
    style: &mut StyleRule<'_, R>,
    matcher: &Matcher<'_, U>,
    removed: &mut usize,
) -> bool {
    let safelist = matcher.safelist;
    let keeps_children = style
        .selectors
        .0
        .iter()
        .any(|selector| mentions(selector, &safelist.deep));

    let before = style.selectors.0.len();
    style.selectors.0.retain(|selector| {
        mentions(selector, &safelist.greedy)
            || mentions(selector, &safelist.deep)
            || selector_can_match(selector, matcher)
    });
    *removed += before - style.selectors.0.len();

    if style.selectors.0.is_empty() {
        // Nested rules are relative to this one, so they go with it.
        *removed += count_selectors(&style.rules);
        return false;
    }
    if !keeps_children {
        prune_rules(&mut style.rules, matcher, removed);
    }
    true
}

fn selector_can_match<U: UsedNames>(selector: &Selector<'_>, matcher: &Matcher<'_, U>) -> bool {
    selector
        .iter_raw_match_order()
        .all(|component| component_can_match(component, matcher))
}

fn component_can_match<U: UsedNames>(component: &Component<'_>, matcher: &Matcher<'_, U>) -> bool {
    match component {
        // Lightning CSS stores names unescaped, so `.md\:flex` arrives here
        // as `md:flex`, the same text the scanner finds in markup.
        Component::Class(name) | Component::ID(name) => matcher.is_used(&name.0),
        // One matching alternative is enough.
        Component::Is(list) | Component::Where(list) | Component::Any(_, list) => list
            .iter()
            .any(|selector| selector_can_match(selector, matcher)),
        // `:not()`, `:has()`, `:nth-child(… of …)`, tags, attributes and
        // pseudo-classes never cause a removal: matching them needs the
        // rendered page, and guessing wrong would delete CSS that is used.
        // `&` always matches because nested rules are only visited when
        // their parent was kept.
        _ => true,
    }
}

/// Returns `true` if a class or id the selector requires matches one of
/// `patterns`. Names inside `:not()` and `:has()` don't count: they describe
/// what the element is *not* or what it contains.
fn mentions(selector: &Selector<'_>, patterns: &[Pattern]) -> bool {
    !patterns.is_empty()
        && selector
            .iter_raw_match_order()
            .any(|component| match component {
                Component::Class(name) | Component::ID(name) => any_matches(patterns, &name.0),
                Component::Is(list) | Component::Where(list) | Component::Any(_, list) => {
                    list.iter().any(|inner| mentions(inner, patterns))
                }
                _ => false,
            })
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

    fn prune_with(css: &str, used: &[&str], options: &PruneOptions) -> PruneOutput {
        let used: BTreeSet<&str> = used.iter().copied().collect();
        match prune(css, &used, &options.clone().minify(true)) {
            Ok(output) => output,
            Err(err) => panic!("prune failed: {err}"),
        }
    }

    fn safelist_options(safelist: Safelist) -> PruneOptions {
        PruneOptions::default().safelist(safelist)
    }

    fn exact(name: &str) -> Pattern {
        Pattern::exact(name)
    }

    fn regex(source: &str) -> Pattern {
        Pattern::regex(source).unwrap_or_else(|err| panic!("{err}"))
    }

    #[rstest]
    #[case::exact_name(".is-open{color:red}", Safelist::default().standard(exact("is-open")), ".is-open{color:red}")]
    #[case::regex_name(".btn-x{color:red}", Safelist::default().standard(regex("^btn-")), ".btn-x{color:red}")]
    #[case::escaped_name(r".md\:flex{display:flex}", Safelist::default().standard(exact("md:flex")), r".md\:flex{display:flex}")]
    #[case::still_needs_other_classes(".is-open .menu{color:red}", Safelist::default().standard(exact("is-open")), "")]
    #[case::id(".x,#nav{color:red}", Safelist::default().standard(exact("nav")), "#nav{color:red}")]
    fn standard_safelist_should_count_names_as_used(
        #[case] css: &str,
        #[case] safelist: Safelist,
        #[case] expected: &str,
    ) {
        assert_eq!(
            prune_with(css, &[], &safelist_options(safelist)).css,
            expected
        );
    }

    #[test]
    fn greedy_safelist_should_keep_selectors_containing_a_match() {
        let options = safelist_options(Safelist::default().greedy(regex("^swiper")));
        let css = ".swiper-slide .unused{color:red}";
        assert_eq!(prune_with(css, &[], &options).css, css);
    }

    #[test]
    fn greedy_safelist_should_still_prune_nested_rules() {
        let options = safelist_options(Safelist::default().greedy(exact("g")));
        let css = ".g{color:red;& .unused{color:blue}}";
        assert_eq!(prune_with(css, &[], &options).css, ".g{color:red}");
    }

    #[test]
    fn deep_safelist_should_keep_nested_rules_too() {
        let options = safelist_options(Safelist::default().deep(exact("prose")));
        let css = ".prose{color:red;& .unused{color:red}}";
        assert_eq!(prune_with(css, &[], &options).css, css);
    }

    #[test]
    fn safelist_should_not_look_inside_not() {
        let options = safelist_options(Safelist::default().greedy(exact("x")));
        assert_eq!(prune_with(".b:not(.x){color:red}", &[], &options).css, "");
    }

    // Keyframes (rule 5): referenced through any property, including
    // prefixed and shorthand forms, or from content.
    #[rstest]
    #[case::animation_name(".a{animation-name:spin}@keyframes spin{to{opacity:0}}")]
    #[case::animation_shorthand(
        ".a{animation:spin 1s linear infinite}@keyframes spin{to{opacity:0}}"
    )]
    #[case::webkit_prefixed(".a{-webkit-animation:spin 1s}@-webkit-keyframes spin{to{opacity:0}}")]
    #[case::through_variable(
        ":root{--anim:spin 1s}.a{animation:var(--anim)}@keyframes spin{to{opacity:0}}"
    )]
    #[case::nested_in_media("@media print{.a{animation:spin 1s}@keyframes spin{to{opacity:0}}}")]
    fn keyframes_should_be_kept_when_referenced(#[case] css: &str) {
        let output = prune_with(css, &["a"], &PruneOptions::default().keyframes(true));
        assert_eq!(output.removed_keyframes, 0);
    }

    #[test]
    fn keyframes_should_be_removed_when_unreferenced() {
        let options = PruneOptions::default().keyframes(true);
        let css = ".a{color:red}@keyframes spin{to{opacity:0}}";
        assert_eq!(prune_with(css, &["a"], &options).css, ".a{color:red}");
    }

    #[test]
    fn keyframes_should_be_removed_when_only_removed_rules_used_them() {
        let options = PruneOptions::default().keyframes(true);
        let css = ".gone{animation:spin 1s}@keyframes spin{to{opacity:0}}";
        assert_eq!(prune_with(css, &[], &options).css, "");
    }

    #[test]
    fn keyframes_should_be_kept_when_content_mentions_them() {
        // e.g. `el.style.animationName = "spin"` in a script.
        let options = PruneOptions::default().keyframes(true);
        let css = "@keyframes spin{to{opacity:0}}";
        assert_eq!(prune_with(css, &["spin"], &options).removed_keyframes, 0);
    }

    #[test]
    fn keyframes_should_be_kept_by_default() {
        let css = "@keyframes spin{to{opacity:0}}";
        assert_eq!(
            prune_with(css, &[], &PruneOptions::default()).removed_keyframes,
            0
        );
    }

    #[test]
    fn keyframes_safelist_should_keep_unreferenced_keyframes() {
        let options = PruneOptions::default()
            .keyframes(true)
            .safelist(Safelist::default().keyframes(exact("spin")));
        let css = "@keyframes spin{to{opacity:0}}";
        assert_eq!(prune_with(css, &[], &options).removed_keyframes, 0);
    }

    // Fonts (rule 5): `font-family` and the `font` shorthand both count.
    #[rstest]
    #[case::font_family(
        ".a{font-family:Inter,sans-serif}@font-face{font-family:Inter;src:url(i.woff2)}"
    )]
    #[case::font_shorthand(".a{font:16px/1.5 Inter}@font-face{font-family:Inter;src:url(i.woff2)}")]
    #[case::quoted_with_spaces(
        r#".a{font-family:"Open Sans"}@font-face{font-family:"Open Sans";src:url(o.woff2)}"#
    )]
    #[case::case_insensitive(".a{font-family:inter}@font-face{font-family:Inter;src:url(i.woff2)}")]
    #[case::through_variable(
        ":root{--font:Inter}.a{font-family:var(--font)}@font-face{font-family:Inter;src:url(i.woff2)}"
    )]
    fn font_face_should_be_kept_when_referenced(#[case] css: &str) {
        let output = prune_with(css, &["a"], &PruneOptions::default().font_face(true));
        assert_eq!(output.removed_font_faces, 0);
    }

    #[test]
    fn font_face_should_be_removed_when_unreferenced() {
        let options = PruneOptions::default().font_face(true);
        let css = ".a{color:red}@font-face{font-family:Inter;src:url(i.woff2)}";
        assert_eq!(prune_with(css, &["a"], &options).css, ".a{color:red}");
    }

    #[test]
    fn font_face_should_be_kept_when_content_mentions_every_word() {
        // e.g. an inline `style="font-family: 'Open Sans'"`.
        let options = PruneOptions::default().font_face(true);
        let css = r#"@font-face{font-family:"Open Sans";src:url(o.woff2)}"#;
        assert_eq!(
            prune_with(css, &["Open", "Sans"], &options).removed_font_faces,
            0
        );
    }

    // Variables (rule 6): follow var() chains; content counts as a reader.
    #[test]
    fn variables_should_keep_those_read_by_kept_rules() {
        let options = PruneOptions::default().variables(true);
        let css = ":root{--used:red;--unused:blue}.a{color:var(--used)}";
        assert_eq!(
            prune_with(css, &["a"], &options).css,
            ":root{--used:red}.a{color:var(--used)}"
        );
    }

    #[test]
    fn variables_should_follow_chains() {
        let options = PruneOptions::default().variables(true);
        let css = ":root{--base:red;--brand:var(--base)}.a{color:var(--brand)}";
        assert_eq!(prune_with(css, &["a"], &options).removed_variables, 0);
    }

    #[test]
    fn variables_should_remove_chains_nobody_reads() {
        let options = PruneOptions::default().variables(true);
        let css = ":root{--base:red;--brand:var(--base)}.a{color:red}";
        assert_eq!(prune_with(css, &["a"], &options).removed_variables, 2);
    }

    #[test]
    fn variables_should_drop_rules_they_emptied() {
        let options = PruneOptions::default().variables(true);
        let css = ":root{--unused:red}.a{color:red}";
        assert_eq!(prune_with(css, &["a"], &options).css, ".a{color:red}");
    }

    #[test]
    fn variables_should_keep_those_named_in_content() {
        // e.g. `getComputedStyle(el).getPropertyValue("--theme")`.
        let options = PruneOptions::default().variables(true);
        let css = ":root{--theme:dark}";
        assert_eq!(prune_with(css, &["--theme"], &options).removed_variables, 0);
    }

    #[test]
    fn variables_should_keep_those_read_by_removed_selectors_only_if_also_read_elsewhere() {
        let options = PruneOptions::default().variables(true);
        let css = ":root{--x:red}.gone{color:var(--x)}";
        assert_eq!(prune_with(css, &[], &options).css, "");
    }

    #[test]
    fn variables_safelist_should_keep_unread_variables() {
        let options = PruneOptions::default()
            .variables(true)
            .safelist(Safelist::default().variables(regex("^brand-")));
        let css = ":root{--brand-red:red}";
        assert_eq!(prune_with(css, &[], &options).removed_variables, 0);
    }

    // Every place a declaration can live must be both read and cleaned.
    #[rstest]
    #[case::media("@media print{:root{--x:red;--y:red}.a{color:var(--x)}}")]
    #[case::supports("@supports (color:red){:root{--x:red;--y:red}.a{color:var(--x)}}")]
    #[case::container("@container (width>1px){:root{--x:red;--y:red}.a{color:var(--x)}}")]
    #[case::scope("@scope (.s){:root{--x:red;--y:red}.a{color:var(--x)}}")]
    #[case::starting_style("@starting-style{:root{--x:red;--y:red}.a{color:var(--x)}}")]
    #[case::moz_document("@-moz-document url-prefix(){:root{--x:red;--y:red}.a{color:var(--x)}}")]
    #[case::named_layer("@layer base{:root{--x:red;--y:red}.a{color:var(--x)}}")]
    #[case::nested_rule(".a{color:var(--x);& .b{--x:red;--y:red}}")]
    #[case::nested_declarations(
        ".a{color:red;&:hover{color:blue}--x:red;--y:red;background:var(--x)}"
    )]
    #[case::legacy_nest(".a{color:var(--x);@nest .b &{--x:red;--y:red}}")]
    fn variables_should_be_tracked_in_every_kind_of_block(#[case] css: &str) {
        let options = PruneOptions::default().variables(true);
        assert_eq!(
            prune_with(css, &["a", "b", "s"], &options).removed_variables,
            1
        );
    }

    #[rstest]
    #[case::supports("@supports (color:red){:root{--y:red}}")]
    #[case::container("@container (width>1px){:root{--y:red}}")]
    #[case::nested_declarations(".a{&:hover{color:red}--y:red}")]
    #[case::legacy_nest(".a{color:red;@nest .b &{--y:red}}")]
    fn variables_should_drop_blocks_they_emptied(#[case] css: &str) {
        let options = PruneOptions::default().variables(true);
        let output = prune_with(css, &["a", "b"], &options).css;
        assert!(!output.contains("--y"), "{output}");
    }

    #[test]
    fn variables_should_keep_named_layers_they_emptied() {
        let options = PruneOptions::default().variables(true);
        assert_eq!(
            prune_with("@layer base{:root{--y:red}}", &[], &options).css,
            "@layer base{}"
        );
    }

    // Animating a registered custom property is legitimate, so variables
    // inside keyframe steps are never removed.
    #[test]
    fn variables_should_leave_keyframes_untouched() {
        let options = PruneOptions::default().variables(true);
        let css = "@keyframes pulse{to{--glow:1}}";
        assert_eq!(prune_with(css, &[], &options).css, css);
    }

    #[test]
    fn variables_should_leave_unknown_properties_alone() {
        let options = PruneOptions::default().variables(true);
        let css = ".a{foo:bar}";
        assert_eq!(prune_with(css, &["a"], &options).css, css);
    }

    #[rstest]
    #[case::supports("@supports (color:red){.a{animation:spin 1s}}@keyframes spin{to{opacity:0}}")]
    #[case::container(
        "@container (width>1px){.a{animation:spin 1s}}@keyframes spin{to{opacity:0}}"
    )]
    #[case::scope("@scope (.s){.a{animation:spin 1s}}@keyframes spin{to{opacity:0}}")]
    #[case::starting_style("@starting-style{.a{animation:spin 1s}}@keyframes spin{to{opacity:0}}")]
    #[case::moz_document(
        "@-moz-document url-prefix(){.a{animation:spin 1s}}@keyframes spin{to{opacity:0}}"
    )]
    #[case::layer("@layer base{.a{animation:spin 1s}}@keyframes spin{to{opacity:0}}")]
    #[case::nested_rule(".x{& .a{animation:spin 1s}}@keyframes spin{to{opacity:0}}")]
    #[case::nested_declarations(
        ".a{&:hover{color:red}animation:spin 1s}@keyframes spin{to{opacity:0}}"
    )]
    #[case::legacy_nest(
        ".x{color:red;@nest .a &{animation:spin 1s}}@keyframes spin{to{opacity:0}}"
    )]
    #[case::string_name(r#".a{animation-name:"spin"}@keyframes "spin"{to{opacity:0}}"#)]
    fn keyframes_should_find_references_in_every_kind_of_block(#[case] css: &str) {
        let options = PruneOptions::default().keyframes(true);
        assert_eq!(
            prune_with(css, &["a", "x", "s"], &options).removed_keyframes,
            0
        );
    }

    #[rstest]
    #[case::inside_media("@media print{@keyframes spin{to{opacity:0}}}", "")]
    #[case::inside_supports("@supports (color:red){@keyframes spin{to{opacity:0}}}", "")]
    #[case::inside_named_layer("@layer base{@keyframes spin{to{opacity:0}}}", "@layer base{}")]
    #[case::string_name(r#"@keyframes "spin"{to{opacity:0}}"#, "")]
    fn keyframes_should_be_removed_wherever_they_are_defined(
        #[case] css: &str,
        #[case] expected: &str,
    ) {
        let options = PruneOptions::default().keyframes(true);
        assert_eq!(prune_with(css, &[], &options).css, expected);
    }

    #[test]
    fn font_face_should_be_kept_when_it_has_no_family() {
        let options = PruneOptions::default().font_face(true);
        let css = "@font-face{src:url(x.woff2)}";
        assert_eq!(prune_with(css, &[], &options).removed_font_faces, 0);
    }

    #[test]
    fn greedy_safelist_should_look_inside_is() {
        let options = safelist_options(Safelist::default().greedy(exact("x")));
        let css = ":is(.x,.y) .unused{color:red}";
        assert_eq!(prune_with(css, &[], &options).css, css);
    }

    #[test]
    fn variables_should_be_kept_by_default() {
        let css = ":root{--unused:red}";
        assert_eq!(
            prune_with(css, &[], &PruneOptions::default()).removed_variables,
            0
        );
    }

    #[rstest]
    #[case::class_and_id(".a,#b{color:red}", &["a", "b"])]
    #[case::escaped(r".md\:flex{display:flex}", &["md:flex"])]
    #[case::inside_pseudo_classes(":is(.a) :not(.b):has(.c){color:red}", &["a", "b", "c"])]
    #[case::nested_and_grouped("@media print{.a{&.b{color:red}}}", &["a", "b"])]
    #[case::no_selectors("@font-face{font-family:X}", &[])]
    #[case::nth_of(":nth-child(2 of .a){color:red}", &["a"])]
    #[case::shadow_dom(":host(.a) ::slotted(.b){color:red}", &["a", "b"])]
    #[case::layer("@layer base{.a{color:red}}", &["a"])]
    #[case::where_and_prefixed_any(":where(.a) :-webkit-any(.b){color:red}", &["a", "b"])]
    #[case::legacy_nest(".a{color:red;@nest .b &{color:red}}", &["a", "b"])]
    fn defined_names_should_list_every_class_and_id(#[case] css: &str, #[case] expected: &[&str]) {
        let names = defined_names(css).ok();
        let expected: BTreeSet<String> = expected.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(names, Some(expected));
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
            fn greedy_safelisted_selectors_always_survive(css in stylesheet(), used in used_names()) {
                let options = PruneOptions::default().safelist(Safelist::default().greedy(Pattern::exact("a")));
                let output = prune_with(&css, &used, &options).css;
                let rules_with_a = |text: &str| text.split('}').filter(|rule| rule.contains(".a")).count();
                prop_assert_eq!(rules_with_a(&output), rules_with_a(&css));
            }

            #[test]
            fn tracking_never_removes_ordinary_style_rules(css in stylesheet(), used in used_names()) {
                let tracked = PruneOptions::default().keyframes(true).font_face(true).variables(true);
                prop_assert_eq!(
                    prune_with(&css, &used, &tracked).css,
                    prune_with(&css, &used, &PruneOptions::default()).css
                );
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
