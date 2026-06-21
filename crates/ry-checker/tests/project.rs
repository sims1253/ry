//! Multi-file project tests. Verifies that functions and S3 methods
//! defined in one file are visible when checking another file in the
//! same project.

use ry_checker::Project;
use ry_core::RParser;

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
