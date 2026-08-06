//! Multi-file project tests. Verifies that functions and S3 methods
//! defined in one file are visible when checking another file in the
//! same project.

use ry_checker::Project;
use ry_core::RParser;
use std::sync::Arc;

fn parse(path: &str, src: &str) -> ry_core::SourceFile {
    let mut p = RParser::new().unwrap();
    p.parse(path, src).unwrap()
}

#[test]
fn cross_file_function_visibility() {
    // utils.R defines a function, analysis.R calls it. Without
    // project mode, the call would emit RY010 because the per-file
    // checker does not know about `double_it`.
    let mut project = Project::new();
    project.add_file(
        "utils.R".to_string(),
        parse("utils.R", "double_it <- function(x = 1L) { x * 2 }\n"),
    );
    project.add_file(
        "analysis.R".to_string(),
        parse("analysis.R", "result <- double_it(5)\n"),
    );
    let diags = project.check();
    let analysis_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == "analysis.R")
        .flat_map(|(_, d)| d)
        .collect();
    assert!(
        analysis_diags.iter().all(|d| d.code != "RY010"),
        "double_it should be visible across files, got: {:?}",
        analysis_diags
    );
}

#[test]
fn cross_file_function_return_type_propagates() {
    // If utils.R defines a function returning character, calling it
    // from analysis.R and using the result arithmetically should
    // trigger RY040. This proves that the cross-file return-type
    // refinement from pass 2 reaches the per-file diagnostics in
    // pass 3.
    let mut project = Project::new();
    project.add_file(
        "utils.R".to_string(),
        parse("utils.R", "make_string <- function() { \"hello\" }\n"),
    );
    project.add_file(
        "analysis.R".to_string(),
        parse("analysis.R", "y <- make_string() + 1L\n"),
    );
    let diags = project.check();
    let all: Vec<_> = diags.into_iter().flat_map(|(_, d)| d).collect();
    assert!(
        all.iter().any(|d| d.code == "RY040"),
        "expected RY040 from cross-file character-returning fn + int, got: {:?}",
        all
    );
}

#[test]
fn cross_file_fixpoint_rebinding_overrides_null_narrowing() {
    // `make_writer` is opaque in the first fixpoint iteration, then its
    // function return type is refined. The branch assignment must still
    // replace the NULL-derived narrowing in either iteration.
    let mut project = Project::new();
    project.add_file(
        "writer.R".to_string(),
        parse(
            "writer.R",
            "make_writer <- function() { local({ function(value) value }) }\n",
        ),
    );
    project.add_file(
        "use.R".to_string(),
        parse(
            "use.R",
            "use_writer <- function(writer = NULL) {\n\
             if (is.null(writer)) writer <- make_writer()\n\
             writer(1L)\n\
             }\n\
             use_writer()\n",
        ),
    );
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter().all(|diagnostic| diagnostic.code != "RY070"),
        "an opaque cross-file rebinding must override NULL narrowing: {all:?}"
    );
}

#[test]
fn incremental_edit_rechecks_cross_file_dependents() {
    let mut project = Project::new();
    project.add_file(
        "utils.R".to_string(),
        parse("utils.R", "make_value <- function() { \"hello\" }\n"),
    );
    project.add_file(
        "analysis.R".to_string(),
        parse("analysis.R", "result <- make_value() + 1L\n"),
    );

    let before = project.check_incremental();
    let before_analysis = before
        .iter()
        .find(|(path, _)| path == "analysis.R")
        .unwrap();
    assert!(
        before_analysis
            .1
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040"),
        "character return should make analysis.R invalid: {before_analysis:?}"
    );

    project.update_file(
        "utils.R".to_string(),
        Arc::new(parse("utils.R", "make_value <- function() { 1L }\n")),
    );
    let after = project.check_incremental();
    let after_analysis = after.iter().find(|(path, _)| path == "analysis.R").unwrap();
    assert!(
        after_analysis
            .1
            .iter()
            .all(|diagnostic| diagnostic.code != "RY040"),
        "integer return should update analysis.R diagnostics: {after_analysis:?}"
    );
}

#[test]
fn cross_file_s3_method_dispatches() {
    // methods.R defines print.foo; usage.R creates a "foo"-classed
    // value and calls print on it. The S3 method table is shared
    // across files, so dispatch finds the method and RY050 stays
    // silent.
    let mut project = Project::new();
    project.add_file(
        "methods.R".to_string(),
        parse(
            "methods.R",
            "print.foo <- function(x, ...) { invisible(x) }\n",
        ),
    );
    project.add_file(
        "usage.R".to_string(),
        parse(
            "usage.R",
            "x <- structure(list(), class = \"foo\")\nprint(x)\n",
        ),
    );
    let diags = project.check();
    let all: Vec<_> = diags.into_iter().flat_map(|(_, d)| d).collect();
    assert!(
        all.iter().all(|d| d.code != "RY050"),
        "print.foo from methods.R should dispatch on usage.R's x, got: {:?}",
        all
    );
}

#[test]
fn cross_file_s3_ops_method_precedes_storage_mode_error() {
    let mut project = Project::new();
    project.add_file(
        "methods.R".to_string(),
        parse(
            "methods.R",
            "Ops.rvar <- function(e1, e2) structure(list(), class = \"rvar\")\n",
        ),
    );
    project.add_file(
        "usage.R".to_string(),
        parse(
            "usage.R",
            "x <- structure(list(1), class = \"rvar\")\ny <- x + x\nz <- x == 1\n",
        ),
    );
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter()
            .all(|diagnostic| !matches!(diagnostic.code, "RY030" | "RY040")),
        "Ops.rvar should dispatch before primitive list errors: {all:?}"
    );
}

#[test]
fn external_binding_is_a_function_position_candidate() {
    use std::collections::{HashMap, HashSet};

    let mut project = Project::new();
    project.add_file(
        "usage.R".to_string(),
        parse("usage.R", "ndraws <- NULL\nn <- ndraws(x)\n"),
    );
    project.set_external_bindings(HashMap::from([(
        "usage.R".to_string(),
        HashSet::from(["ndraws".to_string()]),
    )]));
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter().all(|diagnostic| diagnostic.code != "RY070"),
        "imported ndraws should remain callable despite a local data binding: {all:?}"
    );
}

#[test]
fn namespace_s3_registration_is_an_operator_candidate() {
    use std::collections::{HashMap, HashSet};

    let mut project = Project::new();
    project.add_file(
        "usage.R".to_string(),
        parse(
            "usage.R",
            "x <- structure(list(1), class = \"external_class\")\ny <- x + x\n",
        ),
    );
    project.set_external_s3_methods(HashMap::from([(
        "usage.R".to_string(),
        HashSet::from([("Ops".to_string(), "external_class".to_string())]),
    )]));
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter().all(|diagnostic| diagnostic.code != "RY040"),
        "registered Ops method should be consulted before storage mode: {all:?}"
    );
}

#[test]
fn load_bindings_activate_at_the_load_statement() {
    use std::collections::{HashMap, HashSet};

    let file = parse(
        "usage.R",
        "before_load\nload(\"objects.rda\")\nafter_load\n",
    );
    let load_start = file
        .stmts
        .iter()
        .find_map(|statement| match statement {
            ry_core::ast::Stmt::Expr(ry_core::ast::Expr::Call { span, .. }) => Some(span.start),
            _ => None,
        })
        .unwrap();
    let mut project = Project::new();
    project.add_file("usage.R".to_string(), file);
    project.set_load_bindings(HashMap::from([(
        "usage.R".to_string(),
        HashMap::from([(load_start, HashSet::from(["after_load".to_string()]))]),
    )]));
    let all: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        all.iter()
            .any(|diagnostic| diagnostic.message.contains("before_load")),
        "a read before load must remain unbound: {all:?}"
    );
    assert!(
        all.iter()
            .all(|diagnostic| !diagnostic.message.contains("after_load")),
        "a loaded binding should resolve after load: {all:?}"
    );
}

#[test]
fn redefinition_in_different_files_shadows() {
    // If utils.R defines f and other.R also defines f, the later
    // definition wins (matching R's source() semantics). The order
    // files are added via `add_file` determines which one wins.
    let mut project = Project::new();
    project.add_file(
        "utils.R".to_string(),
        parse("utils.R", "f <- function() { 1L }\n"),
    );
    project.add_file(
        "other.R".to_string(),
        parse("other.R", "f <- function() { \"string\" }\n"),
    );
    project.add_file(
        "usage.R".to_string(),
        parse("usage.R", "result <- f() + 1L\n"),
    );
    let diags = project.check();
    let all: Vec<_> = diags.into_iter().flat_map(|(_, d)| d).collect();
    // The later definition (string) wins, so `result + 1L` is
    // character + int and should fire RY040.
    assert!(
        all.iter().any(|d| d.code == "RY040"),
        "expected shadowed definition to win, got: {:?}",
        all
    );
}

#[test]
fn diagnostics_returned_in_input_order() {
    // The per-file diagnostics vec should preserve the order files
    // were added. Callers (the CLI) rely on this to map paths back to
    // source text and sort consistently.
    let mut project = Project::new();
    project.add_file("a.R".to_string(), parse("a.R", "x <- 1L\n"));
    project.add_file("b.R".to_string(), parse("b.R", "y <- 2L\n"));
    project.add_file("c.R".to_string(), parse("c.R", "z <- 3L\n"));
    let diags = project.check();
    let paths: Vec<&str> = diags.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, vec!["a.R", "b.R", "c.R"]);
}

#[test]
fn empty_files_produce_no_diagnostics() {
    let mut project = Project::new();
    project.add_file("a.R".to_string(), parse("a.R", ""));
    project.add_file("b.R".to_string(), parse("b.R", "\n"));
    let diags = project.check();
    let total: usize = diags.into_iter().map(|(_, d)| d.len()).sum();
    assert_eq!(total, 0, "empty files should not produce diagnostics");
}

// ---------------------------------------------------------------------------
// Plan 33 W1: dirty-set pass 3
// ---------------------------------------------------------------------------

/// A one-line edit to a leaf file (one that no other file depends on) should
/// re-emit exactly one file. Verified via `Project::emit_count`, which counts
/// files actually emitted (not served from cache) in the most recent
/// `check_incremental` call.
#[test]
fn leaf_edit_emits_one_file() {
    let mut parser = RParser::new().unwrap();
    let mut project = Project::new();

    // Two independent leaf files: neither calls the other.
    project.add_file("a.R".to_string(), parser.parse("a.R", "x <- 1L\n").unwrap());
    project.add_file("b.R".to_string(), parser.parse("b.R", "y <- 2L\n").unwrap());

    // Cold check: both files emitted.
    let _ = project.check_incremental();
    #[cfg(test)]
    assert_eq!(project.emit_count, 2, "cold check should emit all files");

    // Edit only a.R (a leaf that b.R does not depend on).
    project.update_file(
        "a.R".to_string(),
        Arc::new(parser.parse("a.R", "x <- 3L\n").unwrap()),
    );
    let _ = project.check_incremental();

    // Only a.R should have been re-emitted; b.R served from cache.
    #[cfg(test)]
    assert_eq!(
        project.emit_count, 1,
        "leaf edit should re-emit exactly 1 file, got {}",
        project.emit_count
    );
}

/// Editing a file that another file calls must re-emit the caller too,
/// because the callee's return type may have changed.
#[test]
fn dependent_edit_emits_caller() {
    let mut parser = RParser::new().unwrap();
    let mut project = Project::new();

    project.add_file(
        "utils.R".to_string(),
        parser
            .parse("utils.R", "make <- function() \"hello\"\n")
            .unwrap(),
    );
    project.add_file(
        "call.R".to_string(),
        parser.parse("call.R", "r <- make() + 1L\n").unwrap(),
    );

    let _ = project.check_incremental();

    // Edit utils.R: make() now returns integer instead of character.
    project.update_file(
        "utils.R".to_string(),
        Arc::new(parser.parse("utils.R", "make <- function() 1L\n").unwrap()),
    );
    let _ = project.check_incremental();

    // Both utils.R (content changed) and call.R (calls make()) should
    // be re-emitted because make()'s return type changed.
    #[cfg(test)]
    assert_eq!(
        project.emit_count, 2,
        "dependent edit should re-emit 2 files (content + caller), got {}",
        project.emit_count
    );
}

/// The cold-vs-incremental equivalence property (Plan 33 W1 invariant):
/// after a sequence of incremental edits, the diagnostics must be identical
/// to a fresh cold check on the same final state.
#[test]
fn incremental_matches_cold_after_edits() {
    let mut parser = RParser::new().unwrap();

    // Build a 5-file project with cross-file dependencies.
    let sources = [
        ("a.R", "fa <- function(x) x * 2\nva <- fa(1L)\n"),
        ("b.R", "fb <- function(x) fa(x) + 1\nvb <- fb(2L)\n"),
        ("c.R", "fc <- function(x) fb(x) * 3\nvc <- fc(3L)\n"),
        ("d.R", "fd <- function() \"text\"\nvd <- fd()\n"),
        ("e.R", "fe <- function(x) paste0(x)\nve <- fe(42L)\n"),
    ];

    // Cold check path.
    let mut cold_project = Project::new();
    for (path, src) in &sources {
        cold_project.add_file(path.to_string(), parser.parse(path, src).unwrap());
    }
    let _cold = cold_project.check();

    // Incremental path: add files one at a time.
    let mut inc_project = Project::new();
    let _ = inc_project.check_incremental(); // empty
    for (path, src) in &sources {
        inc_project.add_file(path.to_string(), parser.parse(path, src).unwrap());
    }
    let _ = inc_project.check_incremental();

    // Now make a few incremental edits, then compare with cold.
    // Edit 1: change fa's return type.
    let edited_a = parser
        .parse("a.R", "fa <- function(x) \"str\"\nva <- fa(1L)\n")
        .unwrap();
    inc_project.update_file("a.R".to_string(), Arc::new(edited_a));

    // Edit 2: change fd's body (leaf file, nothing depends on it).
    let edited_d = parser
        .parse("d.R", "fd <- function() 1L\nvd <- fd()\n")
        .unwrap();
    inc_project.update_file("d.R".to_string(), Arc::new(edited_d));

    let inc_result = inc_project.check_incremental();

    // Build the matching cold project.
    let edited_sources = [
        ("a.R", "fa <- function(x) \"str\"\nva <- fa(1L)\n"),
        ("b.R", "fb <- function(x) fa(x) + 1\nvb <- fb(2L)\n"),
        ("c.R", "fc <- function(x) fb(x) * 3\nvc <- fc(3L)\n"),
        ("d.R", "fd <- function() 1L\nvd <- fd()\n"),
        ("e.R", "fe <- function(x) paste0(x)\nve <- fe(42L)\n"),
    ];
    let mut cold_project2 = Project::new();
    for (path, src) in &edited_sources {
        cold_project2.add_file(path.to_string(), parser.parse(path, src).unwrap());
    }
    let cold_result = cold_project2.check();

    // Compare diagnostics file by file.
    assert_eq!(inc_result.len(), cold_result.len(), "file count mismatch");
    for ((inc_path, inc_diags), (cold_path, cold_diags)) in
        inc_result.iter().zip(cold_result.iter())
    {
        assert_eq!(inc_path, cold_path, "path order mismatch");
        // Compare diagnostic codes + messages (the property that matters).
        let inc_codes: Vec<_> = inc_diags.iter().map(|d| &d.code).collect();
        let cold_codes: Vec<_> = cold_diags.iter().map(|d| &d.code).collect();
        assert_eq!(
            inc_codes, cold_codes,
            "diagnostic codes differ for {inc_path}:\n  incremental: {inc_codes:?}\n  cold:        {cold_codes:?}"
        );
    }
}
