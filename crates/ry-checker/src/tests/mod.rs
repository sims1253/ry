use super::*;
use ry_core::RParser;

mod constructors;
mod data_frames_s3;
mod diagnostics;
mod functions_classes;
mod narrowing;
mod operator_s3_dispatch;
mod ops_fallback;
mod packages_typeshed;
mod quoting_data_mask;
mod scope_resolution;
mod type_inference;
mod typed_maps;

// Shared fixtures used across topic modules.

/// Parse a checker test snippet under the given source path.
pub(super) fn parse_file(path: &str, src: &str) -> SourceFile {
    let mut p = RParser::new().unwrap();
    p.parse(path, src).unwrap()
}

fn check(src: &str) -> Vec<Diagnostic> {
    let mut c = Checker::new("test.R");
    c.check(&parse_file("test.R", src));
    c.take_diagnostics()
}

/// `check` plus a setup step on the `Checker` before the run, for tests
/// that configure external bindings, loaded packages, imported-from
/// metadata, or user stubs.
fn check_with(src: &str, setup: impl FnOnce(&mut Checker)) -> Vec<Diagnostic> {
    let mut c = Checker::new("test.R");
    setup(&mut c);
    c.check(&parse_file("test.R", src));
    c.take_diagnostics()
}

/// Test-only variant of `check` that also returns the final
/// top-level scope so tests can assert on the inferred `RType` of a
/// binding (mode, length, class, columns). Delegates to the public
/// `Checker::check_with_scope`, so the tests exercise the real pass
/// structure and cannot diverge from it.
fn check_with_scope(src: &str) -> (Vec<Diagnostic>, Scope) {
    let mut c = Checker::new("test.R");
    c.check_with_scope(&parse_file("test.R", src))
}

/// `check_with_scope` plus stub typeshed files (file name, raw JSON)
/// written into one temp directory. A file's stem is its package key:
/// `base.json` replaces the embedded base typeshed, any other name
/// becomes a package typeshed.
fn check_with_stubs(src: &str, stub_files: &[(&str, &str)]) -> (Vec<Diagnostic>, Scope) {
    let dir = tempfile::tempdir().unwrap();
    for (name, json) in stub_files {
        std::fs::write(dir.path().join(name), json).unwrap();
    }
    let mut c = Checker::new("test.R");
    c.set_user_stubs(Arc::new(ry_typeshed::load_stub_dir(dir.path()).unwrap()));
    c.check_with_scope(&parse_file("test.R", src))
}

mod factor_new_constructor;
mod structure_constructor;
