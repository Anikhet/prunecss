//! Names that must never be removed, whatever the content says.

/// A name to match: either an exact name or a regular expression.
///
/// Names are compared unescaped and without their `.`, `#` or `--` prefix:
/// write `md:flex`, not `.md\:flex`.
///
/// ```
/// use prunecss_core::Pattern;
///
/// assert!(Pattern::exact("btn").matches("btn"));
/// assert!(Pattern::regex("^btn-")?.matches("btn-primary"));
/// # Ok::<(), prunecss_core::PatternError>(())
/// ```
///
/// How a pattern is stored is private, so the regex engine can change
/// without breaking callers.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Pattern(Repr);

#[derive(Debug, Clone)]
enum Repr {
    Exact(String),
    Regex(regex::Regex),
}

impl Pattern {
    /// A pattern matching exactly `name`.
    #[must_use]
    pub fn exact(name: impl Into<String>) -> Self {
        Self(Repr::Exact(name.into()))
    }

    /// A pattern matching names in which the regular expression `source`
    /// finds a match. Use `^` and `$` to anchor it.
    ///
    /// Matching runs in linear time, so a pattern from a config file cannot
    /// hang the tool.
    ///
    /// # Errors
    ///
    /// Returns [`PatternError`] if `source` is not a valid regular expression.
    pub fn regex(source: &str) -> Result<Self, PatternError> {
        regex::Regex::new(source)
            .map(|regex| Self(Repr::Regex(regex)))
            .map_err(|err| {
                PatternError::new(PatternErrorKind::InvalidRegex, source, err.to_string())
            })
    }

    /// Parses the string form used in config files and on the command line:
    /// `/source/` (optionally followed by flags `i`, `m`, `s`, `x`, `U`, as
    /// in `/^btn-/i`) is a regular expression, anything else an exact name.
    /// This is the same convention PurgeCSS uses for patterns in JSON.
    ///
    /// ```
    /// use prunecss_core::Pattern;
    ///
    /// assert!(Pattern::parse("/^btn-/")?.matches("btn-lg"));
    /// assert!(!Pattern::parse("btn")?.matches("btn-lg"));
    /// # Ok::<(), prunecss_core::PatternError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`PatternError`] if a `/…/` pattern is not a valid regular
    /// expression, is empty (`//` would match every name) or has an unknown
    /// flag. A pattern that looks like a regex is never quietly treated as an
    /// exact name.
    pub fn parse(text: &str) -> Result<Self, PatternError> {
        let Some((source, flags)) = text.strip_prefix('/').and_then(|rest| {
            rest.rfind('/')
                .map(|close| (&rest[..close], &rest[close + 1..]))
        }) else {
            return Ok(Self::exact(text));
        };
        let error =
            |kind: PatternErrorKind, message: String| PatternError::new(kind, text, message);
        if source.is_empty() {
            return Err(error(
                PatternErrorKind::EmptyRegex,
                "an empty regular expression would match every name".to_owned(),
            ));
        }
        let mut builder = regex::RegexBuilder::new(source);
        for flag in flags.chars() {
            match flag {
                'i' => builder.case_insensitive(true),
                'm' => builder.multi_line(true),
                's' => builder.dot_matches_new_line(true),
                'x' => builder.ignore_whitespace(true),
                'U' => builder.swap_greed(true),
                _ => {
                    return Err(error(
                        PatternErrorKind::UnknownFlag,
                        format!("unknown flag `{flag}`; use i, m, s, x or U"),
                    ));
                }
            };
        }
        builder
            .build()
            .map(|regex| Self(Repr::Regex(regex)))
            .map_err(|err| error(PatternErrorKind::InvalidRegex, err.to_string()))
    }

    /// Returns `true` if `name` matches this pattern.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        match &self.0 {
            Repr::Exact(exact) => exact == name,
            Repr::Regex(regex) => regex.is_match(name),
        }
    }
}

/// What was wrong with a pattern. See [`PatternError::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PatternErrorKind {
    /// The regular expression does not compile.
    InvalidRegex,
    /// `//`: an empty regular expression would match every name.
    EmptyRegex,
    /// A `/…/` pattern has a flag other than `i`, `m`, `s`, `x` or `U`.
    UnknownFlag,
}

/// An invalid pattern, from [`Pattern::regex`] or [`Pattern::parse`].
///
/// Patterns are single strings, not stylesheets, so there is no source
/// location: callers reading them from a file know where they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    kind: PatternErrorKind,
    source: String,
    message: String,
}

impl PatternError {
    fn new(kind: PatternErrorKind, source: &str, message: String) -> Self {
        Self {
            kind,
            source: source.to_owned(),
            message,
        }
    }

    /// What kind of problem this was.
    #[must_use]
    pub fn kind(&self) -> PatternErrorKind {
        self.kind
    }

    /// A description of the problem, without the pattern.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The pattern as it was given.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.source
    }
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid pattern `{}`: {}", self.source, self.message)
    }
}

impl std::error::Error for PatternError {}

/// Names to keep regardless of the content. Mirrors PurgeCSS's `safelist`.
///
/// ```
/// use prunecss_core::{Pattern, Safelist};
///
/// let safelist = Safelist::default()
///     .standard(Pattern::exact("is-open"))
///     .greedy(Pattern::regex("^swiper-")?);
/// # Ok::<(), prunecss_core::PatternError>(())
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Safelist {
    /// A matching class or id counts as used, exactly as if the content
    /// contained it. `.is-open .menu` still needs `menu` to be used.
    pub standard: Vec<Pattern>,
    /// A selector containing a matching class or id is kept, together with
    /// every rule nested inside it.
    pub deep: Vec<Pattern>,
    /// A selector containing a matching class or id is kept, even if its
    /// other classes are unused.
    pub greedy: Vec<Pattern>,
    /// Matching `@keyframes` names are never removed.
    pub keyframes: Vec<Pattern>,
    /// Matching custom properties (`--name`, written without the `--`) are
    /// never removed.
    pub variables: Vec<Pattern>,
}

impl Safelist {
    /// Adds a [`Safelist::standard`] pattern.
    #[must_use]
    pub fn standard(mut self, pattern: Pattern) -> Self {
        self.standard.push(pattern);
        self
    }

    /// Adds a [`Safelist::deep`] pattern.
    #[must_use]
    pub fn deep(mut self, pattern: Pattern) -> Self {
        self.deep.push(pattern);
        self
    }

    /// Adds a [`Safelist::greedy`] pattern.
    #[must_use]
    pub fn greedy(mut self, pattern: Pattern) -> Self {
        self.greedy.push(pattern);
        self
    }

    /// Adds a [`Safelist::keyframes`] pattern.
    #[must_use]
    pub fn keyframes(mut self, pattern: Pattern) -> Self {
        self.keyframes.push(pattern);
        self
    }

    /// Adds a [`Safelist::variables`] pattern.
    #[must_use]
    pub fn variables(mut self, pattern: Pattern) -> Self {
        self.variables.push(pattern);
        self
    }
}

/// Returns `true` if any pattern matches `name`.
pub(crate) fn any_matches(patterns: &[Pattern], name: &str) -> bool {
    patterns.iter().any(|pattern| pattern.matches(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::same("btn", true)]
    #[case::prefix_only("btn-primary", false)]
    #[case::case_sensitive("BTN", false)]
    fn exact_pattern_should_match_only_the_same_name(#[case] name: &str, #[case] expected: bool) {
        assert_eq!(Pattern::exact("btn").matches(name), expected);
    }

    #[rstest]
    #[case::anchored_prefix("^btn-", "btn-primary", true)]
    #[case::anchored_prefix_miss("^btn-", "my-btn-x", false)]
    #[case::unanchored("modal", "is-modal-open", true)]
    #[case::tailwind_variant("^md:", "md:flex", true)]
    fn regex_pattern_should_match_where_the_expression_finds_a_match(
        #[case] source: &str,
        #[case] name: &str,
        #[case] expected: bool,
    ) {
        let pattern = Pattern::regex(source).map(|p| p.matches(name)).ok();
        assert_eq!(pattern, Some(expected));
    }

    #[rstest]
    #[case::slashes_mean_regex("/^btn-/", "btn-lg", true)]
    #[case::plain_text_is_exact("btn", "btn-lg", false)]
    #[case::lone_slash_is_exact("/", "/", true)]
    #[case::tailwind_fraction_is_exact("w-1/2", "w-1/2", true)]
    fn parse_should_read_slash_delimited_regexes(
        #[case] text: &str,
        #[case] name: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(
            Pattern::parse(text).map(|p| p.matches(name)).ok(),
            Some(expected)
        );
    }

    #[rstest]
    #[case::case_insensitive_flag("/^btn-/i", "BTN-x", true)]
    #[case::flag_still_anchors("/^btn-/i", "x-btn", false)]
    #[case::inline_case_flag("/(?i)^btn-/", "Btn-x", true)]
    #[case::unicode_class(r"/^\p{L}+$/", "größe", true)]
    fn parse_should_support_regex_flags_and_unicode(
        #[case] text: &str,
        #[case] name: &str,
        #[case] expected: bool,
    ) {
        assert_eq!(
            Pattern::parse(text).map(|p| p.matches(name)).ok(),
            Some(expected)
        );
    }

    #[rstest]
    #[case::empty_regex("//")]
    #[case::unknown_flag("/^a/q")]
    fn parse_should_reject_patterns_that_look_like_broken_regexes(#[case] text: &str) {
        assert!(Pattern::parse(text).is_err());
    }

    #[test]
    fn parse_should_reject_invalid_regexes() {
        assert!(Pattern::parse("/(/").is_err());
    }

    #[test]
    fn regex_pattern_should_reject_invalid_expressions() {
        let err = Pattern::regex("(unclosed").err().map(|e| e.to_string());
        assert!(err.is_some_and(|e| e.starts_with("invalid pattern `(unclosed`:")));
    }

    #[rstest]
    #[case::invalid_regex("/(/", PatternErrorKind::InvalidRegex)]
    #[case::empty_regex("//", PatternErrorKind::EmptyRegex)]
    #[case::unknown_flag("/^a/q", PatternErrorKind::UnknownFlag)]
    fn parse_errors_should_say_what_kind_of_problem_it_was(
        #[case] text: &str,
        #[case] expected: PatternErrorKind,
    ) {
        assert_eq!(
            Pattern::parse(text).err().map(|err| err.kind()),
            Some(expected)
        );
    }

    #[test]
    fn regex_errors_should_be_invalid_regex() {
        let kind = Pattern::regex("(unclosed").err().map(|err| err.kind());
        assert_eq!(kind, Some(PatternErrorKind::InvalidRegex));
    }

    #[test]
    fn pattern_errors_should_keep_the_pattern_and_message_apart() {
        let err = Pattern::parse("/^a/q").err();
        let parts = err.as_ref().map(|err| (err.pattern(), err.message()));
        assert_eq!(
            parts,
            Some(("/^a/q", "unknown flag `q`; use i, m, s, x or U"))
        );
    }

    #[test]
    fn safelist_builders_should_collect_patterns_by_kind() {
        let safelist = Safelist::default()
            .standard(Pattern::exact("a"))
            .deep(Pattern::exact("b"))
            .greedy(Pattern::exact("c"))
            .keyframes(Pattern::exact("d"))
            .variables(Pattern::exact("e"));
        let counts = [
            safelist.standard.len(),
            safelist.deep.len(),
            safelist.greedy.len(),
            safelist.keyframes.len(),
            safelist.variables.len(),
        ];
        assert_eq!(counts, [1; 5]);
    }

    #[test]
    fn any_matches_should_be_false_for_no_patterns() {
        assert!(!any_matches(&[], "anything"));
    }
}
