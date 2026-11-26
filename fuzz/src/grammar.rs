//! A small CSS grammar for structure-aware fuzzing.
//!
//! Random bytes almost never form a stylesheet whose selectors share names
//! with the content, so most of the pruner's decisions go untested. Deriving
//! [`arbitrary::Arbitrary`] on this grammar makes every input a well-formed
//! stylesheet over a tiny alphabet of names, half of which are usually
//! marked used, so keep/remove decisions happen on almost every input.

use std::fmt::Write as _;

use arbitrary::Arbitrary;

/// Deeper nesting than this is rendered as nothing, so generation stays fast.
const MAX_DEPTH: usize = 4;

/// Class and id names: `(unescaped name, how it is written in CSS)`.
///
/// The escaped spellings are the Tailwind-style cases the pruner must
/// unescape before matching.
const NAMES: [(&str, &str); 10] = [
    ("a", "a"),
    ("b", "b"),
    ("c", "c"),
    ("card", "card"),
    ("md:flex", r"md\:flex"),
    ("w-1/2", r"w-1\/2"),
    ("10", r"\31 0"),
    ("w-[10px]", r"w-\[10px\]"),
    ("-x_y", "-x_y"),
    ("é", "é"),
];

/// One of [`NAMES`].
#[derive(Arbitrary, Debug, Clone, Copy)]
pub struct Name(u8);

impl Name {
    fn entry(self) -> (&'static str, &'static str) {
        NAMES[usize::from(self.0) % NAMES.len()]
    }

    /// The name as the content would contain it.
    #[must_use]
    pub fn unescaped(self) -> &'static str {
        self.entry().0
    }

    fn css(self) -> &'static str {
        self.entry().1
    }
}

/// A whole fuzz case: a stylesheet plus the names the content uses.
#[derive(Arbitrary, Debug)]
pub struct Case {
    /// Top-level rules.
    pub rules: Vec<Rule>,
    /// Names treated as used.
    pub used: Vec<Name>,
}

impl Case {
    /// Renders the stylesheet as CSS text.
    #[must_use]
    pub fn css(&self) -> String {
        let mut out = String::new();
        for rule in &self.rules {
            rule.render(&mut out, 0);
        }
        out
    }
}

/// A top-level or grouped rule.
#[derive(Arbitrary, Debug)]
pub enum Rule {
    /// `sel, sel { decls; nested }`
    Style(StyleRule),
    /// `@media … { rules }`
    Media(MediaQuery, Vec<Rule>),
    /// `@supports (display:grid) { rules }`
    Supports(Vec<Rule>),
    /// `@container (width > 1px) { rules }`
    Container(Vec<Rule>),
    /// `@layer [name] { rules }`
    Layer(Option<Name>, Vec<Rule>),
    /// `@layer a, b;`
    LayerStatement(Vec<Name>),
    /// `@scope (.x) { rules }`
    Scope(Compound, Vec<Rule>),
    /// `@starting-style { rules }`
    StartingStyle(Vec<Rule>),
    /// `@keyframes name { … }`, which the pruner must always keep.
    Keyframes(Name),
}

/// Media query preludes.
#[derive(Arbitrary, Debug)]
pub enum MediaQuery {
    /// `print`
    Print,
    /// `(min-width: 768px)`
    MinWidth,
    /// `screen and (max-width: 767.98px)`
    ScreenMaxWidth,
    /// `(width >= 600px)`, the range syntax the printer must not emit.
    Range,
}

/// A style rule, possibly with nested rules.
#[derive(Arbitrary, Debug)]
pub struct StyleRule {
    /// The selector list; an empty list renders as `*`.
    pub selectors: Vec<Complex>,
    /// Declarations.
    pub declarations: Vec<Declaration>,
    /// Nested rules, rendered with an `&` prefix where needed.
    pub nested: Vec<Nested>,
}

/// A rule nested inside a style rule.
#[derive(Arbitrary, Debug)]
pub enum Nested {
    /// `& <combinator> sel { … }` or `&:hover { … }`
    Style(Option<Combinator>, StyleRule),
    /// `@media print { decls }` directly inside a style rule.
    Media(Vec<Declaration>),
}

/// A complex selector: compounds joined by combinators.
#[derive(Arbitrary, Debug)]
pub struct Complex {
    /// The first compound selector.
    pub first: Compound,
    /// The rest, each with the combinator before it.
    pub rest: Vec<(Combinator, Compound)>,
}

/// Combinators.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub enum Combinator {
    /// ` `
    Descendant,
    /// `>`
    Child,
    /// `+`
    NextSibling,
    /// `~`
    SubsequentSibling,
}

/// A compound selector such as `a.btn:hover`.
#[derive(Arbitrary, Debug)]
pub struct Compound {
    /// An optional type selector.
    pub tag: Option<Tag>,
    /// The simple selectors after it; with no tag and no parts, `*`.
    pub parts: Vec<Simple>,
}

/// Type selectors.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub enum Tag {
    /// `div`
    Div,
    /// `a`
    A,
    /// `p`
    P,
}

/// Simple selectors and pseudo-classes.
#[derive(Arbitrary, Debug)]
pub enum Simple {
    /// `.name`
    Class(Name),
    /// `#name`
    Id(Name),
    /// `[hidden]`
    Attribute,
    /// `:hover`
    Hover,
    /// `:root`
    Root,
    /// `::before`
    Before,
    /// `:not(…)`
    Not(Vec<Compound>),
    /// `:is(…)`
    Is(Vec<Compound>),
    /// `:where(…)`
    Where(Vec<Compound>),
    /// `:has(…)`
    Has(Vec<Compound>),
    /// `:nth-child(2 of …)`
    NthOf(Compound),
    /// `&`
    Nesting,
}

/// Declarations.
#[derive(Arbitrary, Debug)]
pub enum Declaration {
    /// `color: red`
    Color,
    /// `margin: 0 auto`
    Margin,
    /// `--x: 1px`
    Custom,
    /// `animation: spin 1s`
    Animation,
    /// `color:` with no value, which the parser keeps as written.
    Invalid,
}

impl Rule {
    fn render(&self, out: &mut String, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match self {
            Self::Style(style) => style.render(out, depth),
            Self::Media(query, rules) => {
                out.push_str(match query {
                    MediaQuery::Print => "@media print",
                    MediaQuery::MinWidth => "@media (min-width: 768px)",
                    MediaQuery::ScreenMaxWidth => "@media screen and (max-width: 767.98px)",
                    MediaQuery::Range => "@media (width >= 600px)",
                });
                render_block(out, rules, depth);
            }
            Self::Supports(rules) => {
                out.push_str("@supports (display: grid)");
                render_block(out, rules, depth);
            }
            Self::Container(rules) => {
                out.push_str("@container (width > 1px)");
                render_block(out, rules, depth);
            }
            Self::Layer(name, rules) => {
                out.push_str("@layer");
                if let Some(name) = name {
                    let _ = write!(out, " l{}", name.0 % 4);
                }
                render_block(out, rules, depth);
            }
            Self::LayerStatement(names) => {
                let names: Vec<String> = names.iter().map(|n| format!("l{}", n.0 % 4)).collect();
                let names = if names.is_empty() {
                    "l0".to_owned()
                } else {
                    names.join(", ")
                };
                let _ = write!(out, "@layer {names};");
            }
            Self::Scope(start, rules) => {
                out.push_str("@scope (");
                start.render(out, depth);
                out.push(')');
                render_block(out, rules, depth);
            }
            Self::StartingStyle(rules) => {
                out.push_str("@starting-style");
                render_block(out, rules, depth);
            }
            Self::Keyframes(name) => {
                let _ = write!(
                    out,
                    "@keyframes k{} {{ from {{ opacity: 0 }} to {{ opacity: 1 }} }}",
                    name.0 % 4
                );
            }
        }
    }
}

fn render_block(out: &mut String, rules: &[Rule], depth: usize) {
    out.push_str(" {\n");
    for rule in rules {
        rule.render(out, depth + 1);
    }
    out.push_str("}\n");
}

impl StyleRule {
    fn render(&self, out: &mut String, depth: usize) {
        self.render_selectors(out, None, depth);
        self.render_body(out, depth);
    }

    fn render_selectors(&self, out: &mut String, nest: Option<Option<Combinator>>, depth: usize) {
        if self.selectors.is_empty() {
            if nest.is_some() {
                out.push('&');
            } else {
                out.push('*');
            }
        }
        for (i, selector) in self.selectors.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            if let Some(combinator) = nest {
                out.push('&');
                // With no combinator, `&` is directly followed by the
                // compound, e.g. `&:hover`.
                if let Some(combinator) = combinator {
                    combinator.render(out);
                }
            }
            selector.render(out, depth);
        }
    }

    fn render_body(&self, out: &mut String, depth: usize) {
        out.push_str(" {");
        for declaration in &self.declarations {
            declaration.render(out);
        }
        for nested in &self.nested {
            if depth >= MAX_DEPTH {
                break;
            }
            match nested {
                Nested::Style(combinator, style) => {
                    style.render_selectors(out, Some(*combinator), depth + 1);
                    style.render_body(out, depth + 1);
                }
                Nested::Media(declarations) => {
                    out.push_str("@media print {");
                    for declaration in declarations {
                        declaration.render(out);
                    }
                    out.push('}');
                }
            }
        }
        out.push_str("}\n");
    }
}

impl Complex {
    fn render(&self, out: &mut String, depth: usize) {
        self.first.render(out, depth);
        for (combinator, compound) in &self.rest {
            combinator.render(out);
            compound.render(out, depth);
        }
    }
}

impl Combinator {
    fn render(self, out: &mut String) {
        out.push_str(match self {
            Self::Descendant => " ",
            Self::Child => " > ",
            Self::NextSibling => " + ",
            Self::SubsequentSibling => " ~ ",
        });
    }
}

impl Compound {
    fn render(&self, out: &mut String, depth: usize) {
        match self.tag {
            Some(Tag::Div) => out.push_str("div"),
            Some(Tag::A) => out.push('a'),
            Some(Tag::P) => out.push('p'),
            None if self.parts.is_empty() => out.push('*'),
            None => {}
        }
        for part in &self.parts {
            part.render(out, depth);
        }
    }
}

impl Simple {
    fn render(&self, out: &mut String, depth: usize) {
        match self {
            Self::Class(name) => {
                out.push('.');
                out.push_str(name.css());
            }
            Self::Id(name) => {
                out.push('#');
                out.push_str(name.css());
            }
            Self::Attribute => out.push_str("[hidden]"),
            Self::Hover => out.push_str(":hover"),
            Self::Root => out.push_str(":root"),
            Self::Before => out.push_str("::before"),
            Self::Not(list) => render_pseudo(out, ":not", list, depth),
            Self::Is(list) => render_pseudo(out, ":is", list, depth),
            Self::Where(list) => render_pseudo(out, ":where", list, depth),
            Self::Has(list) => render_pseudo(out, ":has", list, depth),
            Self::NthOf(compound) => {
                out.push_str(":nth-child(2 of ");
                if depth < MAX_DEPTH {
                    compound.render(out, depth + 1);
                } else {
                    out.push('*');
                }
                out.push(')');
            }
            Self::Nesting => out.push('&'),
        }
    }
}

fn render_pseudo(out: &mut String, name: &str, list: &[Compound], depth: usize) {
    out.push_str(name);
    out.push('(');
    if list.is_empty() || depth >= MAX_DEPTH {
        out.push('*');
    } else {
        for (i, compound) in list.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            compound.render(out, depth + 1);
        }
    }
    out.push(')');
}

impl Declaration {
    fn render(&self, out: &mut String) {
        out.push_str(match self {
            Self::Color => "color: red;",
            Self::Margin => "margin: 0 auto;",
            Self::Custom => "--x: 1px;",
            Self::Animation => "animation: k0 1s;",
            Self::Invalid => "color:;",
        });
    }
}
