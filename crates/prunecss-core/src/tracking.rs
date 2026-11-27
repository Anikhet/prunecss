//! Opt-in removal of unused `@keyframes`, `@font-face` and custom properties.
//!
//! These have no selectors, so they are judged by whether anything that
//! survived selector pruning still refers to them. References are found in
//! the parsed values of every kept declaration, so prefixed properties
//! (`-webkit-animation`), shorthands (`font`, `animation`) and values held
//! in variables all count. Content files count too, because scripts can
//! refer to animations, fonts and variables by name.
//!
//! When in doubt, everything is kept: if any value cannot be printed for
//! inspection, nothing is removed.

use std::borrow::Cow;
use std::collections::BTreeSet;

use lightningcss::declaration::DeclarationBlock;
use lightningcss::properties::Property;
use lightningcss::properties::custom::CustomPropertyName;
use lightningcss::properties::font::FontFamily;
use lightningcss::rules::font_face::FontFaceProperty;
use lightningcss::rules::keyframes::KeyframesName;
use lightningcss::rules::style::StyleRule;
use lightningcss::rules::{CssRule, CssRuleList};
use lightningcss::stylesheet::PrinterOptions;
use lightningcss::traits::ToCss;
use lightningcss::values::ident::Ident;

use crate::pruner::UsedNames;
use crate::rules;
use crate::safelist::{Safelist, any_matches};
use crate::scanner::extract_classes;

/// Which kinds of rule to remove when unused.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Enabled {
    pub(crate) keyframes: bool,
    pub(crate) font_face: bool,
    pub(crate) variables: bool,
}

/// How many of each kind were removed.
#[derive(Debug, Default)]
pub(crate) struct Removed {
    pub(crate) keyframes: usize,
    pub(crate) font_faces: usize,
    pub(crate) variables: usize,
}

pub(crate) fn remove_unused<R: ToCss>(
    rules: &mut CssRuleList<'_, R>,
    used: &impl UsedNames,
    safelist: &Safelist,
    enabled: Enabled,
) -> Removed {
    let mut removed = Removed::default();

    // Variables first: a keyframe or font named only inside a variable that
    // nobody reads is itself unused.
    if enabled.variables
        && let Some(live) = live_variables(rules, used, safelist)
    {
        removed.variables = remove_dead_variables(rules, &live);
    }

    if (enabled.keyframes || enabled.font_face)
        && let Some(text) = reference_text(rules)
    {
        let tokens = extract_classes(&text);
        let lowered = text.to_lowercase();
        retain_rules(rules, &mut |rule| match rule {
            CssRule::Keyframes(keyframes) if enabled.keyframes => {
                let name = keyframes_name(&keyframes.name);
                // References are printed, so look for the name both as it is
                // and as printed: `"my anim"` is referenced as `my\ anim`,
                // `\31spin` as `\31 spin`. A plain substring match keeps
                // too much at worst.
                let printed = Ident(name.into()).to_css_string(PrinterOptions::default());
                let keep = tokens.contains(name)
                    || text.contains(name)
                    || printed.is_ok_and(|printed| text.contains(&printed))
                    || used.contains_name(name)
                    || any_matches(&safelist.keyframes, name);
                removed.keyframes += usize::from(!keep);
                keep
            }
            CssRule::FontFace(font_face) if enabled.font_face => {
                let keep = font_face_is_referenced(&font_face.properties, &lowered, used);
                removed.font_faces += usize::from(!keep);
                keep
            }
            _ => true,
        });
    }

    removed
}

fn keyframes_name<'a>(name: &'a KeyframesName<'_>) -> &'a str {
    match name {
        KeyframesName::Ident(ident) => &ident.0,
        KeyframesName::Custom(string) => string,
    }
}

/// A `@font-face` is referenced when some kept value names one of its
/// families (case-insensitively), or every word of the family appears in the
/// content (`style="font-family: 'Open Sans'"`).
fn font_face_is_referenced(
    properties: &[FontFaceProperty<'_>],
    lowered_text: &str,
    used: &impl UsedNames,
) -> bool {
    let mut families = properties.iter().filter_map(|property| match property {
        FontFaceProperty::FontFamily(family @ FontFamily::FamilyName(_)) => family
            .to_css_string(PrinterOptions::default())
            .ok()
            .map(|printed| printed.trim_matches(['"', '\'']).to_owned()),
        _ => None,
    });
    let Some(first) = families.next() else {
        // No family we can read: keep it rather than guess.
        return true;
    };
    std::iter::once(first).chain(families).any(|family| {
        lowered_text.contains(&family.to_lowercase())
            || family.split_ascii_whitespace().all(|word| {
                // CSS family names are case-insensitive, so content may
                // write `myfont` for `MyFont`.
                used.contains_name(word) || used.contains_name(&word.to_lowercase())
            })
    })
}

/// Visits every declaration block in kept rules, including nested rules and
/// keyframe steps. `@font-face` descriptors are not visited: a font face does
/// not refer to itself.
fn for_each_block<'a, R>(
    rules: &'a CssRuleList<'_, R>,
    visit: &mut impl FnMut(&'a DeclarationBlock<'_>),
) {
    for rule in &rules.0 {
        if let Some(group) = rules::group_rules(rule) {
            for_each_block(group, visit);
            continue;
        }
        match rule {
            CssRule::Style(style) => {
                visit(&style.declarations);
                for_each_block(&style.rules, visit);
            }
            CssRule::Nesting(nesting) => {
                visit(&nesting.style.declarations);
                for_each_block(&nesting.style.rules, visit);
            }
            CssRule::NestedDeclarations(nested) => visit(&nested.declarations),
            CssRule::Keyframes(keyframes) => {
                for keyframe in &keyframes.keyframes {
                    visit(&keyframe.declarations);
                }
            }
            CssRule::LayerBlock(layer) => for_each_block(&layer.rules, visit),
            _ => {}
        }
    }
}

fn properties<'a, 'i>(block: &'a DeclarationBlock<'i>) -> impl Iterator<Item = &'a Property<'i>> {
    block
        .declarations
        .iter()
        .chain(&block.important_declarations)
}

fn defined_variable<'a>(property: &'a Property<'_>) -> Option<&'a str> {
    match property {
        Property::Custom(custom) => match &custom.name {
            CustomPropertyName::Custom(name) => Some(&name.0),
            CustomPropertyName::Unknown(_) => None,
        },
        _ => None,
    }
}

/// All declaration values of kept rules, one per line. `None` if any value
/// cannot be printed, in which case callers must not remove anything.
fn reference_text<R: ToCss>(rules: &CssRuleList<'_, R>) -> Option<String> {
    let mut text = String::new();
    let mut complete = true;
    for_each_block(rules, &mut |block| {
        for property in properties(block) {
            match property.value_to_css_string(PrinterOptions::default()) {
                Ok(value) => {
                    text.push_str(&value);
                    text.push('\n');
                }
                Err(_) => complete = false,
            }
        }
    });
    (complete && other_reference_text(rules, &mut text)).then_some(text)
}

/// Appends the text of everything outside ordinary declaration blocks that
/// can still refer to a variable, keyframe or font: `@page`,
/// `@position-try`, `@font-feature-values`, unknown at-rules and the like
/// (printed whole), and `@container` conditions such as
/// `style(--theme: dark)`. Listing what to skip, rather than what to read,
/// means a rule type nobody thought of keeps CSS instead of deleting it.
/// Returns `false` if anything can't be printed.
fn other_reference_text<R: ToCss>(rules: &CssRuleList<'_, R>, text: &mut String) -> bool {
    rules.0.iter().all(|rule| match rule {
        CssRule::Container(container) => {
            let condition = container
                .condition
                .as_ref()
                .map_or(Ok(String::new()), |condition| {
                    condition.to_css_string(PrinterOptions::default())
                });
            condition.is_ok_and(|condition| {
                text.push_str(&condition);
                text.push('\n');
                true
            }) && other_reference_text(&container.rules, text)
        }
        CssRule::Style(style) => other_reference_text(&style.rules, text),
        CssRule::Nesting(nesting) => other_reference_text(&nesting.style.rules, text),
        CssRule::LayerBlock(layer) => other_reference_text(&layer.rules, text),
        // Declarations are read by `for_each_block`; a font face does not
        // refer to itself; these hold no references.
        CssRule::Keyframes(_)
        | CssRule::FontFace(_)
        | CssRule::NestedDeclarations(_)
        | CssRule::Import(_)
        | CssRule::Namespace(_)
        | CssRule::LayerStatement(_)
        | CssRule::Ignored => true,
        other => match rules::group_rules(other) {
            Some(group) => other_reference_text(group, text),
            None => other
                .to_css_string(PrinterOptions::default())
                .is_ok_and(|printed| {
                    text.push_str(&printed);
                    text.push('\n');
                    true
                }),
        },
    })
}

/// `--name` identifiers in `text`, unescaped so they compare equal to
/// parsed names: the `--brand` in `var(--brand, red)`, `--a.b` for `--a\.b`.
fn variable_references(text: &str) -> impl Iterator<Item = String> {
    identifiers(text)
        .filter(|name| name.starts_with("--") && name.len() > 2)
        .map(Cow::into_owned)
}

/// Works out which custom properties something still reads.
///
/// Readers are ordinary declarations, the content files and the safelist.
/// A variable read only by another variable is live only if that one is,
/// so chains are followed until nothing new is reached.
fn live_variables<R: ToCss>(
    rules: &CssRuleList<'_, R>,
    used: &impl UsedNames,
    safelist: &Safelist,
) -> Option<BTreeSet<String>> {
    let mut definitions: Vec<(String, String)> = Vec::new();
    let mut reader_text = String::new();
    let mut complete = true;

    for_each_block(rules, &mut |block| {
        for property in properties(block) {
            let Ok(value) = property.value_to_css_string(PrinterOptions::default()) else {
                complete = false;
                continue;
            };
            if let Some(name) = defined_variable(property) {
                definitions.push((name.to_owned(), value));
            } else {
                reader_text.push_str(&value);
                reader_text.push('\n');
            }
        }
    });
    if !complete || !other_reference_text(rules, &mut reader_text) {
        return None;
    }

    let mut live: BTreeSet<String> = variable_references(&reader_text).collect();
    live.extend(
        definitions
            .iter()
            .map(|(name, _)| name)
            .filter(|name| {
                used.contains_name(name)
                    || any_matches(&safelist.variables, name.trim_start_matches("--"))
            })
            .cloned(),
    );

    let mut pending: Vec<String> = live.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        for (_, value) in definitions.iter().filter(|(defined, _)| *defined == name) {
            for reference in variable_references(value) {
                if live.insert(reference.clone()) {
                    pending.push(reference);
                }
            }
        }
    }
    Some(live)
}

/// Identifiers in `text` (runs of name characters and escapes), unescaped.
/// Borrowed unless an escape had to be decoded.
fn identifiers(text: &str) -> impl Iterator<Item = Cow<'_, str>> {
    let mut rest = text;
    std::iter::from_fn(move || {
        loop {
            let start = rest.find(|c: char| is_name_char(c) || c == '\\')?;
            let (name, len) = read_identifier(&rest[start..]);
            // An invalid escape reads nothing: step over its `\`.
            rest = &rest[start + len.max(1)..];
            if !name.is_empty() {
                return Some(name);
            }
        }
    })
}

/// Reads the identifier at the start of `text`; returns it unescaped with
/// the number of bytes it took.
fn read_identifier(text: &str) -> (Cow<'_, str>, usize) {
    let mut decoded: Option<String> = None;
    let mut end = 0;
    while let Some(c) = text[end..].chars().next() {
        if is_name_char(c) {
            if let Some(decoded) = &mut decoded {
                decoded.push(c);
            }
            end += c.len_utf8();
        } else if let Some((escaped, len)) =
            (c == '\\').then(|| read_escape(&text[end + 1..])).flatten()
        {
            decoded
                .get_or_insert_with(|| text[..end].to_owned())
                .push(escaped);
            end += 1 + len;
        } else {
            break;
        }
    }
    let name = decoded.map_or(Cow::Borrowed(&text[..end]), Cow::Owned);
    (name, end)
}

/// Decodes the CSS escape that `text` starts with (what follows a `\`):
/// up to six hex digits and one optional whitespace, or any other character
/// but a newline. Returns the character and the bytes it took.
fn read_escape(text: &str) -> Option<(char, usize)> {
    let first = text.chars().next()?;
    if matches!(first, '\n' | '\r' | '\u{c}') {
        return None;
    }
    let digits = text
        .bytes()
        .take(6)
        .take_while(u8::is_ascii_hexdigit)
        .count();
    if digits == 0 {
        return Some((first, first.len_utf8()));
    }
    let code = text[..digits]
        .chars()
        .filter_map(|digit| digit.to_digit(16))
        .fold(0, |code, digit| code * 16 + digit);
    // NUL, surrogates and out-of-range values decode to U+FFFD.
    let escaped = char::from_u32(code)
        .filter(|_| code != 0)
        .unwrap_or('\u{fffd}');
    let rest = &text[digits..];
    let whitespace = if rest.starts_with("\r\n") {
        2
    } else {
        usize::from(rest.starts_with([' ', '\t', '\n', '\r', '\u{c}']))
    };
    Some((escaped, digits + whitespace))
}

/// ASCII letters, digits, `-`, `_` and everything non-ASCII.
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

/// Removes custom property declarations whose names are not in `live`, and
/// rules that this leaves empty. Keyframe steps are left alone: animating a
/// registered custom property is legitimate even if nothing else reads it.
fn remove_dead_variables<R>(rules: &mut CssRuleList<'_, R>, live: &BTreeSet<String>) -> usize {
    let mut removed = 0;
    remove_in_rules(rules, live, &mut removed);
    removed
}

fn remove_in_rules<R>(
    rules: &mut CssRuleList<'_, R>,
    live: &BTreeSet<String>,
    removed: &mut usize,
) {
    rules.0.retain_mut(|rule| {
        if let Some(group) = rules::group_rules_mut(rule) {
            return rules::keep_group(group, |group| remove_in_rules(group, live, removed));
        }
        match rule {
            CssRule::Style(style) => keep_style(style, live, removed),
            CssRule::Nesting(nesting) => keep_style(&mut nesting.style, live, removed),
            CssRule::NestedDeclarations(nested) => {
                let had_content = !nested.declarations.is_empty();
                remove_in_block(&mut nested.declarations, live, removed);
                !had_content || !nested.declarations.is_empty()
            }
            CssRule::LayerBlock(layer) => {
                remove_in_rules(&mut layer.rules, live, removed);
                // Named layers fix cascade order even when empty.
                layer.name.is_some() || !layer.rules.0.is_empty()
            }
            _ => true,
        }
    });
}

/// Removes dead variables from a style rule; it goes if that emptied it.
fn keep_style<R>(
    style: &mut StyleRule<'_, R>,
    live: &BTreeSet<String>,
    removed: &mut usize,
) -> bool {
    let had_content = !style.declarations.is_empty() || !style.rules.0.is_empty();
    remove_in_block(&mut style.declarations, live, removed);
    remove_in_rules(&mut style.rules, live, removed);
    !had_content || !style.declarations.is_empty() || !style.rules.0.is_empty()
}

fn remove_in_block(block: &mut DeclarationBlock<'_>, live: &BTreeSet<String>, removed: &mut usize) {
    let is_live =
        |property: &Property<'_>| defined_variable(property).is_none_or(|name| live.contains(name));
    let before = block.len();
    block.declarations.retain(is_live);
    block.important_declarations.retain(is_live);
    *removed += before - block.len();
}

/// Like `Vec::retain` over a rule tree: `keep` decides for leaf rules, and a
/// grouping rule goes once it has been emptied (named layers always stay).
fn retain_rules<'i, R>(
    rules: &mut CssRuleList<'i, R>,
    keep: &mut impl FnMut(&CssRule<'i, R>) -> bool,
) {
    rules.0.retain_mut(|rule| {
        if let Some(group) = rules::group_rules_mut(rule) {
            return rules::keep_group(group, |group| retain_rules(group, keep));
        }
        if let CssRule::LayerBlock(layer) = rule {
            retain_rules(&mut layer.rules, keep);
            return layer.name.is_some() || !layer.rules.0.is_empty();
        }
        keep(rule)
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rstest::rstest;

    use super::*;
    use crate::{PruneOptions, PruneOutput, prune};

    fn prune_with(css: &str, used: &[&str], options: &PruneOptions) -> PruneOutput {
        let used: BTreeSet<&str> = used.iter().copied().collect();
        match prune(css, &used, &options.clone().minify(true)) {
            Ok(output) => output,
            Err(err) => panic!("prune failed: {err}"),
        }
    }

    #[rstest]
    #[case::plain("--plain", "--plain")]
    #[case::escaped_char(r"--a\.b", "--a.b")]
    #[case::escaped_slash(r"--w-1\/2", "--w-1/2")]
    #[case::hex_with_space(r"--\31 x", "--1x")]
    #[case::hex_ends_at_non_hex(r"--\31x", "--1x")]
    #[case::hex_without_space(r"--\31a", "--\u{31a}")]
    #[case::hex_six_digits(r"--\00003a", "--:")]
    #[case::zero_is_replacement(r"--\0 ", "--\u{fffd}")]
    #[case::non_ascii("--größe", "--größe")]
    fn variable_references_should_unescape_names(#[case] text: &str, #[case] expected: &str) {
        let found: Vec<_> = variable_references(text).collect();
        assert_eq!(found, [expected]);
    }

    #[test]
    fn variable_references_should_find_names_in_var_calls() {
        let found: Vec<_> = variable_references("var(--a, var(--b)) calc(1px--c)").collect();
        assert_eq!(found, ["--a", "--b"]);
    }

    #[rstest]
    #[case::escaped_char(
        r":root{--a\.b:red;--plain:blue}.a{color:var(--a\.b);background:var(--plain)}"
    )]
    #[case::escaped_slash(r":root{--w-1\/2:50%}.a{width:var(--w-1\/2)}")]
    #[case::hex_escape(r":root{--\31 x:red}.a{color:var(--\31 x)}")]
    #[case::escaped_chain(r":root{--a\.b:red;--c\:d:var(--a\.b)}.a{color:var(--c\:d)}")]
    fn variables_should_keep_escaped_names_that_are_read(#[case] css: &str) {
        let options = PruneOptions::default().variables(true);
        assert_eq!(prune_with(css, &["a"], &options).removed_variables, 0);
    }

    #[test]
    fn variables_should_remove_escaped_names_nobody_reads() {
        let options = PruneOptions::default().variables(true);
        let css = r":root{--a\.b:red}.a{color:red}";
        assert_eq!(prune_with(css, &["a"], &options).css, ".a{color:red}");
    }

    #[rstest]
    #[case::escaped_char(r".a{animation:a\.b 1s}@keyframes a\.b{to{opacity:0}}")]
    #[case::hex_escape(r".a{animation:\31 spin 1s}@keyframes \31 spin{to{opacity:0}}")]
    #[case::animation_name(r".a{animation-name:a\.b}@keyframes a\.b{to{opacity:0}}")]
    fn keyframes_should_be_kept_when_an_escaped_name_is_referenced(#[case] css: &str) {
        let options = PruneOptions::default().keyframes(true);
        assert_eq!(prune_with(css, &["a"], &options).removed_keyframes, 0);
    }
}
