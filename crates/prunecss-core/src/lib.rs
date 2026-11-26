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
mod scanner;

pub use pruner::{
    PruneError, PruneErrorKind, PruneOptions, PruneOutput, SourceLocation, UsedNames, prune,
};
pub use scanner::extract_classes;
