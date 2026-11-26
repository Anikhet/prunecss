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

/// `--name` tokens in `text`, such as the `--brand` in `var(--brand, red)`.
fn variable_references(text: &str) -> impl Iterator<Item = String> {
    extract_classes(text)
        .into_iter()
        .filter(|token| token.starts_with("--") && token.len() > 2)
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
