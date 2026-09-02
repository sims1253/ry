use super::*;
use ry_core::RParser;

mod data_frames_s3;
mod diagnostics;
mod functions_classes;
mod narrowing;
mod packages_typeshed;
mod quoting_data_mask;
mod scope_resolution;
mod type_inference;

// Shared fixtures used across topic modules.

/// Parse one checker-test snippet under `path`: the single
/// `RParser::new()` + parse entry point shared by the topic modules in
/// this directory and the inline `#[cfg(test)]` units in `collect.rs`,
/// `infer::binop`, and `infer::index`, so parser setup cannot drift
/// between them.
pub(super) fn parse_snippet(path: &str, src: &str) -> SourceFile {
    let mut p = RParser::new().unwrap();
    p.parse(path, src).unwrap()
}

fn check(src: &str) -> Vec<Diagnostic> {
    let mut c = Checker::new("test.R");
    c.check(&parse_snippet("test.R", src));
    c.take_diagnostics()
}

/// `check` plus a setup step on the `Checker` before the run, for tests
/// that configure external bindings, loaded packages, imported-from
/// metadata, or user stubs.
fn check_with(src: &str, setup: impl FnOnce(&mut Checker)) -> Vec<Diagnostic> {
    let mut c = Checker::new("test.R");
    setup(&mut c);
    c.check(&parse_snippet("test.R", src));
    c.take_diagnostics()
}

/// Test-only variant of `check` that also returns the final
/// top-level scope so tests can assert on the inferred `RType` of a
/// binding (mode, length, class, columns). Delegates to the public
/// `Checker::check_with_scope`, so the tests exercise the real pass
/// structure and cannot diverge from it.
fn check_with_scope(src: &str) -> (Vec<Diagnostic>, Scope) {
    let mut c = Checker::new("test.R");
    c.check_with_scope(&parse_snippet("test.R", src))
}

/// Parse helper for tests that check a `SourceFile` under a custom path
/// (project mode, non-`test.R` names). Also used by `project::tests`.
pub(super) fn parse_file(path: &str, src: &str) -> SourceFile {
    parse_snippet(path, src)
}
