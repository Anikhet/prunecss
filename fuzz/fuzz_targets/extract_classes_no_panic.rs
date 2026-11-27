//! `extract_classes` never panics, and every name it returns is non-empty,
//! has no ASCII whitespace, and is either borrowed from the input or (only
//! when the input has an `&` or a `\`) an owned name found after decoding
//! HTML entities or JavaScript escapes.
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
        match &name {
            // Borrowed names must point into the input.
            std::borrow::Cow::Borrowed(borrowed) => {
                let range = borrowed.as_bytes().as_ptr_range();
                assert!(
                    input.start <= range.start && range.end <= input.end,
                    "class name {name:?} is not a slice of the input"
                );
            }
            // Owned names only come from decoding HTML entities (`&`) or
            // JavaScript escapes such as `\x2d` (`\`).
            std::borrow::Cow::Owned(_) => {
                assert!(
                    source.contains(['&', '\\']),
                    "owned name {name:?} without any entity or escape"
                );
            }
        }
    }
});
