#![doc = include_str!("../README.md")]
//!
//! # How it works
//!
//! The pipeline has three steps:
//! 1. **Scan** content files (HTML, JSX, …) for the class names they use,
//!    with [`extract_classes`].
//! 2. **Match** those names against the selectors in a stylesheet.
//! 3. **Prune** the rules whose selectors can never match.
//!
//! [`prune`] does steps 2 and 3.

mod pruner;
mod rules;
mod safelist;
mod scanner;
mod tracking;

pub use pruner::{
    ParseWarning, PruneError, PruneErrorKind, PruneOptions, PruneOutput, SourceLocation, UsedNames,
    defined_names, prune,
};
pub use safelist::{Pattern, PatternError, Safelist};
pub use scanner::extract_classes;
