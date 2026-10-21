use super::*;

#[test]
fn slot_access_and_nested_replacement_forget_caller_facts() {
    for operation in [
        "x@foo",
        "x@foo <- 2L",
        "x@foo$bar$baz <- 2L",
        "x$foo@bar <- 2L",
    ] {
        let source = format!(
            "`@` <- function(object, name) list(bar=list(baz=1L))\n\
             `@<-` <- function(object, name, value) 11L\n\
             x <- list(foo=list(bar=list(baz=1L)))\n\
             y <- list(1L)\n{operation}\nidentical(x[1L], 11L)\n"
        );
        let (diagnostics, scope) = check_with_scope(&source);
        assert!(diagnostics.is_empty(), "{operation}: {diagnostics:?}");
        for name in ["x", "y"] {
            assert_eq!(
                scope.get(name).unwrap().mode,
                Mode::Opaque,
                "{operation}: {name}"
            );
            assert!(!scope.has_list_origin(name), "{operation}: {name}");
        }
        assert!(scope.uncertainty.effects);
    }
    let diagnostics =
        check("`@<-` <- function(object, name, value) 11L; x <- list(); (x@foo <- 4:6) + 'bad'");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040")
    );
    let diagnostics = check("not_bound@slot_name");
    assert!(
        diagnostics.iter().any(
            |diagnostic| diagnostic.code == "RY010" && diagnostic.message.contains("not_bound")
        )
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("slot_name"))
    );
}

#[test]
fn native_slot_access_preserves_unrelated_diagnostics() {
    let diagnostics =
        check("x <- 1:3; x@.Data; y <- c(0L, 1L); if (length(y >= 2L)) 1L; not_bound");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY093")
    );
    assert!(
        diagnostics.iter().any(
            |diagnostic| diagnostic.code == "RY010" && diagnostic.message.contains("not_bound")
        )
    );
    let (_, scope) = check_with_scope("x <- 1:3; y <- list(1L); x@.Data");
    assert_eq!(scope.get("y").unwrap().mode, Mode::List);
    assert!(scope.has_list_origin("y"));
    let (_, scope) = check_with_scope("x <- list(); y <- list(1L); x@foo <- 2L");
    assert_eq!(scope.get("x").unwrap().mode, Mode::Opaque);
    assert!(!scope.has_list_origin("x"));
    assert_eq!(scope.get("y").unwrap().mode, Mode::List);
}

#[test]
fn data_and_load_make_later_bindings_uncertain() {
    let (data_diagnostics, data_scope) = check_with_scope("data(api)\nprint(apipop)\n");
    assert!(
        data_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010")
    );
    assert_eq!(data_scope.get("api"), Some(&RType::unknown()));

    let load_diagnostics = check("load(\"f.rda\")\nprint(whatever)\n");
    assert!(
        load_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010")
    );

    let source_diagnostics = check("source(\"generated.R\")\nprint(from_source)\n");
    assert!(
        source_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "top-level source() populates .GlobalEnv, which is the current scope"
    );
}

#[test]
fn import_from_applies_metadata_only_to_the_imported_binding() {
    let diagnostics = check_with(
        "df <- data.frame(x = 1L)\nselect(df, x)\nmutate(df, created = missing_name)\n",
        |c| {
            c.set_external_bindings(HashSet::from(["select".to_string()]));
            c.set_imported_from(HashMap::from([("select".to_string(), "dplyr".to_string())]));
        },
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("missing_name")
    }));
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`x`")
        })
    );
}

#[test]
fn typed_package_constants_are_values_not_functions() {
    let file = parse_file(
        "test.R",
        "value <- na_chr\nbad <- na_chr()\nqualified_bad <- rlang::na_chr()\n",
    );
    let mut checker = Checker::new("test.R");
    checker.set_loaded(HashSet::from(["rlang".to_string()]));
    let (diagnostics, scope) = checker.check_with_scope(&file);

    assert_eq!(
        scope.get("value").map(|ty| &ty.mode),
        Some(&Mode::Character)
    );
    assert_eq!(scope.get("value").map(|ty| ty.length), Some(Length::One));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY070" && diagnostic.message.contains("`na_chr`")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY070" && diagnostic.message.contains("`rlang::na_chr`")
    }));
}

#[test]
fn import_from_preserves_typed_package_constant() {
    let file = parse_file("test.R", "value <- na_int\n");
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from(["na_int".to_string()]));
    checker.set_imported_from(HashMap::from([("na_int".to_string(), "rlang".to_string())]));
    let (diagnostics, scope) = checker.check_with_scope(&file);

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("value").map(|ty| &ty.mode), Some(&Mode::Integer));
    assert_eq!(scope.get("value").map(|ty| ty.length), Some(Length::One));
}

#[test]
fn unknown_user_infix_quotes_both_operands_and_returns_unknown() {
    let (diagnostics, scope) =
        check_with_scope("result <- missing_left %custom% missing_right\nafter <- result + 1L\n");
    for name in ["missing_left", "missing_right"] {
        assert!(
            diagnostics.iter().all(|diagnostic| {
                diagnostic.code != "RY010" || !diagnostic.message.contains(name)
            }),
            "unknown infix must quote `{name}`: {diagnostics:?}"
        );
    }
    assert_eq!(scope.get("result").map(|ty| &ty.mode), Some(&Mode::Opaque));
}

#[test]
fn zeallot_destructuring_binds_nested_pattern_symbols() {
    let file = parse_file(
        "test.R",
        "c(first, c(second, third)) %<-% make_value()\nout <- first + second + third\n",
    );
    let mut checker = Checker::new("test.R");
    checker.set_loaded(HashSet::from(["zeallot".to_string()]));
    let (diagnostics, scope) = checker.check_with_scope(&file);
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || !["first", "second", "third"]
                    .iter()
                    .any(|name| diagnostic.message.contains(name))
        }),
        "destructured symbols should be bound: {diagnostics:?}"
    );
    for name in ["first", "second", "third"] {
        assert!(scope.get(name).is_some(), "{name} should be in scope");
    }
}

#[test]
fn future_import_enables_mirrored_destructuring() {
    let file = parse_file("test.R", "make_value() %->% c(left, right)\nleft + right\n");
    let mut checker = Checker::new("test.R");
    checker.set_imported_from(HashMap::from([("%->%".to_string(), "future".to_string())]));
    let (diagnostics, scope) = checker.check_with_scope(&file);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(scope.get("left").is_some());
    assert!(scope.get("right").is_some());
}

#[test]
fn scalar_string_subset_of_atomic_vector_has_length_one() {
    let (diags, scope) = check_with_scope("x <- c(first = 1L, second = 2L)\ny <- x[\"first\"]\n");
    assert!(diags.is_empty(), "{diags:?}");
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.mode, Mode::Integer);
    assert_eq!(y.length, Length::One);
}

#[test]
fn data_frame_scalar_column_subset_drops_to_column_type() {
    let (diags, scope) = check_with_scope(
        "d <- data.frame(a = 1:10, b = 11:20)\nm <- d[, 1]\nn <- d[, \"a\"]\nkept <- d[, 1, drop = FALSE]\n",
    );
    assert!(diags.is_empty(), "{diags:?}");
    for name in ["m", "n"] {
        let column = scope.get(name).expect("selected column should be bound");
        assert_eq!(column.mode, Mode::Integer, "{name}: {column:?}");
        assert_eq!(column.length, Length::Known(10), "{name}: {column:?}");
    }
    let kept = scope
        .get("kept")
        .expect("drop = FALSE result should be bound");
    assert_eq!(kept.mode, Mode::List, "{kept:?}");
    assert_eq!(kept.length, Length::One, "{kept:?}");
    assert!(kept.class.contains("data.frame"), "{kept:?}");
}

#[test]
fn literal_negative_scalar_subscript_preserves_exact_exclusion_length() {
    let (diagnostics, scope) =
        check_with_scope("x <- c(10, 20, 30)\ny <- x[-1]\nif (y > 1) print(1)\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY002"),
        "excluding one element from a known length-three vector leaves length two: {diagnostics:?}"
    );
    assert_eq!(
        scope.get("y").map(|value| value.length),
        Some(Length::Known(2))
    );
}

#[test]
fn list_subset_drops_stale_column_schema() {
    let (diagnostics, scope) =
        check_with_scope("x <- list(a = 1L, b = \"x\")\ny <- x[2]\nmissing <- y$a\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let y = scope.get("y").expect("subset should be bound");
    assert_eq!(y.length, Length::One);
    assert!(
        y.columns.is_none(),
        "subset must not retain the source schema"
    );
    assert_eq!(
        scope.get("missing").map(|value| &value.mode),
        Some(&Mode::Opaque),
        "without a transformed schema, missing name access stays conservative"
    );
}

#[test]
fn condition_union_zero_length_leaf_in_a_nested_union_dominates() {
    // The zero-length dominance recurses into nested union members,
    // mirroring `switch_expr_length_rejected`: a nested union carrying
    // a proven zero-length leaf (`logical<0> | logical<1>`, the
    // comparison join a find-or-NULL helper produces) errors on that
    // branch exactly like a direct zero-length member instead of being
    // silenced by a possibly-valid logical sibling of the OUTER union.
    // Union construction flattens (`union_of`), so no end-to-end source
    // route produces this shape today; this pins the type-level
    // contract directly, next to the flat end-to-end shapes in
    // `condition_union_zero_length_member_dominates_a_logical_sibling`.
    use crate::infer::{ConditionDiagnostic, condition_diagnostic};
    use std::sync::Arc;
    let comparison_join = RType::union(Arc::from(vec![
        RType::new(Mode::Logical, Length::Zero),
        RType::new(Mode::Logical, Length::One),
    ]));
    let nested = RType::union(Arc::from(vec![
        RType::new(Mode::Logical, Length::One),
        comparison_join,
    ]));
    assert!(
        matches!(
            condition_diagnostic(&nested),
            Some(ConditionDiagnostic::Invalid)
        ),
        "a zero-length leaf inside a nested union member dominates the outer union's logical sibling"
    );
    // Without the zero-length leaf the same nesting stays silent:
    // every branch remains possibly valid.
    let quiet = RType::union(Arc::from(vec![
        RType::new(Mode::Logical, Length::One),
        RType::union(Arc::from(vec![
            RType::new(Mode::Logical, Length::One),
            RType::new(Mode::Logical, Length::Unknown),
        ])),
    ]));
    assert!(
        condition_diagnostic(&quiet).is_none(),
        "a nested union without a zero-length leaf keeps the union silence"
    );
}

#[test]
fn vector_string_subset_preserves_non_scalar_length() {
    let (diags, scope) =
        check_with_scope("x <- c(first = 1L, second = 2L)\ny <- x[c(\"first\", \"second\")]\n");
    assert!(diags.is_empty(), "{diags:?}");
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.mode, Mode::Integer);
    assert_ne!(y.length, Length::One);
}

#[test]
fn s4_terra_named_vector_dispatch_fixture_is_clean() {
    let diagnostics = check(include_str!("../../testdata/ok_s4_terra_named_vector.R"));
    assert!(
        diagnostics.is_empty(),
        "S4 dispatch should preserve the method's named-vector result: {diagnostics:?}"
    );
}

// ---- Cross-file variable resolution (known_vars) ---------------

#[test]
fn s4_generics_and_methods_resolve_cross_file() {
    let mut project = Project::new();
    project.add_file(
        "generic.R".to_string(),
        parse_file(
            "generic.R",
            "setGeneric(\"render\", function(x) standardGeneric(\"render\"))\n",
        ),
    );
    project.add_file(
        "method.R".to_string(),
        parse_file(
            "method.R",
            "setClass(\"Document\", representation(id = \"numeric\"))\nsetMethod(\"render\", \"Document\", function(x) c(title = \"ok\"))\nd <- new(\"Document\")\nout <- render(d)\nout[[\"title\"]]\n",
        ),
    );
    let diagnostics: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        diagnostics.is_empty(),
        "cross-file S4 failed: {diagnostics:?}"
    );
}

/// Build a tempdir package project (DESCRIPTION at the root, files
/// relative to it), check it, and return the per-file diagnostics.
/// Mirrors how the project-mode fixtures lay out a real package.
fn project_with(files: &[(&str, &str)]) -> Vec<(String, Vec<Diagnostic>)> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
    let mut project = Project::new();
    for (path, src) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        let full = full.to_string_lossy().to_string();
        project.add_file(full.clone(), parse_file(&full, src));
    }
    project.check()
}

/// Whether `path` (an absolute path key from `Project::check`) ends
/// with the relative fixture path `rel` (written with `/` separators).
/// `Path::ends_with` compares components, so this matches regardless
/// of the platform's path separator.
fn path_matches(path: &str, rel: &str) -> bool {
    std::path::Path::new(path).ends_with(rel.split('/').collect::<std::path::PathBuf>())
}

#[test]
fn cross_file_top_level_variables_resolve_between_files() {
    // File A defines a top-level binding whose RHS is not a function
    // literal (a constant, an opaque ggproto() call result, a list
    // constructor), so it is not in `fns`; file B references it.
    // Without `known_vars`, B would emit RY010 on the name. With it,
    // the reference resolves and the reading file stays clean.
    for (label, definition, use_src) in [
        ("literal constant", "my_const <- 42\n", "x <- my_const\n"),
        (
            "backtick-bound constant",
            "`my_const` <- 42\n",
            "read_constant <- function() my_const + 1\n",
        ),
        (
            "backtick-bound function as value",
            "`helper` <- function(x) x + 1\n",
            "read_helper <- function() { f <- helper; f(1) }\n",
        ),
        (
            "opaque call result",
            "GeomRect <- ggproto(\"GeomRect\", Geom, draw = function() NULL)\n",
            "x <- GeomRect\n",
        ),
        (
            "list constructor",
            "config <- list(timeout = 30, retries = 3)\n",
            "t <- config$timeout\n",
        ),
    ] {
        let diagnostics = project_with(&[("R/a.R", definition), ("R/b.R", use_src)]);
        let reader = diagnostics
            .iter()
            .find(|(path, _)| path_matches(path, "R/b.R"))
            .map(|(_, diags)| diags)
            .unwrap_or_else(|| panic!("{label}: R/b.R diagnostics missing"));
        assert!(
            reader.iter().all(|d| d.code != "RY010"),
            "cross-file {label} should resolve without RY010, got {reader:?}"
        );
    }
}

#[test]
fn open_search_path_does_not_leak_between_project_files() {
    let mut project = Project::new();
    project.add_file(
        "attached.R".to_string(),
        parse_file("attached.R", "library(fakepkg123)\nfrom_fakepkg\n"),
    );
    project.add_file(
        "isolated.R".to_string(),
        parse_file("isolated.R", "must_stay_unbound\n"),
    );
    let diagnostics = project.check();
    let attached_diagnostics = diagnostics
        .iter()
        .find(|(path, _)| path == "attached.R")
        .map(|(_, diagnostics)| diagnostics)
        .expect("attached file diagnostics");
    assert!(
        attached_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010")
    );

    let isolated_diagnostics = diagnostics
        .iter()
        .find(|(path, _)| path == "isolated.R")
        .map(|(_, diagnostics)| diagnostics)
        .expect("isolated file diagnostics");
    assert!(isolated_diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("must_stay_unbound")
    }));
}

#[test]
fn scripts_share_top_level_known_vars() {
    // Scripts under inst/ (outside the package's R/ code) still
    // share top-level bindings with the rest of the project.
    let diagnostics = project_with(&[
        ("inst/examples/a.R", "h <- list(pre = 1L)\n"),
        ("inst/examples/b.R", "x <- h[[\"pre\"]]\n"),
    ]);
    let reader = diagnostics
        .iter()
        .find(|(path, _)| path_matches(path, "inst/examples/b.R"))
        .map(|(_, diags)| diags)
        .expect("reading script diagnostics");
    assert!(
        reader.iter().all(|d| d.code != "RY010"),
        "sourced scripts must share top-level bindings: {reader:?}"
    );
}

#[test]
fn testthat_script_sees_package_library_functions() {
    // tests/testthat code retains access to the package's R/
    // library functions even though it lives outside R/.
    let diagnostics = project_with(&[
        ("R/hidden.R", "hidden <- function() 1L\n"),
        ("tests/testthat/test-hidden.R", "x <- hidden()\n"),
    ]);
    let test = diagnostics
        .iter()
        .find(|(path, _)| path_matches(path, "tests/testthat/test-hidden.R"))
        .map(|(_, diags)| diags)
        .expect("testthat script diagnostics");
    assert!(
        test.iter().all(|d| d.code != "RY010"),
        "testthat code must retain access to package functions: {test:?}"
    );
}

#[test]
fn genuinely_undefined_variable_still_triggers_ry010() {
    // Sanity: a name that is NOT defined in any file of the project
    // (and is not a typeshed function or dataset) must still emit
    // RY010. `known_vars` only suppresses diagnostics for names we
    // have actually seen assigned.
    let diagnostics = project_with(&[("a.R", "x <- totally_undefined_thing\n")]);
    let all: Vec<_> = diagnostics.into_iter().flat_map(|(_, d)| d).collect();
    assert!(
        all.iter().any(|d| d.code == "RY010"),
        "genuinely undefined variable should still trigger RY010, got {all:?}"
    );
}

#[test]
fn dplyr_filter_and_stats_filter_resolve_differently() {
    // `dplyr::filter(df, ...)` resolves
    // against the dplyr typeshed (data.frame return) while
    // `stats::filter(x, ...)` resolves against base's stats `filter`
    // (a time-series filter, opaque). The two must NOT be confused.
    let (_, scope) = check_with_scope("df <- mtcars\na <- dplyr::filter(df, mpg > 20)\n");
    let a = scope.get("a").expect("a bound");
    assert!(
        a.class.contains("data.frame"),
        "dplyr::filter should return a data.frame-classed value, got class {:?}",
        a.class
    );
    let (_, scope2) = check_with_scope("b <- stats::filter(1:10, rep(1, 3))\n");
    let b = scope2.get("b").expect("b bound");
    assert!(
        !b.class.contains("data.frame"),
        "stats::filter must NOT be data.frame-classed, got class {:?}",
        b.class
    );
}

#[test]
fn namespace_qualified_call_resolves_via_typeshed() {
    // `stats::rnorm(10)` should resolve through the typeshed as
    // `rnorm` (prefix stripped) and return a double vector, with no
    // RY010. We assert both the diagnostic silence AND the inferred
    // return type.
    let (diags, scope) = check_with_scope("x <- stats::rnorm(10)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "qualified call `stats::rnorm(10)` should not emit RY010, got {:?}",
        diags
    );
    let t = scope.get("x").expect("x should be bound after assignment");
    assert!(
        matches!(t.mode, Mode::Double),
        "stats::rnorm(10) should infer as Double, got {:?}",
        t
    );
}

#[test]
fn external_unenumerable_marker_suppresses_ry010_for_its_file() {
    // The `SERIALIZED_BINDINGS_UNENUMERABLE` marker still flows from the
    // unstubbed-attached-package path (tests/examples with unstubbed
    // Suggests). It must suppress RY010 for THAT file only — never
    // project-wide. The over-cap fix removed the package-global sysdata route;
    // this is the remaining legitimate, file-local open search path.
    let diags = check_with("x <- genuinely_unbound_name\n", |c| {
        c.set_external_bindings(HashSet::from([
            ry_core::SERIALIZED_BINDINGS_UNENUMERABLE.to_string()
        ]));
    });
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "the unenumerable marker should suppress RY010 for its file: {diags:?}"
    );
}

#[test]
fn parameter_default_preserves_lexical_and_list_origin_markers() {
    // A defaulted parameter shadows a captured name while preserving its
    // lexical-function and list-origin markers.
    let mut scope = Scope::default();
    scope.insert("d", RType::new(Mode::List, Length::One));
    scope.insert_narrowed("d", RType::unknown());
    // Layer every marker the name can carry in a captured scope.
    scope.set_function_alias("d", "stats::median".to_string());
    scope.mark_lexical_function("d", Span::default());
    scope.mark_list_origin("d");

    scope.insert_parameter_default("d", RType::new(Mode::Integer, Length::One));

    assert_eq!(
        scope.get("d"),
        Some(&RType::new(Mode::Integer, Length::One)),
        "default value must be installed"
    );
    assert!(
        scope.is_lexical_function("d"),
        "lexical-function marker must survive a defaulted parameter"
    );
    assert!(
        scope.has_list_origin("d"),
        "list-origin marker must survive a defaulted parameter"
    );
    assert!(
        scope.function_alias("d").is_none(),
        "function alias must be cleared"
    );
    assert!(
        !scope.narrowed_bindings.contains("d"),
        "narrowed marker must be cleared"
    );
    assert!(
        scope.is_parameter("d") && scope.is_default_parameter("d"),
        "parameter and default-parameter markers must be set"
    );
}

/// `insert_narrowed` refines a binding without rebinding the name, so a
/// list-origin binding narrowed by a flow guard keeps its provenance: the
/// narrowed branch scope's marker survives into the branch merge, and a
/// subset of the narrowed value is still provably list-shaped for RY101.
#[test]
fn narrowed_binding_keeps_list_origin_marker() {
    let mut scope = Scope::default();
    scope.insert("parts", RType::new(Mode::List, Length::Unknown));
    scope.mark_list_origin("parts");

    scope.insert_narrowed("parts", RType::new(Mode::List, Length::One));

    assert_eq!(
        scope.get("parts"),
        Some(&RType::new(Mode::List, Length::One)),
        "narrowed type must be installed"
    );
    assert!(
        scope.has_list_origin("parts"),
        "list-origin marker must survive narrowing"
    );
    assert!(
        scope.narrowed_bindings.contains("parts"),
        "narrowed marker must be set"
    );

    // A genuine rebinding still clears the marker: `insert` is a real
    // assignment, not a refinement.
    scope.insert("parts", RType::new(Mode::Character, Length::Unknown));
    assert!(
        !scope.has_list_origin("parts"),
        "a plain rebinding clears list origin"
    );
}

// testthat block environments (#368). test_that(), describe(), and it()
// all route through test_code(), which evaluates the captured body via
// eval(code, new.env(parent = caller_env)) (verified in R): bindings
// created inside one block never reach a sibling block or the file top
// level, the parent chain stays readable, and `<<-` climbs past the
// fresh environment. The contract is callee-specific — an ordinary
// braced argument still evaluates in the caller (#350).

/// With testthat attached the callee identity is not proven; the
/// conservative write-through uncertainty leaves the loop variable as an
/// unknown file-level binding, which R's function-mode call-head lookup
/// skips in favor of the file-level function.
#[test]
fn attached_testthat_blocks_keep_sibling_call_heads_resolvable() {
    let source = "prob <- function(x) sum(x)\n\
                  test_that(\"block A\", {\n\
                    for (prob in list(1, 2)) NULL\n\
                  })\n\
                  test_that(\"block B\", {\n\
                    prob(1:2)\n\
                  })\n";
    let diagnostics = check_with(source, |c| {
        c.set_bare_loaded(HashSet::from(["testthat".to_string()]));
    });
    assert!(
        diagnostics.is_empty(),
        "an attached testthat must not flatten sibling blocks: {diagnostics:?}"
    );
}

/// Verified in R: `<<-` climbs past the per-test environment into the
/// file scope. The binding exists afterwards; only its type is unknown.
#[test]
fn superassignment_escapes_the_testthat_block() {
    let (diagnostics, scope) = check_with_scope("test_that(\"a\", {\n  esc <<- 1L\n})\nesc\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(
        scope.get("esc").is_some(),
        "a superassigned name must exist at the file level"
    );
}

/// Sequencing inside one test body is unchanged: local assignments stay
/// visible to later expectations in the same block.
#[test]
fn within_block_bindings_stay_visible_to_later_expectations() {
    let diagnostics = check(include_str!("../../testdata/ok_testthat_block_scope.R"));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

/// The runtime-verified oracle: sibling isolation, same-block shadowing,
/// parent-chain reads, and `<<-` escape, with the exact surviving
/// diagnostics pinned.
#[test]
fn testthat_block_environment_oracle() {
    let diagnostics = check(include_str!(
        "../../testdata/oracle/testthat_block_environment.R"
    ));
    let codes: Vec<&str> = diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, vec!["RY070", "RY040"], "{diagnostics:?}");
}
