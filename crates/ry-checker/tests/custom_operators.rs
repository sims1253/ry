use ry_checker::Checker;
use ry_core::{Mode, RParser};

fn result_mode(source: &str) -> Mode {
    let file = RParser::new()
        .unwrap()
        .parse("operators.R", source)
        .unwrap();
    let (diagnostics, scope) = Checker::new("operators.R").check_with_scope(&file);
    assert!(diagnostics.is_empty(), "{source}\n{diagnostics:?}");
    scope.get("result").unwrap().mode
}

#[test]
fn literal_custom_operators_ignore_operands_and_keep_their_return_mode() {
    for (source, expected) in [
        (
            "`&` <- function(...) 'ok'; result <- missing_name & 1L",
            Mode::Character,
        ),
        (
            "`|` <- function(...) 1L; result <- missing_name | 1L",
            Mode::Integer,
        ),
        (
            "'+' <- function(...) 'ok'; result <- missing_name + 1L",
            Mode::Character,
        ),
        (
            "`+` <- function(...) 'ok'; result <- 'a' + missing_name",
            Mode::Character,
        ),
        (
            "`/` <- function(e1, e2) 1L; result <- 1L / 2L",
            Mode::Integer,
        ),
        (
            "`==` <- function(...) 1L; result <- NA == NA",
            Mode::Integer,
        ),
        (
            "lazy <- function(...) 'ok'; `+` <- lazy; result <- 'a' + missing_name",
            Mode::Character,
        ),
        (
            "`+` <- function(...) 'ok'; x <- 1L; ignored <- (x <- 'bad') + stop('unused'); result <- x * 2L",
            Mode::Integer,
        ),
        (
            "`+` <- function(...) 'ok'; result <- (`+` <- function(...) 1L) + 2L",
            Mode::Character,
        ),
    ] {
        assert_eq!(result_mode(source), expected);
    }
}

#[test]
fn custom_unknown_calls_forget_possible_operand_effects_without_eager_errors() {
    for source in [
        "`+` <- function(e1,e2) { force(e1); 1L }; ignored <- assign('*', function(...) 1L) + 2L; result <- 'a' * 1L",
        "callee <- function() 'old'; `+` <- function(e1,e2) { force(e1); 1L }; ignored <- assign('callee', function() 1L) + 2L; result <- callee() * 2L",
        "`+` <- function(e1,e2) { force(e1); 1L }; x <- 'old'; ignored <- (x <- 1L) + 2L; result <- x * 2L",
        "`+` <- function(e1,e2) { force(e1); 1L }; x <- 'old'; ignored <- assign('x', 1L) + 2L; result <- x * 2L",
        "f <- function(`+`) missing_name + 1L; result <- f",
        "`+` <- function(...) 'old'; f <- function() missing_name + 1L; `+` <- function(...) 1L; result <- f",
        r"`\x2b` <- function(...) 'ok'; result <- missing_name + 1L",
    ] {
        result_mode(source);
    }
}

#[test]
fn concrete_data_skips_function_lookup_without_resurrecting_stale_functions() {
    assert_eq!(result_mode("`+` <- 7L; result <- 1L + 2L"), Mode::Integer);
    assert_eq!(
        result_mode("`+` <- function(...) 'old'; `+` <- 7L; result <- 1L + 2L"),
        Mode::Opaque
    );
    assert_eq!(
        result_mode("`+` <- function() 'unused'; result <- 1L + 2L"),
        Mode::Opaque
    );
    result_mode("`+` <- function(...) 'ok'; f <- function() { `+` <- 7L; 'a' + 1L }; result <- f");
}

#[test]
fn uncertain_effects_survive_branch_loop_and_later_assignment() {
    let prefix = "`+` <- function(e1,e2) { force(e1); 1L }; x <- 'old'; ";
    for body in [
        "ignored <- TRUE && (assign('x', 1L) + 2L); result <- x * 2L",
        "ignored <- if (TRUE) assign('x', 1L) + 2L else 0L; result <- x * 2L",
        "if (TRUE) { ignored <- assign('x', 1L) + 2L }; result <- x * 2L",
        "for (i in 1L) { ignored <- assign('x', 1L) + 2L }; result <- x * 2L",
        "done <- FALSE; while (!done) { ignored <- assign('x', 1L) + 2L; done <- TRUE }; result <- x * 2L",
        "ignored <- assign('x', 1L) + 2L; x <- 2L; class(x) <- 'integer'; result <- x",
    ] {
        assert_eq!(result_mode(&format!("{prefix}{body}")), Mode::Opaque);
    }
}

#[test]
fn uncertain_custom_effects_do_not_export_later_reference_identity() {
    let source = "`+` <- function(e1,e2) { force(e1); 1L }; x <- 'old'; ignored <- assign('x', 1L) + 2L; x <- 2L; result <- x";
    let file = RParser::new()
        .unwrap()
        .parse("operators.R", source)
        .unwrap();
    let mut checker = Checker::new("operators.R");
    checker.enable_reference_capture();
    assert!(checker.check(&file).is_empty());
    let facts = checker.take_reference_facts();
    let reference = facts
        .references
        .iter()
        .find(|reference| reference.span.start == source.len() - 1)
        .unwrap();
    assert_eq!(
        reference.resolution,
        ry_checker::ReferenceResolution::Unsupported
    );
    assert!(reference.definition.is_none());
    assert!(reference.type_at_reference.is_none());
}

#[test]
fn project_refinement_carries_escaped_operator_formals_and_resets_after_edits() {
    let mut project = ry_checker::Project::new();
    let mut parser = RParser::new().unwrap();
    project.add_file(
        "functions.R".into(),
        parser
            .parse("functions.R", r"f <- function(`\x2b`) 1L + 2L")
            .unwrap(),
    );
    project.add_file(
        "use.R".into(),
        parser
            .parse("use.R", "out <- f(function(...) 'ok') == 'ok'")
            .unwrap(),
    );
    let diagnostics = project.check_incremental();
    assert!(
        diagnostics
            .iter()
            .all(|(_, diagnostics)| diagnostics.is_empty()),
        "{diagnostics:?}"
    );
    project.update_file(
        "functions.R".into(),
        std::sync::Arc::new(
            parser
                .parse("functions.R", "f <- function(z) 1L + 2L")
                .unwrap(),
        ),
    );
    assert!(
        project
            .check_incremental()
            .iter()
            .flat_map(|(_, diagnostics)| diagnostics)
            .any(|diagnostic| diagnostic.code == "RY033")
    );
}

#[test]
fn escaped_operator_environment_changes_refresh_unrelated_files_and_returns() {
    let sources = [
        ("mask.R", "f <- function(z) 0L"),
        ("helper.R", "g <- function() 1L + 2L"),
        ("use.R", "out <- g() == 'ok'"),
    ];
    let make_project = |mask: &str| {
        let mut project = ry_checker::Project::new();
        let mut parser = RParser::new().unwrap();
        for (path, source) in sources {
            let source = if path == "mask.R" { mask } else { source };
            project.add_file(path.into(), parser.parse(path, source).unwrap());
        }
        project
    };
    let mut warm = make_project(sources[0].1);
    assert!(
        warm.check_incremental()
            .iter()
            .flat_map(|(_, diagnostics)| diagnostics)
            .any(|diagnostic| diagnostic.code == "RY033")
    );
    for mask in [r"f <- function(`\x2b`) 0L", sources[0].1] {
        warm.update_file(
            "mask.R".into(),
            std::sync::Arc::new(RParser::new().unwrap().parse("mask.R", mask).unwrap()),
        );
        let warm_diagnostics = warm.check_incremental();
        let cold_diagnostics = make_project(mask).check();
        assert_eq!(warm_diagnostics, cold_diagnostics, "mask source: {mask}");
    }
}
