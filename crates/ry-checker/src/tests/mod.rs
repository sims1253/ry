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

fn check(src: &str) -> Vec<Diagnostic> {
    let mut p = RParser::new().unwrap();
    let f = p.parse("test.R", src).unwrap();
    let mut c = Checker::new("test.R");
    c.check(&f);
    c.take_diagnostics()
}

/// Test-only variant of `check` that also returns the final
/// top-level scope so tests can assert on the inferred `RType` of a
/// binding (mode, length, class, columns). Mirrors what `Checker::check`
/// does internally, but keeps the scope around for inspection.
fn check_with_scope(src: &str) -> (Vec<Diagnostic>, Scope) {
    let mut p = RParser::new().unwrap();
    let f = p.parse("test.R", src).unwrap();
    let mut c = Checker::new("test.R");
    // Mirror `Checker::check`'s pass structure so user-fn return
    // types are refined before we walk for the final scope.
    c.collect_fns(&f.stmts);
    for _ in 0..MAX_FIXPOINT_DEPTH {
        let before = (*c.return_slots).clone();
        let names: Vec<String> = c.fn_table.fns.keys().cloned().collect();
        for name in names {
            c.refine_fn_return(&name);
        }
        if c.return_slots.0 == before.0 {
            break;
        }
    }
    let mut scope = Scope::default();
    for s in &f.stmts {
        c.check_stmt(s, &mut scope);
    }
    (c.take_diagnostics(), scope)
}

/// Parse helper for project-mode tests, mirroring the one in
/// `project::tests`.
fn parse_file(path: &str, src: &str) -> SourceFile {
    let mut p = RParser::new().unwrap();
    p.parse(path, src).unwrap()
}
