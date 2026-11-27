//! The output re-parses, and pruning it again with every class and id name
//! it contains removes nothing (removal is stable).
//!
//! Input: `css 0xFF content`. Used names are `extract_classes(content)` plus
//! every whitespace-separated word of the content.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (css, content) = prunecss_fuzz::split_input(data);
    prunecss_fuzz::check_removal_is_stable(&css, &prunecss_fuzz::used_names(&content));
});
