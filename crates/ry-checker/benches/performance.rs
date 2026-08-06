use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use ry_checker::{Checker, Project};
use ry_core::{RParser, SourceFile};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn glue_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/vendor/glue/R");
    let mut paths: Vec<PathBuf> = fs::read_dir(root)
        .expect("read vendored glue sources")
        .map(|entry| entry.expect("read vendored glue entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("R"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .expect("glue source is inside the crate")
                .to_string_lossy()
                .into_owned();
            let source = fs::read_to_string(path).expect("read vendored glue source");
            (relative, source)
        })
        .collect()
}

fn parse_sources(sources: &[(String, String)]) -> Vec<(String, SourceFile)> {
    let mut parser = RParser::new().expect("initialize R parser");
    sources
        .iter()
        .map(|(path, source)| {
            let file = parser
                .parse(path, source)
                .expect("parse vendored glue source");
            (path.clone(), file)
        })
        .collect()
}

fn synthetic_source() -> String {
    (0..20_000)
        .map(|i| format!("x{i} <- c({i}, {}) * 2", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Peak RSS in kilobytes, read from `/proc/self/status`.
/// Returns 0 on non-Linux or if unavailable.
#[cfg(target_os = "linux")]
fn peak_rss_kb() -> u64 {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: u64 = rest
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
            return kb;
        }
    }
    0
}

#[cfg(not(target_os = "linux"))]
fn peak_rss_kb() -> u64 {
    0
}

// ---------------------------------------------------------------------------
// Existing benchmarks (cold checks)
// ---------------------------------------------------------------------------

fn parse_large(c: &mut Criterion) {
    let source = glue_sources()
        .into_iter()
        .map(|(_, source)| source)
        .collect::<Vec<_>>()
        .join("\n");
    let mut parser = RParser::new().expect("initialize R parser");
    c.bench_function("parse_large", |b| {
        b.iter(|| {
            parser
                .parse("glue-all.R", black_box(&source))
                .expect("parse glue")
        });
    });
}

fn check_project_glue(c: &mut Criterion) {
    let parsed = parse_sources(&glue_sources());
    let mut project = Project::new();
    for (path, file) in parsed {
        project.add_file(path, file);
    }
    c.bench_function("check_project_glue", |b| {
        b.iter(|| black_box(project.check()));
    });
}

fn check_single_synthetic(c: &mut Criterion) {
    let source = synthetic_source();
    let mut parser = RParser::new().expect("initialize R parser");
    let file = parser
        .parse("synthetic.R", &source)
        .expect("parse synthetic source");
    c.bench_function("check_single_synthetic", |b| {
        b.iter(|| {
            let mut checker = Checker::new("synthetic.R");
            black_box(checker.check(black_box(&file)));
        });
    });
}

// ---------------------------------------------------------------------------
// Incremental benchmarks (Plan 33 W0)
//
// These measure the warm `check_incremental` path — the one the LSP
// server exercises on every debounce tick. Each bench primes a
// Project with a full cold `check()`, then measures only the cost of
// one incremental edit + `check_incremental()`.
//
// The four scenarios from the plan:
//   1. Edit a file that other files depend on.
//   2. Edit a leaf file that nothing depends on (the number W1/W2
//      should move).
//   3. Add/remove a `library()` call (project-wide invalidation).
//   4. Cold `check` baseline (already in `check_project_glue` above).
//
// Each benchmark also prints peak RSS via `throughput`/custom
// measurement so CI can track memory regressions.
// ---------------------------------------------------------------------------

/// Shared setup: build a primed Project from the vendored glue corpus,
/// return it along with the parsed sources and the parser for edits.
fn primed_project() -> (Project, Vec<(String, String)>, RParser) {
    let sources = glue_sources();
    let parsed = parse_sources(&sources);
    let mut project = Project::new();
    for (path, file) in parsed {
        project.add_file(path, file);
    }
    // Prime the pass-1 cache with a cold check.
    black_box(project.check());
    let parser = RParser::new().expect("initialize R parser");
    (project, sources, parser)
}

/// Find the path and source of a specific vendored file.
fn find_source(sources: &[(String, String)], suffix: &str) -> (String, String) {
    sources
        .iter()
        .find(|(path, _)| path.ends_with(suffix))
        .cloned()
        .unwrap_or_else(|| panic!("no vendored source ending with {suffix}"))
}

/// Benchmark: warm `check_incremental` after a one-line edit to a file
/// that other files depend on (`glue.R` is the main module).
fn warm_edit_dependent(c: &mut Criterion) {
    let (mut project, sources, mut parser) = primed_project();
    let (edited_path, original) = find_source(&sources, "glue.R");
    let edited_src = format!("{original}\n.ry_bench_value <- 1L\n");

    c.bench_function("warm_edit_dependent", |b| {
        b.iter(|| {
            let changed = parser
                .parse(&edited_path, black_box(&edited_src))
                .expect("reparse");
            project.update_file(edited_path.clone(), changed);
            black_box(project.check_incremental());
        });
    });

    let rss = peak_rss_kb();
    eprintln!("[warm_edit_dependent] peak RSS: {rss} kB");
}

/// Benchmark: warm `check_incremental` after a one-line edit to a leaf
/// file that nothing depends on (`zzz.R` is glue's load hook — no other
/// file references its functions). This is the number W1 and W2 exist
/// to move.
fn warm_edit_leaf(c: &mut Criterion) {
    let (mut project, sources, mut parser) = primed_project();
    let (edited_path, original) = find_source(&sources, "zzz.R");
    let edited_src = format!("{original}\n.ry_bench_value <- 1L\n");

    c.bench_function("warm_edit_leaf", |b| {
        b.iter(|| {
            let changed = parser
                .parse(&edited_path, black_box(&edited_src))
                .expect("reparse");
            project.update_file(edited_path.clone(), changed);
            black_box(project.check_incremental());
        });
    });

    let rss = peak_rss_kb();
    eprintln!("[warm_edit_leaf] peak RSS: {rss} kB");
}

/// Benchmark: warm `check_incremental` after adding/removing a
/// `library()` call. This invalidates project-wide because `loaded`
/// is a project-wide union (see Plan 33 K2).
fn warm_edit_library(c: &mut Criterion) {
    let (mut project, sources, mut parser) = primed_project();
    let (edited_path, original) = find_source(&sources, "utils.R");
    let with_library = format!("library(tools)\n{original}");
    let without_library = original.clone();

    let mut toggle = true;
    c.bench_function("warm_edit_library", |b| {
        b.iter(|| {
            toggle = !toggle;
            let src = if toggle {
                &with_library
            } else {
                &without_library
            };
            let changed = parser.parse(&edited_path, black_box(src)).expect("reparse");
            project.update_file(edited_path.clone(), changed);
            black_box(project.check_incremental());
        });
    });

    let rss = peak_rss_kb();
    eprintln!("[warm_edit_library] peak RSS: {rss} kB");
}

// ---------------------------------------------------------------------------
// Legacy LSP edit simulation (kept for comparison with the above)
// ---------------------------------------------------------------------------

fn lsp_edit_sim(c: &mut Criterion) {
    let sources = glue_sources();
    let parsed: Vec<(String, Arc<SourceFile>)> = parse_sources(&sources)
        .into_iter()
        .map(|(path, file)| (path, Arc::new(file)))
        .collect();
    let edited_index = sources
        .iter()
        .position(|(path, _)| path.ends_with("glue.R"))
        .unwrap_or(0);
    let (edited_path, original) = &sources[edited_index];
    let edited_sources = [
        format!("{original}\n.ry_bench_value <- 1L\n"),
        format!("{original}\n.ry_bench_value <- 2L\n"),
    ];
    let mut parser = RParser::new().expect("initialize R parser");
    let mut edit = 0usize;
    let mut project = Project::new();
    for (path, file) in &parsed {
        project.add_file(path.clone(), file.as_ref().clone());
    }
    black_box(project.check_incremental());

    c.bench_function("lsp_edit_sim", |b| {
        b.iter(|| {
            edit ^= 1;
            let changed = Arc::new(
                parser
                    .parse(edited_path, black_box(&edited_sources[edit]))
                    .expect("reparse edited glue source"),
            );
            // Model the LSP's all-document cache snapshot: unchanged files
            // clone only Arc handles, while the changed AST is forwarded to
            // the persistent Project and deep-cloned once into its cache.
            let cached_files: Vec<_> = parsed
                .iter()
                .map(|(path, file)| {
                    (
                        path.clone(),
                        if path == edited_path {
                            Arc::clone(&changed)
                        } else {
                            Arc::clone(file)
                        },
                    )
                })
                .collect();
            black_box(cached_files);
            project.update_file(edited_path.clone(), changed.as_ref().clone());
            black_box(project.check_incremental());
        });
    });
}

criterion_group! {
    name = performance;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = parse_large, check_project_glue, check_single_synthetic,
              warm_edit_dependent, warm_edit_leaf, warm_edit_library,
              lsp_edit_sim
}
criterion_main!(performance);
