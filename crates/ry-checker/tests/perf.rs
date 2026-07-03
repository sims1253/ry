//! Performance regression test (PLAN.md Phase 0.4).
//!
//! `#[ignore]`'d so CI is opt-in. Run with `cargo test -p ry-checker --test
//! perf -- --ignored --nocapture`. Generates a 20k-line file, parses +
//! checks it, and asserts wall time under 2 seconds (release-mode budget).
//! Today the parser is O(n^2) (`char_col` rescans from byte 0 per node,
//! PLAN finding 2); this test fails until Phase 1.1 lands.

use std::io::Write;
use std::time::Instant;

use ry_checker::Checker;
use ry_core::RParser;

#[test]
#[ignore]
fn large_file_checks_under_two_seconds() {
    let lines: Vec<String> = (0..20_000)
        .map(|i| format!("x{i} <- c({i}, {}) * 2", i + 1))
        .collect();
    let src = lines.join("\n");

    let mut tmp_path = std::env::temp_dir();
    tmp_path.push(format!("ry_perf_{}.R", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp_path).expect("create temp file");
        f.write_all(src.as_bytes()).expect("write temp file");
    }

    let start = Instant::now();
    let mut parser = RParser::new().expect("parser init");
    let file = parser
        .parse("perf.R", &src)
        .unwrap_or_else(|e| panic!("parse: {e}"));
    let mut c = Checker::new("perf.R");
    c.check(&file);
    let _ = c.take_diagnostics();
    let elapsed = start.elapsed();

    let _ = std::fs::remove_file(&tmp_path);

    assert!(
        elapsed.as_secs_f64() < 2.0,
        "20k-line check took {:.3}s (budget 2.0s)",
        elapsed.as_secs_f64()
    );
}
