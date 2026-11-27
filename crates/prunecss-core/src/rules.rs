//! Shared helpers for walking Lightning CSS rule trees.

use lightningcss::rules::{CssRule, CssRuleList};

/// The rules inside a conditional grouping rule (`@media`, `@supports`,
/// `@container`, `@scope`, `@starting-style`, `@-moz-document`).
///
/// Such a group has no meaning of its own, so it can go once it is empty.
/// `@layer` blocks are not included: a named layer fixes cascade order even
/// when empty, so callers handle layers separately.
pub(crate) fn group_rules_mut<'a, 'i, R>(
    rule: &'a mut CssRule<'i, R>,
) -> Option<&'a mut CssRuleList<'i, R>> {
    match rule {
        CssRule::Media(group) => Some(&mut group.rules),
        CssRule::Supports(group) => Some(&mut group.rules),
        CssRule::Container(group) => Some(&mut group.rules),
        CssRule::Scope(group) => Some(&mut group.rules),
        CssRule::StartingStyle(group) => Some(&mut group.rules),
        CssRule::MozDocument(group) => Some(&mut group.rules),
        _ => None,
    }
}

/// Read-only version of [`group_rules_mut`].
pub(crate) fn group_rules<'a, 'i, R>(rule: &'a CssRule<'i, R>) -> Option<&'a CssRuleList<'i, R>> {
    match rule {
        CssRule::Media(group) => Some(&group.rules),
        CssRule::Supports(group) => Some(&group.rules),
        CssRule::Container(group) => Some(&group.rules),
        CssRule::Scope(group) => Some(&group.rules),
        CssRule::StartingStyle(group) => Some(&group.rules),
        CssRule::MozDocument(group) => Some(&group.rules),
        _ => None,
    }
}

/// Runs `prune` on a group's rules and returns whether the group should stay:
/// it goes only if pruning emptied it (an already empty group is left alone).
pub(crate) fn keep_group<'i, R>(
    rules: &mut CssRuleList<'i, R>,
    prune: impl FnOnce(&mut CssRuleList<'i, R>),
) -> bool {
    let was_empty = rules.0.is_empty();
    prune(rules);
    was_empty || !rules.0.is_empty()
}
