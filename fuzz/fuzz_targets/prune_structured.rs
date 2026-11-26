//! Structure-aware: generate well-formed stylesheets from a small grammar
//! (see `prunecss_fuzz::grammar`) so the pruner's keep/remove logic runs on
//! nearly every input, then check every invariant the byte-level targets do.
#![no_main]

use std::collections::BTreeSet;

use libfuzzer_sys::fuzz_target;
use prunecss_fuzz::grammar::Case;

fuzz_target!(|case: Case| {
    let css = case.css();
    let used: BTreeSet<&str> = case.used.iter().map(|name| name.unescaped()).collect();
    prunecss_fuzz::check_no_panic(&css, &used);
    prunecss_fuzz::check_idempotent(&css, &used);
    prunecss_fuzz::check_removal_is_stable(&css, &used);
});
