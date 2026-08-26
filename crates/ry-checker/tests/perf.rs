//! Performance regression tests.
//!
//! `#[ignore]`'d so CI is opt-in. Run with `cargo test -p ry-checker --test
//! perf -- --ignored --nocapture`. Generates a 20k-line file, parses +
//! checks it, and asserts wall time under 2 seconds (release-mode budget).
//! The budget guards the linear-time parsing contract (the parser was
//! once O(n^2): `char_col` rescanned from byte 0 per node).
//!
//! `Project::check` pass 3 is rayon-parallel
//! (per-file emitters share the Arc tables read-only), and the CLI's
//! parse loop runs through a rayon thread-local parser pool. The 2s
//! budgets below are unchanged -- parallelism is a bonus for
//! multi-file/multi-core runs, not a license to regress single-file
//! latency.

use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// A 100-file `Project` used to deep-clone the shared
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

/// Warm `check_incremental` after a one-line edit. Models the LSP
/// debounce path: prime a Project with a cold `check()`, then edit one
/// file and call `check_incremental()`. The budget catches regressions
/// in the incremental path that the cold-check tests cannot see.
///
/// The incremental path (`update_file` +
/// `check_incremental`) is the hot path the LSP exercises on every
/// keystroke. This test guards its wall-clock cost.
#[test]
#[ignore]
fn warm_edit_checks_quickly() {
    // Build a 100-file project (same as hundred_file_project_checks_quickly).
    let mut parser = RParser::new().expect("parser init");
    let mut project = Project::new();
    for i in 0..100 {
        let src =
            format!("f{i} <- function(x) x * {i}\ng{i} <- function(x) f{i}(x) + 1\nh <- f{i}(2)\n");
        let file = parser
            .parse(&format!("file{i}.R"), &src)
            .unwrap_or_else(|e| panic!("parse file{i}: {e}"));
        project.add_file(format!("file{i}.R"), file);
    }
    // Prime the incremental cache.
    let _ = project.check();

    // Edit one file and measure incremental check time.
    let edited = parser
        .parse(
            "file0.R",
            "f0 <- function(x) x * 999\ng0 <- function(x) f0(x) + 1\nh <- f0(2)\n",
        )
        .expect("reparse file0");
    project.update_file("file0.R".to_string(), Arc::new(edited));

    let start = Instant::now();
    let diags = project.check_incremental();
    let elapsed = start.elapsed();

    assert_eq!(diags.len(), 100, "one diagnostic-vec per file");
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "warm incremental check took {:.3}s (budget 1.0s)",
        elapsed.as_secs_f64()
    );
}

// ===========================================================================
// Complexity scaling rather than only wall-clock budgets
//
// The three budget tests above guard absolute latency. They are
// timing-sensitive: a single `< 4` ratio on a 2× size step sits exactly
// on the quadratic boundary and flips with CI noise. These scaling
// tests instead measure the **fitted log-log slope** across multiple
// geometric sizes. A linear algorithm has slope ≈ 1; quadratic ≈ 2;
// exponential ≫ 2. The slope is a *rate-of-growth* measurement, far
// less sensitive to absolute machine speed than a wall-clock budget.
//
// The acceptance oracle is historical falsification. Each of the three
// regression classes below was once a real defect; temporarily
// restoring any one makes its corresponding scaling test fail because
// the fitted slope blows past the sub-quadratic budget:
//
//   1. **Parser rescanning** (commit 89eddd2): `char_col` rescanned the
//      entire source from byte 0 for every AST node — O(n²) parsing.
//      Restoring it makes `scaling_file_length` report slope ≈ 2.
//
//   2. **Pipe exponential** (commit e8d7408): pipe desugaring re-inferred
//      the entire LHS at every stage — O(2ⁿ) inference. Restoring it
//      makes `scaling_pipe_length` report slope ≫ 2.
//
//   3. **Force-flow double walk** (commit e8d7408): `statement_force_flow`
//      walked each if-branch twice (once for force, once for
//      fall-through) — O(2ᵈ) collection. Restoring it makes
//      `scaling_branch_depth` report slope ≫ 2.
//
// All tests are `#[ignore]`'d like the budget tests; CI runs them via
// `cargo test -p ry-checker --test perf --release -- --ignored`.
// ===========================================================================

/// Fitted log-log slope above this value is treated as a complexity
/// regression. 1.5 is the midpoint between linear (1.0) and quadratic
/// (2.0). Every dimension in the fixed code is linear-time, so 1.5
/// leaves wide margin against timing jitter while still catching both
/// quadratic (slope → 2) and exponential (slope ≫ 2) blow-ups.
const SLOPE_BUDGET: f64 = 1.5;

/// For a 2× size step, the time ratio of a quadratic algorithm is 4.0.
/// This cross-check catches a localised cliff (e.g. the very largest
/// size spiking) that a global least-squares fit might smooth over.
/// 3.5 sits between linear (2.0) and quadratic (4.0).
const RATIO_BUDGET: f64 = 3.5;

/// Minimum number of geometric data points for a reliable slope fit.
/// Four points give three consecutive ratios.
const MIN_SIZES: usize = 4;

/// Measure the median per-call wall-clock time of `body`.
///
/// Fast operations (sub-millisecond) are **batched**: `body` runs in a
/// tight loop sized so each sample is ~5 ms, then the total is divided by
/// the batch count. Batching amortises timer-granularity and scheduling
/// jitter, giving a stable median even when a single call is only a few
/// tens of microseconds. Expensive operations (≥ 200 ms) are never
/// batched and get only 3 samples so the test cannot stall CI.
fn timed_median(mut body: impl FnMut()) -> Duration {
    // Warm up: caches, branch predictors, page faults.
    body();
    let probe = Instant::now();
    body();
    let single = probe.elapsed();
    if single >= Duration::from_millis(200) {
        // Expensive: three individual runs, median.
        let mut samples: Vec<Duration> = (0..3)
            .map(|_| {
                let start = Instant::now();
                body();
                start.elapsed()
            })
            .collect();
        samples.sort();
        return samples[1];
    }
    // Fast: batch to ~5 ms per sample so the timer has signal to work
    // with, then take the median of 11 batched samples.
    // Size the batch so each sample takes ~5 ms. Guard against a
    // zero-duration probe (instant return) by defaulting to 10 000.
    let batch = (5_000_000 / single.as_nanos().max(1) as u64).clamp(1, 10_000) as usize;
    let mut samples: Vec<Duration> = (0..11)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..batch {
                body();
            }
            start.elapsed() / batch as u32
        })
        .collect();
    samples.sort();
    samples[5]
}

/// Least-squares log-log slope: `time ∝ size^slope`.
///
/// Computed from `(size, time_seconds)` pairs. Uses **all** geometric
/// points (multi-ratio evidence), unlike a single consecutive ratio
/// known to be unreliable on the quadratic boundary.
fn log_log_slope(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    let (sx, sy, sxx, sxy) =
        points
            .iter()
            .fold((0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64), |acc, &(s, t)| {
                let lx = s.ln();
                let ly = t.max(1e-12).ln();
                (acc.0 + lx, acc.1 + ly, acc.2 + lx * lx, acc.3 + lx * ly)
            });
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-30 {
        f64::INFINITY
    } else {
        (n * sxy - sx * sy) / denom
    }
}

/// Assert that growth across `sizes` / `times` is comfortably
/// sub-quadratic. Reports the fitted slope, all data points, and each
/// consecutive ratio on failure so a regression is easy to diagnose.
fn assert_subquadratic_scaling(label: &str, sizes: &[usize], times: &[Duration]) {
    assert!(
        sizes.len() >= MIN_SIZES,
        "{label}: need >= {MIN_SIZES} sizes for a stable slope, got {}",
        sizes.len(),
    );
    let points: Vec<(f64, f64)> = sizes
        .iter()
        .zip(times)
        .map(|(&s, t)| (s as f64, t.as_secs_f64()))
        .collect();

    let slope = log_log_slope(&points);
    assert!(
        slope < SLOPE_BUDGET,
        "{label}: fitted log-log slope {slope:.3} >= {SLOPE_BUDGET} (sub-quadratic budget)\n\
         {label}: points (size, seconds): {points:?}",
    );

    // Multi-ratio cross-check: every consecutive pair must stay below
    // the quadratic prediction.
    for w in points.windows(2) {
        let ratio = w[1].1 / w[0].1.max(1e-12);
        assert!(
            ratio < RATIO_BUDGET,
            "{label}: time ratio {ratio:.2} >= {RATIO_BUDGET} between sizes {:.0}→{:.0}\n\
             {label}: points (size, seconds): {points:?}",
            w[0].0,
            w[1].0,
        );
    }

    eprintln!("[{label}] slope {slope:.3}  points {points:?}");
}

// ---- input generators ----------------------------------------------------

/// `n` assignment lines, each producing ~3 AST nodes so the parser's
/// per-node `span` call (once O(n²)) is exercised.
fn gen_file_source(n: usize) -> String {
    (0..n)
        .map(|i| format!("v{i} <- c({i}, {}) * 2", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A left-associative pipe chain of `n` stages. The historical defect
/// re-inferred the entire LHS at every stage (O(2ⁿ)); the fix caches
/// the inferred type.
fn gen_pipe_chain(n: usize) -> String {
    let stages = vec!["f"; n].join(" |> ");
    format!("f <- function(x) x\nresult <- 1L |> {stages}\n")
}

/// A function whose body is a left-nested `if` chain of depth `d`,
/// each branch referencing parameter `x`. The historical force-flow
/// defect walked each branch twice → O(2ᵈ); the fix fuses both facts
/// into one pass → O(d).
fn gen_nested_branches(d: usize) -> String {
    let mut body = String::from("x");
    for i in (0..d).rev() {
        body = format!("if (x > {i}) {{ {body} }} else {{ x }}");
    }
    format!("g <- function(x) {body}\n")
}

/// `k` files, each defining one function. `Project::check` is rayon-
/// parallel per file, so growth should be linear in `k`.
fn gen_project_files(k: usize) -> Vec<(String, String)> {
    (0..k)
        .map(|i| {
            (
                format!("file{i}.R"),
                format!("f{i} <- function(x) x * {i}\n"),
            )
        })
        .collect()
}

/// A single expression with `n` nested function-call layers:
/// `f(f(f(…f(1L)…)))`. Call inference recurses to depth `n`.
fn gen_call_depth(n: usize) -> String {
    let inner = "f(".repeat(n);
    let close = ")".repeat(n);
    format!("f <- function(x) x\nresult <- {inner}1L{close}\n")
}

// ---- scaling tests -------------------------------------------------------

/// **File length / parser rescanning.** The parser once computed each
/// node's column by rescanning the source from byte 0 (O(n²) total,
/// 47 s on 20 k lines). The fix uses tree-sitter's column directly.
/// Restoring the rescan makes the slope approach 2.0.
#[test]
#[ignore]
fn scaling_file_length() {
    let sizes = [1_000usize, 2_000, 4_000, 8_000];
    let mut parser = RParser::new().expect("parser init");
    let times: Vec<Duration> = sizes
        .iter()
        .map(|&n| {
            let src = gen_file_source(n);
            timed_median(|| {
                parser.parse("file.R", &src).expect("parse");
            })
        })
        .collect();
    assert_subquadratic_scaling("file_length", &sizes, &times);
}

/// **Pipe length / pipe exponential.** A pipe chain of `n` stages once
/// re-inferred the entire LHS at every stage (O(2ⁿ)). The fix reuses
/// the already-inferred LHS type. Restoring the re-inference makes the
/// slope blow far past 2.0.
#[test]
#[ignore]
fn scaling_pipe_length() {
    let sizes = [8usize, 12, 16, 20];
    let mut parser = RParser::new().expect("parser init");
    let times: Vec<Duration> = sizes
        .iter()
        .map(|&n| {
            let src = gen_pipe_chain(n);
            timed_median(|| {
                let file = parser.parse("pipe.R", &src).expect("parse");
                let mut c = Checker::new("pipe.R");
                c.check(&file);
                let _ = c.take_diagnostics();
            })
        })
        .collect();
    assert_subquadratic_scaling("pipe_length", &sizes, &times);
}

/// **Branch depth / force-flow double walk.** The required-parameter
/// force-flow analysis once walked each if-branch twice (O(2ᵈ)). The
/// fix fuses force and fall-through into a single walk (O(d)).
/// Restoring the double walk makes the slope blow past 2.0.
#[test]
#[ignore]
fn scaling_branch_depth() {
    let sizes = [10usize, 14, 18, 22];
    let mut parser = RParser::new().expect("parser init");
    let times: Vec<Duration> = sizes
        .iter()
        .map(|&d| {
            let src = gen_nested_branches(d);
            timed_median(|| {
                let file = parser.parse("branch.R", &src).expect("parse");
                let mut c = Checker::new("branch.R");
                c.check(&file);
                let _ = c.take_diagnostics();
            })
        })
        .collect();
    assert_subquadratic_scaling("branch_depth", &sizes, &times);
}

/// **Project size.** `Project::check` runs each file through the
/// checker (rayon-parallel). Growth should be linear in file count.
#[test]
#[ignore]
fn scaling_project_size() {
    let sizes = [50usize, 100, 200, 400];
    let mut parser = RParser::new().expect("parser init");
    let times: Vec<Duration> = sizes
        .iter()
        .map(|&k| {
            let files = gen_project_files(k);
            let parsed: Vec<(String, ry_core::SourceFile)> = files
                .iter()
                .map(|(path, src)| (path.clone(), parser.parse(path, src).expect("parse")))
                .collect();
            timed_median(|| {
                let mut project = Project::new();
                for (path, file) in &parsed {
                    project.add_file(path.clone(), file.clone());
                }
                let _ = project.check();
            })
        })
        .collect();
    assert_subquadratic_scaling("project_size", &sizes, &times);
}

/// **Call depth.** A single expression with `n` nested calls. Inference
/// recurses to depth `n`. Growth should be linear.
#[test]
#[ignore]
fn scaling_call_depth() {
    let sizes = [8usize, 12, 16, 20];
    let mut parser = RParser::new().expect("parser init");
    let times: Vec<Duration> = sizes
        .iter()
        .map(|&n| {
            let src = gen_call_depth(n);
            timed_median(|| {
                let file = parser.parse("call.R", &src).expect("parse");
                let mut c = Checker::new("call.R");
                c.check(&file);
                let _ = c.take_diagnostics();
            })
        })
        .collect();
    assert_subquadratic_scaling("call_depth", &sizes, &times);
}
