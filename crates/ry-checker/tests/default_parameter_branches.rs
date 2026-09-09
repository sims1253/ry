//! A defaulted parameter's recorded type describes only the omitted-argument
//! call shape (`Checker::diagnostic_parameter_type`). These fixtures pin the
//! package-level consequence for issue #343: ggplot2's `geom_boxplot` gains a
//! `character` type for `position` because a testthat call site omits it, and
//! the else branch of `if (is.character(position))` kept that mode for `$`.
use ry_checker::Project;
use ry_config::Config;
use ry_core::RParser;

/// The `geom_boxplot` shape from ggplot2 R/geom-boxplot.R:163-167, including
/// the then-branch conditional reassignment named in the issue.
const GEOM_SOURCE: &str = r#"
geom_boxplot <- function(position = "dodge2", varwidth = FALSE) {
  if (is.character(position)) {
    if (varwidth == TRUE) position <- position_dodge2(preserve = "single")
  } else {
    if (identical(position$preserve, "total") & varwidth == TRUE) {
      position$preserve <- "single"
    }
  }
  position
}
"#;

fn check_package(test_file: &str) -> Vec<(String, Vec<ry_checker::Diagnostic>)> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("R")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests").join("testthat")).unwrap();
    std::fs::write(
        dir.path().join("DESCRIPTION"),
        "Package: fixture\nVersion: 0.0.0\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("R").join("geom.R"), GEOM_SOURCE).unwrap();
    std::fs::write(
        dir.path()
            .join("tests")
            .join("testthat")
            .join("test-geom.R"),
        test_file,
    )
    .unwrap();
    let geom_path = dir.path().join("R").join("geom.R");
    let test_path = dir
        .path()
        .join("tests")
        .join("testthat")
        .join("test-geom.R");
    let mut parser = RParser::new().unwrap();
    let geom_file = parser
        .parse(geom_path.to_str().unwrap(), GEOM_SOURCE)
        .unwrap();
    let test_file_parsed = parser
        .parse(test_path.to_str().unwrap(), test_file)
        .unwrap();
    let context = ry_workspace::resolve_workspace_context(
        dir.path(),
        &Config::default(),
        ry_workspace::ResolutionEnvironment {
            files: vec![&geom_file, &test_file_parsed],
            user_stubs: &Default::default(),
        },
    )
    .unwrap();
    let mut project = Project::new();
    project.add_file(geom_path.to_string_lossy().into(), geom_file);
    project.add_file(test_path.to_string_lossy().into(), test_file_parsed);
    project.set_loaded(context.attached_packages);
    project.set_bare_loaded(context.bare_bindings);
    project.set_external_bindings(context.external_bindings);
    project.set_imported_from(context.imported_bindings);
    project.set_external_s3_methods(context.s3_methods);
    project.check()
}

fn dollar_diagnostics(results: &[(String, Vec<ry_checker::Diagnostic>)]) -> usize {
    results
        .iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .filter(|diagnostic| diagnostic.code == "RY061")
        .count()
}

#[test]
fn omitting_testthat_call_does_not_type_the_else_branch_as_the_default() {
    let results =
        check_package("test_that(\"shape\", {\n  p <- geom_boxplot()\n  expect_true(TRUE)\n})\n");
    assert_eq!(
        dollar_diagnostics(&results),
        0,
        "the else branch of is.character(position) must not keep the default's character mode: {results:?}"
    );
}

#[test]
fn mixed_testthat_calls_do_not_type_the_else_branch_as_the_default() {
    let results = check_package(
        "test_that(\"shape\", {\n  p1 <- geom_boxplot(position = \"dodge\")\n  p2 <- geom_boxplot()\n})\n",
    );
    assert_eq!(
        dollar_diagnostics(&results),
        0,
        "a supplied character call site plus an omitting one must not leave the default's mode in the else branch: {results:?}"
    );
}

#[test]
fn supplying_testthat_calls_only_stay_silent() {
    let results = check_package(
        "test_that(\"shape\", {\n  p1 <- geom_boxplot(position = \"dodge\")\n  p2 <- geom_boxplot(position = position_dodge2())\n})\n",
    );
    assert_eq!(
        dollar_diagnostics(&results),
        0,
        "supplying call sites never typed the parameter from its default: {results:?}"
    );
}
