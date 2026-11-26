//! `extract_classes` never panics, and every name it returns is a non-empty
//! slice of the input with no ASCII whitespace in it.
#![no_main]

use libfuzzer_sys::fuzz_target;
use prunecss_core::extract_classes;

fuzz_target!(|data: &[u8]| {
    let source = String::from_utf8_lossy(data);
    let input = source.as_bytes().as_ptr_range();
    for name in extract_classes(&source) {
        assert!(!name.is_empty(), "empty class name from {source:?}");
        assert!(
            !name.bytes().any(|b| b.is_ascii_whitespace()),
            "class name {name:?} contains whitespace"
        );
        // Names are borrowed from the input, so they must point into it.
        let range = name.as_bytes().as_ptr_range();
        assert!(
            input.start <= range.start && range.end <= input.end,
            "class name {name:?} is not a slice of the input"
        );
    }
});
