//! cargo-fuzz target: parse-and-check.
//!
//! Parse arbitrary UTF-8 input and run the full `ry_checker::Checker` on it.
//! Seeds come from `crates/ry-checker/testdata/` and
//! `crates/ry-checker/testdata/vendor/`.
//!
//! Assertions:
//!   - No panic: neither the parser nor the checker may unwind.
//!   - No R1 violation: every diagnostic span has `start <= end`,
//!     both within the source length, and both on character boundaries.
#![no_main]

use libfuzzer_sys::fuzz_target;
use ry_checker::Checker;
use ry_core::RParser;

fuzz_target!(|data: &[u8]| {
    let src = String::from_utf8_lossy(data);

    let mut parser = match RParser::new() {
        Ok(p) => p,
        Err(_) => return,
    };

    let file = match parser.parse("fuzz.R", &src) {
        Ok(f) => f,
        Err(_) => return,
    };

    let mut checker = Checker::new("fuzz.R");
    checker.check(&file);
    let diagnostics = checker.take_diagnostics();

    // R1: every diagnostic span is valid.
    let len = src.len();
    for diag in &diagnostics {
        let span = diag.span;
        assert!(
            span.start <= span.end,
            "R1: reversed diagnostic span {:?} ({})",
            span,
            diag.code,
        );
        assert!(
            span.end <= len,
            "R1: diagnostic span {:?} ({}) exceeds {} source bytes",
            span,
            diag.code,
            len,
        );
        assert!(
            src.is_char_boundary(span.start),
            "R1: diagnostic start is not a UTF-8 boundary: {:?} ({})",
            span,
            diag.code,
        );
        assert!(
            src.is_char_boundary(span.end),
            "R1: diagnostic end is not a UTF-8 boundary: {:?} ({})",
            span,
            diag.code,
        );
    }
});
