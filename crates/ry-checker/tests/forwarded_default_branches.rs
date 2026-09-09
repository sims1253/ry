//! Cross-file package controls for the forwarded-default invalidation
//! (#342): the omitting call site lives in a testthat runner file, as in
//! ggplot2 (`compute_bins` / `bin_breaks_bins` in `R/bin.R`, omitting calls
//! in `tests/testthat/test-bin.R`).
use ry_checker::Project;
use ry_config::Config;
use ry_core::RParser;

const FUNCTIONS: &str = r#"
callee <- function(x_range, bins = 30) {
  if (bins == 1) 1 else 2
}
"#;

const REBINDING_CALLER: &str = r#"
caller <- function(x, bins = NULL) {
  bins <- allow_lambda(bins)
  callee(range, bins)
}
"#;

const FORWARDING_CALLER: &str = r#"
caller <- function(x, bins = NULL) {
  callee(range, bins)
}
"#;

fn check_package(caller: &str, test_calls: &str) -> Vec<(String, Vec<ry_checker::Diagnostic>)> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("R")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests").join("testthat")).unwrap();
    std::fs::write(
        dir.path().join("DESCRIPTION"),
        "Package: fixture\nVersion: 0.0.0\n",
    )
    .unwrap();
    let functions_path = dir.path().join("R").join("functions.R");
    let caller_path = dir.path().join("R").join("caller.R");
    let test_path = dir
        .path()
        .join("tests")
        .join("testthat")
        .join("test-calls.R");
    std::fs::write(&functions_path, FUNCTIONS).unwrap();
    std::fs::write(&caller_path, caller).unwrap();
    std::fs::write(&test_path, test_calls).unwrap();
    let mut parser = RParser::new().unwrap();
    let functions_file = parser
        .parse(functions_path.to_str().unwrap(), FUNCTIONS)
        .unwrap();
    let caller_file = parser.parse(caller_path.to_str().unwrap(), caller).unwrap();
    let test_file = parser
        .parse(test_path.to_str().unwrap(), test_calls)
        .unwrap();
    let context = ry_workspace::resolve_workspace_context(
        dir.path(),
        &Config::default(),
        ry_workspace::ResolutionEnvironment {
            files: vec![&functions_file, &caller_file, &test_file],
            user_stubs: &Default::default(),
        },
    )
    .unwrap();
    let mut project = Project::new();
    project.add_file(functions_path.to_string_lossy().into(), functions_file);
    project.add_file(caller_path.to_string_lossy().into(), caller_file);
    project.add_file(test_path.to_string_lossy().into(), test_file);
    project.set_loaded(context.attached_packages);
    project.set_bare_loaded(context.bare_bindings);
    project.set_external_bindings(context.external_bindings);
    project.set_imported_from(context.imported_bindings);
    project.set_external_s3_methods(context.s3_methods);
    project.check()
}

fn condition_diagnostics(results: &[(String, Vec<ry_checker::Diagnostic>)]) -> usize {
    results
        .iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .filter(|diagnostic| diagnostic.code == "RY001")
        .count()
}

#[test]
fn cross_file_rebinding_caller_does_not_forward_its_default() {
    let results = check_package(
        REBINDING_CALLER,
        "test_that(\"bins\", {\n  z <- caller(c(0, 1))\n})\n",
    );
    assert_eq!(
        condition_diagnostics(&results),
        0,
        "the rebinding replaces the caller default before the forwarded call: {results:?}"
    );
}

#[test]
fn cross_file_genuine_forwarding_keeps_the_true_positive() {
    let results = check_package(
        FORWARDING_CALLER,
        "test_that(\"bins\", {\n  z <- caller(c(0, 1))\n})\n",
    );
    assert_eq!(
        condition_diagnostics(&results),
        1,
        "an omitted call site really delivers the caller's NULL default to the callee: {results:?}"
    );
}
