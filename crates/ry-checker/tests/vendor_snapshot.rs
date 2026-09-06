//! Vendored CRAN package regression net.
//!
//! Runs `Project::check` over the vendored `glue` R sources and
//! snapshots every diagnostic as `path:line:col CODE message`, sorted.
//! The snapshot is triaged in a comment block below: each diagnostic is
//! either a true positive or a known limitation with its planned fix.
//!
//! Update the snapshot with `cargo test -p ry-checker --test vendor_snapshot
//! -- --nocapture` after accepting with `INSTA_UPDATE=always` when the
//! diagnostics intentionally change.
//!
//! glue is MIT-licensed; see `testdata/vendor/glue/LICENSE`.

use std::collections::HashMap;

use ry_checker::Project;
use ry_core::RParser;
use ry_core::parser::byte_col_to_char_col;

/// The vendored package's `R/` directory, rooted at the crate's
/// testdata dir.
const VENDOR_DIR: &str = "testdata/vendor/glue/R";

/// Render a project's diagnostics as a sorted list of
/// `path:line:col CODE message` strings. Line/col are 1-based; the
/// column is converted from the span's byte column to a character
/// column (mirroring `format::line_col`) so non-ASCII source lines
/// report the right column.
fn render_diags(
    per_file: &[(String, Vec<ry_checker::Diagnostic>)],
    srcs: &HashMap<String, String>,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for (path, diags) in per_file {
        let src = srcs.get(path);
        for d in diags {
            let line = d.span.line + 1;
            // Convert the byte column to a 1-based character column.
            let col = match src {
                Some(s) => {
                    let line_text = source_line(s, d.span.start);
                    byte_col_to_char_col(line_text, d.span.col) + 1
                }
                None => d.span.col + 1,
            };
            // Use only the file stem (not the full vendored path) to
            // keep the snapshot stable across checkout locations.
            let short = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            lines.push(format!("{short}:{line}:{col} {} {}", d.code, d.message));
        }
    }
    lines.sort();
    lines
}

/// Borrow the single source line containing byte offset `pos`.
fn source_line(src: &str, pos: usize) -> &str {
    let bounded = pos.min(src.len());
    let start = src[..bounded].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = src[bounded..]
        .find('\n')
        .map(|i| bounded + i)
        .unwrap_or(src.len());
    src.get(start..end).unwrap_or("")
}

#[test]
fn glue_vendor_snapshot() {
    // -----------------------------------------------------------------
    // Triage of the glue vendor snapshot.
    //
    // The snapshot is EMPTY: zero diagnostics across the whole glue
    // package.
    //
    // It MUST stay empty: any future diagnostic on glue is a
    // regression. A second vendor package is pinned separately to keep
    // the net honest now that glue is clean.
    // -----------------------------------------------------------------

    let rendered = check_vendor(VENDOR_DIR);
    insta::assert_yaml_snapshot!("glue_vendor", rendered);
}

/// Load every `.R` file under `testdata/vendor/<subdir>/R`, run
/// `Project::check`, and return the diagnostics rendered as a sorted
/// list of `file:line:col CODE message` strings (using the file stem
/// for path stability).
fn check_vendor(vendor_subdir: &str) -> Vec<String> {
    let vendor_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(vendor_subdir);
    let mut paths: Vec<_> = std::fs::read_dir(&vendor_root)
        .expect("vendored package must exist")
        .map(|entry| entry.expect("read vendor entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "R"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "vendored package has no R sources");
    let mut parser = RParser::new().expect("parser init");
    let files: Vec<_> = paths
        .iter()
        .map(|path| {
            let source = ry_workspace::read_r_source(path).expect("read vendored R source");
            parser
                .parse(&path.to_string_lossy(), &source)
                .expect("parse vendored R source")
        })
        .collect();
    let workspace = ry_workspace::resolve_workspace_context(
        vendor_root.parent().unwrap(),
        &ry_config::Config::default(),
        ry_workspace::ResolutionEnvironment {
            files: files.iter().collect(),
            user_stubs: &Default::default(),
        },
    )
    .expect("resolve vendored package");
    let mut project = Project::new();
    project.set_loaded(workspace.attached_packages);
    project.set_bare_loaded(workspace.bare_bindings);
    project.set_external_bindings(workspace.external_bindings);
    project.set_imported_from(workspace.imported_bindings);
    project.set_external_s3_methods(workspace.s3_methods);
    project.set_load_bindings(workspace.load_bindings);
    let srcs = files
        .iter()
        .map(|file| (file.path.clone(), file.source.clone()))
        .collect();
    for file in files {
        project.add_file(file.path.clone(), file);
    }
    render_diags(&project.check(), &srcs)
}

#[test]
fn purrr_vendor_snapshot() {
    // -----------------------------------------------------------------
    // Triage of the purrr vendor snapshot (purrr 1.2.2, MIT). The second
    // vendor net alongside glue; purrr is the flagship tidyverse
    // functional-programming package.
    //
    // purrr ships a NAMESPACE, so this runs with the same metadata the
    // CLI applies: `importFrom` bindings, S3 registrations, and
    // `useDynLib(purrr, .registration = TRUE)`.
    //
    // Whole-package imports resolve rlang/vctrs functions and constants.
    // The remaining RY032 reports a scalar requirement for `before` in
    // prepend(); its earlier stopifnot check rejects invalid lengths at runtime.
    //
    // purrr's own C-backed entry points (map_impl, map2_impl, pmap_impl)
    // are NOT in the snapshot and must stay out: they are passed as bare
    // symbols to `call_with_cleanup`, which the `.registration = TRUE`
    // declaration licenses. Dropping the NAMESPACE, or the
    // registration gate, makes all three reappear.
    // -----------------------------------------------------------------

    let rendered = check_vendor("testdata/vendor/purrr/R");
    insta::assert_yaml_snapshot!("purrr_vendor", rendered);
}
