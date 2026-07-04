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
    // Triage of the glue vendor snapshot.
    //
    // After PLAN Phase 1 (round 3), this snapshot is EMPTY -- zero
    // diagnostics across the whole glue package. All 12 of the original
    // false positives were resolved:
    //
    // RY002 (was 6x, DOMINANT -- fixed in Phase 1.1): RY002 now fires
    //   ONLY when the condition length is Known(n > 1), never on
    //   Unknown. The six sites (`!inherits(...)`, `too_wide`,
    //   `should_collapse`, `!requireNamespace(...)`) are all
    //   scalar-boolean conditions typed length-Unknown.
    //
    // RY010 (was 3x -- fixed across Phase 1.3/1.4):
    //   * glue.R:187 `glue_`, glue.R:319 `trim_` -- Phase 1.3 models
    //     `.Call`/`.C`/`.Fortran`/... so the C-entry-point first arg is
    //     no longer treated as a variable reference.
    //   * utils.R:90 `delayedAssign` -- Phase 1.4 added it to the
    //     typeshed.
    //
    // RY070 (was 3x -- fixed across Phase 1.2/1.4):
    //   * color.R:123 `color_fun` -- Phase 1.2 null-narrowing: the
    //     else branch of an `is.null` guard narrows NULL away.
    //   * glue.R:191 `lengths` -- Phase 1.4 added `lengths` to the
    //     typeshed AND fixed R's function/value namespace separation at
    //     call sites (a local non-function binding does not shadow a
    //     same-named function in a call).
    //
    // The snapshot MUST stay empty: any future diagnostic on glue is a
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
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let vendor_root = std::path::Path::new(manifest_dir).join(vendor_subdir);

    let mut entries: Vec<_> = match std::fs::read_dir(&vendor_root) {
        Ok(e) => e.flatten().collect(),
        Err(e) => {
            eprintln!(
                "vendor_snapshot: could not read {}; skipping. ({})",
                vendor_root.display(),
                e
            );
            return Vec::new();
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
    render_diags(&per_file, &srcs)
}

#[test]
fn purrr_vendor_snapshot() {
    // -----------------------------------------------------------------
    // Triage of the purrr vendor snapshot (purrr 1.2.2, MIT). The second
    // vendor net, added in PLAN Phase 1 (round 3) to keep the regression
    // net honest now that glue is clean. purrr is the flagship tidyverse
    // functional-programming package and the target of Phase 2.3's
    // higher-order modeling, so most of these are EXPECTED to disappear
    // once package awareness (Phase 2.1/2.2) and purrr modeling (Phase
    // 2.3) land. None are true positives.
    //
    // RY010 (dominant): cross-package function names not yet modeled --
    //   rlang (quo_get_expr, eval_tidy, is_bare_list, is_quosure,
    //   as_quosure, is_bare_formula, obj_is_list), vctrs (vec_set_union),
    //   and purrr's own C-backed impls (map_impl, map2_impl, pmap_impl).
    //   These resolve once the package typeshed (Phase 2.2) covers rlang
    //   and vctrs and purrr's internal helpers are registered.
    //
    // RY001 (2x): the `if (length(x))` idiom. `length()` returns an
    //   integer length-1; R silently coerces integer->logical in `if`.
    //   The rule warns about that coercion, which is harmless and
    //   idiomatic here. Known limitation: RY001 could special-case the
    //   `if (length(.))` / `if (nrow(.))` numeric-truthiness idiom.
    //
    // RY002 + RY032 (2x, same root cause): `%in%` is typed with the RHS
    //   length instead of the LHS length. `x %in% c("a","b")` where x is
    //   length 1 returns a length-1 logical, but ry models it as length
    //   2 (the RHS). That wrong length then drives RY002 (condition len
    //   2) and RY032 (`&&` on a len-2 operand). Modeling bug in `%in%`;
    //   a separate fix from Phase 1's scope, surfaced by this net.
    //
    // Summary: 0 true positives. The net is doing its job -- it caught a
    //   real `%in%` length-modeling bug and a stack of cross-package
    //   names that Phase 2 must cover.
    // -----------------------------------------------------------------

    let rendered = check_vendor("testdata/vendor/purrr/R");
    insta::assert_yaml_snapshot!("purrr_vendor", rendered);
}
