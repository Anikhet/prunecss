//! Remove CSS rules whose selectors can never match the scanned content.
//!
//! Matching is a "bag of names" check, the same conservative model PurgeCSS
//! uses: a selector is kept when every class and id it *requires* appears in
//! the set of used names. Page structure (combinators like `>`), tags,
//! attributes and pseudo-classes are not checked, so they never cause a
//! removal. That keeps more CSS than strictly needed, but never deletes CSS a
//! page uses.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::BuildHasher;
use std::sync::{Arc, RwLock};

use lightningcss::error::ParserError;
use lightningcss::rules::style::StyleRule;
use lightningcss::rules::{CssRule, CssRuleList};
use lightningcss::selector::{Component, PseudoClass, Selector};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::targets::{Browsers, Features, Targets};
use lightningcss::vendor_prefix::VendorPrefix;

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

/// The set [`extract_classes`](crate::extract_classes) returns.
impl UsedNames for BTreeSet<Cow<'_, str>> {
    fn contains_name(&self, name: &str) -> bool {
        self.contains(name)
    }
}

impl<S: BuildHasher> UsedNames for HashSet<Cow<'_, str>, S> {
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
    /// How many kept selectors require at least one class or id. Zero while
    /// `removed_selectors` is not usually means the content was not the
    /// right content.
    pub kept_named_selectors: usize,
    /// Rules and declarations that could not be parsed and were dropped,
    /// in input order.
    pub warnings: Vec<ParseWarning>,
    /// How many `@keyframes` rules were removed.
    pub removed_keyframes: usize,
    /// How many `@font-face` rules were removed.
    pub removed_font_faces: usize,
    /// How many custom property declarations were removed.
    pub removed_variables: usize,
}

/// A rule or declaration the parser could not read. Like a browser,
/// prunecss skips it, so it is missing from the output; callers should tell
/// the user.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseWarning {
    /// What could not be read.
    pub message: String,
    /// Where it starts in the input, if known.
    pub location: Option<SourceLocation>,
    /// Whether the rule or declaration is missing from the output, judged
    /// by what the parser kept. Unknown at-rules and pseudo-classes, and
    /// media queries the parser recovers, are warned about but kept
    /// exactly as written.
    pub dropped: bool,
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
/// to recover or nests blocks (rules, at-rules, `:is(`, functions) more
/// than 40 levels deep, and a [`PruneErrorKind::Print`] error if printing
/// fails.
///
/// # Stack use
///
/// Parsing and printing recurse once per nesting level. The depth limit
/// keeps the deepest accepted stylesheet within about 1 MiB of stack in a
/// debug build (less in release), so `prune` is safe on the 2 MiB stack
/// Rust gives spawned threads and `cargo test`. Call it with less than
/// about 1 MiB of stack to spare and deeply nested input can still
/// overflow.
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
    // Browsers drop a leading byte-order mark; left in, it would become
    // part of the first selector.
    let css = css.strip_prefix('\u{feff}').unwrap_or(css);
    let (masked, masks) = mask_legacy_names(css);
    let css = masked.as_deref().unwrap_or(css);
    let warnings = Arc::new(RwLock::new(Vec::new()));
    let mut sheet = parse_sheet(css, Some(Arc::clone(&warnings)))?;
    // Whether a warned-about rule was dropped is judged by what the parser
    // kept, so note where every parsed rule starts before pruning.
    let any_warnings = warnings.read().is_ok_and(|warnings| !warnings.is_empty());
    let rule_starts = if any_warnings {
        rule_starts(&sheet.rules)
    } else {
        Vec::new()
    };

    let matcher = Matcher {
        used,
        safelist: &options.safelist,
        kept_named: Cell::new(0),
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

    repair_legacy_pseudo_classes(&mut sheet.rules);

    // Printing only; `StyleSheet::minify` is deliberately not called because
    // it also merges rules and rewrites values, which is not our job.
    let printer_options = PrinterOptions {
        minify: options.minify,
        targets: Targets {
            // Without targets the printer writes the newest, shortest
            // syntax (`#00000080` for `rgba(0,0,0,.5)`, `(width >= 768px)`
            // for `(min-width: 768px)`), which older browsers ignore. Old
            // targets make it keep widely supported forms.
            browsers: Some(conservative_browsers()),
            include: Features::MediaQueries,
            // ...but never flatten CSS nesting: that is a structural
            // rewrite, not printing, and the user wrote nesting on purpose.
            exclude: Features::Nesting,
        },
        ..PrinterOptions::default()
    };
    let printed = sheet
        .to_css(printer_options)
        .map_err(|err| PruneError::from_lightningcss(PruneErrorKind::Print, &err))?;
    let warnings = warnings
        .read()
        .map(|errors| classify_warnings(css, &rule_starts, &errors))
        .unwrap_or_default()
        .into_iter()
        .map(|mut warning| {
            warning.message = masks.unmask(warning.message);
            warning
        })
        .collect();

    Ok(PruneOutput {
        css: with_charset(masks.unmask(printed.code), css, options.minify),
        warnings,
        removed_selectors,
        kept_named_selectors: matcher.kept_named.get(),
        removed_keyframes: tracked.keyframes,
        removed_font_faces: tracked.font_faces,
        removed_variables: tracked.variables,
    })
}

/// A selector name Lightning CSS misreads, so [`mask_legacy_names`] hides
/// it before parsing.
struct Legacy {
    /// The name after the colon(s), in lower case.
    name: &'static str,
    /// After `::` (pseudo-element), `:` (pseudo-class), or either.
    position: Colons,
    /// The start of its mask; digits fill the rest.
    mask_prefix: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Colons {
    One,
    Either,
}

/// - `::-ms-input-placeholder` (IE/Edge pseudo-element) is read as
///   `::-moz-placeholder`, and after parsing the two can't be told apart.
/// - The pseudo-classes `:-ms-input-placeholder` (IE 10-11) and
///   `:-moz-placeholder` (Firefox 4-18) are read as prefixed
///   `:placeholder-shown` and would print as `:-ms-placeholder-shown` /
///   `:-moz-placeholder-shown`, which no browser supports. Masking the
///   text reaches every position, `:nth-child(… of …)` included, which
///   [`repair_selector`] cannot rebuild.
///
/// `::-moz-placeholder` is read correctly and is left alone.
const LEGACY_NAMES: [Legacy; 2] = [
    Legacy {
        name: "-ms-input-placeholder",
        position: Colons::Either,
        mask_prefix: "-pcss-ms-",
    },
    Legacy {
        name: "-moz-placeholder",
        position: Colons::One,
        mask_prefix: "-pcss-moz-",
    },
];

/// The masks used for one stylesheet, to undo with [`Masks::unmask`].
#[derive(Debug, Default)]
struct Masks(Vec<(String, &'static str)>);

impl Masks {
    fn unmask(&self, text: String) -> String {
        self.0.iter().fold(text, |text, (mask, name)| {
            if text.contains(mask.as_str()) {
                text.replace(mask.as_str(), name)
            } else {
                text
            }
        })
    }
}

/// Replaces each [`LEGACY_NAMES`] entry, in any ASCII case, with a name
/// Lightning CSS keeps as written; printing and [`Masks::unmask`] restore
/// it. A mask has the same length as the name, so warning columns still
/// point into the original text, and it is chosen so it does not occur in
/// `css`, so unmasking never rewrites text that was there already.
fn mask_legacy_names(css: &str) -> (Option<String>, Masks) {
    let mut masked: Option<String> = None;
    let mut masks = Masks::default();
    for legacy in &LEGACY_NAMES {
        let text = masked.as_deref().unwrap_or(css);
        let starts = legacy_name_starts(text, legacy);
        if starts.is_empty() {
            continue;
        }
        let Some(mask) = unused_mask(text, legacy.mask_prefix, legacy.name.len()) else {
            continue;
        };
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for start in starts {
            out.push_str(text.get(copied..start).unwrap_or_default());
            out.push_str(&mask);
            copied = start + legacy.name.len();
        }
        out.push_str(text.get(copied..).unwrap_or_default());
        masks.0.push((mask, legacy.name));
        masked = Some(out);
    }
    (masked, masks)
}

/// Byte offsets where `legacy.name` starts after its colon(s) in `css`.
fn legacy_name_starts(css: &str, legacy: &Legacy) -> Vec<usize> {
    let bytes = css.as_bytes();
    css.match_indices(':')
        .map(|(colon, _)| colon + 1)
        .filter(|&start| {
            let is_element = start >= 2 && bytes.get(start - 2) == Some(&b':');
            (legacy.position == Colons::Either || !is_element)
                && bytes
                    .get(start..start + legacy.name.len())
                    .is_some_and(|name| name.eq_ignore_ascii_case(legacy.name.as_bytes()))
        })
        .collect()
}

/// A mask `prefix` + digits, `width` bytes long, that does not occur in
/// `css` in any case. Only windows starting with `prefix` can collide, and
/// there are fewer of those than candidates tried, so one is always free.
fn unused_mask(css: &str, prefix: &str, width: usize) -> Option<String> {
    let bytes = css.as_bytes();
    let taken: HashSet<Vec<u8>> = css
        .match_indices('-')
        .filter_map(|(start, _)| bytes.get(start..start + width))
        .filter(|window| {
            window
                .get(..prefix.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(prefix.as_bytes()))
        })
        .map(<[u8]>::to_ascii_lowercase)
        .collect();
    let digits = width.checked_sub(prefix.len())?;
    (0..=taken.len())
        .map(|n| format!("{prefix}{n:0digits$}"))
        .find(|mask| mask.len() == width && !taken.contains(mask.as_bytes()))
}

/// Deeper nesting than this is refused. Real stylesheets nest a handful of
/// levels; too many overflow the stack in the parser and abort the process.
///
/// Measured in a debug build on a 2 MiB thread with every option on, the
/// costliest construct, nested `@media` (about 24 KiB of stack per level),
/// overflows at 88 levels; 40 leaves more than half the stack free. The
/// `..._on_a_2_mib_stack` tests prove it for each kind of nesting. Keep
/// the number in [`prune`]'s docs in sync.
const MAX_NESTING: usize = 40;

type WarningSink<'i> = Arc<RwLock<Vec<lightningcss::error::Error<ParserError<'i>>>>>;

fn parse_sheet<'i>(
    css: &'i str,
    warnings: Option<WarningSink<'i>>,
) -> Result<StyleSheet<'i>, PruneError> {
    if exceeds_nesting(css, MAX_NESTING) {
        return Err(PruneError {
            kind: PruneErrorKind::Parse,
            message: format!("the stylesheet is nested more than {MAX_NESTING} levels deep"),
            location: None,
        });
    }
    let parser_options = ParserOptions {
        // Skip a broken rule instead of failing the whole file, as browsers
        // do; real-world stylesheets often contain a few. Each one is
        // reported through `warnings`.
        error_recovery: true,
        warnings,
        ..ParserOptions::default()
    };
    StyleSheet::parse(css, parser_options)
        .map_err(|err| PruneError::from_lightningcss(PruneErrorKind::Parse, &err))
}

/// Whether `css` nests blocks (`{`, `(`, `[` and functions) more than
/// `limit` levels deep, as the parser will see them.
///
/// This runs on every stylesheet, so it is a single linear pass that stops
/// as soon as the limit is passed.
fn exceeds_nesting(css: &str, limit: usize) -> bool {
    let mut tokens = Tokenizer::new(css);
    // Like the parser, a closing bracket only ends the innermost block when
    // it matches; a stray `]` inside `f(` is just a token. Counting every
    // closer would let `f(]f(]…` hide its depth.
    let mut open: Vec<u8> = Vec::new();
    while let Some(token) = tokens.next_token() {
        match token {
            Token::Open(close) => {
                open.push(close);
                if open.len() > limit {
                    return true;
                }
            }
            Token::Close(close) if open.last() == Some(&close) => {
                open.pop();
            }
            _ => {}
        }
    }
    false
}

/// What [`Tokenizer`] reports: only what decides the block structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// A block or function opens; the byte is the one that closes it.
    Open(u8),
    /// `)`, `]` or `}`.
    Close(u8),
    Semicolon,
    /// Whitespace, a comment, `<!--` or `-->`.
    Trivia,
    /// Any other token.
    Other,
}

/// A port of the token boundaries of `cssparser`'s tokenizer (CSS Syntax
/// §4), which Lightning CSS parses with. It must agree with it exactly:
/// where a string, comment or `url(` ends decides which brackets count, and
/// any disagreement either hides nesting (a stack overflow) or invents it
/// (a refused stylesheet). Newlines are `\n`, `\r\n`, `\r` and `\x0C`, as
/// after the spec's preprocessing.
struct Tokenizer<'a> {
    css: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

const URL: [char; 3] = ['u', 'r', 'l'];

impl<'a> Tokenizer<'a> {
    fn new(css: &'a str) -> Self {
        Self {
            css,
            bytes: css.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn has_newline_at(&self, offset: usize) -> bool {
        matches!(self.peek(offset), Some(b'\n' | b'\r' | b'\x0C'))
    }

    fn starts_with(&self, text: &[u8]) -> bool {
        self.bytes
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with(text))
    }

    fn next_token(&mut self) -> Option<Token> {
        let byte = self.peek(0)?;
        let token = match byte {
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0C' => {
                self.pos += 1;
                Token::Trivia
            }
            b'"' | b'\'' => {
                self.string(byte);
                Token::Other
            }
            b'#' => {
                self.pos += 1;
                if self.is_ident_start() || matches!(self.peek(0), Some(b'0'..=b'9' | b'-')) {
                    self.name();
                }
                Token::Other
            }
            b'(' => self.open(b')'),
            b'[' => self.open(b']'),
            b'{' => self.open(b'}'),
            b')' | b']' | b'}' => {
                self.pos += 1;
                Token::Close(byte)
            }
            b';' => {
                self.pos += 1;
                Token::Semicolon
            }
            b'+' | b'-' | b'.' if self.starts_number() => self.numeric(),
            b'-' if self.starts_with(b"-->") => {
                self.pos += 3;
                Token::Trivia
            }
            b'-' if self.is_ident_start() => self.ident_like(),
            b'0'..=b'9' => self.numeric(),
            b'/' if self.peek(1) == Some(b'*') => {
                self.comment();
                Token::Trivia
            }
            b'<' if self.starts_with(b"<!--") => {
                self.pos += 4;
                Token::Trivia
            }
            b'@' => {
                self.pos += 1;
                if self.is_ident_start() {
                    self.name();
                }
                Token::Other
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'\0' => self.ident_like(),
            b'\\' if !self.has_newline_at(1) => self.ident_like(),
            _ if !byte.is_ascii() => self.ident_like(),
            _ => {
                self.pos += 1;
                Token::Other
            }
        };
        Some(token)
    }

    fn open(&mut self, close: u8) -> Token {
        self.pos += 1;
        Token::Open(close)
    }

    fn consume_newline(&mut self) {
        let crlf = self.peek(0) == Some(b'\r') && self.peek(1) == Some(b'\n');
        self.pos += if crlf { 2 } else { 1 };
    }

    fn is_ident_start(&self) -> bool {
        match self.peek(0) {
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'\0') => true,
            // cssparser checks the byte at offset 1 for `-\`, which is the
            // backslash itself, so `-\` always starts an identifier there.
            Some(b'-') => match self.peek(1) {
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'\0' | b'\\') => true,
                Some(next) => !next.is_ascii(),
                None => false,
            },
            Some(b'\\') => !self.has_newline_at(1),
            Some(byte) => !byte.is_ascii(),
            None => false,
        }
    }

    fn starts_number(&self) -> bool {
        let digit = |offset| self.peek(offset).is_some_and(|b: u8| b.is_ascii_digit());
        match self.peek(0) {
            Some(b'+' | b'-') => digit(1) || (self.peek(1) == Some(b'.') && digit(2)),
            Some(b'.') => digit(1),
            _ => false,
        }
    }

    fn skip_digits(&mut self) {
        while self.peek(0).is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    fn numeric(&mut self) -> Token {
        let digit = |this: &Self, offset| this.peek(offset).is_some_and(|b: u8| b.is_ascii_digit());
        if matches!(self.peek(0), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        self.skip_digits();
        if self.peek(0) == Some(b'.') && digit(self, 1) {
            self.pos += 1;
            self.skip_digits();
        }
        if matches!(self.peek(0), Some(b'e' | b'E'))
            && (digit(self, 1) || (matches!(self.peek(1), Some(b'+' | b'-')) && digit(self, 2)))
        {
            self.pos += 2;
            self.skip_digits();
        }
        if self.peek(0) == Some(b'%') {
            self.pos += 1;
        } else if self.is_ident_start() {
            // A unit: `1url(` is a dimension followed by a parenthesis.
            self.name();
        }
        Token::Other
    }

    /// Consumes a name and returns whether it reads `url`, ignoring ASCII
    /// case, once escapes are decoded (`u\72l` is `url` too).
    fn name(&mut self) -> bool {
        let mut length = 0;
        let mut is_url = true;
        let mut push = |c: char| {
            is_url = is_url && URL.get(length).is_some_and(|u| c.eq_ignore_ascii_case(u));
            length += 1;
        };
        while let Some(byte) = self.peek(0) {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' => {
                    self.pos += 1;
                    push(char::from(byte));
                }
                b'\0' => {
                    self.pos += 1;
                    push(char::REPLACEMENT_CHARACTER);
                }
                b'\\' if !self.has_newline_at(1) => {
                    self.pos += 1;
                    push(self.escape());
                }
                _ if !byte.is_ascii() => {
                    self.pos += 1;
                    push(char::REPLACEMENT_CHARACTER);
                }
                _ => break,
            }
        }
        is_url && length == URL.len()
    }

    /// Consumes an escape after its backslash, which is not followed by a
    /// newline, and returns the character it stands for.
    fn escape(&mut self) -> char {
        let Some(byte) = self.peek(0) else {
            return char::REPLACEMENT_CHARACTER;
        };
        if byte.is_ascii_hexdigit() {
            let mut value = 0_u32;
            let mut digits = 0;
            while digits < 6 {
                let Some(digit) = self.peek(0).and_then(|b| char::from(b).to_digit(16)) else {
                    break;
                };
                value = value * 16 + digit;
                digits += 1;
                self.pos += 1;
            }
            // One whitespace after a hex escape belongs to it, newlines
            // included: `"\41` + newline does not end the string.
            match self.peek(0) {
                Some(b' ' | b'\t') => self.pos += 1,
                Some(b'\n' | b'\r' | b'\x0C') => self.consume_newline(),
                _ => {}
            }
            return char::from_u32(value)
                .filter(|c| *c != '\0')
                .unwrap_or(char::REPLACEMENT_CHARACTER);
        }
        let c = self
            .css
            .get(self.pos..)
            .and_then(|rest| rest.chars().next())
            .unwrap_or(char::REPLACEMENT_CHARACTER);
        self.pos += c.len_utf8().max(1);
        if c == '\0' {
            char::REPLACEMENT_CHARACTER
        } else {
            c
        }
    }

    fn string(&mut self, quote: u8) {
        self.pos += 1;
        while let Some(byte) = self.peek(0) {
            match byte {
                _ if byte == quote => {
                    self.pos += 1;
                    return;
                }
                // An unescaped newline ends a (bad) string and is not part
                // of it.
                b'\n' | b'\r' | b'\x0C' => return,
                b'\\' => {
                    self.pos += 1;
                    if self.has_newline_at(0) {
                        self.consume_newline();
                    } else if self.peek(0).is_some() {
                        self.escape();
                    }
                }
                _ => self.pos += 1,
            }
        }
    }

    fn comment(&mut self) {
        self.pos += 2;
        self.pos = self
            .css
            .get(self.pos..)
            .and_then(|rest| rest.find("*/"))
            .map_or(self.bytes.len(), |end| self.pos + end + 2);
    }

    fn ident_like(&mut self) -> Token {
        let is_url = self.name();
        if self.peek(0) != Some(b'(') {
            return Token::Other;
        }
        self.pos += 1;
        if is_url && self.unquoted_url() {
            Token::Other
        } else {
            Token::Open(b')')
        }
    }

    /// After `url(`: consumes an unquoted URL token and returns `true`, or
    /// returns `false` without consuming anything when a quote follows,
    /// which makes `url(` an ordinary function.
    fn unquoted_url(&mut self) -> bool {
        let rest = self.bytes.get(self.pos..).unwrap_or_default();
        let blank = rest
            .iter()
            .take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0C'))
            .count();
        match rest.get(blank) {
            Some(b'"' | b'\'') => return false,
            None => {
                self.pos = self.bytes.len();
                return true;
            }
            Some(_) => self.pos += blank,
        }
        while let Some(byte) = self.peek(0) {
            match byte {
                b')' => {
                    self.pos += 1;
                    return true;
                }
                b' ' | b'\t' | b'\n' | b'\r' | b'\x0C' => {
                    self.url_end();
                    return true;
                }
                b'\x01'..=b'\x08' | b'\x0B' | b'\x0E'..=b'\x1F' | b'\x7F' | b'"' | b'\'' | b'(' => {
                    self.pos += 1;
                    self.bad_url();
                    return true;
                }
                b'\\' => {
                    self.pos += 1;
                    if self.has_newline_at(0) {
                        self.bad_url();
                        return true;
                    }
                    self.escape();
                }
                _ => self.pos += 1,
            }
        }
        true
    }

    /// Whitespace inside an unquoted URL: only `)` may follow.
    fn url_end(&mut self) {
        while let Some(byte) = self.peek(0) {
            self.pos += 1;
            match byte {
                b')' => return,
                b' ' | b'\t' | b'\n' | b'\r' | b'\x0C' => {}
                _ => return self.bad_url(),
            }
        }
    }

    /// The rest of a broken URL, up to an unescaped `)`.
    fn bad_url(&mut self) {
        while let Some(byte) = self.peek(0) {
            self.pos += 1;
            match byte {
                b')' => return,
                b'\\' if matches!(self.peek(0), Some(b')' | b'\\')) => self.pos += 1,
                _ => {}
            }
        }
    }
}

/// Browser versions old enough that the printer keeps widely supported
/// syntax: `rgba()` rather than 8-digit hex, `transparent` rather than
/// `#0000`. Versions are `major << 16 | minor << 8`.
fn conservative_browsers() -> Browsers {
    Browsers {
        ie: Some(11 << 16),
        edge: Some(12 << 16),
        safari: Some(9 << 16),
        ios_saf: Some(9 << 16),
        chrome: Some(49 << 16),
        firefox: Some(45 << 16),
        android: Some((4 << 16) | (4 << 8)),
        ..Browsers::default()
    }
}

/// Lightning CSS drops `@charset` and prints escapes such as `\201C` as the
/// raw character, so declare UTF-8 whenever the output needs it; otherwise a
/// page in another encoding would garble it.
fn with_charset(code: String, input: &str, minify: bool) -> String {
    let had_charset = input.trim_start_matches('\u{feff}').starts_with("@charset");
    if code.is_ascii() && !had_charset {
        return code;
    }
    let separator = if minify { "" } else { "\n" };
    format!("@charset \"UTF-8\";{separator}{code}")
}

impl ParseWarning {
    fn from_lightningcss(err: &LightningError<'_>, dropped: bool) -> Self {
        Self {
            dropped,
            message: err.kind.to_string(),
            location: err.loc.as_ref().map(|loc| SourceLocation {
                line: loc.line.saturating_add(1),
                column: loc.column,
            }),
        }
    }
}

type LightningError<'i> = lightningcss::error::Error<ParserError<'i>>;

/// A position as Lightning CSS reports it: 0-based line, 1-based column
/// in UTF-16 code units. Ordered like the text.
type LineColumn = (u32, u32);

/// Turns parser errors into warnings, deciding `dropped` by what survived
/// rather than by error kind: the same kind can mean either (an empty
/// `@supports` prelude is "unexpected end of input" and dropped, a broken
/// media feature is "invalid media query" and kept as written).
///
/// A rule the parser kept appears in `rule_starts`. An error inside the
/// prelude of a kept rule (before its `{` or `;`) was recovered in place,
/// so nothing went missing. Anywhere else, the error is about a rule or
/// declaration that has no place in the parsed sheet: it was dropped. The
/// one exception is an error at the very end of input after nothing but
/// stray closing brackets, which drops nothing.
fn classify_warnings(
    css: &str,
    rule_starts: &[LineColumn],
    errors: &[LightningError<'_>],
) -> Vec<ParseWarning> {
    let position = |err: &LightningError<'_>| err.loc.as_ref().map(|loc| (loc.line, loc.column));
    // The kept rule that starts last at or before `at`.
    let owner = |at: LineColumn| {
        let after = rule_starts.partition_point(|start| *start <= at);
        after
            .checked_sub(1)
            .and_then(|index| rule_starts.get(index))
            .copied()
    };
    let mut targets: Vec<LineColumn> = errors
        .iter()
        .filter_map(position)
        .flat_map(|at| [Some(at), owner(at)])
        .flatten()
        .collect();
    targets.sort_unstable();
    targets.dedup();
    let offsets = byte_offsets(css, &targets);
    let offset_of = |at: LineColumn| {
        targets
            .binary_search(&at)
            .ok()
            .and_then(|index| offsets.get(index).copied())
    };
    let junk_tail = offsets.iter().any(|&offset| offset >= css.len()) && tail_is_junk(css);
    let mut prelude_ends = BTreeMap::new();
    errors
        .iter()
        .map(|err| {
            let kept = position(err).and_then(|at| {
                let offset = offset_of(at)?;
                let in_kept_prelude = owner(at).and_then(offset_of).is_some_and(|start| {
                    let end = *prelude_ends
                        .entry(start)
                        .or_insert_with(|| prelude_end(css, start));
                    offset <= end
                });
                Some(in_kept_prelude || (offset >= css.len() && junk_tail))
            });
            ParseWarning::from_lightningcss(err, !kept.unwrap_or(false))
        })
        .collect()
}

/// Where every parsed rule starts, sorted. Nested declarations are left
/// out: they are not rules the user wrote, so an error among them belongs
/// to the enclosing rule's block.
fn rule_starts<R>(rules: &CssRuleList<'_, R>) -> Vec<LineColumn> {
    fn collect<R>(rules: &CssRuleList<'_, R>, starts: &mut Vec<LineColumn>) {
        for rule in &rules.0 {
            let loc = match rule {
                CssRule::Media(rule) => Some(rule.loc),
                CssRule::Import(rule) => Some(rule.loc),
                CssRule::Style(rule) => Some(rule.loc),
                CssRule::Keyframes(rule) => Some(rule.loc),
                CssRule::FontFace(rule) => Some(rule.loc),
                CssRule::FontPaletteValues(rule) => Some(rule.loc),
                CssRule::FontFeatureValues(rule) => Some(rule.loc),
                CssRule::Page(rule) => Some(rule.loc),
                CssRule::Supports(rule) => Some(rule.loc),
                CssRule::CounterStyle(rule) => Some(rule.loc),
                CssRule::Namespace(rule) => Some(rule.loc),
                CssRule::MozDocument(rule) => Some(rule.loc),
                CssRule::Nesting(rule) => Some(rule.loc),
                CssRule::Viewport(rule) => Some(rule.loc),
                CssRule::CustomMedia(rule) => Some(rule.loc),
                CssRule::LayerStatement(rule) => Some(rule.loc),
                CssRule::LayerBlock(rule) => Some(rule.loc),
                CssRule::Property(rule) => Some(rule.loc),
                CssRule::Container(rule) => Some(rule.loc),
                CssRule::Scope(rule) => Some(rule.loc),
                CssRule::StartingStyle(rule) => Some(rule.loc),
                CssRule::ViewTransition(rule) => Some(rule.loc),
                CssRule::PositionTry(rule) => Some(rule.loc),
                CssRule::Unknown(rule) => Some(rule.loc),
                // Anything else has no location; an error about it is
                // reported as dropped, the safe side.
                _ => None,
            };
            starts.extend(loc.map(|loc| (loc.line, loc.column)));
            if let Some(group) = rules::group_rules(rule) {
                collect(group, starts);
            }
            match rule {
                CssRule::Style(style) => collect(&style.rules, starts),
                CssRule::Nesting(nesting) => collect(&nesting.style.rules, starts),
                CssRule::LayerBlock(layer) => collect(&layer.rules, starts),
                _ => {}
            }
        }
    }
    let mut starts = Vec::new();
    collect(rules, &mut starts);
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// Converts sorted Lightning CSS positions to byte offsets in one pass,
/// counting lines and columns exactly as cssparser does.
fn byte_offsets(css: &str, targets: &[LineColumn]) -> Vec<usize> {
    let bytes = css.as_bytes();
    let mut offsets = Vec::with_capacity(targets.len());
    let mut pending = targets.iter().peekable();
    let (mut line, mut column, mut index) = (0_u32, 1_u32, 0);
    while let Some(&byte) = bytes.get(index) {
        while pending.next_if(|at| **at <= (line, column)).is_some() {
            offsets.push(index);
        }
        if pending.peek().is_none() {
            break;
        }
        index += 1;
        match byte {
            b'\n' | b'\x0C' => (line, column) = (line.saturating_add(1), 1),
            b'\r' => {
                if bytes.get(index) == Some(&b'\n') {
                    index += 1;
                }
                (line, column) = (line.saturating_add(1), 1);
            }
            // Continuation bytes add nothing; a 4-byte character is two
            // UTF-16 code units.
            0x80..=0xBF => {}
            0xF0..=0xFF => column = column.saturating_add(2),
            _ => column = column.saturating_add(1),
        }
    }
    offsets.resize(targets.len(), bytes.len());
    offsets
}

/// Where the prelude of the rule starting at `start` ends: the offset of
/// its `{` or `;`, or the end of input.
fn prelude_end(css: &str, start: usize) -> usize {
    let mut tokens = Tokenizer::new(css);
    tokens.pos = start;
    let mut open: Vec<u8> = Vec::new();
    loop {
        let at = tokens.pos;
        match tokens.next_token() {
            None => return css.len(),
            Some(Token::Open(b'}') | Token::Semicolon) if open.is_empty() => return at,
            Some(Token::Open(close)) => open.push(close),
            Some(Token::Close(close)) if open.last() == Some(&close) => {
                open.pop();
            }
            Some(_) => {}
        }
    }
}

/// Whether `css` ends with nothing but whitespace, comments and stray
/// closing brackets after its last complete rule or declaration.
fn tail_is_junk(css: &str) -> bool {
    let mut tokens = Tokenizer::new(css);
    let mut open: Vec<u8> = Vec::new();
    let mut junk = true;
    while let Some(token) = tokens.next_token() {
        match token {
            Token::Open(close) => {
                open.push(close);
                junk = false;
            }
            Token::Close(close) if open.last() == Some(&close) => {
                open.pop();
                junk |= close == b'}';
            }
            Token::Semicolon if !matches!(open.last(), Some(b')' | b']')) => junk = true,
            Token::Close(_) | Token::Trivia => {}
            Token::Semicolon | Token::Other => junk = false,
        }
    }
    junk
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
    let sheet = parse_sheet(css.strip_prefix('\u{feff}').unwrap_or(css), None)?;
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

/// Works around a Lightning CSS misreading: it parses the legacy
/// pseudo-classes `:-moz-placeholder` (Firefox 4-18) and
/// `:-ms-input-placeholder` (IE 10-11) as prefixed `:placeholder-shown` and
/// would print `:-moz-placeholder-shown` / `:-ms-placeholder-shown`, which no
/// browser supports. Those prefixed forms can only come from these two
/// spellings, so they are turned back into exactly what was written.
///
/// [`mask_legacy_names`] already hides the plain spellings from the
/// parser; this catches escaped ones such as `:-moz-place\68 older`, in
/// every position it can reach. (It cannot reach `:nth-child(… of …)`:
/// Lightning CSS offers no way to change those selectors.)
fn repair_legacy_pseudo_classes<R>(rules: &mut CssRuleList<'_, R>) {
    for rule in &mut rules.0 {
        if let CssRule::Scope(scope) = rule {
            for list in [&mut scope.scope_start, &mut scope.scope_end]
                .into_iter()
                .flatten()
            {
                list.0.iter_mut().for_each(repair_selector);
            }
        }
        if let Some(group) = rules::group_rules_mut(rule) {
            repair_legacy_pseudo_classes(group);
            continue;
        }
        let style = match rule {
            CssRule::Style(style) => style,
            CssRule::Nesting(nesting) => &mut nesting.style,
            CssRule::LayerBlock(layer) => {
                repair_legacy_pseudo_classes(&mut layer.rules);
                continue;
            }
            _ => continue,
        };
        for selector in &mut style.selectors.0 {
            repair_selector(selector);
        }
        repair_legacy_pseudo_classes(&mut style.rules);
    }
}

fn repair_selector(selector: &mut Selector<'_>) {
    for component in selector.iter_mut_raw_match_order() {
        let legacy_name = match component {
            Component::NonTSPseudoClass(PseudoClass::PlaceholderShown(prefix)) => {
                if *prefix == VendorPrefix::Moz {
                    Some("-moz-placeholder")
                } else if *prefix == VendorPrefix::Ms {
                    Some("-ms-input-placeholder")
                } else {
                    None
                }
            }
            Component::Is(list)
            | Component::Where(list)
            | Component::Any(_, list)
            | Component::Negation(list)
            | Component::Has(list) => {
                list.iter_mut().for_each(repair_selector);
                None
            }
            Component::Slotted(inner) | Component::Host(Some(inner)) => {
                repair_selector(inner);
                None
            }
            _ => None,
        };
        if let Some(name) = legacy_name {
            *component = Component::NonTSPseudoClass(PseudoClass::Custom { name: name.into() });
        }
    }
}

/// Decides whether a class or id name keeps a selector alive.
struct Matcher<'a, U> {
    used: &'a U,
    safelist: &'a Safelist,
    /// Kept selectors that require a class or id, for [`PruneOutput`].
    kept_named: Cell<usize>,
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
    let mut keep: Vec<bool> = style
        .selectors
        .0
        .iter()
        .map(|selector| {
            mentions(selector, &safelist.greedy)
                || mentions(selector, &safelist.deep)
                || selector_can_match(selector, matcher)
        })
        .collect();
    let keeps_some = keep.iter().any(|kept| *kept);
    // Prune the nested rules first: whether the parent list's specificity
    // must be preserved depends on whether any nested rule survives.
    if keeps_some && !keeps_children {
        prune_rules(&mut style.rules, matcher, removed);
    }
    if keeps_some && !style.rules.0.is_empty() {
        keep_nesting_specificity(&style.selectors.0, &mut keep);
    }
    let mut decisions = keep.into_iter();
    style
        .selectors
        .0
        .retain(|_| decisions.next().unwrap_or(true));
    *removed += before - style.selectors.0.len();
    let named = style
        .selectors
        .0
        .iter()
        .filter(|selector| names_something(selector))
        .count();
    matcher.kept_named.set(matcher.kept_named.get() + named);

    if style.selectors.0.is_empty() {
        // Nested rules are relative to this one, so they go with it.
        *removed += count_selectors(&style.rules);
        return false;
    }
    true
}

/// In nested rules `&` matches like `:is(parent list)`, so its specificity
/// is that of the most specific selector in the list. If pruning would drop
/// that selector while keeping others, nested rules would silently lose to
/// rules they used to beat. Keep the most specific one in that case.
fn keep_nesting_specificity(selectors: &[Selector<'_>], keep: &mut [bool]) {
    let kept_max = selectors
        .iter()
        .zip(keep.iter())
        .filter(|(_, kept)| **kept)
        .map(|(selector, _)| selector.specificity())
        .max();
    let Some(kept_max) = kept_max else {
        // Nothing kept: the whole rule and its children go.
        return;
    };
    let most_specific_removed = selectors
        .iter()
        .enumerate()
        .filter(|(index, selector)| !keep[*index] && selector.specificity() > kept_max)
        .max_by_key(|(_, selector)| selector.specificity());
    if let Some((index, _)) = most_specific_removed {
        keep[index] = true;
    }
}

/// Whether the selector itself requires a class or id (not counting names
/// inside `:not()` and similar).
fn names_something(selector: &Selector<'_>) -> bool {
    selector
        .iter_raw_match_order()
        .any(|component| matches!(component, Component::Class(_) | Component::ID(_)))
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

/// Every selector in `rules`, at any depth, for [`PruneOutput`]'s count
/// when a parent rule takes them with it.
fn count_selectors<R>(rules: &CssRuleList<'_, R>) -> usize {
    rules
        .0
        .iter()
        .map(|rule| {
            if let Some(group) = rules::group_rules(rule) {
                return count_selectors(group);
            }
            match rule {
                CssRule::Style(style) => style.selectors.0.len() + count_selectors(&style.rules),
                CssRule::Nesting(nesting) => {
                    nesting.style.selectors.0.len() + count_selectors(&nesting.style.rules)
                }
                CssRule::LayerBlock(layer) => count_selectors(&layer.rules),
                _ => 0,
            }
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
        assert_ne!(prune_min(css, &[]), "");
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
        let css = ".b{color:red;&:hover{color:red}& .c,& .d{color:red}@media print{color:red;.e{color:red}}}";
        let removed = prune(css, &used, &PruneOptions::default())
            .map(|output| output.removed_selectors)
            .ok();
        assert_eq!(removed, Some(5));
    }

    // Third review: `--stats` missed selectors inside at-rules nested in a
    // removed rule.
    #[rstest]
    #[case::media(".b{color:red;@media print{.c,.d{color:red}}}", 3)]
    #[case::supports(".b{color:red;@supports (color:red){.c{color:red}}}", 2)]
    #[case::media_in_nested_rule(".b{& .c{@media print{.d{color:red}}}}", 3)]
    #[case::layer(".b{@layer x{.c{color:red}}}", 2)]
    fn prune_should_count_selectors_in_at_rules_removed_with_their_parent(
        #[case] css: &str,
        #[case] expected: usize,
    ) {
        let used = BTreeSet::from(["a"]);
        let removed = prune(css, &used, &PruneOptions::default())
            .map(|output| output.removed_selectors)
            .ok();
        assert_eq!(removed, Some(expected));
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
    fn prune_should_count_kept_selectors_that_name_a_class_or_id() {
        let used = BTreeSet::from(["a"]);
        let css = ".a,p,.b{color:red}#x .a,[hidden]{color:red}";
        let kept = prune(css, &used, &PruneOptions::default())
            .map(|output| output.kept_named_selectors)
            .ok();
        assert_eq!(kept, Some(1));
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

    // Found in the pre-release review: readers outside style rules.
    #[rstest]
    #[case::container_style_query(
        ":root{--theme:dark}@container style(--theme: dark){.x{color:red}}"
    )]
    #[case::page_rule(":root{--m:1cm}@page{margin:var(--m)}")]
    #[case::position_try(":root{--w:10px}@position-try --p{width:var(--w)}")]
    #[case::unknown_at_rule(":root{--gap:1rem}@custom-thing{gap:var(--gap)}")]
    fn variables_should_count_readers_outside_style_rules(#[case] css: &str) {
        let options = PruneOptions::default().variables(true);
        assert_eq!(prune_with(css, &["x"], &options).removed_variables, 0);
    }

    #[rstest]
    #[case::quoted_name_with_space(
        r#"@keyframes "my anim"{to{opacity:0}}.x{animation-name:"my anim"}"#
    )]
    #[case::escaped_digit(r"@keyframes \31spin{to{opacity:0}}.x{animation-name:\31spin}")]
    fn keyframes_should_match_escaped_and_quoted_names(#[case] css: &str) {
        let options = PruneOptions::default().keyframes(true);
        assert_eq!(prune_with(css, &["x"], &options).removed_keyframes, 0);
    }

    #[rstest]
    #[case::page_rule("@font-face{font-family:Inter;src:url(i.woff2)}@page{font-family:Inter}")]
    #[case::font_feature_values(
        "@font-face{font-family:Icons;src:url(i.woff2)}@font-feature-values Icons{@swash{fancy:1}}"
    )]
    fn font_face_should_count_references_outside_style_rules(#[case] css: &str) {
        let options = PruneOptions::default().font_face(true);
        assert_eq!(prune_with(css, &[], &options).removed_font_faces, 0);
    }

    #[test]
    fn font_face_should_match_content_case_insensitively() {
        let options = PruneOptions::default().font_face(true);
        let css = "@font-face{font-family:MyFont;src:url(m.woff2)}";
        assert_eq!(prune_with(css, &["myfont"], &options).removed_font_faces, 0);
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

    // Lightning CSS reads the legacy pseudo-classes `:-moz-placeholder`
    // (Firefox 4-18) and `:-ms-input-placeholder` (IE 10-11) as prefixed
    // `:placeholder-shown` and would print `:-moz-placeholder-shown`, which
    // no browser supports. Found by the benchmark correctness diff on Bulma.
    #[rstest]
    #[case::moz("input:-moz-placeholder{color:red}")]
    #[case::ms("input:-ms-input-placeholder{color:red}")]
    #[case::inside_is(":is(input:-moz-placeholder,textarea){color:red}")]
    #[case::nested("input{color:red;&:-ms-input-placeholder{color:red}}")]
    #[case::in_media("@media print{input:-moz-placeholder{color:red}}")]
    #[case::pseudo_element_unchanged("input::-moz-placeholder{color:red}")]
    // Second review: Lightning CSS reads this as `::-moz-placeholder`.
    #[case::ms_pseudo_element("input::-ms-input-placeholder{color:red}")]
    #[case::standard_unchanged("input:placeholder-shown{color:red}")]
    // Third review: positions the repair did not reach.
    #[case::nth_child_of("input:nth-child(2 of input:-moz-placeholder){color:red}")]
    #[case::nth_last_child_of("input:nth-last-child(2 of :-ms-input-placeholder){color:red}")]
    #[case::upper_case("input:-MOZ-PLACEHOLDER{color:red}")]
    #[case::followed_by_hover("input:-moz-placeholder:hover{color:red}")]
    fn prune_should_print_legacy_placeholder_selectors_as_written(#[case] css: &str) {
        assert_eq!(prune_min(css, &[]), css.to_ascii_lowercase());
    }

    #[rstest]
    #[case::scope_start(
        "@scope (input:-moz-placeholder){.a{color:red}}",
        "@scope(input:-moz-placeholder){.a{color:red}}"
    )]
    #[case::scope_end(
        "@scope (.a) to (input:-ms-input-placeholder){.a{color:red}}",
        "@scope(.a) to (input:-ms-input-placeholder){.a{color:red}}"
    )]
    fn prune_should_print_legacy_placeholders_in_scope_preludes_as_written(
        #[case] css: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, &["a"]), expected);
    }

    // Escaped spellings are not masked; the selector repair still fixes
    // them wherever it can reach.
    #[rstest]
    #[case::selector(
        r"input:-moz-place\68 older{color:red}",
        "input:-moz-placeholder{color:red}"
    )]
    #[case::scope(
        r"@scope (input:-moz-place\68 older){.a{color:red}}",
        "@scope(input:-moz-placeholder){.a{color:red}}"
    )]
    fn prune_should_repair_escaped_legacy_placeholders(#[case] css: &str, #[case] expected: &str) {
        assert_eq!(prune_min(css, &["a"]), expected);
    }

    // Third review: pseudo-element names ignore ASCII case, and the hiding
    // place must not rewrite text the stylesheet already contains.
    #[rstest]
    #[case::upper_case(
        "input::-MS-INPUT-PLACEHOLDER{color:red}",
        "input::-ms-input-placeholder{color:red}"
    )]
    #[case::mixed_case(
        "input::-Ms-Input-Placeholder{color:red}",
        "input::-ms-input-placeholder{color:red}"
    )]
    #[case::old_mask_written_literally(
        "input::-prunecss-ms-input-placeholder{color:red}input::-ms-input-placeholder{color:red}",
        "input::-prunecss-ms-input-placeholder{color:red}input::-ms-input-placeholder{color:red}"
    )]
    #[case::new_mask_written_literally(
        "input::-pcss-ms-000000000000{color:red}input::-pcss-ms-000000000001{color:red}input::-ms-input-placeholder{color:red}",
        "input::-pcss-ms-000000000000{color:red}input::-pcss-ms-000000000001{color:red}input::-ms-input-placeholder{color:red}"
    )]
    fn prune_should_print_the_ms_placeholder_pseudo_element_faithfully(
        #[case] css: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, &[]), expected);
    }

    #[test]
    fn unused_mask_should_skip_masks_the_text_contains_in_any_case() {
        let css = "-pcss-ms-000000000000 -PCSS-MS-000000000001";
        assert_eq!(
            unused_mask(css, "-pcss-ms-", 21).as_deref(),
            Some("-pcss-ms-000000000002")
        );
    }

    #[test]
    fn prune_should_report_warning_columns_of_the_original_text() {
        let column = |css: &str| {
            prune(css, &BTreeSet::from(["a", "b"]), &PruneOptions::default())
                .ok()
                .and_then(|output| output.warnings.first().and_then(|w| w.location))
                .map(|location| location.column)
        };
        // `::-xx-input-placeholder` has the same length and is kept as is.
        assert_eq!(
            column(".a::-ms-input-placeholder{} .a >>> .b{}"),
            column(".a::-xx-input-placeholder{} .a >>> .b{}")
        );
    }

    // Pre-release review: reprinting must not modernise values that older
    // browsers can't read (8-digit hex breaks IE11, old Safari and Edge).
    #[rstest]
    #[case::rgba(".a{color:rgba(0,0,0,.5)}", ".a{color:rgba(0,0,0,.5)}")]
    #[case::hsla(".a{color:hsla(0,0%,100%,.3)}", ".a{color:rgba(255,255,255,.3)}")]
    #[case::transparent(".a{border-color:transparent}", ".a{border-color:transparent}")]
    #[case::nesting_kept(".a{&:hover{color:red}}", ".a{&:hover{color:red}}")]
    fn prune_should_print_values_older_browsers_understand(
        #[case] css: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(prune_min(css, &["a"]), expected);
    }

    // Escapes such as `\201C` are printed as the raw character, so the
    // output must declare its encoding or a non-UTF-8 page garbles it.
    #[rstest]
    #[case::non_ascii_output(r#".a:before{content:"\201C"}"#)]
    #[case::input_charset(r#"@charset "utf-8";.a:before{content:"x"}"#)]
    fn prune_should_declare_utf8_when_needed(#[case] css: &str) {
        assert!(prune_min(css, &["a"]).starts_with(r#"@charset "UTF-8";"#));
    }

    // Second review: a byte-order mark (Sass adds one) must not end up in
    // the first selector.
    #[rstest]
    #[case::bom_only("\u{feff}.a{color:red}", ".a{color:red}")]
    #[case::bom_and_charset(
        "\u{feff}@charset \"UTF-8\";.a{color:red}",
        "@charset \"UTF-8\";.a{color:red}"
    )]
    fn prune_should_ignore_a_leading_byte_order_mark(#[case] css: &str, #[case] expected: &str) {
        assert_eq!(prune_min(css, &["a"]), expected);
    }

    #[test]
    fn prune_should_not_add_a_charset_to_ascii_output() {
        assert_eq!(prune_min(".a{color:red}", &["a"]), ".a{color:red}");
    }

    // Pre-release review: rules the parser rejects are dropped. That must
    // be reported, never silent.
    #[rstest]
    #[case::shadow_piercing_combinator(".x >>> .y{color:red}")]
    #[case::deep_combinator(".x /deep/ .y{color:red}")]
    #[case::two_pseudo_elements(".x::before::after{color:red}")]
    fn prune_should_warn_about_rules_it_could_not_parse(#[case] css: &str) {
        let used = BTreeSet::from(["x", "y"]);
        let output = prune(css, &used, &PruneOptions::default());
        let warnings = output.map(|output| output.warnings).unwrap_or_default();
        assert_eq!(
            warnings.first().and_then(|w| w.location).map(|l| l.line),
            Some(1),
            "{warnings:?}"
        );
    }

    // Second review: Lightning CSS also warns about rules it keeps as
    // written; those are not dropped and must not look like it.
    #[rstest]
    #[case::unknown_at_rule("@tailwind base;.x{color:red}")]
    #[case::unknown_pseudo_class(".x:unknown-pseudo{color:red}")]
    #[case::unknown_pseudo_inside_is(":is(:foo,.x){color:red}")]
    #[case::stray_closing_brace(".x{color:red}}")]
    fn prune_should_not_report_kept_rules_as_dropped(#[case] css: &str) {
        let used = BTreeSet::from(["x"]);
        let output = prune(css, &used, &PruneOptions::default());
        let dropped = output.map(|output| output.warnings.iter().filter(|w| w.dropped).count());
        assert_eq!(dropped.ok(), Some(0));
    }

    fn dropped_flags(css: &str) -> Vec<bool> {
        let used = BTreeSet::from(["a", "x", "y"]);
        match prune(css, &used, &PruneOptions::default()) {
            Ok(output) => output.warnings.iter().map(|w| w.dropped).collect(),
            Err(err) => panic!("{err}"),
        }
    }

    // Third review: `dropped` must say whether the rule disappeared, not
    // guess from the error kind. Lightning CSS reports an empty at-rule
    // prelude as "unexpected end of input" and drops the rule; it reports
    // an invalid media feature as "invalid media query" and keeps it.
    #[rstest]
    #[case::supports_without_condition("@supports{.a{color:red}}.a{color:blue}")]
    #[case::keyframes_without_name("@keyframes{}.a{color:red}")]
    #[case::counter_style_without_name("@counter-style{}.a{color:red}")]
    #[case::font_feature_values_without_family("@font-feature-values{}.a{color:red}")]
    #[case::font_palette_values_without_name("@font-palette-values{}.a{color:red}")]
    #[case::moz_document_without_condition("@-moz-document{.a{color:red}}")]
    #[case::property_without_name("@property{}.a{color:red}")]
    #[case::namespace_without_url("@namespace;.a{color:red}")]
    #[case::nested_rule_in_media("@media print{.x >>> .y{color:red}}")]
    #[case::nested_rule_in_style_rule(".a{color:red;.x >>> .y{color:red}}")]
    #[case::invalid_declaration(".a{color:red; !!!; background:blue}")]
    #[case::rule_after_stray_brace(".a{}}.x{color:red}")]
    fn prune_should_report_rules_the_parser_dropped_as_dropped(#[case] css: &str) {
        assert_eq!(dropped_flags(css).first(), Some(&true));
    }

    #[rstest]
    #[case::media_feature_without_value("@media screen and (min-width:){.a{color:red}}")]
    #[case::container_feature_without_value("@container (min-width:){.a{color:red}}")]
    #[case::media_feature_with_two_values("@media (min-width: foo bar){.a{color:red}}")]
    #[case::container_without_condition("@container{.a{color:red}}")]
    #[case::deprecated_nest(".a{color:red;@nest .x &{color:red}}")]
    #[case::unknown_pseudo_class_in_scope("@scope (.a:foo){.a{color:red}}")]
    #[case::multi_line("\n@media screen\r\n  and (min-width:)\x0c{.a{color:red}}")]
    #[case::after_wide_characters("/* 😀é */ @media (min-width:){.a{color:red}}")]
    fn prune_should_report_rules_the_parser_kept_as_kept(#[case] css: &str) {
        let flags = dropped_flags(css);
        assert_eq!(
            (flags.is_empty(), flags.contains(&true)),
            (false, false),
            "{css:?}"
        );
    }

    #[test]
    fn prune_should_report_dropped_rules_as_dropped() {
        let used = BTreeSet::from(["x", "y"]);
        let output = prune(".x >>> .y{color:red}", &used, &PruneOptions::default());
        let dropped = output.map(|output| output.warnings.iter().filter(|w| w.dropped).count());
        assert_eq!(dropped.ok(), Some(1));
    }

    #[test]
    fn prune_should_not_warn_about_valid_css() {
        let used = BTreeSet::from(["a"]);
        let output = prune(".a{color:red}", &used, &PruneOptions::default());
        assert_eq!(output.map(|output| output.warnings.len()).ok(), Some(0));
    }

    // Pre-release review: `&` in a nested rule matches like `:is(list)`, so
    // its specificity is that of the parent list's most specific selector.
    // Removing that one would make `& .c` lose to rules it used to beat.
    #[test]
    fn prune_should_keep_the_most_specific_parent_selector_of_nested_rules() {
        let css = ".a,#gone{& .c{color:red}}";
        assert_eq!(prune_min(css, &["a", "c"]), css);
    }

    // Found by fuzzing: if every nested rule goes, nothing needs the extra
    // specificity, and keeping the selector anyway made pruning unstable.
    #[test]
    fn prune_should_not_keep_a_specific_parent_when_its_nested_rules_go() {
        let css = ".a,#gone{color:red;& .unused{color:red}}";
        assert_eq!(prune_min(css, &["a"]), ".a{color:red}");
    }

    #[test]
    fn prune_should_remove_less_specific_parent_selectors_of_nested_rules() {
        let css = "#a,.gone{& .c{color:red}}";
        assert_eq!(prune_min(css, &["a", "c"]), "#a{& .c{color:red}}");
    }

    #[test]
    fn prune_should_still_remove_parents_without_nested_rules() {
        assert_eq!(prune_min(".a,#gone{color:red}", &["a"]), ".a{color:red}");
    }

    // Pre-release review: deep nesting overflowed the stack and aborted.
    #[rstest]
    #[case::functional_pseudo_classes(format!("{}.a{}{{color:red}}", ":is(".repeat(3000), ")".repeat(3000)))]
    #[case::grouping_rules(format!("{}.a{{color:red}}{}", "@media print{".repeat(10_000), "}".repeat(10_000)))]
    #[case::nested_rules(format!("{}color:red{}", ".a{".repeat(10_000), "}".repeat(10_000)))]
    fn prune_should_refuse_absurdly_deep_nesting(#[case] css: String) {
        let used = BTreeSet::from(["a"]);
        let err = prune(&css, &used, &PruneOptions::default()).err();
        assert!(err.is_some_and(|err| err.message().contains("nested more than")));
    }

    // Second review: an unquoted url() is one token, even with quotes or
    // parentheses inside it, so it can neither hide nesting nor add to it.
    #[test]
    fn prune_should_count_nesting_after_an_unquoted_url_with_a_quote() {
        let css = format!(".a{{b:url(x\")}}{}", ".a{".repeat(5000));
        let err = prune(&css, &BTreeSet::from(["a"]), &PruneOptions::default()).err();
        assert!(err.is_some_and(|err| err.message().contains("nested more than")));
    }

    #[test]
    fn prune_should_not_count_parentheses_inside_unquoted_urls() {
        let css = ".a{b:url(a(b)}".repeat(150);
        assert!(prune(&css, &BTreeSet::from(["a"]), &PruneOptions::default()).is_ok());
    }

    /// Runs `prune` on a thread with a modest stack, so a stack overflow
    /// shows up as this test binary aborting rather than passing by luck on
    /// a big main-thread stack.
    fn prune_on_small_stack(css: String) -> Result<PruneOutput, PruneError> {
        prune_on_stack(css, 4 * 1024 * 1024, PruneOptions::default())
    }

    fn prune_on_stack(
        css: String,
        stack_size: usize,
        options: PruneOptions,
    ) -> Result<PruneOutput, PruneError> {
        std::thread::Builder::new()
            .stack_size(stack_size)
            .spawn(move || prune(&css, &BTreeSet::from(["a"]), &options))
            .unwrap_or_else(|err| panic!("could not spawn: {err}"))
            .join()
            .unwrap_or_else(|_| panic!("prune panicked"))
    }

    fn deep(prefix: &str, open: &str, close: &str) -> String {
        format!(
            "{prefix}{}color:red{}",
            open.repeat(20_000),
            close.repeat(20_000)
        )
    }

    // Third review: the pre-check must read strings, escapes, comments and
    // `url(` exactly as the CSS tokenizer does, or deep input slips past it
    // and overflows the stack.
    #[rstest]
    #[case::bad_string_ended_by_cr(deep("@x '\r;", ".a{", "}"))]
    #[case::bad_string_ended_by_form_feed(deep("@x '\x0c;", ".a{", "}"))]
    #[case::bad_string_ended_by_crlf(deep("@x \"\r\n;", ".a{", "}"))]
    #[case::bad_string_then_is(deep("@x '\r;", ":is(", ")"))]
    #[case::bad_string_then_media(deep("@x '\r;", "@media print{", "}"))]
    #[case::escaped_quote_in_string(deep("@x 'a\\'b' ;", ".a{", "}"))]
    #[case::escaped_newline_in_string(deep("@x 'a\\\r\nb' ;", ".a{", "}"))]
    #[case::hex_escape_eats_newline(deep("@x 'a\\41\n}}}}' ;", ".a{", "}"))]
    #[case::comment_with_quote(deep("/* ' */", ".a{", "}"))]
    #[case::quote_then_comment(deep("@x \"/*\" ;", ".a{", "}"))]
    #[case::identifier_ending_in_url(deep(".a{b:xurl(", "[", "]"))]
    #[case::hash_url(deep(".a{b:#url(", "[", "]"))]
    #[case::dimension_url(deep(".a{b:1url(", "[", "]"))]
    #[case::escaped_url_with_quote(deep(".a{b:u\\72l(x\")", ".a{", "}"))]
    #[case::url_with_escaped_paren(deep(".a{b:url(x\\))", ".a{", "}"))]
    #[case::mismatched_closers(deep(".a{b:", "f(]", ")"))]
    #[case::cdo_before_url(deep("<!--url(x)", ".a{", "}"))]
    fn prune_should_refuse_deep_nesting_however_it_is_disguised(#[case] css: String) {
        let err = prune_on_small_stack(css).err();
        assert_eq!(
            err.map(|err| err.message().contains("nested more than")),
            Some(true)
        );
    }

    #[rstest]
    #[case::brackets_in_strings(r#".a{content:"((([[[{{{"}"#.repeat(200))]
    #[case::escaped_quotes(r#".a{content:"\"(\""}"#.repeat(200))]
    #[case::brackets_in_comments(".a{color:red}/* ((( */".repeat(200))]
    #[case::unquoted_url_with_escaped_paren(r".a{b:url(x\)(()}".repeat(200))]
    fn prune_should_not_count_brackets_the_tokenizer_ignores(#[case] css: String) {
        assert_eq!(prune_on_small_stack(css).map(|_| ()), Ok(()));
    }

    /// A stylesheet whose deepest block is `depth` levels down (rounded
    /// down to what `kind` can express).
    fn nested(kind: &str, depth: usize) -> String {
        // (before, repeated opener, levels it opens, inner, closer, after,
        // levels the rest adds)
        let (before, open, per_open, inner, close, after, extra) = match kind {
            "rules" => ("", ".a{", 1, "color:red", "}", "", 0),
            "media" => ("", "@media print{", 1, ".a{color:red}", "}", "", 1),
            "supports" => ("", "@supports (color:red){", 1, ".a{color:red}", "}", "", 1),
            "container" => (
                "",
                "@container (width>1px){",
                1,
                ".a{color:red}",
                "}",
                "",
                1,
            ),
            "scope" => ("", "@scope (.a){", 1, ".a{color:red}", "}", "", 1),
            "media_in_rules" => ("", ".a{@media print{", 2, "color:red", "}}", "", 0),
            "is" => ("", ":is(", 1, ".a", ")", "{color:red}", 0),
            "has" => (".a", ":has(", 1, ".a", ")", "{color:red}", 0),
            "nth_of" => ("", ":nth-child(1 of ", 1, ".a", ")", "{color:red}", 0),
            "calc" => (".a{width:", "calc(", 1, "1px", ")", "}", 1),
            _ => panic!("unknown kind {kind}"),
        };
        let repeat = (depth - extra) / per_open;
        format!(
            "{before}{}{inner}{}{after}",
            open.repeat(repeat),
            close.repeat(repeat)
        )
    }

    // Third review: 100 levels of `@media` overflowed the 2 MiB stack that
    // `cargo test` and spawned threads get by default, in debug builds.
    // The allowed depth must be safe there, with every option on.
    #[rstest]
    fn prune_should_handle_the_deepest_allowed_nesting_on_a_2_mib_stack(
        #[values(
            "rules",
            "media",
            "supports",
            "container",
            "scope",
            "media_in_rules",
            "is",
            "has",
            "nth_of",
            "calc"
        )]
        kind: &str,
    ) {
        let css = nested(kind, MAX_NESTING);
        assert_eq!(
            (
                exceeds_nesting(&css, MAX_NESTING - 1),
                exceeds_nesting(&css, MAX_NESTING)
            ),
            (true, false)
        );
        let options = PruneOptions::default()
            .keyframes(true)
            .font_face(true)
            .variables(true);
        let output = prune_on_stack(css, 2 * 1024 * 1024, options);
        assert_eq!(output.map(|_| ()), Ok(()));
    }

    #[rstest]
    fn prune_should_refuse_one_level_more_than_allowed(
        #[values("rules", "media", "is", "media_in_rules")] kind: &str,
    ) {
        let css = nested(kind, MAX_NESTING + 2);
        let err = prune_on_stack(css, 2 * 1024 * 1024, PruneOptions::default()).err();
        assert_eq!(
            err.map(|err| err.message().contains("nested more than")),
            Some(true)
        );
    }

    #[test]
    fn prune_should_accept_ordinary_nesting_depth() {
        let css = format!(
            "{}.a{{color:red}}{}",
            "@media print{".repeat(20),
            "}".repeat(20)
        );
        assert!(prune_min(&css, &["a"]).contains(".a{color:red}"));
    }

    // Known Lightning CSS printer issues found by fuzzing (bug 8 in
    // fuzz/regressions/README.md): token lists in unparsed values are
    // printed without separators. These tests pin today's behaviour so a
    // Lightning CSS upgrade that changes it is noticed; when one fails
    // because the output is now correct, update the expectation and remove
    // the matching skip in fuzz/src/lib.rs.
    #[rstest]
    #[case::signed_number_after_number(".a{--x:1+2px}", ".a{--x:12px}")]
    #[case::comment_as_separator(".a{--y:a/**/b}", ".a{--y:ab}")]
    #[case::ident_then_number(".a{x:a.0rem}", ".a{x:a0rem}")]
    fn known_printer_issue_merges_adjacent_tokens(#[case] css: &str, #[case] printed: &str) {
        assert_eq!(prune_min(css, &["a"]), printed);
    }

    // The same values written the way real stylesheets write them survive.
    #[rstest]
    #[case::spaced_numbers(".a{--x:1 +2px}")]
    #[case::calc(".a{--x:calc(1px + 2px)}")]
    #[case::spaced_comment(".a{--y:a /* note */ b}")]
    fn ordinary_custom_property_values_are_preserved(#[case] css: &str) {
        let printed = prune_min(css, &["a"]);
        assert_eq!(
            printed.split_ascii_whitespace().count(),
            css.replace("/* note */ ", "")
                .split_ascii_whitespace()
                .count(),
            "{printed}"
        );
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

        /// Text the tokenizer treats specially, to disguise deep nesting.
        fn tricky_prefix() -> impl Strategy<Value = String> {
            let piece = prop::sample::select(vec![
                "'", "\"", "\r", "\n", "\x0c", "\r\n", "\\", "/*", "*/", "url(", "xurl(", "#url(",
                "u\\72l(", "1url(", "<!--", "-->", ")", "]", "}", "(", "[", "{", ";", "@x ",
                ":is(", " ", "a", "\\41", "é",
            ]);
            prop::collection::vec(piece, 0..8).prop_map(|pieces| pieces.concat())
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(64))]
            // Whatever precedes it, deep nesting is refused or harmless:
            // never a stack overflow.
            #[test]
            fn prune_never_overflows_the_stack(
                prefix in tricky_prefix(),
                nesting in prop::sample::select(vec![(".a{", "}"), (":is(", ")"), ("@media print{", "}"), ("[", "]")]),
            ) {
                let css = format!("{prefix}{}x{}", nesting.0.repeat(5_000), nesting.1.repeat(5_000));
                let _ = prune_on_small_stack(css);
            }
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

            // More used names can only keep more: every rule kept with a
            // smaller used set is also kept with a larger one.
            #[test]
            fn prune_is_monotonic_in_used_names(
                css in stylesheet(),
                smaller in used_names(),
                extra in used_names(),
            ) {
                let larger: Vec<&str> = smaller.iter().chain(&extra).copied().collect();
                let kept_small = prune_min(&css, &smaller);
                let kept_large = prune_min(&css, &larger);
                for rule in kept_small.split_inclusive('}') {
                    prop_assert!(kept_large.contains(rule), "{rule} lost when adding {extra:?}");
                }
            }

            // Pruning only removes: every output rule is an input rule.
            #[test]
            fn prune_never_invents_rules(css in stylesheet(), used in used_names()) {
                let everything = prune_min(&css, &["a", "b", "c"]);
                for rule in prune_min(&css, &used).split_inclusive('}') {
                    prop_assert!(everything.contains(rule), "{rule} not in input");
                }
            }

            // A standard safelist entry behaves exactly like the content using it.
            #[test]
            fn standard_safelist_equals_using_the_name(css in stylesheet(), used in used_names()) {
                let options = PruneOptions::default().safelist(Safelist::default().standard(Pattern::exact("a")));
                let with_a: Vec<&str> = used.iter().copied().chain(["a"]).collect();
                prop_assert_eq!(prune_with(&css, &used, &options).css, prune_min(&css, &with_a));
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
