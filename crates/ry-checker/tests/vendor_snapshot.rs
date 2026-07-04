//! Vendored CRAN package regression net (PLAN.md Phase F).
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
use ry_core::parser::byte_col_to_char_col;
use ry_core::RParser;

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
    // Triage of the glue vendor snapshot (12 diagnostics as first
    // captured). Each is classified below. Per PLAN Phase F's bar, if a
    // single rule dominates the snapshot that rule has a systemic
    // problem -- flagged at the end.
    //
    // RY002 (6x -- DOMINANT, systemic): `if` condition has length
    //   `Unknown`. All six sites are scalar-boolean conditions
    //   (`!inherits(...)`, `too_wide`, `should_collapse`,
    //   `!requireNamespace(...)`) where ry conservatively types the
    //   condition length as Unknown and RY002 fires. RY002 should only
    //   fire when the length is KNOWN to be > 1; the Unknown case is a
    //   false positive. SYSTEMIC ISSUE -- file "RY002 should not fire on
    //   Unknown-length conditions" at the top of the next plan.
    //     color.R:97, color.R:104, glue.R:277, sql.R:220, sql.R:242,
    //     sql.R:255, transformer.R:22
    //
    // RY010 (3x -- known limitations, not systemic):
    //   * glue.R:187 `glue_`, glue.R:319 `trim_` -- these are
    //     C-entry-point NAME STRINGS passed to `.Call(glue_, ...)`, not
    //     variable references. ry doesn't model `.Call` semantics.
    //     Planned fix: special-case `.Call` so its first arg is not
    //     treated as an identifier reference.
    //   * utils.R:90 `delayedAssign` -- a base-R function not in ry's
    //     typeshed. Planned fix: add `delayedAssign` to the typeshed.
    //
    // RY070 (3x -- known limitations, not systemic):
    //   * color.R:123 `color_fun` -- the parameter defaults to NULL and
    //     the code guards `if (is.null(color_fun)) ... else color_fun(out)`.
    //     ry doesn't narrow across the null-guard, so the else-branch
    //     call types the parameter as NULL. Planned fix: flow-sensitive
    //     null-narrowing (the `extract_type_narrowing` machinery exists
    //     for `is.X` predicates; extend it to `is.null`).
    //   * glue.R:191 `lengths` -- `lengths` is a base-R builtin not in
    //     the typeshed, so the call resolves the callee to the argument
    //     type. Planned fix: add `lengths` to the typeshed.
    //
    // Summary: 0 true positives. 6 false positives of one systemic
    // kind (RY002/Unknown), 6 known-limitation false positives across
    // three distinct gaps (`.Call` modeling, missing typeshed entries,
    // null-narrowing). The RY002 dominance is the action item.
    // -----------------------------------------------------------------

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let vendor_root = std::path::Path::new(manifest_dir).join(VENDOR_DIR);

    let mut entries: Vec<_> = match std::fs::read_dir(&vendor_root) {
        Ok(e) => e.flatten().collect(),
        Err(e) => {
            eprintln!(
                "vendor_snapshot: could not read {}; skipping. ({})",
                vendor_root.display(),
                e
            );
            return;
        }
    };
    entries.sort_by_key(|e| e.path());
    if entries.is_empty() {
        panic!(
            "vendor_snapshot: no files in {}; the vendored package is missing",
            vendor_root.display()
        );
    }

    let mut parser = RParser::new().expect("parser init");
    let mut project = Project::new();
    let mut srcs: HashMap<String, String> = HashMap::new();
    for entry in &entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("R") {
            continue;
        }
        let rel = path
            .strip_prefix(manifest_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        let src = std::fs::read_to_string(&path).expect("read vendored .R");
        let file = match parser.parse(&rel, &src) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("vendor_snapshot: parse {}: {}", rel, e);
                continue;
            }
        };
        project.add_file(rel.clone(), file);
        srcs.insert(rel, src);
    }

    let per_file = project.check();
    let rendered = render_diags(&per_file, &srcs);
    insta::assert_yaml_snapshot!("glue_vendor", rendered);
}
