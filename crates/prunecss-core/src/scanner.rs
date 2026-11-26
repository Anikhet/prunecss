//! Find the class and id names that content files might use.
//!
//! The scanner does not parse HTML, JSX, Vue or any other language. It splits
//! the text into *candidate* tokens and returns all of them, the same approach
//! as Tailwind's Rust scanner. Returning too many candidates is harmless: a
//! candidate only keeps CSS if a selector names it. Returning too few would
//! delete CSS the page uses, so every rule here errs on the side of more.

use std::collections::BTreeSet;

/// Returns every candidate class or id name in `source`.
///
/// This finds names in plain markup (`class="card"`), JSX (`className`),
/// JavaScript strings and object keys (`clsx("btn", { active: on })`),
/// Vue (`:class`), Svelte (`class:active`), Astro (`class:list`) and
/// Tailwind-style names with variants and arbitrary values (`md:flex`,
/// `w-1/2`, `grid-cols-[repeat(2,1fr)]`).
///
/// Names built at runtime, such as `` `btn-${size}` ``, cannot be found by
/// any static scanner; safelist them instead.
///
/// Names are borrowed from `source`, so no strings are copied.
///
/// ```
/// use prunecss_core::extract_classes;
///
/// let found = extract_classes(r#"<div className={clsx("card", { active: on })}>"#);
/// assert!(found.contains("card"));
/// assert!(found.contains("active"));
/// ```
#[must_use]
pub fn extract_classes(source: &str) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    // Two readings of the same text, because brackets are ambiguous: in
    // `grid-cols-[repeat(2,1fr)]` they hold part of one Tailwind name, while
    // in `clsx(['a', { b: on }])` they are JavaScript punctuation. Taking the
    // union of both readings finds the names either way.
    for brackets in [Brackets::KeepWhole, Brackets::Split] {
        for token in Tokens::new(source, brackets) {
            add_candidates(token, &mut names);
        }
    }
    names
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Brackets {
    /// Keep `[…]` groups whole, as in Tailwind arbitrary values.
    KeepWhole,
    /// Treat brackets as separators, as in JavaScript arrays.
    Split,
}

/// Bytes that end a name everywhere: no class name in markup contains them.
fn is_hard_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'"' | b'\'' | b'`' | b'\\')
}

/// Bytes that end a name, except inside `[…]` when brackets are kept whole
/// (`data-[state=open]:block`, `[&>*]:p-4`, `grid-cols-[repeat(2,1fr)]`).
fn is_soft_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'<' | b'>' | b'=' | b'{' | b'}' | b'(' | b')' | b',' | b';' | b'$'
    )
}

/// Splits text into tokens at delimiter bytes.
///
/// Every delimiter is ASCII, so the byte offsets it cuts at are always
/// `char` boundaries and multi-byte UTF-8 names stay intact. Each byte is
/// looked at once, so the scan is linear in the input size.
#[derive(Debug)]
struct Tokens<'a> {
    source: &'a str,
    pos: usize,
    brackets: Brackets,
}

impl<'a> Tokens<'a> {
    fn new(source: &'a str, brackets: Brackets) -> Self {
        Self {
            source,
            pos: 0,
            brackets,
        }
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let bytes = self.source.as_bytes();
        let mut depth = 0_usize;
        let mut start = self.pos;

        while self.pos < bytes.len() {
            let byte = bytes[self.pos];
            let ends_token = if is_hard_delimiter(byte) {
                // A name never spans whitespace or quotes, so an unbalanced
                // `[` cannot swallow the rest of the file.
                depth = 0;
                true
            } else {
                match self.brackets {
                    Brackets::Split => is_soft_delimiter(byte) || matches!(byte, b'[' | b']'),
                    Brackets::KeepWhole => {
                        match byte {
                            b'[' => depth += 1,
                            b']' => depth = depth.saturating_sub(1),
                            _ => {}
                        }
                        depth == 0 && is_soft_delimiter(byte)
                    }
                }
            };

            self.pos += 1;
            if ends_token {
                if self.pos - 1 > start {
                    return Some(&self.source[start..self.pos - 1]);
                }
                start = self.pos;
            }
        }

        (self.pos > start).then(|| &self.source[start..self.pos])
    }
}

/// Adds `token` and the shorter names hidden inside it.
///
/// `.modal` (a selector string) also yields `modal`, `active:` (an object
/// key) yields `active`, `class:active` (Svelte) and `styles.button` (CSS
/// modules) yield `active` and `button`. Extra candidates cost nothing.
fn add_candidates<'a>(token: &'a str, names: &mut BTreeSet<&'a str>) {
    const EDGE_PUNCTUATION: [char; 7] = ['.', '#', ':', '!', '?', '&', '|'];

    names.insert(token);
    let trimmed = token
        .trim_start_matches(EDGE_PUNCTUATION)
        .trim_end_matches(EDGE_PUNCTUATION);
    if trimmed.is_empty() {
        return;
    }
    names.insert(trimmed);
    names.extend(trimmed.split([':', '.']).filter(|part| !part.is_empty()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn assert_finds(source: &str, expected: &[&str]) {
        let found = extract_classes(source);
        let missing: Vec<&&str> = expected
            .iter()
            .filter(|name| !found.contains(**name))
            .collect();
        assert!(missing.is_empty(), "missing {missing:?} in {found:?}");
    }

    #[rstest]
    #[case::single_class(r#"<div class="btn">"#, &["btn"])]
    #[case::several_classes(r#"<div class="btn btn-primary">"#, &["btn", "btn-primary"])]
    #[case::single_quotes("<div class='btn'>", &["btn"])]
    #[case::extra_whitespace("<div class=\"  a \n\t b  \">", &["a", "b"])]
    #[case::uppercase_attribute(r#"<DIV CLASS="btn">"#, &["btn"])]
    #[case::unquoted_value("<div class=btn>", &["btn"])]
    #[case::id_attribute(r#"<nav id="main-nav">"#, &["main-nav"])]
    #[case::fragment_link(r##"<a href="#top">"##, &["top"])]
    fn extract_classes_should_find_names_in_html(#[case] source: &str, #[case] expected: &[&str]) {
        assert_finds(source, expected);
    }

    #[rstest]
    #[case::class_name(r#"<div className="card">"#, &["card"])]
    #[case::clsx_strings(r#"className={clsx("btn", 'btn-lg')}"#, &["btn", "btn-lg"])]
    #[case::clsx_object_keys("clsx({ active: isOn, disabled: off })", &["active", "disabled"])]
    #[case::quoted_object_keys(r#"cx({ "is-open": open, 'has-error': err })"#, &["is-open", "has-error"])]
    #[case::ternary(r#"className={on ? "on" : "off"}"#, &["on", "off"])]
    #[case::template_literal_static_parts("`card ${big ? 'card-lg' : ''} shadow`", &["card", "card-lg", "shadow"])]
    #[case::array_join(r#"["a", "b"].join(" ")"#, &["a", "b"])]
    #[case::class_list_add("el.classList.add('is-visible')", &["is-visible"])]
    #[case::query_selector(r#"document.querySelector(".modal")"#, &["modal"])]
    #[case::get_element_by_id(r#"getElementById("sidebar")"#, &["sidebar"])]
    #[case::css_modules("<div className={styles.button}>", &["button"])]
    #[case::json_escaped_quotes(r#"{"html":"<p class=\"lead\">"}"#, &["lead"])]
    fn extract_classes_should_find_names_in_javascript(
        #[case] source: &str,
        #[case] expected: &[&str],
    ) {
        assert_finds(source, expected);
    }

    #[rstest]
    #[case::vue_bound_object(r#"<p :class="{ active: on, 'text-danger': err }">"#, &["active", "text-danger"])]
    #[case::vue_bound_array(r#"<p :class="[isBig ? 'big' : 'small']">"#, &["big", "small"])]
    #[case::svelte_directive("<p class:active={on}>", &["active"])]
    #[case::astro_class_list(r"<p class:list={['a', { b: on }]}>", &["a", "b"])]
    #[case::angular_ng_class(r#"<p [ngClass]="{'is-open': open}">"#, &["is-open"])]
    fn extract_classes_should_find_names_in_framework_templates(
        #[case] source: &str,
        #[case] expected: &[&str],
    ) {
        assert_finds(source, expected);
    }

    #[rstest]
    #[case::responsive_variant(r#"class="md:flex""#, &["md:flex"])]
    #[case::stacked_variants(r#"class="dark:md:hover:bg-red-500""#, &["dark:md:hover:bg-red-500"])]
    #[case::fraction(r#"class="w-1/2""#, &["w-1/2"])]
    #[case::decimal(r#"class="p-1.5""#, &["p-1.5"])]
    #[case::arbitrary_value(r#"class="w-[10px]""#, &["w-[10px]"])]
    #[case::arbitrary_with_commas(
        r#"class="grid-cols-[repeat(auto-fill,minmax(0,1fr))]""#,
        &["grid-cols-[repeat(auto-fill,minmax(0,1fr))]"]
    )]
    #[case::arbitrary_variant(r#"class="[&>*]:p-4""#, &["[&>*]:p-4"])]
    #[case::important_prefix(r#"class="!mt-0""#, &["!mt-0"])]
    #[case::important_suffix(r#"class="mt-0!""#, &["mt-0!"])]
    #[case::container_query(r#"class="@md:flex""#, &["@md:flex"])]
    #[case::child_variant(r#"class="*:p-4""#, &["*:p-4"])]
    #[case::negative_value(r#"class="-mt-2""#, &["-mt-2"])]
    #[case::unicode(r#"class="größe""#, &["größe"])]
    fn extract_classes_should_keep_tailwind_names_whole(
        #[case] source: &str,
        #[case] expected: &[&str],
    ) {
        assert_finds(source, expected);
    }

    #[rstest]
    #[case::empty("")]
    #[case::only_delimiters(" \"'`<>={}();,\\$")]
    fn extract_classes_should_find_nothing_in_text_without_names(#[case] source: &str) {
        assert_eq!(extract_classes(source), BTreeSet::new());
    }

    #[test]
    fn extract_classes_should_borrow_substrings_of_the_input() {
        let source = r#"<div class="a b" :class="{ c: on }">"#;
        let found = extract_classes(source);
        assert!(found.iter().all(|name| source.contains(name)));
    }

    // Rule 10 from the research: no quadratic blow-up on long minified lines
    // full of brackets, as Tailwind's scanner once had.
    #[test]
    fn extract_classes_should_stay_linear_on_pathological_input() {
        let source = "[".repeat(200_000) + &"a(".repeat(200_000);
        let start = std::time::Instant::now();
        let _ = extract_classes(&source);
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }

    mod properties {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            // HTML splits class lists on ASCII whitespace only, so other
            // whitespace characters can legally be part of a name.
            fn candidates_are_non_empty_and_free_of_ascii_whitespace(source in ".{0,300}") {
                for name in extract_classes(&source) {
                    prop_assert!(!name.is_empty());
                    prop_assert!(!name.bytes().any(|b| b.is_ascii_whitespace()));
                    prop_assert!(!name.contains(['"', '\'', '`']));
                }
            }

            #[test]
            fn every_word_of_a_class_attribute_is_found(
                words in prop::collection::vec("[a-z][a-z0-9:/_-]{0,12}", 1..6),
            ) {
                let source = format!(r#"<div class="{}">"#, words.join(" "));
                let found = extract_classes(&source);
                for word in &words {
                    prop_assert!(found.contains(word.as_str()), "missing {word}");
                }
            }
        }
    }
}
