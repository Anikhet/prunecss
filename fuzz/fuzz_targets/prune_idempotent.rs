//! Pruning the output again with the same used names changes nothing.
//!
//! Input: `css 0xFF content`. Used names are `extract_classes(content)` plus
//! every whitespace-separated word of the content.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (css, content) = prunecss_fuzz::split_input(data);
    prunecss_fuzz::check_idempotent(&css, &prunecss_fuzz::used_names(&content));
});
