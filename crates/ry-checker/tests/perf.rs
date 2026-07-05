//! Performance regression test (PLAN.md Phase 0.4).
//!
//! `#[ignore]`'d so CI is opt-in. Run with `cargo test -p ry-checker --test
//! perf -- --ignored --nocapture`. Generates a 20k-line file, parses +
//! checks it, and asserts wall time under 2 seconds (release-mode budget).
//! Today the parser is O(n^2) (`char_col` rescans from byte 0 per node,
//! PLAN finding 2); this test fails until Phase 1.1 lands.
//!
//! PLAN Phase 3.2 note: `Project::check` pass 3 is now rayon-parallel
//! (per-file emitters share the Arc tables read-only), and the CLI's
//! parse loop runs through a rayon thread-local parser pool. The 2s
//! budgets below are unchanged -- parallelism is a bonus for
//! multi-file/multi-core runs, not a license to regress single-file
//! latency.

use std::io::Write;
use std::time::Instant;

use ry_checker::Checker;
use ry_checker::Project;
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

/// PLAN Phase D1: a 100-file `Project` used to deep-clone the shared
/// `FnTable`/`ReturnSlots` once per file in pass 3. The tables are now
/// `Arc`-shared, so only the handle is cloned. This is a wall-clock
/// budget (not an allocation counter) and is `#[ignore]`'d like the
/// single-file perf test.
#[test]
#[ignore]
fn hundred_file_project_checks_quickly() {
    let mut parser = RParser::new().expect("parser init");
    let mut project = Project::new();
    for i in 0..100 {
        // Each file defines a function and calls one from another file,
        // so the shared FnTable is non-trivial and the fixpoint loop runs.
        let src =
            format!("f{i} <- function(x) x * {i}\ng{i} <- function(x) f{i}(x) + 1\nh <- f{i}(2)\n");
        let file = parser
            .parse(&format!("file{i}.R"), &src)
            .unwrap_or_else(|e| panic!("parse file{i}: {e}"));
        project.add_file(format!("file{i}.R"), file);
    }

    let start = Instant::now();
    let diags = project.check();
    let elapsed = start.elapsed();

    assert_eq!(diags.len(), 100, "one diagnostic-vec per file");
    assert!(
        elapsed.as_secs_f64() < 2.0,
        "100-file project check took {:.3}s (budget 2.0s)",
        elapsed.as_secs_f64()
    );
}
