//! `prune` must never panic, whatever the stylesheet and content.
//!
//! Input: `css 0xFF content` (see `prunecss_fuzz::split_input`). The used
//! names come from `extract_classes(content)`, exactly as the CLI does it.
#![no_main]

use libfuzzer_sys::fuzz_target;
use prunecss_core::extract_classes;

fuzz_target!(|data: &[u8]| {
    let (css, content) = prunecss_fuzz::split_input(data);
    prunecss_fuzz::check_no_panic(&css, &extract_classes(&content));
});
