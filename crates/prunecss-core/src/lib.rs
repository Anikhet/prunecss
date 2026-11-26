//! Core engine for `prunecss`.
//!
//! The pipeline has three steps:
//! 1. **Scan** content files (HTML, JSX, …) for the class names they use.
//! 2. **Match** those names against the selectors in a stylesheet.
//! 3. **Prune** the rules whose selectors can never match.
//!
//! Only step 1 exists so far; see [`scanner`].

pub mod scanner;
