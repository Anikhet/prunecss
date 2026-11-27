//! Find the class and id names that content files might use.
//!
//! The scanner does not parse HTML, JSX, Vue or any other language. It splits
//! the text into *candidate* tokens and returns all of them, the same approach
//! as Tailwind's Rust scanner. Returning too many candidates is harmless: a
//! candidate only keeps CSS if a selector names it. Returning too few would
//! delete CSS the page uses, so every rule here errs on the side of more.

use std::borrow::Cow;
use std::collections::BTreeSet;

use rustc_hash::FxHashSet;

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
/// Names are borrowed from `source`; only names that exist after decoding
/// HTML entities (`&amp;`, `&quot;`, `&colon;`, `&#38;`, …) or JavaScript
/// escapes (`\x41`, `\u0041`, `\u{41}`) are copied.
///
/// ```
/// use prunecss_core::extract_classes;
///
/// let found = extract_classes(r#"<div className={clsx("card", { active: on })}>"#);
/// assert!(found.contains("card"));
/// assert!(found.contains("active"));
/// ```
#[must_use]
pub fn extract_classes(source: &str) -> BTreeSet<Cow<'_, str>> {
    // Most tokens are repeats (`class`, `div`, the same class names), so
    // de-duplicate in a fast hash set and sort only the unique names once.
    let mut names: FxHashSet<&str> = FxHashSet::default();
    collect(source, &mut |name| {
        names.insert(name);
    });
    let mut sorted: BTreeSet<Cow<'_, str>> =
        names.iter().map(|name| Cow::Borrowed(*name)).collect();
    // Server-rendered HTML escapes attribute text (`[&amp;_p]:mt-2`,
    // `class=&quot;badge&quot;`), sometimes twice when escaped markup is put
    // inside another attribute (`&amp;quot;`). Scripts escape characters
    // in strings (`"btn\x2dlg"` is `btn-lg`). Scan decoded copies as well,
    // decoding again until nothing changes; the few names that only exist
    // after decoding are the only ones that get copied.
    let mut decoded = decode_escapes(source);
    for pass in 1..=MAX_DECODE_PASSES {
        let Some(text) = decoded else { break };
        let mut decoded_names: FxHashSet<&str> = FxHashSet::default();
        collect(&text, &mut |name| {
            // `sorted` also holds what earlier passes found, so a name
            // found again is not copied again.
            if !names.contains(name) && !sorted.contains(name) {
                decoded_names.insert(name);
            }
        });
        sorted.extend(
            decoded_names
                .iter()
                .map(|name| Cow::Owned((*name).to_owned())),
        );
        decoded = if pass < MAX_DECODE_PASSES {
            decode_escapes(&text)
        } else {
            None
        };
    }
    sorted
}

/// How many times escaped text is decoded. Markup escaped three times is
/// already rare; the cap bounds the work on text like `&amp;amp;amp;…`,
/// which would otherwise need one full pass per `amp;`.
const MAX_DECODE_PASSES: usize = 3;

/// Emits every candidate in `text`.
fn collect<'t>(text: &'t str, emit: &mut impl FnMut(&'t str)) {
    // Two readings of the same text, because brackets are ambiguous: in
    // `grid-cols-[repeat(2,1fr)]` they hold part of one Tailwind name, while
    // in `clsx(['a', { b: on }])` they are JavaScript punctuation. Taking the
    // union of both readings finds the names either way.
    // Without brackets, `+` or `~` the two readings give the same tokens.
    let readings: &[Brackets] = if text.contains(['[', ']', '+', '~']) {
        &[
            Brackets::KeepWhole,
            Brackets::KeepWholeQuotesEnd,
            Brackets::Split,
        ]
    } else {
        &[Brackets::KeepWhole]
    };
    for &brackets in readings {
        for token in Tokens::new(text, brackets) {
            add_candidates(token, emit);
            if let Some(unescaped) = after_js_escape(text, token) {
                add_candidates(unescaped, emit);
            }
        }
    }
}

/// `"card\nbtn"` in a script is the two names `card` and `btn`, but the
/// backslash splits it into `card` and `nbtn`. For a token right after a
/// backslash, also try it without the escape's letters.
fn after_js_escape<'t>(text: &'t str, token: &'t str) -> Option<&'t str> {
    // `token` is a slice of `text`, so this is its byte offset.
    let start = (token.as_ptr() as usize).checked_sub(text.as_ptr() as usize)?;
    if start == 0 || text.as_bytes()[start - 1] != b'\\' {
        return None;
    }
    let bytes = token.as_bytes();
    let skip = match bytes.first()? {
        b'n' | b'r' | b't' | b'f' | b'v' | b'b' | b'0' => 1,
        b'u' if bytes.len() > 5 && bytes[1..5].iter().all(u8::is_ascii_hexdigit) => 5,
        b'x' if bytes.len() > 3 && bytes[1..3].iter().all(u8::is_ascii_hexdigit) => 3,
        _ => return None,
    };
    // Every skipped byte is ASCII, so this is a char boundary.
    Some(&token[skip..]).filter(|rest| !rest.is_empty())
}

/// Decodes the HTML entities and JavaScript hex escapes in `text`. Returns
/// `None` when there is nothing to decode, so most files are scanned once
/// and nothing is allocated for them.
fn decode_escapes(text: &str) -> Option<String> {
    let mut decoded: Option<String> = None;
    // `text[..copied]` is already in `decoded` (or needed no change).
    let mut copied = 0;
    let mut search = 0;
    while let Some(offset) = text[search..].find(['&', '\\']) {
        let at = search + offset;
        // `&` and `\` are ASCII, so `at + 1` is a char boundary.
        let after = &text[at + 1..];
        let escape = if text.as_bytes()[at] == b'&' {
            decode_one(after)
        } else {
            decode_js_escape(after)
        };
        match escape {
            Some((ch, used)) => {
                let out = decoded.get_or_insert_with(|| String::with_capacity(text.len()));
                out.push_str(&text[copied..at]);
                out.push(ch);
                copied = at + 1 + used;
                search = copied;
            }
            None => search = at + 1,
        }
    }
    let mut out = decoded?;
    out.push_str(&text[copied..]);
    Some(out)
}

/// Decodes one JavaScript escape that names a character by its code, at
/// the start of `text` (just after `\`): `x41`, `u0041` or `u{41}`.
/// Returns the character and how many bytes the escape used.
///
/// Letter escapes such as `\n` are left to [`after_js_escape`]: they stand
/// for whitespace, which only splits names.
fn decode_js_escape(text: &str) -> Option<(char, usize)> {
    let bytes = text.as_bytes();
    // `get` returns `None` instead of slicing inside a multi-byte char.
    let (digits, used) = match bytes.first()? {
        b'x' => (text.get(1..3)?, 3),
        b'u' if bytes.get(1) == Some(&b'{') => {
            // At most six hex digits fit in a code point: `u{10FFFF}`.
            let close = bytes.iter().take(9).position(|&byte| byte == b'}')?;
            (text.get(2..close)?, close + 1)
        }
        b'u' => (text.get(1..5)?, 5),
        _ => return None,
    };
    // `from_str_radix` would also accept a leading `+`.
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let code = u32::from_str_radix(digits, 16).ok()?;
    Some((char::from_u32(code)?, used))
}

/// Named entities that encoders emit for ASCII characters, from the HTML
/// standard's list. Characters outside ASCII cannot be part of a Tailwind
/// name's punctuation, so only these are needed. (`&hyphen;`, `&dash;` and
/// `&tilde;` are not here on purpose: they stand for U+2010 and U+02DC, not
/// for `-` and `~`.)
const NAMED_ENTITIES: [(&str, char); 46] = [
    ("amp", '&'),
    ("AMP", '&'),
    ("quot", '"'),
    ("QUOT", '"'),
    ("apos", '\''),
    ("lt", '<'),
    ("LT", '<'),
    ("gt", '>'),
    ("GT", '>'),
    ("nbsp", ' '),
    ("Tab", '\t'),
    ("NewLine", '\n'),
    ("colon", ':'),
    ("semi", ';'),
    ("lsqb", '['),
    ("lbrack", '['),
    ("rsqb", ']'),
    ("rbrack", ']'),
    ("lpar", '('),
    ("rpar", ')'),
    ("lcub", '{'),
    ("lbrace", '{'),
    ("rcub", '}'),
    ("rbrace", '}'),
    ("sol", '/'),
    ("bsol", '\\'),
    ("num", '#'),
    ("period", '.'),
    ("comma", ','),
    ("excl", '!'),
    ("quest", '?'),
    ("equals", '='),
    ("plus", '+'),
    ("ast", '*'),
    ("midast", '*'),
    ("percnt", '%'),
    ("dollar", '$'),
    ("commat", '@'),
    ("lowbar", '_'),
    ("UnderBar", '_'),
    ("verbar", '|'),
    ("vert", '|'),
    ("VerticalLine", '|'),
    ("Hat", '^'),
    ("grave", '`'),
    ("DiacriticalGrave", '`'),
];

/// The longest name in [`NAMED_ENTITIES`] (`DiacriticalGrave`).
const MAX_ENTITY_NAME: usize = 16;

/// Entities that browsers also decode without the `;`, for compatibility
/// with old markup (`[&amp_p]:mt-2`). Only the ASCII ones matter here.
const LEGACY_ENTITIES: [(&str, char); 9] = [
    ("amp", '&'),
    ("AMP", '&'),
    ("quot", '"'),
    ("QUOT", '"'),
    ("lt", '<'),
    ("LT", '<'),
    ("gt", '>'),
    ("GT", '>'),
    ("nbsp", ' '),
];

/// Decodes one entity at the start of `text` (just after `&`), returning the
/// character and how many bytes the entity used, including any `;`.
fn decode_one(text: &str) -> Option<(char, usize)> {
    if let Some(number) = text.strip_prefix('#') {
        return decode_numeric(number).map(|(ch, used)| (ch, used + 1));
    }
    // Entity names are ASCII, so `name_len` is a char boundary. Capping the
    // look-ahead keeps a long word after `&` from being scanned twice.
    let name_len = text
        .bytes()
        .take(MAX_ENTITY_NAME + 1)
        .take_while(u8::is_ascii_alphanumeric)
        .count();
    let name = &text[..name_len];
    if text.as_bytes().get(name_len) == Some(&b';')
        && let Some(&(_, ch)) = NAMED_ENTITIES.iter().find(|(known, _)| *known == name)
    {
        return Some((ch, name_len + 1));
    }
    // Browsers decode a legacy entity even when letters follow (`&ampx` is
    // `&x` in text), so match a prefix. Decoding too eagerly only adds
    // candidates: the raw text is scanned as well.
    LEGACY_ENTITIES
        .iter()
        .find(|(known, _)| name.starts_with(known))
        .map(|&(known, ch)| (ch, known.len()))
}

/// Decodes the digits of a numeric reference (after `&#`): `38;`, `x26;`,
/// or the same without the `;`, which browsers also accept.
fn decode_numeric(text: &str) -> Option<(char, usize)> {
    let (prefix, radix) = if text.starts_with(['x', 'X']) {
        (1, 16)
    } else {
        (0, 10)
    };
    // The prefix and the digits are ASCII, so these are char boundaries.
    let digits = text[prefix..]
        .bytes()
        .take_while(|byte| char::from(*byte).is_digit(radix))
        .count();
    if digits == 0 {
        return None;
    }
    let code = u32::from_str_radix(&text[prefix..prefix + digits], radix).ok()?;
    let mut used = prefix + digits;
    if text.as_bytes().get(used) == Some(&b';') {
        used += 1;
    }
    Some((char::from_u32(code)?, used))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Brackets {
    /// Keep `[…]` groups whole, quotes included, as in Tailwind arbitrary
    /// values (`bg-[url('/a.png')]`).
    KeepWhole,
    /// Keep `[…]` groups whole but end names at quotes, for names quoted
    /// inside a JavaScript array: `["w-[10px]", "p-4"]`. Without this, the
    /// outer `[` would swallow both names.
    KeepWholeQuotesEnd,
    /// Treat brackets as separators, as in JavaScript arrays.
    Split,
}

/// Bytes that end a name everywhere: no class name in markup contains them.
fn is_hard_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'`' | b'\\')
}

/// Quotes end a name, except inside `[…]` when brackets are kept whole:
/// `bg-[url('/a.png')]`, `before:content-['']`, `font-['Inter']`.
fn is_quote(byte: u8) -> bool {
    matches!(byte, b'"' | b'\'')
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
            let quote_inside_brackets =
                is_quote(byte) && self.brackets == Brackets::KeepWhole && depth > 0;
            let ends_token =
                if is_hard_delimiter(byte) || (is_quote(byte) && !quote_inside_brackets) {
                    // A name never spans whitespace, so an unbalanced `[` cannot
                    // swallow the rest of the file.
                    depth = 0;
                    true
                } else {
                    match self.brackets {
                        // `+` and `~` also split here: `querySelector(".a+.b")`.
                        Brackets::Split => {
                            is_soft_delimiter(byte) || matches!(byte, b'[' | b']' | b'+' | b'~')
                        }
                        Brackets::KeepWhole | Brackets::KeepWholeQuotesEnd => {
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

/// Emits `token` and the shorter names hidden inside it.
///
/// `.modal` (a selector string) also yields `modal`, `active:` (an object
/// key) yields `active`, `class:active` (Svelte) and `styles.button` (CSS
/// modules) yield `active` and `button`, and `/about#team` (a link)
/// yields `team`. Extra candidates cost nothing.
fn add_candidates<'t>(token: &'t str, emit: &mut impl FnMut(&'t str)) {
    const EDGE_PUNCTUATION: [char; 7] = ['.', '#', ':', '!', '?', '&', '|'];

    emit(token);
    let trimmed = token
        .trim_start_matches(EDGE_PUNCTUATION)
        .trim_end_matches(EDGE_PUNCTUATION);
    if trimmed.is_empty() {
        return;
    }
    if trimmed.len() != token.len() {
        emit(trimmed);
    }
    // `#` splits too: `/about#team` links to the `#team` rule. A `#` is
    // never part of a Tailwind name outside `[…]`, and a part inside one
    // (`bg-[#fff]`) is only an extra candidate.
    if trimmed.contains([':', '.', '#']) {
        for part in trimmed
            .split([':', '.', '#'])
            .filter(|part| !part.is_empty())
        {
            emit(part);
        }
    }
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

    // Found in the pre-release review: each of these deleted used CSS.
    #[rstest]
    #[case::quoted_url_value(r#"<div class="bg-[url('/a.png')]">"#, &["bg-[url('/a.png')]"])]
    #[case::empty_content_value(r#"<p class="before:content-['']">"#, &["before:content-['']"])]
    #[case::quoted_font(r#"<p class="font-['Inter']">"#, &["font-['Inter']"])]
    #[case::double_quotes_in_single(r#"<p class='content-["x"]'>"#, &[r#"content-["x"]"#])]
    #[case::arbitrary_property(r#"<p class="[content:'']">"#, &["[content:'']"])]
    #[case::entity_quotes(r#"<div data-title="<span class=&quot;badge&quot;>">"#, &["badge"])]
    #[case::entity_ampersand(r#"<p class="[&amp;_p]:mt-2">"#, &["[&_p]:mt-2"])]
    #[case::entity_apostrophe(r#"<p class="before:content-[&#x27;&#x27;]">"#, &["before:content-['']"])]
    #[case::entity_space(r#"<p class="a&#32;b">"#, &["a", "b"])]
    #[case::query_adjacent(r#"querySelector(".a+.b")"#, &["a", "b"])]
    #[case::query_sibling(r#"querySelector(".c~.d")"#, &["c", "d"])]
    #[case::js_newline_escape(r#"const html = "card\nbtn";"#, &["card", "btn"])]
    // Second review: an outer JS/Vue array must not swallow quoted names.
    #[case::react_children_array(r#"children:[(0,r.jsx)("div",{className:"w-[10px] p-4"})]"#, &["w-[10px]", "p-4"])]
    #[case::vue_class_array(r#"<p :class="['w-[10px]', on && 'p-4']">"#, &["w-[10px]", "p-4"])]
    #[case::clsx_array(r#"clsx(["grid-cols-[1fr_2fr]", "bg-[url('/a.png')]"])"#, &["grid-cols-[1fr_2fr]", "bg-[url('/a.png')]"])]
    #[case::js_unicode_escape(r#"const html = "card\u0020btn";"#, &["btn"])]
    fn extract_classes_should_find_names_the_review_found_missing(
        #[case] source: &str,
        #[case] expected: &[&str],
    ) {
        assert_finds(source, expected);
    }

    // Third review: each of these deleted CSS a page uses.
    #[rstest]
    #[case::double_escaped_quotes(
        r#"<div data-x="&lt;p class=&amp;quot;badge&amp;quot;&gt;">"#,
        &["badge"]
    )]
    #[case::double_escaped_ampersand(r#"<p class="[&amp;amp;_p]:mt-2">"#, &["[&_p]:mt-2"])]
    #[case::entity_colon(r#"<p class="hover&colon;underline">"#, &["hover:underline"])]
    #[case::entity_square_brackets(r#"<p class="w-&lsqb;10px&rsqb;">"#, &["w-[10px]"])]
    #[case::entity_brack_names(r#"<p class="w-&lbrack;10px&rbrack;">"#, &["w-[10px]"])]
    #[case::entity_solidus(r#"<p class="w-1&sol;2">"#, &["w-1/2"])]
    #[case::entity_lowbar_and_num(r#"<p class="[&amp;&lowbar;p]:mt-2 &num;x">"#, &["[&_p]:mt-2", "x"])]
    #[case::entity_punctuation(
        r#"<p class="a&excl; &quest;b c&period;d e&comma;f g&equals;h">"#,
        &["a!", "?b", "d", "f", "h"]
    )]
    #[case::entity_math(r#"<p class="&ast;:p-4 w-&lsqb;50&percnt;&rsqb; [&plus;_p]:m-0">"#, &["*:p-4", "w-[50%]", "[+_p]:m-0"])]
    #[case::entity_at(r#"<p class="&commat;md:flex">"#, &["@md:flex"])]
    #[case::legacy_amp_without_semicolon(r#"<p class="[&amp_p]:mt-2">"#, &["[&_p]:mt-2"])]
    #[case::legacy_quot_without_semicolon(r#"<i data-x="<b class=&quotbadge&quot>">"#, &["badge"])]
    #[case::numeric_without_semicolon(r#"<p class="[&#38_p]:mt-2">"#, &["[&_p]:mt-2"])]
    #[case::fragment_in_path(r#"<a href="/about#team">"#, &["team"])]
    #[case::js_hex_escape(r#"const c = "\x41bc";"#, &["Abc"])]
    #[case::js_unicode_escape_letter(r#"const c = "\u0041bc";"#, &["Abc"])]
    #[case::js_braced_unicode_escape(r#"const c = "\u{41}bc";"#, &["Abc"])]
    #[case::js_escape_mid_name(r#"const c = "btn\x2dlg";"#, &["btn-lg"])]
    fn extract_classes_should_find_names_the_third_review_found_missing(
        #[case] source: &str,
        #[case] expected: &[&str],
    ) {
        assert_finds(source, expected);
    }

    #[test]
    fn extract_classes_should_borrow_names_that_exist_before_decoding() {
        let found = extract_classes(r#"<p class="hover&colon;underline hover">"#);
        let owned: Vec<&str> = found
            .iter()
            .filter(|name| matches!(name, Cow::Owned(_)))
            .map(AsRef::as_ref)
            .collect();
        // `hover` and `underline` are in the raw text, so only the joined
        // name is copied.
        assert_eq!(owned, vec!["hover:underline"]);
    }

    #[test]
    fn extract_classes_should_stop_decoding_after_a_few_passes() {
        // Each pass decodes one legacy `&amp`; the fourth is never run.
        let found = extract_classes("x&ampampampamplty");
        let names: Vec<&str> = found.iter().map(AsRef::as_ref).collect();
        assert_eq!(
            names,
            vec![
                "x&ampampampamplty",
                "x&ampampamplty",
                "x&ampamplty",
                "x&amplty"
            ]
        );
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
        assert!(found.iter().all(|name| source.contains(name.as_ref())));
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
                    // Quotes may appear inside `[…]` (`bg-[url('x')]`), backticks never.
                    prop_assert!(!name.contains('`'));
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
