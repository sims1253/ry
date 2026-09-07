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
        ("direct.R", "direct <- 1L == 'ok'"),
        ("condition.R", "if (g()) 1L"),
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

#[test]
fn s3_registration_does_not_hide_primitive_diagnostics_or_real_custom_bindings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("R")).unwrap();
    std::fs::write(
        dir.path().join("DESCRIPTION"),
        "Package: fixture\nVersion: 0.0.0\n",
    )
    .unwrap();
    let path = dir.path().join("R/code.R");
    for (namespace, source, expect_primitive_error) in [
        (
            "S3method(\"+\", foo)\n",
            "`+.foo` <- function(e1,e2) e1; out <- 'bad' + 1L",
            true,
        ),
        (
            "S3method(\"+\", foo)\nimportFrom(otherpkg, \"+\")\n",
            "out <- missing_name + 1L",
            false,
        ),
        (
            "S3method(\"+\", foo)\n",
            "`+` <- function(...) 'ok'; out <- missing_name + 1L",
            false,
        ),
        (
            "S3method(grid.draw, foo)\nimport(grid)\n",
            "draw <- grid.draw; out <- lapply(list(), grid.draw)",
            false,
        ),
    ] {
        std::fs::write(dir.path().join("NAMESPACE"), namespace).unwrap();
        std::fs::write(&path, source).unwrap();
        let path = path.to_str().unwrap();
        let file = RParser::new().unwrap().parse(path, source).unwrap();
        let context = ry_workspace::resolve_workspace_context(
            dir.path(),
            &ry_config::Config::default(),
            ry_workspace::ResolutionEnvironment {
                files: vec![&file],
                user_stubs: &Default::default(),
            },
        )
        .unwrap();
        let mut project = ry_checker::Project::new();
        project.add_file(path.into(), file);
        project.set_loaded(context.attached_packages);
        project.set_bare_loaded(context.bare_bindings);
        project.set_external_bindings(context.external_bindings);
        project.set_imported_from(context.imported_bindings);
        project.set_external_s3_methods(context.s3_methods);
        let diagnostics = project.check();
        if expect_primitive_error {
            assert!(
                diagnostics
                    .iter()
                    .flat_map(|(_, diagnostics)| diagnostics)
                    .any(|diagnostic| diagnostic.code == "RY040")
            );
        } else {
            assert!(
                diagnostics
                    .iter()
                    .all(|(_, diagnostics)| diagnostics.is_empty()),
                "{diagnostics:?}"
            );
        }
    }
}

#[test]
fn unrelated_escaped_names_do_not_mask_operators() {
    for prefix in [
        r"`foo\`` <- 1L;",
        r"f <- function(`foo\``) 1L;",
        r"globalVariables('foo\\bar');",
    ] {
        let source = format!("{prefix} out <- 'bad' + 1L; comparison <- 1L == 'ok'");
        let file = RParser::new().unwrap().parse("escaped.R", &source).unwrap();
        let mut checker = Checker::new("escaped.R");
        let diagnostics = checker.check(&file);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY040"),
            "{source}: {diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY033"),
            "{source}: {diagnostics:?}"
        );
    }
    for (prefix, operator) in [
        (r"`\053` <- function(...) 'ok';", "+"),
        (r"`<\x3d` <- function(...) 'ok';", "<="),
    ] {
        result_mode(&format!("{prefix} result <- missing_name {operator} 1L"));
    }
}
