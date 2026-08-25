use super::*;
use ry_core::RParser;

#[test]
fn attach_makes_later_search_path_bindings_uncertain() {
    let diagnostics = check(
        "before_attach\nattach(dataset)\nafter_attach\nf <- function() { nested_after_attach }\ng <- function() {\n  attach(local_data)\n  local_after_attach\n  inner <- function() nested_local_after_attach\n}\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("before_attach")
    }));
    for name in [
        "after_attach",
        "nested_after_attach",
        "local_after_attach",
        "nested_local_after_attach",
    ] {
        assert!(
            diagnostics.iter().all(|diagnostic| {
                diagnostic.code != "RY010" || !diagnostic.message.contains(name)
            }),
            "{name} should be uncertain after attach(): {diagnostics:?}"
        );
    }
}

#[test]
fn require_makes_later_search_path_bindings_uncertain() {
    let diagnostics = check("require(unstubbed_package)\nfrom_attached_package\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "unstubbed require() must open the search path: {diagnostics:?}"
    );
}

#[test]
fn open_scope_mutations_only_affect_later_bindings_and_nested_functions() {
    let diagnostics = check(
        "before_library\nlibrary(fakepkg123)\nafter_library\nf <- function() nested_after_library\nlibrary(package_name, character.only = TRUE)\nafter_dynamic_library\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("before_library")
    }));
    let known_package_diagnostics = check("library(dplyr)\nstill_not_a_dplyr_thing\n");
    assert!(known_package_diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("still_not_a_dplyr_thing")
    }));
    for name in [
        "after_library",
        "nested_after_library",
        "after_dynamic_library",
    ] {
        assert!(
            diagnostics.iter().all(|diagnostic| {
                diagnostic.code != "RY010" || !diagnostic.message.contains(name)
            }),
            "{name} should be uncertain after an open-scope library call: {diagnostics:?}"
        );
    }
}

#[test]
fn source_without_local_does_not_open_a_function_scope() {
    let diagnostics = check(
        "f <- function() {\n\
           source(\"generated.R\")\n\
           genuinely_missing\n\
         }\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("genuinely_missing")
    }));
}

#[test]
fn source_local_controls_use_normal_r_argument_matching() {
    for call in [
        "source(\"generated.R\", TRUE)",
        "source(exprs = expression(generated_binding <- TRUE), local = TRUE)",
        "source(local = TRUE, file = \"generated.R\")",
        "source(file = \"generated.R\", lo = TRUE)",
        "source(\"generated.R\", local = unknown_environment())",
    ] {
        let diagnostics = check(&format!(
            "f <- function() {{\n  {call}\n  generated_binding\n}}\n"
        ));
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "{call} should conservatively open the caller scope: {diagnostics:?}"
        );
    }

    for call in [
        "source(\"generated.R\", FALSE)",
        "source(file = \"generated.R\", local = FALSE)",
    ] {
        let diagnostics = check(&format!(
            "f <- function() {{\n  {call}\n  genuinely_missing\n}}\n"
        ));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("genuinely_missing")
        }));
    }
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
fn source_cpp_makes_later_scope_bindings_uncertain() {
    let diagnostics = check(
        "before_source\nRcpp::sourceCpp(\"generated.cpp\")\nafter_source\nf <- function() nested_after_source\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("before_source")
    }));
    for name in ["after_source", "nested_after_source"] {
        assert!(
            diagnostics.iter().all(|diagnostic| {
                diagnostic.code != "RY010" || !diagnostic.message.contains(name)
            }),
            "{name} should be uncertain after sourceCpp(): {diagnostics:?}"
        );
    }
}

#[test]
fn local_callable_does_not_inherit_stub_scope_effect() {
    let diagnostics = check(
        "factory <- function() function(x) x\nattach <- factory()\nattach(dataset)\nstill_missing\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("still_missing")
    }));
}

#[test]
fn shiny_test_server_injects_reactive_bindings() {
    let diagnostics = check(
        "library(shiny)\ntestServer(NULL, {\n  session$setInputs(x = 1L)\n  input$x\n  output$value\n})\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "testServer expr should receive session/input/output: {diagnostics:?}"
    );
}

#[test]
fn shiny_stub_marks_reactive_code_arguments_as_quoted() {
    let diagnostics = check(
        "library(shiny)\n\
         reactive(missing_reactive)\n\
         observe(missing_observe)\n\
         observeEvent(missing_event, missing_handler)\n\
         eventReactive(missing_event_reactive, missing_value_reactive)\n\
         isolate(missing_isolate)\n\
         renderText(missing_text)\n\
         renderPrint(missing_print)\n\
         renderUI(missing_ui)\n\
         renderPlot(missing_plot)\n\
         renderTable(missing_table)\n\
         renderDataTable(missing_data_table)\n\
         renderImage(missing_image)\n\
         testServer(NULL, missing_test_server)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "Shiny reactive code arguments must be quoted: {diagnostics:?}"
    );
}

#[test]
fn import_from_applies_metadata_only_to_the_imported_binding() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "test.R",
            "df <- data.frame(x = 1L)\nselect(df, x)\nmutate(df, created = missing_name)\n",
        )
        .unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from(["select".to_string()]));
    checker.set_imported_from(HashMap::from([("select".to_string(), "dplyr".to_string())]));
    checker.check(&file);

    assert!(checker.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("missing_name")
    }));
    assert!(
        checker.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`x`")
        })
    );
}

#[test]
fn typed_package_constants_are_values_not_functions() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "test.R",
            "value <- na_chr\nbad <- na_chr()\nqualified_bad <- rlang::na_chr()\n",
        )
        .unwrap();
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
fn local_callable_shadows_typed_package_constant() {
    let diagnostics = check(
        "library(rlang)\nna_chr <- function() \"ok\"\nvalue <- na_chr()\nf <- function(na_chr) na_chr()\nf(function() \"formal\")\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "lexical callables must win over attached package constants: {diagnostics:?}"
    );
}

#[test]
fn import_from_preserves_typed_package_constant() {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", "value <- na_int\n").unwrap();
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
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "test.R",
            "c(first, c(second, third)) %<-% make_value()\nout <- first + second + third\n",
        )
        .unwrap();
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
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse("test.R", "make_value() %->% c(left, right)\nleft + right\n")
        .unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_imported_from(HashMap::from([("%->%".to_string(), "future".to_string())]));
    let (diagnostics, scope) = checker.check_with_scope(&file);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(scope.get("left").is_some());
    assert!(scope.get("right").is_some());
}

#[test]
fn unresolved_destructuring_operator_quotes_its_operands() {
    let diagnostics = check("c(unbound) %<-% make_value()\n");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.code != "RY010" || !diagnostic.message.contains("unbound")
    }));
}

#[test]
fn embrace_resolves_bound_formal_outside_data_mask() {
    let diags = check("f <- function(x) {{ x }}\n");
    assert!(
        diags.is_empty(),
        "bound embrace should be silent: {diags:?}"
    );
}

#[test]
fn embrace_resolves_formal_in_function_scope_not_data_mask() {
    let diags = check(
        "library(dplyr)\nf <- function(df, value) mutate(df, out = {{ value }})\nf(data.frame(value = 1L), 2L)\n",
    );
    assert!(
        diags.is_empty(),
        "embrace should bypass mask columns: {diags:?}"
    );
}

#[test]
fn embrace_unbound_symbol_emits_ry010() {
    let diags = check("f <- function(x) {{ typo }}\n");
    assert!(
        diags
            .iter()
            .any(|diagnostic| diagnostic.code == "RY010" && diagnostic.message.contains("typo")),
        "unbound embrace should emit RY010: {diags:?}"
    );
}

#[test]
fn data_pronoun_resolves_known_column() {
    let diags = check("library(dplyr)\nmutate(data.frame(known = 1L), out = .data$known)\n");
    assert!(
        diags.is_empty(),
        "known `.data` column should resolve: {diags:?}"
    );
}

#[test]
fn data_pronoun_double_bracket_resolves_known_column() {
    let diags = check("library(dplyr)\nmutate(data.frame(known = 1L), out = .data[[\"known\"]])\n");
    assert!(
        diags.is_empty(),
        "known `.data` column should resolve: {diags:?}"
    );
}

#[test]
fn data_pronoun_missing_known_column_emits_ry060() {
    let diags = check("library(dplyr)\nmutate(data.frame(known = 1L), out = .data$missing)\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY060"),
        "missing `.data` column should emit RY060: {diags:?}"
    );
}

#[test]
fn data_pronoun_on_opaque_mask_is_silent() {
    let diags = check("library(dplyr)\nf <- function(df) mutate(df, out = .data$anything)\n");
    assert!(
        diags.is_empty(),
        "opaque `.data` access should be silent: {diags:?}"
    );
}

#[test]
fn env_pronoun_resolves_enclosing_binding() {
    let diags = check(
        "library(dplyr)\nf <- function(df, bound) mutate(df, out = .env$bound)\nf(data.frame(bound = 1L), 2L)\n",
    );
    assert!(
        diags.is_empty(),
        "`.env` should use lexical scope: {diags:?}"
    );
}

#[test]
fn env_pronoun_double_bracket_resolves_enclosing_binding() {
    let diags =
        check("library(dplyr)\nf <- function(df, bound) mutate(df, out = .env[[\"bound\"]])\n");
    assert!(
        diags.is_empty(),
        "`.env` should use lexical scope: {diags:?}"
    );
}

#[test]
fn env_pronoun_unbound_binding_emits_ry010() {
    let diags = check("library(dplyr)\nf <- function(df) mutate(df, out = .env$unbound)\n");
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("unbound")
        }),
        "unbound `.env` access should emit RY010: {diags:?}"
    );
}

#[test]
fn bare_data_pronoun_inside_mask_is_silent() {
    let diags = check("library(dplyr)\nmutate(data.frame(x = 1L), out = .data)\n");
    assert!(
        diags.is_empty(),
        "bare `.data` should be silent in a mask: {diags:?}"
    );
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
fn condition_union_with_a_valid_logical_member_is_silent() {
    let diagnostics = check("x <- if (runif(1) > 0.5) logical(0) else TRUE\nif (x) print(1)\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY001"),
        "a possibly-valid condition must not be reported as definitely invalid: {diagnostics:?}"
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

// ---- Cross-file variable resolution (known_vars) ---------------
#[test]
fn s4_terra_named_vector_dispatch_fixture_is_clean() {
    let diagnostics = check(include_str!("../../testdata/ok_s4_terra_named_vector.R"));
    assert!(
        diagnostics.is_empty(),
        "S4 dispatch should preserve the method's named-vector result: {diagnostics:?}"
    );
}

#[test]
fn s4_signature_form_dispatches() {
    let diagnostics = check(
        "setClass(\"C\", slots = c(value = \"numeric\"))\nsetMethod(\"labels\", signature(\"C\"), function(object) c(label = \"ok\"))\nx <- new(\"C\")\ny <- labels(x)\ny[[\"label\"]]\n",
    );
    assert!(
        diagnostics.is_empty(),
        "signature dispatch failed: {diagnostics:?}"
    );
}

#[test]
fn s4_named_signature_form_dispatches() {
    let diagnostics = check(
        "setClass(\"SpatExtent\", slots = c(value = \"numeric\"))\nsetMethod(\"as.vector\", signature(x = \"SpatExtent\"), function(x) c(xmin = 1))\nx <- new(\"SpatExtent\")\nv <- as.vector(x)\nv[[\"xmin\"]]\n",
    );
    assert!(
        diagnostics.is_empty(),
        "named signature dispatch failed: {diagnostics:?}"
    );
}

#[test]
fn s4_declared_and_undeclared_slot_access_and_assignment_are_silent() {
    let diagnostics = check(
        "setClass(\"C\", representation(value = \"numeric\"))\nx <- new(\"C\")\na <- x@value\nb <- x@undeclared\nx@value <- 1\nx@undeclared <- 2\n",
    );
    assert!(
        diagnostics.is_empty(),
        "S4 slots should be conservative: {diagnostics:?}"
    );
}

#[test]
fn named_vector_columns_survive_transpose_data_frame_constructors() {
    let diagnostics = check(
        "v <- c(alpha = 1, beta = 2)\na <- data.frame(t(v))\nb <- as.data.frame(t(v))\na$alpha\nb$beta\n",
    );
    assert!(
        diagnostics.is_empty(),
        "named columns were lost: {diagnostics:?}"
    );
}

#[test]
fn unknown_vector_names_do_not_fabricate_data_frame_schema() {
    let diagnostics =
        check("make_row <- function(v) data.frame(t(v))\nrow <- make_row(c(1, 2))\nrow$anything\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY060"),
        "unknown names must produce an opaque data-frame schema: {diagnostics:?}"
    );
}

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

#[test]
fn cross_file_literal_variable_resolves() {
    // File A defines a top-level constant `my_const <- 42`; file B
    // references it. Without `known_vars`, B would emit RY010 on
    // `my_const`. With `known_vars`, the reference resolves to
    // opaque and no diagnostic fires.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("R")).unwrap();
    std::fs::write(dir.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
    let a = dir.path().join("R/a.R").to_string_lossy().to_string();
    let b = dir.path().join("R/b.R").to_string_lossy().to_string();
    assert!(crate::project::is_package_library_file(&a));
    let mut project = Project::new();
    project.add_file(a.clone(), parse_file(&a, "my_const <- 42\n"));
    project.add_file(b.clone(), parse_file(&b, "x <- my_const\n"));
    let diags = project.check();
    let b_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == &b)
        .flat_map(|(_, d)| d)
        .collect();
    assert!(
        b_diags.iter().all(|d| d.code != "RY010"),
        "cross-file literal variable should not trigger RY010, got {:?}",
        b_diags
    );
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
fn cross_file_opaque_call_variable_resolves() {
    // File A defines `GeomRect <- ggproto("GeomRect", Geom, ...)`.
    // The RHS is a CALL (not a function literal), so it would not
    // be in `fns`; previously any reference from file B would fire
    // RY010. With `known_vars`, `GeomRect` resolves to opaque.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("R")).unwrap();
    std::fs::write(dir.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
    let geom = dir.path().join("R/geom.R").to_string_lossy().to_string();
    let user = dir.path().join("R/user.R").to_string_lossy().to_string();
    let mut project = Project::new();
    project.add_file(
        geom.clone(),
        parse_file(
            &geom,
            "GeomRect <- ggproto(\"GeomRect\", Geom, draw = function() NULL)\n",
        ),
    );
    project.add_file(user.clone(), parse_file(&user, "x <- GeomRect\n"));
    let diags = project.check();
    let user_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == &user)
        .flat_map(|(_, d)| d)
        .collect();
    assert!(
        user_diags.iter().all(|d| d.code != "RY010"),
        "cross-file ggproto-defined variable should not trigger RY010, got {:?}",
        user_diags
    );
}

#[test]
fn cross_file_list_constructor_variable_resolves() {
    // File A defines `config <- list(timeout = 30, retries = 3)`:
    // a list constructor, not a function. File B references it.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("R")).unwrap();
    std::fs::write(dir.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
    let config = dir.path().join("R/config.R").to_string_lossy().to_string();
    let main = dir.path().join("R/main.R").to_string_lossy().to_string();
    let mut project = Project::new();
    project.add_file(
        config.clone(),
        parse_file(&config, "config <- list(timeout = 30, retries = 3)\n"),
    );
    project.add_file(main.clone(), parse_file(&main, "t <- config$timeout\n"));
    let diags = project.check();
    let main_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == &main)
        .flat_map(|(_, d)| d)
        .collect();
    assert!(
        main_diags.iter().all(|d| d.code != "RY010"),
        "cross-file list-constructor variable should not trigger RY010, got {:?}",
        main_diags
    );
}

#[test]
fn scripts_share_top_level_known_vars() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("inst/examples")).unwrap();
    std::fs::write(dir.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
    let defining = dir
        .path()
        .join("inst/examples/a.R")
        .to_string_lossy()
        .to_string();
    let reading = dir
        .path()
        .join("inst/examples/b.R")
        .to_string_lossy()
        .to_string();
    let mut project = Project::new();
    project.add_file(
        defining.clone(),
        parse_file(&defining, "h <- list(pre = 1L)\n"),
    );
    project.add_file(reading.clone(), parse_file(&reading, "x <- h[[\"pre\"]]\n"));
    let diagnostics: Vec<_> = project
        .check()
        .into_iter()
        .find(|(path, _)| path == &reading)
        .unwrap()
        .1;
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "sourced scripts must share top-level bindings: {diagnostics:?}"
    );
}

#[test]
fn function_self_read_before_assignment_reports_ry010() {
    let diagnostics = check("f <- function() { h <- h[[\"pre\"]] }\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY010")
    );
}

#[test]
fn testthat_script_sees_package_library_functions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("R")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests/testthat")).unwrap();
    std::fs::write(dir.path().join("DESCRIPTION"), "Package: fixture\n").unwrap();
    let library = dir.path().join("R/hidden.R").to_string_lossy().to_string();
    let test = dir
        .path()
        .join("tests/testthat/test-hidden.R")
        .to_string_lossy()
        .to_string();
    let mut project = Project::new();
    project.add_file(
        library.clone(),
        parse_file(&library, "hidden <- function() 1L\n"),
    );
    project.add_file(test.clone(), parse_file(&test, "x <- hidden()\n"));
    let diagnostics = project
        .check()
        .into_iter()
        .find(|(path, _)| path == &test)
        .unwrap()
        .1;
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "testthat code must retain access to package functions: {diagnostics:?}"
    );
}

#[test]
fn genuinely_undefined_variable_still_triggers_ry010() {
    // Sanity: a name that is NOT defined in any file of the project
    // (and is not a typeshed function or dataset) must still emit
    // RY010. `known_vars` only suppresses diagnostics for names we
    // have actually seen assigned.
    let mut project = Project::new();
    project.add_file(
        "a.R".to_string(),
        parse_file("a.R", "x <- totally_undefined_thing\n"),
    );
    let diags = project.check();
    let a_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == "a.R")
        .flat_map(|(_, d)| d)
        .collect();
    assert!(
        a_diags.iter().any(|d| d.code == "RY010"),
        "genuinely undefined variable should still trigger RY010, got {:?}",
        a_diags
    );
}

#[test]
fn same_file_top_level_assignment_in_known_vars() {
    // Single-file mode: a top-level assignment `x <- 1L` puts `x`
    // in `known_vars`. Referencing `x` BEFORE its assignment in the
    // same file (use-before-def at the top level) does NOT trigger
    // RY010. R's `source()` semantics evaluate top-to-bottom so
    // this would error at runtime, but for static checking we
    // prioritize suppressing false positives over catching
    // use-before-def (matching the documented behavior of `known_vars`).
    let diags = check("y <- x\nx <- 1L\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "top-level use-before-def should not trigger RY010 (matches cross-file semantics), got {:?}",
        diags
    );
}

// ---- Namespace-qualified identifiers (pkg::name) ----
//
// The parser preserves the full `pkg::name` spelling in `Expr::Ident`.
// The checker must (a) suppress RY010 for these in value and
// statement position (we don't model other packages' exports), and
// (b) still resolve `pkg::fn(args)` calls by stripping the prefix
// for typeshed lookups.
#[test]
fn namespace_qualified_value_does_not_emit_ry010() {
    // `x <- S7::class_any` -- the RHS is a cross-package value
    // reference. We can't resolve S7's export table, so we treat
    // it as opaque and stay silent (no RY010).
    let diags = check("x <- S7::class_any\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "qualified value `S7::class_any` should not emit RY010, got {:?}",
        diags
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
fn namespace_qualified_statement_does_not_emit_ry010() {
    // Reexport pattern: a bare `rlang::set_names` in statement
    // position (common in purrr/dplyr reexport files). This is the
    // form produced by the parser for `pkg::name` at the top level.
    let diags = check("rlang::set_names\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "qualified statement `rlang::set_names` should not emit RY010, got {:?}",
        diags
    );
}

#[test]
fn namespace_qualified_backtick_operator_does_not_emit_ry010() {
    // `magrittr::`%>%`` -- a backticked infix operator reexported
    // from another package. The RHS name contains `%`, which makes
    // a good regression test that the `::` suppression isn't
    // confused by special characters.
    let diags = check("magrittr::`%>%`\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "qualified `magrittr::`%>%`` should not emit RY010, got {:?}",
        diags
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
fn namespace_qualified_triple_colon_value_does_not_emit_ry010() {
    // `pkg:::name` (triple colon, internal access) must be treated
    // the same way as `::` for RY010 suppression.
    let diags = check("x <- stats:::internal_helper\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "triple-colon qualified value should not emit RY010, got {:?}",
        diags
    );
}

#[test]
fn namespace_qualified_call_to_unknown_package_function_is_silent() {
    // `tibble::tibble(...)` -- `tibble` is not in our typeshed, so
    // the call resolves to opaque. Crucially, no RY010 should fire
    // on the function name itself (it's a qualified cross-package
    // reference).
    let diags = check("x <- tibble::tibble(a = 1L)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "qualified call to non-typeshed fn should not emit RY010, got {:?}",
        diags
    );
}

#[test]
fn bare_unbound_identifier_still_emits_ry010() {
    // Regression guard: suppressing RY010 for `pkg::name` must NOT
    // accidentally suppress it for genuinely unbound bare names.
    // `totally_undefined_thing` has no `::` and is not in scope,
    // the typeshed, or the FnTable, so it must still fire RY010.
    let diags = check("x <- totally_undefined_thing\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "bare unbound identifier should still emit RY010, got {:?}",
        diags
    );
}

#[test]
fn backtick_percent_operator_not_unbound() {
    // A backtick-quoted operator name like `` `%+%` `` is commonly a
    // user-defined or package-imported infix operator. The parser
    // preserves the backticks in the identifier name, and we cannot
    // resolve such names against any scope, typeshed, or FnTable.
    // The checker must suppress RY010 and return opaque.
    let diags = check("x <- `%+%`\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "backtick `%+%` operator should not emit RY010, got {:?}",
        diags
    );
}

#[test]
fn backtick_builtin_operator_symbol_not_unbound() {
    // A backtick-quoted built-in operator symbol like `` `+` `` is
    // referenced as a value (e.g. passed to `Reduce`). Suppress
    // RY010: these are R language primitives we don't model as
    // scope-bound variables.
    let diags = check("x <- `+`\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "backtick `+` operator should not emit RY010, got {:?}",
        diags
    );
}

#[test]
fn backtick_pipe_operator_not_unbound() {
    // `` `%>%` `` (magrittr pipe) referenced as a bare backtick
    // identifier should not emit RY010. This pattern appears in
    // package reexport code (`magrittr::`%>%`` is already covered
    // by the `::` check; the bare backtick form is covered here).
    let diags = check("x <- `%>%`\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "backtick `%>%` operator should not emit RY010, got {:?}",
        diags
    );
}

#[test]
fn lexical_nested_function_shadows_same_named_project_function_signature() {
    let diags = check(
        "helper <- function(required) required\n\nouter <- function() {\n  helper <- function() 1L\n  helper()\n}\nouter()\n",
    );
    assert!(diags.iter().all(|d| d.code != "RY091"), "got {diags:?}");
}

#[test]
fn lexical_function_shadows_base_signature() {
    // W7: a function literal defined inside an enclosing body shadows
    // the typeshed/base signature, so RY090/RY091 must not fire against
    // base::inherits' parameters.
    let diags = check(
        "f <- function(topics) {\n  inherits <- function(type) function(x) x$inherits_from(type)\n  topics$apply(inherits(\"return\"))\n}\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY090" && d.code != "RY091"),
        "got {diags:?}"
    );
}

#[test]
fn lexical_function_shadows_base_eval() {
    // Same lookup-order bug with a different base name (base::eval).
    let diags =
        check("g <- function(quo) {\n  eval <- function() .Call(\"x\", quo)\n  eval()\n}\n");
    assert!(
        diags.iter().all(|d| d.code != "RY090" && d.code != "RY091"),
        "got {diags:?}"
    );
}

// ===== search-path-unknown audit (W20/W21d) =====
//
// `mark_search_path_unknown()` suppresses RY010 (unbound-variable) for the
// rest of a scope. It is a conservative open-search-path flag set whenever a
// construct can introduce arbitrary bindings. These tests pin every call site
// so the suppression stays intentional and scoped, and so an accidental
// widening — e.g. re-introducing the package-global disabling the W20 fix
// removed for oversized serialized data — is caught immediately.
#[test]
fn plain_unbound_reference_fires_ry010() {
    // Negative control for the whole audit block: with no search-path-opening
    // construct, a real miss must still fire RY010. This is the invariant the
    // W20 fix restored (oversized sysdata used to suppress this project-wide).
    let diags = check("x <- genuinely_unbound_name\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "a plain unbound reference must fire RY010: {diags:?}"
    );
}

#[test]
fn external_unenumerable_marker_suppresses_ry010_for_its_file() {
    // The `SERIALIZED_BINDINGS_UNENUMERABLE` marker still flows from the
    // unstubbed-attached-package path (tests/examples with unstubbed
    // Suggests). It must suppress RY010 for THAT file only — never
    // project-wide. The W20 fix removed the package-global sysdata route;
    // this is the remaining legitimate, file-local open search path.
    let mut p = RParser::new().unwrap();
    let f = p.parse("test.R", "x <- genuinely_unbound_name\n").unwrap();
    let mut c = Checker::new("test.R");
    c.set_external_bindings(HashSet::from([
        ry_core::SERIALIZED_BINDINGS_UNENUMERABLE.to_string()
    ]));
    c.check(&f);
    let diags = c.take_diagnostics();
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "the unenumerable marker should suppress RY010 for its file: {diags:?}"
    );
}

#[test]
fn library_of_unknown_package_opens_search_path() {
    let diags = check("library(definitely_not_a_real_pkg_xyz)\nx <- some_unbound_value\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "library() of an unstubbed package opens the search path: {diags:?}"
    );
}

#[test]
fn library_character_only_without_literal_opens_search_path() {
    let diags = check("pkg <- \"x\"\nlibrary(pkg, character.only = TRUE)\ny <- still_unbound\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "library(character.only=TRUE) without a literal name opens the search path: {diags:?}"
    );
}

#[test]
fn data_and_load_calls_open_search_path() {
    let diags = check("data(some_dataset)\nload(\"workspace.rda\")\nz <- unbound_after_load\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "data()/load() open the search path: {diags:?}"
    );
}

#[test]
fn attach_opens_search_path_via_unknown_bindings_scope_effect() {
    // `attach()` is the canonical `ScopeEffect::UnknownBindings` loader.
    let diags = check("attach(list(a = 1))\nw <- unbound_after_attach\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "attach() opens the search path: {diags:?}"
    );
}
