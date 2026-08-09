//! cargo-fuzz target: parse.
//!
//! Fuzz the R parser (`ry_core::RParser::parse`) with arbitrary UTF-8 input.
//! Seeds come from `crates/ry-checker/testdata/` and
//! `crates/ry-checker/testdata/vendor/`.
//!
//! Assertions:
//!   - No panic: the parser must return a `Result`, never unwind.
//!   - No R1 violation: every parse-error span has `start <= end`,
//!     both within the source length, and both on character boundaries.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ry_core::RParser;

fuzz_target!(|data: &[u8]| {
    // The parser takes &str; lossy-convert invalid UTF-8 so the fuzzer
    // still exercises edge cases in the byte stream without crashing on
    // the str boundary itself.
    let src = String::from_utf8_lossy(data);

    let mut parser = match RParser::new() {
        Ok(p) => p,
        Err(_) => return,
    };

    let file = match parser.parse("fuzz.R", &src) {
        Ok(f) => f,
        Err(_) => return,
    };

    // R1: every parse-error span is valid.
    let len = src.len();
    for span in &file.parse_errors {
        assert!(
            span.start <= span.end,
            "R1: reversed parse-error span {:?}",
            span,
        );
        assert!(
            span.end <= len,
            "R1: parse-error span {:?} exceeds {} source bytes",
            span,
            len,
        );
        assert!(
            src.is_char_boundary(span.start),
            "R1: parse-error start is not a UTF-8 boundary: {:?}",
            span,
        );
        assert!(
            src.is_char_boundary(span.end),
            "R1: parse-error end is not a UTF-8 boundary: {:?}",
            span,
        );
    }
});
