//! Find the CSS class names used in markup.

use std::collections::BTreeSet;

/// Returns every class name found in `class="…"` or `className="…"`
/// attributes in `source`.
///
/// Names are borrowed from `source`, so no strings are copied.
///
/// ```
/// use prunecss_core::scanner::extract_classes;
///
/// let classes = extract_classes(r#"<div class="card card--big">"#);
/// assert_eq!(classes.into_iter().collect::<Vec<_>>(), ["card", "card--big"]);
/// ```
#[must_use]
pub fn extract_classes(source: &str) -> BTreeSet<&str> {
    let mut classes = BTreeSet::new();
    let bytes = source.as_bytes();
    let mut pos = 0;

    while let Some((value_start, value_end)) = next_class_value(bytes, pos) {
        classes.extend(source[value_start..value_end].split_ascii_whitespace());
        pos = value_end + 1;
    }
    classes
}

/// Attribute names that hold class lists, lowercase.
const CLASS_ATTRIBUTES: [&[u8]; 2] = [b"classname", b"class"];

/// Finds the next quoted `class`/`className` value at or after `from`.
///
/// Returns the byte range of the text between the quotes. Every index we
/// stop at is an ASCII byte, so the range is always a valid `str` boundary.
fn next_class_value(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i < bytes.len() {
        if let Some(name_end) = match_attribute_name(bytes, i)
            && let Some(range) = quoted_value_after(bytes, name_end)
        {
            return Some(range);
        }
        i += 1;
    }
    None
}

/// If a class attribute name starts at `i`, returns the index just past it.
fn match_attribute_name(bytes: &[u8], i: usize) -> Option<usize> {
    // The name must start a new attribute, so `data-class` and `subclass`
    // don't count.
    let starts_attribute = i > 0 && bytes[i - 1].is_ascii_whitespace();
    if !starts_attribute {
        return None;
    }
    CLASS_ATTRIBUTES.iter().find_map(|name| {
        let end = i + name.len();
        let candidate = bytes.get(i..end)?;
        candidate.eq_ignore_ascii_case(name).then_some(end)
    })
}

/// Parses `\s* = \s* "value"` starting at `i`, returning the value's range.
fn quoted_value_after(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    let i = skip_whitespace(bytes, i);
    if bytes.get(i) != Some(&b'=') {
        return None;
    }
    let i = skip_whitespace(bytes, i + 1);
    let quote = *bytes.get(i).filter(|b| matches!(b, b'"' | b'\''))?;
    let start = i + 1;
    let len = bytes.get(start..)?.iter().position(|&b| b == quote)?;
    Some((start, start + len))
}

fn skip_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::single_class(r#"<div class="btn">"#, &["btn"])]
    #[case::several_classes(r#"<div class="btn btn-primary">"#, &["btn", "btn-primary"])]
    #[case::single_quotes("<div class='btn'>", &["btn"])]
    #[case::jsx_class_name(r#"<div className="card">"#, &["card"])]
    #[case::extra_whitespace("<div class=\"  a \n\t b  \">", &["a", "b"])]
    #[case::empty_attribute(r#"<div class="">"#, &[])]
    #[case::several_elements(r#"<a class="x"></a><b class="y"></b>"#, &["x", "y"])]
    #[case::duplicates_collapse(r#"<a class="x"></a><b class="x"></b>"#, &["x"])]
    #[case::spaces_around_equals(r#"<div class = "btn">"#, &["btn"])]
    #[case::uppercase_attribute(r#"<DIV CLASS="btn">"#, &["btn"])]
    fn extract_classes_should_find_class_names(#[case] source: &str, #[case] expected: &[&str]) {
        let found: Vec<&str> = extract_classes(source).into_iter().collect();
        assert_eq!(found, expected);
    }

    #[rstest]
    #[case::data_attribute(r#"<div data-class="nope">"#)]
    #[case::subclass_attribute(r#"<div subclass="nope">"#)]
    #[case::no_markup("just some text")]
    #[case::unclosed_quote(r#"<div class="never-closed"#)]
    #[case::unquoted_value("<div class=nope>")]
    fn extract_classes_should_ignore_non_class_text(#[case] source: &str) {
        assert!(extract_classes(source).is_empty());
    }
}
