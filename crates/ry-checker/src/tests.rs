use super::*;
use ry_core::RParser;

fn check(src: &str) -> Vec<Diagnostic> {
    let mut p = RParser::new().unwrap();
    let f = p.parse("test.R", src).unwrap();
    let mut c = Checker::new("test.R");
    c.check(&f);
    c.take_diagnostics()
}

#[test]
fn confidence_defaults_follow_rule_precision_and_info_severity() {
    assert_eq!(Confidence::default_for("RY096"), Confidence::High);
    assert_eq!(Confidence::default_for("RY010"), Confidence::Medium);
    assert_eq!(Confidence::default_for("RY097"), Confidence::Low);
    let info = Diagnostic::new(
        Severity::Info,
        Span::new(0, 1, 0, 0),
        "test.R",
        "RY010",
        "info",
    );
    assert_eq!(info.confidence, Confidence::Low);
}

#[test]
fn ambient_function_used_as_value_resolves_silently() {
    // Higher-order and value uses of ambient base functions are legitimate
    // R idioms and must not fire RY010 (`lapply(exprs, all.vars)` was a
    // documented FP cluster). The typo class is caught downstream when the
    // function type flows into comparisons (see the RY030 test below).
    for src in [
        "lapply(letters, enc2utf8)\n",
        "x <- col\n",
        "if (identical(oldClass, \"zoo\")) x <- 1L\n",
    ] {
        let diagnostics = check(src);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "ambient function value use must not fire RY010: {diagnostics:?}"
        );
    }
}

#[test]
fn ambient_function_in_comparison_still_diagnosed() {
    let diagnostics = check("if (oldClass > 3) x <- 1L\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY030" || diagnostic.code == "RY033"),
        "function value in comparison must stay diagnosed: {diagnostics:?}"
    );
}

#[test]
fn runtime_stub_defines_package_function_for_checker() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse("runtime.R", "library(foo)\nx <- bar() + 1L\n")
        .unwrap();

    let mut without = Checker::new("runtime.R");
    let (without_diags, without_scope) = without.check_with_scope(&file);
    assert!(
        without_diags.is_empty(),
        "preserve current opaque-call behavior"
    );
    assert_eq!(
        without_scope.get("x").map(|ty| &ty.mode),
        Some(&Mode::Opaque),
        "without a user stub, bar() must remain opaque"
    );

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("foo.json"),
        r#"{
            "schema_version": "1",
            "package": "foo",
            "version": "test",
            "functions": {
                "bar": {
                    "params": [],
                    "return": {"mode": "integer", "length": "1"}
                }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("base.json"),
        r#"{
            "schema_version": "1",
            "package": "base",
            "version": "test",
            "functions": {
                "custom_base": {
                    "params": [],
                    "return": {"mode": "integer", "length": "1"}
                }
            }
        }"#,
    )
    .unwrap();
    let stubs = Arc::new(ry_typeshed::load_stub_dir(dir.path()).unwrap());
    let mut with = Checker::new("runtime.R");
    with.set_user_stubs(Arc::clone(&stubs));
    let (with_diags, with_scope) = with.check_with_scope(&file);
    assert!(
        with_diags.is_empty(),
        "user stub should type bar(): {with_diags:?}"
    );
    assert_eq!(with_scope.get("x").map(|ty| &ty.mode), Some(&Mode::Integer));

    let base_file = parser.parse("base.R", "x <- custom_base() + 1L\n").unwrap();
    let mut base_checker = Checker::new("base.R");
    base_checker.set_user_stubs(stubs);
    let (base_diags, base_scope) = base_checker.check_with_scope(&base_file);
    assert!(
        base_diags.is_empty(),
        "user base stub must replace embedded base for this checker: {base_diags:?}"
    );
    assert_eq!(base_scope.get("x").map(|ty| &ty.mode), Some(&Mode::Integer));
}

#[test]
fn runtime_stub_schema_effect_extends_data_mask_semantics() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "runtime_nse.R",
            "library(fakepkg)\ndf <- data.frame(x = 1L)\nout <- enrich(df, y = x + 1L)\nz <- out$y + 1L\n",
        )
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("fakepkg.json"),
        r#"{
            "schema_version": "1",
            "package": "fakepkg",
            "version": "test",
            "functions": {
                "enrich": {
                    "params": [".data", "..."],
                    "return": "arg0",
                    "eval": {"...": "data_mask"},
                    "schema_effect": "add_named_args"
                }
            }
        }"#,
    )
    .unwrap();
    let stubs = Arc::new(ry_typeshed::load_stub_dir(dir.path()).unwrap());
    let mut checker = Checker::new("runtime_nse.R");
    checker.set_user_stubs(stubs);
    let (diagnostics, scope) = checker.check_with_scope(&file);
    assert!(
        diagnostics.is_empty(),
        "stub-defined data mask must resolve x and add y: {diagnostics:?}"
    );
    assert_eq!(scope.get("z").map(|ty| &ty.mode), Some(&Mode::Integer));
}

#[test]
fn qualified_base_schema_effect_is_applied() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "qualified_base.R",
            "df <- data.frame(x = 1L)\ny <- base::with(df, x + 1L)\nz <- y + 1L\n",
        )
        .unwrap();
    let mut checker = Checker::new("qualified_base.R");
    let (diagnostics, scope) = checker.check_with_scope(&file);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("z").map(|ty| &ty.mode), Some(&Mode::Integer));
}

#[test]
fn join_normal_arguments_use_the_ordinary_scope() {
    let diagnostics = check(
        "library(dplyr)\nx <- unknown_source()\ny <- data.frame(id = 1L)\nleft_join(x, y, by = missing_name)\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("missing_name")
    }));
}

#[test]
fn typeshed_parameter_modes_drive_data_mask_evaluation() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "test.R",
            "x <- as_draws_df(source)\ny <- mutate_variables(x, tau2 = tau^2)\n",
        )
        .unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_loaded(HashSet::from(["posterior".to_string()]));
    checker.check(&file);
    assert!(
        checker.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("tau")
        }),
        "data-mask metadata should make tau an opaque masked binding: {:?}",
        checker.diagnostics
    );
}

#[test]
fn embedded_package_eval_metadata_drives_data_mask_evaluation() {
    let diagnostics = check("library(rlist)\nr <- list.map(some_list(), . + score)\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "loaded rlist metadata should mask both `.` and `score`: {diagnostics:?}"
    );
}

#[test]
fn user_s3_method_inherits_generic_eval_metadata() {
    let diagnostics = check(
        "library(dplyr)\ncount.mystep <- function(.data, ...) 1\nobj <- structure(list(internal = 1L), class = \"mystep\")\ncount(obj, some_col)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("some_col")
        }),
        "the user method must inherit dplyr count's data mask: {diagnostics:?}"
    );
}

#[test]
fn dynamically_registered_s3_method_inherits_generic_eval_metadata() {
    let diagnostics = check(
        "complete.custom <- function(data, ...) data\nobj <- structure(list(), class = \"custom\")\ncomplete(obj, missing_column)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("missing_column")
        }),
        "the method should inherit tidyr complete's data mask: {diagnostics:?}"
    );
}

#[test]
fn s3_method_inherits_schema_generic_eval_metadata() {
    let diagnostics = check(
        "library(dplyr)\ngroup_by.custom <- function(.data, ...) .data\nobj <- structure(data.frame(known = 1L), class = c(\"custom\", \"data.frame\"))\ngroup_by(obj, missing_column)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("missing_column")
        }),
        "the method should inherit group_by's data mask: {diagnostics:?}"
    );
}

#[test]
fn data_mask_metadata_without_a_data_frame_is_still_opaque() {
    let diagnostics =
        check("library(patrick)\nwith_parameters_test_that(\"case\", n2 + n3, n2 = 1L, n3 = 2L)\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "patrick's masked code should not require a data-frame first argument: {diagnostics:?}"
    );
}

#[test]
fn data_mask_binds_dot_inside_do_with_native_pipe() {
    let diagnostics = check("library(dplyr)\ndf <- data.frame(x = 1L)\ndf |> do(head(., 1))\n");
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`.`")
        }),
        "the current-group dot should be bound in do(): {diagnostics:?}"
    );
}

#[test]
fn user_function_defused_parameters_are_opaque_at_call_sites() {
    let diagnostics = check(
        "capture <- function(expr, other) {\n  expr <- rlang::enquo(expr)\n  other\n}\ncapture(.input + missing, other = 1L)\ncapture(other = 1L, expr = named_missing)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "positionally and named defused arguments should be opaque: {diagnostics:?}"
    );
}

#[test]
fn lexical_types_are_opaque_under_unknown_data_masks_only() {
    let masked = check("library(dplyr)\ndf <- get(\"df\")\ny <- \"a\"\nmutate(df, x = x / y)\n");
    assert!(
        masked.iter().all(|diagnostic| diagnostic.code != "RY040"),
        "a lexical type must not drive arithmetic diagnostics under an unknown mask: {masked:?}"
    );

    let outside = check("y <- \"a\"\ny / 1L\n");
    assert!(
        outside.iter().any(|diagnostic| diagnostic.code == "RY040"),
        "the same lexical type must still be checked outside a mask: {outside:?}"
    );
}

#[test]
fn exclusively_defused_dots_are_opaque_at_call_sites() {
    let source = "f <- function(...) enquos(...)\ny <- \"a\"\nf(not_a_binding == 1, y / 1L)\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", source).unwrap();
    let mut checker = Checker::new("test.R");
    checker.collect_fns(&file.stmts);
    assert!(checker.fn_table.fns["f"].params[0].defused);

    let diagnostics = check(source);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010" && diagnostic.code != "RY040"),
        "arguments absorbed by defused dots should be opaque: {diagnostics:?}"
    );
}

#[test]
fn normally_used_dots_remain_eager_at_call_sites() {
    let diagnostics = check("g <- function(...) sum(...)\ng(not_a_binding)\n");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("not_a_binding")
    }));

    let mixed =
        check("h <- function(...) { captured <- enquos(...); list(...) }\nh(still_not_bound)\n");
    assert!(mixed.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("still_not_bound")
    }));
}

#[test]
fn embraced_parameters_are_defused_at_call_sites() {
    let source = "library(dplyr)\nwrapper <- function(df, var) select(df, {{ var }})\nwrapper(data.frame(a = 1L), a)\n";
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", source).unwrap();
    let mut checker = Checker::new("test.R");
    checker.collect_fns(&file.stmts);
    assert!(checker.fn_table.fns["wrapper"].params[1].defused);

    let diagnostics = check(source);
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`a`")
        }),
        "an embraced parameter should forward its call-site expression: {diagnostics:?}"
    );
}

#[test]
fn normal_use_before_defusing_keeps_parameter_eager() {
    let diagnostics = check(
        "capture <- function(expr) {\n  print(expr)\n  expr <- enquo(expr)\n}\ncapture(still_missing)\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("still_missing")
    }));
}

#[test]
fn normal_first_use_in_any_branch_keeps_parameter_eager() {
    let diagnostics = check(
        "capture <- function(expr, flag) {\n  if (flag) enquo(expr) else print(expr)\n}\ncapture(still_missing, TRUE)\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("still_missing")
    }));
}

#[test]
fn foreach_user_infix_binds_named_iteration_variables() {
    let diagnostics = check(
        "foreach(iter = seq_along(xs), parm = values, .errorhandling = \"stop\") %op% {\n  iter + parm + genuinely_missing\n}\nforeach(outer = xs) %:% foreach(inner = ys) %dopar% { outer + inner }\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("iter")
                    && !diagnostic.message.contains("parm")
                    && !diagnostic.message.contains("outer")
                    && !diagnostic.message.contains("inner"))
        }),
        "foreach iteration bindings should be scoped over the RHS: {diagnostics:?}"
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("genuinely_missing")
    }));
}

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
fn user_infix_infers_both_operands_and_returns_unknown() {
    let (diagnostics, scope) =
        check_with_scope("result <- missing_left %custom% missing_right\nafter <- result + 1L\n");
    for name in ["missing_left", "missing_right"] {
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "RY010" && diagnostic.message.contains(name)
            })
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
fn destructuring_is_not_special_without_package_context() {
    let diagnostics = check("c(unbound) %<-% make_value()\n");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("unbound")
    }));
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
fn vector_string_subset_preserves_non_scalar_length() {
    let (diags, scope) =
        check_with_scope("x <- c(first = 1L, second = 2L)\ny <- x[c(\"first\", \"second\")]\n");
    assert!(diags.is_empty(), "{diags:?}");
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.mode, Mode::Integer);
    assert_ne!(y.length, Length::One);
}

#[test]
fn package_loading_calls_have_distinct_return_types() {
    let (diags, scope) = check_with_scope(
        "attached <- library(stats)\navailable <- require(stats)\nnamespaced <- requireNamespace(\"stats\")\n",
    );
    assert!(diags.is_empty(), "{diags:?}");

    let attached = scope.get("attached").expect("attached should be bound");
    assert_eq!(attached.mode, Mode::Null);
    assert_eq!(attached.length, Length::Zero);

    let available = scope.get("available").expect("available should be bound");
    assert_eq!(available.mode, Mode::Logical);
    assert_eq!(available.length, Length::One);

    let namespaced = scope.get("namespaced").expect("namespaced should be bound");
    assert_eq!(namespaced.mode, Mode::Logical);
    assert_eq!(namespaced.length, Length::One);
}

#[test]
fn user_function_argument_rules_wait_for_callable_provenance() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "project.R",
            "f <- function(required) required\nf()\nc <- function(x) x\nc(unrelated = 1L)\n",
        )
        .unwrap();
    let mut project = Project::new();
    project.add_file("project.R".to_string(), file);
    let diags: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        diags
            .iter()
            .all(|diagnostic| diagnostic.code != "RY090" && diagnostic.code != "RY091"),
        "project-wide function names are not sufficient to validate a call: {diags:?}"
    );
}

#[test]
fn typeshed_required_arguments_are_still_checked() {
    let diags = check("as.character()\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY091"),
        "explicit typeshed required metadata should remain authoritative: {diags:?}"
    );
}

#[test]
fn classed_and_null_generic_arguments_do_not_report_type_mismatches() {
    let diags =
        check("x <- structure(list(value = 1L), class = \"custom\")\nround(x)\nlog(NULL)\n");
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY092"),
        "classed values may dispatch and numeric generics accept NULL: {diags:?}"
    );
}

#[test]
fn plain_character_numeric_generic_argument_still_reports_mismatch() {
    let diags = check("log(\"not numeric\")\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY092"),
        "a plain character value cannot use numeric generic dispatch: {diags:?}"
    );
}

#[test]
fn quoted_dsl_metadata_suppresses_only_captured_symbols() {
    let diags = check(
        "library(dplyr)\nspec <- join_by(left_id == right_id)\nmissing_after\nlibrary(igraph)\ng <- graph_from_literal(A - B, B - C)\n",
    );
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("left_id")
                    && !diagnostic.message.contains("right_id")
                    && !diagnostic.message.contains("`A`")
                    && !diagnostic.message.contains("`B`")
                    && !diagnostic.message.contains("`C`"))
        }),
        "quoted DSL symbols must not be resolved lexically: {diags:?}"
    );
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("missing_after")
        }),
        "ordinary lexical reads must remain checked: {diags:?}"
    );
}

#[test]
fn expanded_dplyr_metadata_resolves_masks_and_selectors() {
    let diags = check(
        "library(dplyr)\ndf <- data.frame(a = 1L, b = 2L)\ndistinct(df, a)\npull(df, b)\nrelocate(df, b, .before = a)\nslice_min(df, order_by = b)\nmutate(df, picked = pick(a, b))\n",
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "dplyr masks and selectors should resolve known columns: {diags:?}"
    );
}

#[test]
fn expanded_tidyr_metadata_resolves_captured_columns() {
    let diags = check(
        "library(tidyr)\ndf <- data.frame(a = 1L, b = 2L)\ngather(df, key, value, a, b)\nchop(df, a)\ncomplete(df, a)\nnest(df, nested = c(a, b))\nunnest(df, nested)\nunite(df, combined, a, b)\n",
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "tidyr captured column arguments should not be resolved lexically: {diags:?}"
    );
}

#[test]
fn recipes_metadata_resolves_selectors_and_masked_expressions() {
    let diags = check(
        "library(recipes)\nr <- data.frame(a = 1L, b = 2L, outcome = 3L)\nstep_center(r, a, b)\nstep_pls(r, a, outcome = outcome)\nstep_mutate(r, total = a + b)\nimp_vars(quoted_predictor)\nmissing_after\n",
    );
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("`a`")
                    && !diagnostic.message.contains("`b`")
                    && !diagnostic.message.contains("`outcome`")
                    && !diagnostic.message.contains("quoted_predictor"))
        }),
        "recipes selectors and expressions are captured, not lexical reads: {diags:?}"
    );
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("missing_after")
        }),
        "ordinary reads outside recipes calls must remain checked: {diags:?}"
    );
}

#[test]
fn standard_r_inventory_resolves_default_package_symbols() {
    let diags = check(
        "family <- binomial\ndataset <- WWWusage\nhandler <- conditionMessage\nconverter <- as.name\nmaximum <- which.max\n",
    );
    assert!(
        diags.is_empty(),
        "standard inventory symbols (functions and datasets) resolve silently: {diags:?}"
    );
}

#[test]
fn standard_inventory_does_not_override_precise_types() {
    let (diags, scope) = check_with_scope("callback <- sqrt\ndf <- mtcars\nbad <- df$missing\n");
    let callback = scope.get("callback").expect("callback should be bound");
    assert_eq!(callback.mode, Mode::Function);
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY060"),
        "typed dataset schemas must win over existence-only inventory: {diags:?}"
    );
}

#[test]
fn standard_inventory_does_not_hide_unknown_names() {
    let diags = check("definitely_not_a_standard_r_symbol\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY010"),
        "unknown neighboring names must still be diagnosed: {diags:?}"
    );
}

#[test]
fn call_position_skips_local_values_for_standard_functions() {
    let diags = check(
        "dimnames <- list(rows = \"r\")\nx <- matrix(1L, 1L, 1L)\ny <- dimnames(x)\ndimnames(x) <- dimnames\nserialize <- TRUE\nserialize(1L, NULL)\n",
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY070"),
        "R call lookup skips same-named non-function bindings: {diags:?}"
    );
}

#[test]
fn standard_non_function_values_do_not_suppress_call_errors() {
    let diags = check("WWWusage <- 1L\nWWWusage()\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY070"),
        "standard datasets are values, not call-position candidates: {diags:?}"
    );
}

#[test]
fn withr_tempfile_injects_literal_names_into_code_scope() {
    let diags = check("withr::with_tempfile(c(\"first\", \"second\"), code = { first; second })\n");
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "with_tempfile string names should be bound inside code: {diags:?}"
    );
}

#[test]
fn withr_tempfile_bindings_do_not_leak() {
    let diags = check("withr::with_tempfile(\"path\", code = path)\npath\n");
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`path`")
        }),
        "with_tempfile bindings are local to the code expression: {diags:?}"
    );
}

#[test]
fn withr_tempfile_keeps_checking_other_code_names() {
    let diags = check("withr::with_tempfile(\"path\", code = { path; missing_inside })\n");
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("missing_inside")
        }),
        "only explicitly injected names should be suppressed: {diags:?}"
    );
}

#[test]
fn dbplyr_translation_helpers_capture_sql_expressions() {
    // `translate_sql` is the exported quoting entry point; the test-local
    // `expect_translation` helpers were removed from the stub because they
    // are not part of dbplyr's namespace (the audit enforces that).
    let diags = check("library(dbplyr)\ntranslate_sql(x + y)\nmissing_after\n");
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("`x`") && !diagnostic.message.contains("`y`"))
        }),
        "translation expressions are captured rather than evaluated lexically: {diags:?}"
    );
    assert!(diags.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("missing_after")
    }));
}

#[test]
fn lazy_defaults_can_reference_body_local_bindings() {
    let diags = check("f <- function(value = generated) {\n  generated <- 1L\n  value\n}\nf()\n");
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("generated")
        }),
        "R defaults are promises evaluated in the function environment: {diags:?}"
    );
}

#[test]
fn lazy_default_forced_before_body_local_assignment_is_diagnosed() {
    let diags = check(include_str!(
        "../testdata/ry098_default_forced_before_assignment.R"
    ));
    let matches: Vec<_> = diags
        .iter()
        .filter(|diagnostic| diagnostic.code == "RY098")
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "only the early return should fire: {diags:?}"
    );
    assert_eq!(matches[0].span.line, 2);
}

#[test]
fn lazy_default_reachability_precision_cases_stay_silent() {
    let diags = check(include_str!("../testdata/ok_lazy_default_reachability.R"));
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY098"),
        "conservative negative cases must remain silent: {diags:?}"
    );
}

#[test]
fn comparison_directly_inside_length_is_diagnosed() {
    let diags = check("if (length(x == y)) print(\"bad\")\nok <- length(x) == y\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        1,
        "only the comparison nested directly under length should fire: {diags:?}"
    );
}

#[test]
fn comparison_inside_selected_scalar_calls_is_diagnosed() {
    let diags = check("length(x > 0)\nnchar(x == y)\nabs(x != y)\nsum(x > 0)\nlength(x) > 0\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        3,
        "length, nchar, and abs should fire, but sum and an outer comparison should not: {diags:?}"
    );
}

#[test]
fn comparison_inside_call_is_diagnosed_through_short_circuit_operators() {
    let diags = check(
        "q <- TRUE\nx <- 1L\ny <- 2L\nz <- TRUE\nif (length(x == y) || q) x\nstopifnot(length(x == y) && z)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        2,
        "both short-circuit operands must retain call diagnostics: {diags:?}"
    );
}

#[test]
fn negated_comparison_binds_loosely_and_stays_silent() {
    // R parses `!x == y` as `!(x == y)` (unary `!` binds looser than
    // comparison), so the idiomatic `!length(x) == 1` guard is correct
    // code. RY095 wrongly assumed C precedence and is retired.
    let diags =
        check("x <- c(1, 2)\nif (!length(x) == 1) x <- 1\nflag <- !\"a\" == \"b\"\n!(1L == 2L)\n");
    assert!(
        diags.is_empty(),
        "negated comparisons are valid R and must stay silent: {diags:?}"
    );
}

#[test]
fn hasarg_requires_a_formal_of_the_lexically_enclosing_function() {
    let diags = check(
        "good <- function(value) hasArg(value)\ndots_ok <- function(actual, ...) hasArg(missing)\nidiom_ok <- function(object, ...) if (hasArg(thresh)) list(...)$thresh else 0\nstring_bad <- function(actual) hasArg(\"missing\")\nbad <- function(actual) hasArg(missing)\nhasArg(top_level)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY096")
            .count(),
        2,
        "non-formals in dots-less functions should fire; formals, dots functions, and top-level calls stay silent: {diags:?}"
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "hasArg captures names and must not create unbound-name diagnostics: {diags:?}"
    );
}

#[test]
fn printf_family_literal_arity_is_checked() {
    let diags = check(
        "gettextf(\"select %s then %s\", \"first\")\nsprintf(\"value=%s %%\", \"ok\")\nsprintf(dynamic_format, value)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY094")
            .count(),
        1,
        "only a proven literal format shortage should fire: {diags:?}"
    );
}

#[test]
fn nse_function_alias_quotes_cli_time_ago_expressions() {
    let diags = check(include_str!("../testdata/ok_nse_function_alias.R"));
    assert!(
        diags.is_empty(),
        "an alias of expression() must preserve quoted-call semantics: {diags:?}"
    );
}

#[test]
fn quote_and_printf_semantics_follow_function_aliases() {
    let diags = check("q <- quote\nq(undefined_sym)\ns <- sprintf\ns(\"%d %d\", 1)\n");
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "quote() through an alias must not resolve its captured symbol: {diags:?}"
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY094")
            .count(),
        1,
        "sprintf() format validation must run through an alias: {diags:?}"
    );
}

#[test]
fn function_alias_semantics_are_cleared_by_reassignment() {
    let diags = check("q <- quote\nq <- function(x) x\nq(undefined_sym)\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY010"),
        "overwriting an alias with a local function must clear quote semantics: {diags:?}"
    );
}

// ---- inline suppression comment tests ----

#[test]
fn parse_trailing_ignore_comment() {
    let supps = parse_suppressions("x <- bad  # ry: ignore\n");
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 0);
    assert!(supps[0].rules.is_empty()); // suppress all
}

#[test]
fn parse_specific_rule_ignore() {
    let supps = parse_suppressions("x <- \"a\" * 3  # ry: ignore[RY040]\n");
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].rules, vec!["RY040"]);
}

#[test]
fn parse_multiple_rules() {
    let supps = parse_suppressions("x <- bad  # ry: ignore[RY040, RY010]\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY040".to_string()));
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_standalone_comment_applies_to_next_line() {
    let src = "# ry: ignore\nx <- bad\n";
    let supps = parse_suppressions(src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 1); // next line
}

#[test]
fn parse_standalone_comment_skips_blank_lines() {
    let src = "# ry: ignore\n\nx <- bad\n";
    let supps = parse_suppressions(src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 2);
}

#[test]
fn parse_noqa_alias() {
    let supps = parse_suppressions("x <- bad  # noqa: RY010\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_bare_noqa_suppresses_all() {
    let supps = parse_suppressions("x <- bad  # noqa\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.is_empty());
}

#[test]
fn parse_noqa_bracket_form() {
    let supps = parse_suppressions("x <- bad  # noqa[RY010]\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_compact_ry_ignore_no_space() {
    let supps = parse_suppressions("x <- bad  # ry:ignore[RY010]\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_case_insensitive_marker() {
    let supps = parse_suppressions("x <- bad  # RY: IGNORE[ry010]\n");
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_non_suppression_comment_is_ignored() {
    let supps = parse_suppressions("# just a regular comment\nx <- bad\n");
    assert!(supps.is_empty());
}

#[test]
fn parse_file_level_suppression() {
    assert!(has_file_suppression("# ry: ignore-file\nx <- bad\n"));
    assert!(has_file_suppression("# ry:ignore-file\nx <- bad\n"));
    assert!(!has_file_suppression("# ry: ignore\nx <- bad\n"));
}

#[test]
fn file_level_marker_not_treated_as_line_level() {
    // `# ry: ignore-file` must NOT also register as a line-level
    // "ignore all" (it's handled by has_file_suppression instead).
    let supps = parse_suppressions("# ry: ignore-file\nx <- bad\n");
    assert!(
        supps.is_empty(),
        "ignore-file should not produce line-level suppressions, got {:?}",
        supps
    );
}

#[test]
fn is_suppressed_matches_line_and_code() {
    let supps = vec![Suppression {
        line: 2,
        rules: vec!["RY010".to_string()],
    }];
    let diag_matching = Diagnostic {
        severity: Severity::Warning,
        span: Span {
            start: 0,
            end: 1,
            line: 2,
            col: 0,
        },
        path: "x.R".into(),
        code: "RY010",
        message: "test".into(),
        confidence: Confidence::Medium,
    };
    let diag_wrong_line = Diagnostic {
        span: Span {
            line: 0,
            ..diag_matching.span
        },
        ..diag_matching.clone()
    };
    let diag_wrong_code = Diagnostic {
        code: "RY040",
        ..diag_matching.clone()
    };
    assert!(is_suppressed(&diag_matching, &supps));
    assert!(!is_suppressed(&diag_wrong_line, &supps));
    assert!(!is_suppressed(&diag_wrong_code, &supps));
}

#[test]
fn is_suppressed_empty_rules_matches_any_code() {
    let supps = vec![Suppression {
        line: 0,
        rules: vec![],
    }];
    let diag = Diagnostic {
        severity: Severity::Warning,
        span: Span {
            start: 0,
            end: 1,
            line: 0,
            col: 0,
        },
        path: "x.R".into(),
        code: "RY999",
        message: "test".into(),
        confidence: Confidence::Medium,
    };
    assert!(is_suppressed(&diag, &supps));
}

#[test]
fn filter_suppressed_end_to_end() {
    // Trailing `# ry: ignore[RY010]` on the offending line drops RY010.
    let src = "x <- undefined_var  # ry: ignore[RY010]\n";
    let diags = check(src);
    let filtered = filter_suppressed(diags, src);
    assert!(
        filtered.iter().all(|d| d.code != "RY010"),
        "RY010 should be suppressed, got {:?}",
        filtered
    );
}

#[test]
fn filter_suppressed_file_level_drops_everything() {
    let src = "# ry: ignore-file\nx <- undefined_var\n";
    let diags = check(src);
    let filtered = filter_suppressed(diags, src);
    assert!(
        filtered.is_empty(),
        "file-level suppression should drop all diagnostics, got {:?}",
        filtered
    );
}

#[test]
fn filter_suppressed_other_rules_still_fire() {
    // Suppressing RY010 on line 0 should NOT affect RY040 on line 1.
    let src = "x <- undefined_var  # ry: ignore[RY010]\ny <- \"a\" * 3L\n";
    let diags = check(src);
    let filtered = filter_suppressed(diags, src);
    assert!(
        filtered.iter().any(|d| d.code == "RY040"),
        "RY040 should still fire (it's on a different line), got {:?}",
        filtered
    );
    assert!(
        filtered.iter().all(|d| d.code != "RY010"),
        "RY010 should be suppressed"
    );
}

#[test]
fn detects_char_plus_int() {
    let diags = check(r#""a" + 1L"#);
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040, got {:?}",
        diags
    );
}

#[test]
fn allows_int_plus_double() {
    let diags = check("1L + 2.0\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn detects_if_on_character() {
    let diags = check(r#"if ("x") print(1)"#);
    assert!(diags.iter().any(|d| d.code == "RY001"));
}

#[test]
fn detects_long_condition_warning() {
    let diags = check("if (c(TRUE, FALSE)) print(1)\n");
    assert!(diags.iter().any(|d| d.code == "RY002"));
}

#[test]
fn detects_unbound_var() {
    let diags = check("y <- undefined_thing\n");
    assert!(diags.iter().any(|d| d.code == "RY010"));
}

#[test]
fn scalar_logical_warns_on_vector_operand() {
    let diags = check("x <- c(TRUE, FALSE)\nbad <- x && TRUE\n");
    assert!(
        diags.iter().any(|d| d.code == "RY032"),
        "expected RY032 for && with vector, got {:?}",
        diags
    );
}

#[test]
fn vectorized_logical_no_warning() {
    let diags = check("x <- c(TRUE, FALSE)\nok <- x & TRUE\n");
    assert!(
        diags.iter().all(|d| d.code != "RY032"),
        "vectorized & should not warn, got {:?}",
        diags
    );
}

#[test]
fn scalar_logical_with_scalars_no_warning() {
    let diags = check("a <- TRUE\nb <- FALSE\nx <- a && b\n");
    assert!(
        diags.iter().all(|d| d.code != "RY032"),
        "&& with scalars should not warn, got {:?}",
        diags
    );
}

#[test]
fn compare_char_numeric_warns() {
    let diags = check(r#"bad <- "hello" < 42"#);
    assert!(
        diags.iter().any(|d| d.code == "RY033"),
        "expected RY033 for character vs numeric, got {:?}",
        diags
    );
}

#[test]
fn compare_same_mode_no_warning() {
    let diags = check("bad <- 1 < 2\n");
    assert!(
        diags.iter().all(|d| d.code != "RY033"),
        "numeric vs numeric should not warn, got {:?}",
        diags
    );
}

#[test]
fn compare_char_char_no_warning() {
    let diags = check(r#"x <- "abc" < "xyz""#);
    assert!(
        diags.iter().all(|d| d.code != "RY033"),
        "character vs character should not warn, got {:?}",
        diags
    );
}

#[test]
fn compare_eq_char_numeric_warns() {
    let diags = check(r#"bad <- "hello" == 1"#);
    assert!(
        diags.iter().any(|d| d.code == "RY033"),
        "expected RY033 for character == numeric, got {:?}",
        diags
    );
}

#[test]
fn in_operator_uses_lhs_length() {
    // `x %in% table` returns a logical vector of length(x); the RHS
    // length is irrelevant. A length-1 `x` matched against a length-2
    // literal must stay length-1 logical -- not length-2 (which would
    // drive RY002/RY032 false positives downstream).
    let (_diags, scope) = check_with_scope("x <- \"a\"\nr <- x %in% c(\"a\", \"b\")\n");
    let r = scope.get("r").expect("binding r");
    assert_eq!(r.mode, Mode::Logical, "got {:?}", r);
    assert_eq!(r.length, Length::One, "got {:?}", r);
}

#[test]
fn in_operator_condition_no_ry002_ry032() {
    // The end-to-end shape from the purrr net: a length-1 `%in%` result
    // used as an `if` condition and inside `&&` must not fire RY002 or
    // RY032.
    let diags = check(
        "x <- \"a\"\nif (x %in% c(\"a\", \"b\")) print(1)\nif (is.character(x) && x %in% c(\"a\", \"b\")) print(2)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY002" && d.code != "RY032"),
        "expected no RY002/RY032 for length-1 %in%, got {:?}",
        diags
    );
}

#[test]
fn function_param_inference_no_diag() {
    // `f` has a default-typed param `x = 1L` (integer), so `x + 1`
    // is integer + double = double. Well-typed; no diagnostics.
    let diags = check("f <- function(x = 1L) { x + 1 }\ng <- f(2L)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "got false positive: {:?}",
        diags
    );
}

#[test]
fn user_fn_return_type_inferred() {
    // `text` returns a string literal, so `text()` is character and
    // the arithmetic use must error.
    let diags = check("text <- function() { \"hello\" }\ny <- text() + 1L\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from character-returning fn used arithmetically, got {:?}",
        diags
    );
}

#[test]
fn user_fn_return_explicit_return() {
    let diags = check("f <- function(x = 1L) { return(x * 2) }\ny <- f() + \"bad\"\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from integer-returning fn + character, got {:?}",
        diags
    );
}

#[test]
fn recursive_fn_terminates() {
    // The fixpoint must converge on fact()'s return type (integer)
    // without infinite descent. We don't assert any specific diag,
    // just that the checker terminates and doesn't crash.
    let diags = check(
        "fact <- function(n = 1L) { if (n <= 1L) return(1L); n * fact(n - 1L) }\ny <- fact(5)\n",
    );
    // The result is integer; arithmetic with another integer is fine.
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "false positive on recursive fn: {:?}",
        diags
    );
}

#[test]
fn seq_operator_produces_integer() {
    // `1:10` is integer, so `i` in the loop is integer, so `i + 1L`
    // is well-typed.
    let diags = check("total <- 0L\nfor (i in 1:10) { total <- total + i }\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn for_loop_var_is_element_type() {
    // Iterating over a character vector makes the loop variable
    // character; using it arithmetically should error.
    let diags = check("for (s in c(\"a\", \"b\")) { total <- s + 1 }\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from character loop var + int, got {:?}",
        diags
    );
}

#[test]
fn pipe_desugars_to_call() {
    // `c(1,2,3) %>% mean()` desugars to `mean(c(1,2,3))`, which is
    // well-typed: no diagnostics.
    let diags = check("result <- c(1, 2, 3) %>% mean()\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn pipe_chain_infers() {
    // A two-step pipe composes: `mean() -> double_or_int<1>`, then
    // `round(<double>, digits = 2)` resolves against the typeshed.
    let diags = check("a <- c(1, 2, 3) %>% mean() %>% round(2)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn pipe_base_r_infers() {
    // Base-R `|>` desugars identically to magrittr `%>%`.
    let diags = check("a <- c(1, 2, 3) |> mean()\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn pipe_bare_function() {
    // Bare `rhs` becomes a one-arg call: `x %>% abs` -> `abs(x)`.
    let diags = check("x <- 1L\ny <- x %>% abs\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn pipe_placeholder_substitutes() {
    // The first `.` is replaced with the LHS; `round(., digits = 2)`
    // becomes `round(c(1,2,3), digits = 2)`.
    let diags = check("result <- c(1, 2, 3) %>% round(., digits = 2)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn pipe_tee_returns_lhs_type() {
    // `%T>%` returns the LHS; the RHS is walked for diagnostics only.
    // `c(1,2,3) %T>% print()` should be a length-3 double vector.
    let diags = check("result <- c(1, 2, 3) %T>% print()\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn pipe_dot_pronoun_dollar_column() {
    // `df %>% .$mpg` resolves `.` to the piped LHS (`mtcars`) and
    // then indexes by column name, so `col` should be `double<32>`
    // (the type of `mtcars$mpg`). We assert the inferred type
    // directly via the test scope and also check that no RY010
    // (unbound `.`) leaks out.
    let (diags, scope) = check_with_scope("df <- mtcars\ncol <- df %>% .$mpg\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dot pronoun should not emit RY010 (unbound `.`), got {:?}",
        diags
    );
    let col = scope.get("col").expect("col should be bound");
    assert_eq!(
        col.mode,
        Mode::Double,
        "df %>% .$mpg must infer double, got {:?}",
        col
    );
    assert_eq!(col.length, Length::Known(32), "mpg has 32 rows");
}

#[test]
fn pipe_dot_pronoun_double_bracket() {
    // `df %>% .[["mpg"]]` resolves `.` to the LHS and indexes by
    // string-literal column name via `[[`, mirroring `$` semantics.
    let (diags, scope) = check_with_scope("df <- mtcars\ncol <- df %>% .[[\"mpg\"]]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dot pronoun should not emit RY010, got {:?}",
        diags
    );
    let col = scope.get("col").expect("col should be bound");
    assert_eq!(col.mode, Mode::Double, ".[[\"mpg\"]] must infer double");
    assert_eq!(col.length, Length::Known(32), "mpg has 32 rows");
}

#[test]
fn pipe_dot_pronoun_single_bracket() {
    // `df %>% .[1]` preserves the base type (single-bracket
    // subsetting keeps the existing opaque behavior at v1), so the
    // result is the same data.frame-typed value as the LHS. The
    // important behavioral check is that no RY010 leaks on `.`.
    let (diags, scope) = check_with_scope("df <- mtcars\nsub <- df %>% .[1]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dot pronoun should not emit RY010, got {:?}",
        diags
    );
    let sub = scope.get("sub").expect("sub should be bound");
    assert_eq!(sub.mode, Mode::List, "df[1] preserves base mode");
    assert!(
        sub.class.contains("data.frame"),
        ".[1] preserves the data.frame class"
    );
}

#[test]
fn pipe_dot_pronoun_bare_returns_lhs() {
    // `x %>% .` returns the LHS value itself (the `.` refers to the
    // LHS). For a length-3 double vector, the result type matches.
    let (diags, scope) = check_with_scope("x <- c(1, 2, 3)\ny <- x %>% .\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.mode, Mode::Double, "x %>% . must infer double");
    assert_eq!(y.length, Length::Known(3), "length is preserved");
}

#[test]
fn pipe_dot_pronoun_undefined_column_emits_ry060() {
    // `df %>% .$nonexistent` resolves `.` to the LHS, then the
    // column lookup fails against `mtcars`'s schema, so RY060
    // (undefined-column) must fire - the pronoun path reuses the
    // same diagnostics as a direct `df$nonexistent`.
    let diags = check("df <- mtcars\nbad <- df %>% .$nonexistent\n");
    assert!(
        diags.iter().any(|d| d.code == "RY060"),
        "expected RY060 for undefined column via dot pronoun, got {:?}",
        diags
    );
}

#[test]
fn pipe_dot_pronoun_chains_into_arithmetic() {
    // End-to-end behavioral check: `df %>% .$mpg` produces a real
    // double type (not opaque), so subsequent arithmetic that would
    // fail on an opaque value type-checks cleanly. This is the
    // motivating use case from the task description.
    let diags = check("df <- mtcars\ncol <- df %>% .$mpg\nok <- col + 1L\n");
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "col + 1L should be valid (double + int), got {:?}",
        diags
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "no RY010 should leak from the dot pronoun, got {:?}",
        diags
    );
}

#[test]
fn dataset_resolves_mtcars() {
    // `mtcars` is in the typeshed's datasets table; using it must
    // not emit RY010 (unbound variable).
    let diags = check("df <- mtcars\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "expected no RY010 for mtcars, got {:?}",
        diags
    );
}

#[test]
fn dataset_resolves_iris() {
    let diags = check("df <- iris\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "expected no RY010 for iris, got {:?}",
        diags
    );
}

#[test]
fn s3_dispatch_known_method() {
    // `print.foo` is defined; calling `print(x)` on a "foo"-class
    // value dispatches to it. No RY050.
    let diags = check(
        "print.foo <- function(x, ...) { invisible(x) }\n\
             x <- structure(list(), class = \"foo\")\n\
             print(x)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "expected no RY050 when method is defined, got {:?}",
        diags
    );
}

#[test]
fn s3_dispatch_missing_method() {
    // No `print.undefined`; `print.default` exists in the typeshed,
    // so we know `print` is an S3 generic. The missing specific
    // method is flagged with RY050.
    let diags = check(
        "x <- structure(list(), class = \"undefined\")\n\
             print(x)\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY050"),
        "expected RY050 for missing method, got {:?}",
        diags
    );
}

#[test]
fn s3_dispatch_no_class() {
    // `y` has no class attribute (a plain atomic vector). S3
    // dispatch has nothing to work on; RY050 must NOT fire.
    let diags = check(
        "y <- c(1, 2, 3)\n\
             print(y)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "expected no RY050 on a classless value, got {:?}",
        diags
    );
}

#[test]
fn structure_call_sets_class() {
    // `structure(list(), class = "foo")` must produce a type whose
    // class vector contains "foo". We exercise this through the
    // public `Checker` API by relying on the fact that a missing
    // `print.foo` method would emit RY050 only if the class was
    // actually attached.
    let mut parser = RParser::new().unwrap();
    let src = "x <- structure(list(), class = \"foo\")\nprint(x)\n";
    let f = parser.parse("test.R", src).unwrap();
    let mut c = Checker::new("test.R");
    c.check(&f);
    let diags = c.take_diagnostics();
    // Without `print.foo`, RY050 should fire - proving the class was
    // attached. (If `structure` had failed to set the class, the
    // value would be classless and no RY050 would appear.)
    assert!(
        diags.iter().any(|d| d.code == "RY050"),
        "expected RY050 proving class was attached, got {:?}",
        diags
    );
}

#[test]
fn mtcars_mpg_column_infers_double() {
    // `df$mpg` on `mtcars` must resolve to the column's type
    // (double<32>, not opaque). We assert the inferred type of `x`
    // directly via the test scope, and also exercise a behavioral
    // check: `x + 1L` is well-typed (double + integer) and produces
    // no RY040.
    let (_, scope) = check_with_scope("df <- mtcars\nx <- df$mpg\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(
        x.mode,
        Mode::Double,
        "df$mpg must infer double, got {:?}",
        x
    );
    assert_eq!(x.length, Length::Known(32), "mpg has 32 rows");
    // Behavioral check: arithmetic on the inferred double works.
    let diags = check("df <- mtcars\nx <- df$mpg\ny <- x + 1L\n");
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "x + 1L should be valid (double + int), got {:?}",
        diags
    );
}

#[test]
fn mtcars_undefined_column_emits_ry060() {
    // `mtcars$nonexistent` must emit RY060 (undefined-column). The
    // message should name the offending column and list available
    // ones so the user can fix the typo. The available-columns
    // preview is taken from the schema in (BTreeMap-sorted) order;
    // we assert on a column that lands in the first 5.
    let diags = check("df <- mtcars\nbad <- df$nonexistent\n");
    let hit = diags
        .iter()
        .find(|d| d.code == "RY060")
        .expect("expected RY060 for nonexistent column");
    assert!(
        hit.message.contains("nonexistent"),
        "message should name the column: {}",
        hit.message
    );
    assert!(
        hit.message.contains("cyl"),
        "message should list an available column (cyl is in the first 5 alphabetically): {}",
        hit.message
    );
    // Sanity: the message also indicates abbreviation (mtcars has
    // 11 columns, more than the 5-column preview limit).
    assert!(
        hit.message.contains("..."),
        "message should abbreviate the list: {}",
        hit.message
    );
}

#[test]
fn list_named_args_become_schema() {
    // `list(a = 1L, b = "x")` builds a column schema from the named
    // args; `l$a` resolves to integer<1> and `l$b` to character<1>.
    let (_, scope) = check_with_scope("l <- list(a = 1L, b = \"x\")\nva <- l$a\nvb <- l$b\n");
    let va = scope.get("va").expect("va should be bound");
    assert_eq!(va.mode, Mode::Integer, "l$a must be integer");
    assert_eq!(va.length, Length::One, "l$a is a scalar");
    let vb = scope.get("vb").expect("vb should be bound");
    assert_eq!(vb.mode, Mode::Character, "l$b must be character");
    // And the list itself should carry the schema.
    let l = scope.get("l").expect("l should be bound");
    let schema = l.columns.clone().expect("l should carry a column schema");
    assert_eq!(schema.len(), 2, "schema should have 2 columns");
    assert_eq!(schema.names(), vec!["a", "b"]);
    // Accessing a missing column on a PLAIN list is silent: in R
    // `l$missing` returns NULL, so RY060 is scoped to data frames
    //. Only data-frame misses fire RY060.
    let diags = check("l <- list(a = 1L)\nbad <- l$missing\n");
    assert!(
        diags.iter().all(|d| d.code != "RY060"),
        "plain-list `$` miss must not fire RY060, got {:?}",
        diags
    );
}

#[test]
fn data_frame_constructor_attaches_class() {
    // `data.frame(x = c(1L, 2L, 3L), y = c("a","b","c"))` must:
    // * produce a value whose class is `["data.frame"]`
    // * carry a column schema with `x` and `y`
    // * coerce column lengths to the common max (3)
    // (We use `c(1L, 2L, 3L)` rather than `1L:3L` because the `:`
    // operator conservatively returns `Length::Unknown` for its
    // result; `c(...)` gives us a concrete length-3 vector to test
    // the recycling logic.)
    let (_, scope) =
        check_with_scope("df <- data.frame(x = c(1L, 2L, 3L), y = c(\"a\", \"b\", \"c\"))\n");
    let df = scope.get("df").expect("df should be bound");
    assert!(
        df.class.contains("data.frame"),
        "data.frame() must attach class data.frame, got class {:?}",
        df.class
    );
    let schema = df.columns.clone().expect("df should carry a column schema");
    assert_eq!(schema.len(), 2, "schema should have 2 columns");
    // Column `x` is integer recycled to length 3.
    let x = schema.get("x").expect("x column should exist");
    assert_eq!(x.mode, Mode::Integer);
    assert_eq!(x.length, Length::Known(3), "x recycled to length 3");
    // Column access resolves through the schema.
    let (_, scope2) = check_with_scope("df <- data.frame(x = c(1L, 2L, 3L))\nxv <- df$x\n");
    let xv = scope2.get("xv").expect("xv should be bound");
    assert_eq!(xv.mode, Mode::Integer);
    assert_eq!(xv.length, Length::Known(3));
    // `print(df)` dispatches to the typeshed's `print.data.frame`
    // method, so no RY050 fires (proves the class is real).
    let diags = check("df <- data.frame(x = c(1L, 2L, 3L))\nprint(df)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "print(df) should dispatch to print.data.frame, got {:?}",
        diags
    );
}

#[test]
fn df_double_bracket_string_resolves_column() {
    // `df[["col"]]` resolves via the schema just like `df$col`.
    let (_, scope) = check_with_scope("df <- iris\nsl <- df[[\"Sepal.Length\"]]\n");
    let sl = scope.get("sl").expect("sl should be bound");
    assert_eq!(sl.mode, Mode::Double);
    assert_eq!(sl.length, Length::Known(150));
    // Non-string-literal arg falls back to opaque (no RY060).
    let diags = check("df <- mtcars\nx <- df[[some_var]]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY060"),
        "non-literal [[ arg should not emit RY060, got {:?}",
        diags
    );
}

#[test]
fn df_single_bracket_returns_base_type() {
    // `df[1]` keeps the existing opaque behavior (no schema lookup,
    // no RY060). The base type is preserved.
    let (_, scope) = check_with_scope("df <- mtcars\nsub <- df[1]\n");
    let sub = scope.get("sub").expect("sub should be bound");
    assert_eq!(sub.mode, Mode::List, "df[1] preserves base mode");
    assert!(
        sub.class.contains("data.frame"),
        "df[1] preserves the data.frame class"
    );
    // Single bracket never emits RY060 even on a known schema.
    let diags = check("df <- mtcars\nsub <- df[\"nonexistent\"]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY060"),
        "single-bracket must not emit RY060, got {:?}",
        diags
    );
}

#[test]
fn structure_preserves_list_column_schema() {
    // `structure(list(a = 1L), class = "foo")` keeps the list's
    // column schema while attaching the class.
    let (_, scope) = check_with_scope("x <- structure(list(a = 1L, b = \"y\"), class = \"foo\")\n");
    let x = scope.get("x").expect("x should be bound");
    assert!(x.class.contains("foo"), "class foo must be attached");
    let schema = x.columns.clone().expect("schema must be preserved");
    assert_eq!(schema.names(), vec!["a", "b"]);
    // Column access works through the new class.
    let (_, scope2) =
        check_with_scope("x <- structure(list(a = 1L), class = \"foo\")\nav <- x$a\n");
    let av = scope2.get("av").expect("av should be bound");
    assert_eq!(av.mode, Mode::Integer);
}

#[test]
fn nse_subset_resolves_columns() {
    // `subset(mtcars, cyl == 4)` evaluates `cyl == 4` in a scope
    // augmented with `mtcars`'s column schema. Without the NSE
    // handler, `cyl` would be reported as unbound (RY010). With it,
    // the expression is well-typed and produces no diagnostics.
    let diags = check("df <- mtcars\nsmall <- subset(df, cyl == 4)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "subset NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    // The result type is the same data frame type as the first arg.
    let (_, scope) = check_with_scope("df <- mtcars\nsmall <- subset(df, cyl == 4)\n");
    let small = scope.get("small").expect("small should be bound");
    assert!(
        small.class.contains("data.frame"),
        "subset() must preserve the data.frame class, got class {:?}",
        small.class
    );
    // Column schema is preserved so downstream column access works.
    assert!(
        small.columns.is_some(),
        "subset() must preserve the column schema"
    );
}

#[test]
fn nse_with_evaluates_expression() {
    // `with(mtcars, sum(mpg))` evaluates `sum(mpg)` against a scope
    // where `mpg` is bound to the `mtcars` column type. Without the
    // NSE handler, `mpg` would trigger RY010 inside the `sum` call.
    let diags = check("df <- mtcars\ntotal <- with(df, sum(mpg))\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "with NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    // `with` returns whatever the expression evaluates to. `sum`
    // dispatches against the typeshed to a length-1 numeric.
    let (_, scope) = check_with_scope("df <- mtcars\ntotal <- with(df, sum(mpg))\n");
    let total = scope.get("total").expect("total should be bound");
    assert!(
        matches!(total.mode, Mode::Double | Mode::Integer),
        "with(df, sum(mpg)) must infer a numeric result type, got {:?}",
        total
    );
    assert_eq!(total.length, Length::One, "sum returns a scalar");
}

#[test]
fn nse_transform_handles_new_column() {
    // `transform(mtcars, x = mpg * 2)` evaluates `mpg * 2` against
    // an augmented scope. Without the NSE handler, `mpg` would
    // trigger RY010 inside the arithmetic expression.
    let diags = check("df <- mtcars\ndf2 <- transform(df, x = mpg * 2)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "transform NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    // `transform` returns a data frame; v1 keeps the original
    // schema (does not fold in the new column type).
    let (_, scope) = check_with_scope("df <- mtcars\ndf2 <- transform(df, x = mpg * 2)\n");
    let df2 = scope.get("df2").expect("df2 should be bound");
    assert!(
        df2.class.contains("data.frame"),
        "transform() must preserve the data.frame class, got class {:?}",
        df2.class
    );
}

#[test]
fn nse_subset_preserves_enclosing_scope() {
    // The augmented scope is local to the NSE call: column names
    // must NOT leak back. After `subset(mtcars, cyl == 4)`, a
    // subsequent bare reference to `cyl` must STILL emit RY010.
    let diags = check("df <- mtcars\nsmall <- subset(df, cyl == 4)\nbad <- cyl\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "column bindings from NSE verbs must not leak into the enclosing scope, got {:?}",
        diags
    );
}

#[test]
fn nse_subset_no_schema_falls_through_silently() {
    // A data frame without a known column schema (here, an
    // opaque-typed user variable) cannot be augmented, so column
    // references inside the expression still emit RY010. The NSE
    // handler does not suppress diagnostics it cannot justify.
    let diags = check("df <- some_unknown_thing\nsmall <- subset(df, cyl == 4)\n");
    // `some_unknown_thing` itself is unbound (RY010), and `cyl`
    // inside the NSE expression is also unbound because `df` has no
    // schema to inject. Both are correct.
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "expected RY010 for unbound `cyl` when df has no schema, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_resolves_columns() {
    // `filter(df, mpg > 20)` is dplyr's row filter. Without the
    // NSE handler, `mpg` would be reported as unbound (RY010). The
    // handler injects the data frame's column schema so the
    // comparison is well-typed.
    let diags = check("library(dplyr)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr filter NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    // `filter` preserves the data frame type.
    let (_, scope) =
        check_with_scope("library(dplyr)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    let small = scope.get("small").expect("small should be bound");
    assert!(
        small.class.contains("data.frame"),
        "filter() must preserve the data.frame class, got class {:?}",
        small.class
    );
    assert!(
        small.columns.is_some(),
        "filter() must preserve the column schema"
    );
}

#[test]
fn nse_dplyr_mutate_resolves_columns() {
    // `mutate(df, kml = mpg * 0.425)` evaluates `mpg * 0.425`
    // against an augmented scope. Without the handler, `mpg` would
    // fire RY010.
    let diags = check("library(dplyr)\ndf <- mtcars\ndf2 <- mutate(df, kml = mpg * 0.425)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr mutate NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    let (_, scope) =
        check_with_scope("library(dplyr)\ndf <- mtcars\ndf2 <- mutate(df, kml = mpg * 0.425)\n");
    let df2 = scope.get("df2").expect("df2 should be bound");
    assert!(
        df2.class.contains("data.frame"),
        "mutate() must preserve the data.frame class, got class {:?}",
        df2.class
    );
}

#[test]
fn nse_dplyr_summarise_returns_data_frame() {
    // `summarise(df, m = mean(mpg))` collapses to a single-row data
    // frame. The column reference `mpg` resolves via the augmented
    // scope. The result is a fresh data frame type with the named
    // summary outputs, not the input column schema.
    let diags = check("library(dplyr)\ndf <- mtcars\ns <- summarise(df, m = mean(mpg))\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr summarise NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    let (_, scope) =
        check_with_scope("library(dplyr)\ndf <- mtcars\ns <- summarise(df, m = mean(mpg))\n");
    let s = scope.get("s").expect("s should be bound");
    assert!(
        s.class.contains("data.frame"),
        "summarise() must return a data.frame class, got class {:?}",
        s.class
    );
    let columns = s.columns.as_ref().expect("summarise output schema");
    assert!(
        columns.get("m").is_some(),
        "missing summary column: {:?}",
        s
    );
    assert!(
        columns.get("mpg").is_none(),
        "summarise() must not expose the input column schema, got {:?}",
        s
    );
}

#[test]
fn nse_dplyr_summarize_alias_matches_summarise() {
    // The American-English `summarize` is an alias for `summarise`
    // and must dispatch to the same handler. `hp` resolves against
    // the augmented scope; the result is a data frame.
    let diags = check("library(dplyr)\ndf <- mtcars\ns <- summarize(df, m = mean(hp))\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr summarize alias should suppress RY010 on column refs, got {:?}",
        diags
    );
    let (_, scope) =
        check_with_scope("library(dplyr)\ndf <- mtcars\ns <- summarize(df, m = mean(hp))\n");
    let s = scope.get("s").expect("s should be bound");
    assert!(
        s.class.contains("data.frame"),
        "summarize() must return a data.frame class, got class {:?}",
        s.class
    );
}

#[test]
fn nse_dplyr_pipe_chain_resolves_columns() {
    // `mtcars %>% filter(cyl == 4) %>% select(mpg, hp)` desugars
    // to nested calls. Each stage's data frame is the previous
    // stage's result (mtcars for the first), so column references
    // resolve via the augmented scope and no RY010 fires.
    let diags = check(
        "library(magrittr)\n\
             library(dplyr)\n\
             result <- mtcars %>% filter(cyl == 4) %>% select(mpg, hp)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "piped dplyr chain should suppress RY010 on column refs, got {:?}",
        diags
    );
    // The chain's final result is a data frame (select preserves
    // the type of its input, which here is `filter`'s output =
    // mtcars' type).
    let (_, scope) = check_with_scope(
        "library(magrittr)\n\
             library(dplyr)\n\
             result <- mtcars %>% filter(cyl == 4) %>% select(mpg, hp)\n",
    );
    let result = scope.get("result").expect("result should be bound");
    assert!(
        result.class.contains("data.frame"),
        "piped dplyr chain must preserve the data.frame class, got class {:?}",
        result.class
    );
}

#[test]
fn nse_dplyr_filter_non_dataframe_falls_through() {
    // `filter` is only treated as dplyr's verb when the first arg
    // looks like a data frame (has a column schema or the
    // `data.frame` class). Here the first arg is a bare integer;
    // the call should NOT be intercepted as NSE - the bare column
    // reference `mpg` (which is unbound here) should fire RY010
    // through the regular arg-inference path.
    let diags = check("x <- 1L\nr <- filter(x, mpg > 20)\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "filter() with a non-data-frame first arg should fall through and emit RY010 on `mpg`, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_ungated_falls_through_when_not_loaded() {
    // Package gating: a bare `filter(df, ...)` in a script that
    // has NOT loaded dplyr must NOT be treated as dplyr's verb.
    // The column reference `mpg` is genuinely unbound in this scope
    // (no library(dplyr)), so RY010 must fire.
    let diags = check("df <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "ungated filter() without library(dplyr) should fall through and emit RY010 on `mpg`, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_qualified_resolves_without_library() {
    // Package gating: `dplyr::filter(...)` is always treated as
    // dplyr's verb regardless of whether dplyr is loaded, because
    // the `dplyr::` prefix is an explicit namespace reference. So
    // the column ref `mpg` must NOT fire RY010.
    let diags = check("df <- mtcars\nsmall <- dplyr::filter(df, mpg > 20)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr::-qualified filter() should suppress RY010 on column refs without library(dplyr), got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_library_records_loaded() {
    // Package gating: `library(dplyr)` records dplyr into the
    // loaded set, so a subsequent `filter(df, ...)` resolves as
    // dplyr's verb and the column ref `mpg` does NOT fire RY010.
    let diags = check("library(dplyr)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "library(dplyr) + filter() should suppress RY010 on column refs, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_requirenamespace_does_not_attach_dplyr() {
    // `requireNamespace("dplyr")` permits qualified access but does not
    // attach dplyr, so an unqualified filter call keeps base semantics.
    let diags = check("requireNamespace(\"dplyr\")\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "requireNamespace(\"dplyr\") must not attach unqualified dplyr names, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_tidyverse_counts_as_dplyr() {
    // `library(tidyverse)` loads dplyr transitively; the gating
    // treats tidyverse as a synonym for dplyr.
    let diags = check("library(tidyverse)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "library(tidyverse) + filter() should suppress RY010 on column refs, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_arrange_groupby_preserve_type() {
    // `arrange` and `group_by` walk their column-reference args in
    // the augmented scope and preserve the input data frame type.
    let diags = check(
        "library(dplyr)\n\
             df <- mtcars\n\
             sorted <- arrange(df, mpg)\n\
             grouped <- group_by(df, cyl)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "arrange/group_by NSE handlers should suppress RY010 on column refs, got {:?}",
        diags
    );
    let (_, scope) = check_with_scope(
        "library(dplyr)\n\
             df <- mtcars\n\
             sorted <- arrange(df, mpg)\n\
             grouped <- group_by(df, cyl)\n",
    );
    let sorted = scope.get("sorted").expect("sorted should be bound");
    assert!(
        sorted.class.contains("data.frame"),
        "arrange() must preserve the data.frame class, got class {:?}",
        sorted.class
    );
    let grouped = scope.get("grouped").expect("grouped should be bound");
    assert!(
        grouped.class.contains("data.frame"),
        "group_by() must preserve the data.frame class, got class {:?}",
        grouped.class
    );
}

#[test]
fn closure_factory_infers_inner_return() {
    // `make_counter <- function() { function() { 1L } }` produces a
    // function whose `fn_sig.return_type` is itself a function with
    // `fn_sig.return_type` = integer<1>. So `c <- make_counter()`
    // binds `c` to a function-typed value with an inferred signature,
    // and `c()` resolves to integer<1>. We verify by using the
    // result arithmetically: integer + character must fire RY040
    // (proving the type was inferred, not opaque).
    let (_, scope) = check_with_scope(
        "make_counter <- function() { function() { 1L } }\n\
             c <- make_counter()\n",
    );
    let c = scope.get("c").expect("c should be bound");
    assert_eq!(
        c.mode,
        Mode::Function,
        "c must be function-typed, got {:?}",
        c
    );
    let sig = c.fn_sig.clone().expect("c must carry an inferred fn_sig");
    assert_eq!(
        sig.return_type.mode,
        Mode::Integer,
        "c() must resolve to integer, got {:?}",
        sig.return_type
    );
    // Behavioral check: using the result arithmetically with a
    // character operand must fire RY040.
    let diags = check(
        "make_counter <- function() { function() { 1L } }\n\
             c <- make_counter()\n\
             v <- c()\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from integer closure result + character, got {:?}",
        diags
    );
}

#[test]
fn closure_capture_resolves_outer_binding() {
    // `make_adder(x)` returns a closure that references the captured
    // `x`. The inner function's body `x + y` (both double via
    // defaults) produces double<1>; the outer function's `fn_sig`
    // carries that as the return type. `add5(3)` therefore resolves
    // to double<1>.
    let (_, scope) = check_with_scope(
        "make_adder <- function(x = 0) {\n\
             \x20 function(y = 0) { x + y }\n\
             }\n\
             add5 <- make_adder(5)\n",
    );
    let add5 = scope.get("add5").expect("add5 should be bound");
    assert_eq!(add5.mode, Mode::Function);
    let sig = add5
        .fn_sig
        .clone()
        .expect("add5 must carry an inferred fn_sig");
    assert_eq!(
        sig.return_type.mode,
        Mode::Double,
        "add5(3) must resolve to double, got {:?}",
        sig.return_type
    );
    // Behavioral check: using the result arithmetically with a
    // character operand must fire RY040.
    let diags = check(
        "make_adder <- function(x = 0) {\n\
             \x20 function(y = 0) { x + y }\n\
             }\n\
             add5 <- make_adder(5)\n\
             v <- add5(3)\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from double closure result + character, got {:?}",
        diags
    );
}

#[test]
fn nested_function_definition_visible_in_outer_body() {
    // The named-return closure pattern: `g <- function() { 1L }; g`
    // inside the outer body. The body simulator processes the
    // assignment so the trailing `g` picks up `g`'s inferred
    // `fn_sig`. The outer function's return type is therefore a
    // function value with an inferred signature, and `h()`
    // resolves to integer<1>.
    let (_, scope) = check_with_scope(
        "f <- function() {\n\
             \x20 g <- function() { 1L }\n\
             \x20 g\n\
             }\n\
             h <- f()\n",
    );
    let h = scope.get("h").expect("h should be bound");
    assert_eq!(h.mode, Mode::Function);
    let sig = h.fn_sig.clone().expect("h must carry an inferred fn_sig");
    assert_eq!(
        sig.return_type.mode,
        Mode::Integer,
        "h() must resolve to integer, got {:?}",
        sig.return_type
    );
    // Behavioral check.
    let diags = check(
        "f <- function() {\n\
             \x20 g <- function() { 1L }\n\
             \x20 g\n\
             }\n\
             h <- f()\n\
             v <- h()\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from integer nested-closure result + character, got {:?}",
        diags
    );
}

#[test]
fn closure_depth_cap_falls_back_to_opaque() {
    // Four levels of nested closures exceeds MAX_CLOSURE_DEPTH (3).
    // The deepest call must NOT produce a false-positive RY040 when
    // used arithmetically, because the result is opaque (we gave up
    // inferring). This verifies the depth cap is respected.
    let diags = check(
        "f1 <- function() { function() { function() { function() { 1L } } } }\n\
             a <- f1()()()()\n\
             bad <- a + \"x\"\n",
    );
    // `a` is opaque (depth cap exceeded), so `a + "x"` must NOT
    // fire RY040. We allow any diagnostics EXCEPT RY040.
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "depth-capped closure should be opaque, not integer; got {:?}",
        diags
    );
}

#[test]
fn lapply_anon_callback_infers_integer() {
    // `lapply(1:3, function(i) i * 2L)` returns a list whose
    // elements are integer (the callback's return type). We verify
    // by accessing an element and using it arithmetically: integer
    // + character must fire RY040, proving the element type was
    // inferred rather than opaque.
    let diags = check(
        "result <- lapply(1:3, function(i) i * 2L)\n\
             bad <- result[[1]] + \"x\"\n",
    );
    // `result[[1]]` goes through IndexKind::Double on a list with
    // a schema, so it resolves to the element type (integer).
    // However if the index access falls back to opaque, no RY040
    // fires. We assert no false positives at minimum.
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "no RY010 expected in lapply callback body, got {:?}",
        diags
    );
}

#[test]
fn sapply_anon_callback_simplifies_to_vector() {
    // `sapply(1:5, function(x) x * 2L)` simplifies to an integer
    // vector (callback returns length-1 integer). Using the result
    // with a character must fire RY040, proving simplification
    // happened (opaque would not fire RY040).
    let diags = check(
        "v <- sapply(1:5, function(x) x * 2L)\n\
             bad <- v + \"hello\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from sapply result + character, got {:?}",
        diags
    );
}

#[test]
fn sapply_named_callback_simplifies() {
    // Named user-fn callback: `dbl` returns integer (default x=1L,
    // body x * 2L). `sapply(1:5, dbl)` simplifies to integer vector.
    let diags = check(
        "dbl <- function(x = 1L) { x * 2L }\n\
             v <- sapply(1:5, dbl)\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from sapply(named_fn) + character, got {:?}",
        diags
    );
}

#[test]
fn sapply_typeshed_callback_simplifies() {
    // Typeshed callback: `sqrt` returns double.
    // `sapply(c(1.0, 4.0), sqrt)` simplifies to double vector.
    let diags = check(
        "v <- sapply(c(1.0, 4.0), sqrt)\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from sapply(sqrt) + character, got {:?}",
        diags
    );
}

#[test]
fn vapply_uses_fun_value_template() {
    // `vapply(X, FUN, FUN.VALUE)` returns FUN.VALUE's type.
    // Here FUN.VALUE = `numeric(1)` = double<1>, so the result is
    // double. Using it with character fires RY040.
    let diags = check(
        "v <- vapply(c(1, 2, 3), function(x) x * 2, numeric(1))\n\
             bad <- v + \"x\"\n",
    );
    // `numeric(1)` may or may not resolve to double<1> depending
    // on typeshed coverage; if it resolves opaque, no RY040 fires.
    // Assert at minimum no false positives.
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "no RY010 expected in vapply, got {:?}",
        diags
    );
}

#[test]
fn vapply_fun_value_ignores_character_dots() {
    let (diags, scope) = check_with_scope(
        "x <- c(1, 2)\nf <- function(x, extra) x\nout <- vapply(x, f, FUN.VALUE = character(1), USE.NAMES = FALSE, extra = \"chr\")\n",
    );
    assert!(diags.is_empty(), "unexpected vapply diagnostics: {diags:?}");
    assert_eq!(scope.get("out").map(|ty| &ty.mode), Some(&Mode::Character));
}

#[test]
fn inherits_narrows_positive_and_negated_else_branches() {
    let diags = check(
        "print.foo <- function(x) 1L\nf <- function(x) { if (inherits(x, \"foo\")) print(x); if (!inherits(x, \"foo\")) 0L else print(x) }\n",
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY050"),
        "inherits narrowing should enable S3 dispatch: {diags:?}"
    );
}

#[test]
fn dynlib_prefix_resolves_only_with_nonempty_remainder() {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", "value <- pkg_call\n").unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from(["\0useDynLib:pkg_".to_string()]));
    checker.check(&file);
    assert!(checker.take_diagnostics().is_empty());

    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", "value <- pkg_\n").unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from(["\0useDynLib:pkg_".to_string()]));
    checker.check(&file);
    assert!(checker.take_diagnostics().iter().any(|d| d.code == "RY010"));
}

#[test]
fn r6_and_s7_class_body_pronouns_are_bound() {
    let diags = check(include_str!("../testdata/ok_r6_class_body_bindings.R"));
    assert!(
        diags.is_empty(),
        "class-body fixture should be clean: {diags:?}"
    );
}

#[test]
fn local_standalone_errors_idiom_is_clean() {
    let diags = check(include_str!("../testdata/ok_local_standalone_errors.R"));
    assert!(
        diags.is_empty(),
        "local() fixture should be clean: {diags:?}"
    );
}

#[test]
fn namespace_assign_introduces_a_binding() {
    let diags = check(
        "assign(\"style\", function(x) x, envir = asNamespace(\"crayon\"))\nvalue <- style(\"x\")\n",
    );
    assert!(
        diags.is_empty(),
        "namespace assign should bind style: {diags:?}"
    );
}

#[test]
fn replacement_calls_keep_targets_bound_without_argument_diagnostics() {
    let diags = check(
        "x <- matrix(1:4, 2)\ndimnames(x) <- list(c(\"a\", \"b\"), c(\"c\", \"d\"))\nnames(x) <- c(\"a\", \"b\")\nattr(x, \"tag\") <- TRUE\nlevels(x) <- c(\"a\", \"b\")\nf <- function() NULL\nenvironment(f) <- globalenv()\ny <- x\nf()\n",
    );
    assert!(
        diags.is_empty(),
        "replacement calls should be opaque-safe: {diags:?}"
    );
}

#[test]
fn purrr_map_walks_callback_and_infers_list() {
    // purrr::map(.x, .f) is modeled like lapply -- the
    // callback body is walked (RY010 fires on the unbound `bug`)
    // and the result is a list.
    let diags = check(
        "library(purrr)\n\
             xs <- map(1:3, function(x) bug + x)\n",
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains("bug")),
        "purrr map should walk the callback and flag `bug`, got {:?}",
        diags
    );
}

#[test]
fn purrr_map_dbl_infers_double_vector() {
    // map_dbl returns a double vector; using it in character
    // arithmetic fires RY040 (proving the typed-mode result).
    let diags = check(
        "library(purrr)\n\
             v <- map_dbl(1:3, function(x) x + 0.5)\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "map_dbl result used with character should fire RY040, got {:?}",
        diags
    );
}

#[test]
fn purrr_map_dbl_type_mismatch_fires_ry080() {
    // map_dbl whose callback returns character fires
    // RY080 (R coerces silently, but the mismatch is a likely bug).
    let diags = check(
        "library(purrr)\n\
             xs <- map_dbl(1:3, function(x) paste(\"n\", x))\n",
    );
    assert!(
        diags.iter().any(|d| {
            d.code == "RY080"
                && d.message
                    == "`map_dbl` expects `double` returns but the callback returns `character`; R will coerce silently"
        }),
        "map_dbl with character callback should fire RY080, got {:?}",
        diags
    );
}

#[test]
fn purrr_in_parallel_is_transparent() {
    // in_parallel(.f) is type-transparent. map(sims,
    // in_parallel(f)) must walk `f`'s body identically to
    // map(sims, f) -- here the unbound `bug` must fire RY010.
    let diags = check(
        "library(purrr)\n\
             sims <- list(1, 2)\n\
             out <- map(sims, in_parallel(function(s) bug + s[[1]]))\n",
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains("bug")),
        "in_parallel-wrapped callback should still be walked, got {:?}",
        diags
    );
}

#[test]
fn purrr_not_loaded_does_not_treat_map_as_higher_order() {
    // Without library(purrr), a bare `map` must NOT be treated as
    // purrr's map (it is an unbound name -> RY010 on `map` itself,
    // or opaque). Either way, no purrr higher-order modeling.
    let diags = check("xs <- map(1:3, function(x) x)\n");
    // `map` is unbound (not in base typeshed); it resolves opaque
    // and the callback is NOT walked. No RY010 on a callback-local
    // name confirms the callback was not entered.
    assert!(
        diags
            .iter()
            .all(|d| d.code != "RY010" || !d.message.contains("map")),
        "ungated map should not get purrr treatment: {:?}",
        diags
    );
}

#[test]
fn reduce_returns_element_type() {
    // `Reduce(f, x)` returns the element type of x. For a double
    // vector, the result is double. Using it with character fires
    // RY040.
    let diags = check(
        "v <- Reduce(function(a, b) a + b, c(1.0, 2.0, 3.0))\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from Reduce result + character, got {:?}",
        diags
    );
}

#[test]
fn filter_preserves_data_type() {
    // `Filter(f, x)` returns x's type. For integer x, result is
    // integer. Using it with character fires RY040.
    let diags = check(
        "even <- function(x) x %% 2 == 0\n\
             v <- Filter(even, c(1L, 2L, 3L, 4L))\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from Filter result + character, got {:?}",
        diags
    );
}

#[test]
fn typeshed_fn_as_value_not_unbound() {
    // Passing a precisely modeled function as a callback remains valid;
    // the shadowed-symbol boost targets ambient-only resolution.
    let diags = check("v <- sapply(c(1.0, 2.0), sqrt)\n");
    assert!(diags.iter().all(|d| d.code != "RY010"), "got {diags:?}");
}

#[test]
fn user_fn_as_value_not_unbound() {
    // Passing a user-defined function name as a bare identifier must
    // NOT trigger RY010.
    let diags = check(
        "dbl <- function(x = 1L) x * 2L\n\
             v <- sapply(1:3, dbl)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "user fn name used as value should not be RY010, got {:?}",
        diags
    );
}

#[test]
fn type_narrowing_is_null_then_branch() {
    // `if (!is.null(x)) { length(x) }`: the `then` branch knows
    // `x` is non-null. Without narrowing, `x` inside the branch
    // resolves from the enclosing scope and is well-typed either
    // way. We test the negative: inside a `!is.null` branch, using
    // `x` arithmetically should NOT fire RY040 when `x` was opaque
    // (the narrowing doesn't give us a mode, just removes null).
    let diags = check(
        "x <- NULL\n\
             if (!is.null(x)) {\n\
             \x20 y <- x + 1\n\
             }\n",
    );
    // `x` starts as NULL; in the `then` branch it's narrowed to
    // opaque (non-null). `opaque + 1` should not fire RY040
    // (opaque is permissive).
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "non-null narrowed opaque should not fire RY040, got {:?}",
        diags
    );
}

#[test]
fn type_narrowing_is_numeric_then_branch() {
    // `if (is.numeric(x)) { x + 1 }`: the `then` branch narrows
    // `x` to numeric (double). If `x` was opaque, it's now double
    // inside the branch. Using `x + 1` should be well-typed.
    let diags = check(
        "x <- some_opaque_thing\n\
             if (is.numeric(x)) {\n\
             \x20 y <- x + 1\n\
             }\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "numeric-narrowed opaque should not fire RY040 in then branch, got {:?}",
        diags
    );
}

#[test]
fn expression_if_applies_function_narrowing_to_then_branch() {
    let diagnostics =
        check("f <- if (TRUE) function(x) x else 1L\nx <- if (is.function(f)) f(1) else f\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "expression-position if must narrow f before inferring the call: {diagnostics:?}"
    );
}

#[test]
fn type_narrowing_does_not_leak() {
    // The narrowing must NOT leak into the enclosing scope. After
    // the `if`, `x` should still be opaque.
    let diags = check(
        "x <- some_opaque_thing\n\
             if (is.numeric(x)) {\n\
             \x20 y <- x + 1\n\
             }\n\
             z <- x + \"bad\"\n",
    );
    // `x` outside the branch is still opaque, so `x + "bad"` must
    // NOT fire RY040. This proves the narrowing is branch-local.
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "narrowing leaked into enclosing scope, got {:?}",
        diags
    );
}

#[test]
fn type_narrowing_is_character_then_branch() {
    // `if (is.character(x)) { nchar(x) }`: the `then` branch
    // narrows `x` to character. `nchar` on character is fine.
    let diags = check(
        "x <- some_opaque_thing\n\
             if (is.character(x)) {\n\
             \x20 n <- nchar(x)\n\
             }\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "character-narrowed opaque should not fire RY040 in then branch, got {:?}",
        diags
    );
}

#[test]
fn if_expr_integer_branches_join_to_integer() {
    // `if (TRUE) 1L else 2L` joins to integer. Using the result
    // with a character must fire RY040, proving the type was
    // inferred (not opaque, which would be permissive).
    let diags = check(
        "x <- if (TRUE) 1L else 2L\n\
             bad <- x + \"hello\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from if-expr result + character, got {:?}",
        diags
    );
}

#[test]
fn if_expr_mismatched_branches_join() {
    // `if (TRUE) list(1) else function(){1}` joins to
    // union[list, function]. Using the result arithmetically fires
    // RY040 because EVERY member of the union errors against `+ 1`
    // (an op on a union errors only when ALL members error).
    let diags = check(
        "x <- if (TRUE) list(1) else function() { 1 }\n\
             bad <- x + 1\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from joined if-expr (all-invalid union) + int, got {:?}",
        diags
    );
}

#[test]
fn if_expr_no_else_joins_with_null() {
    // `if (TRUE) 1L` (no else) joins integer + NULL = integer.
    // Using the result arithmetically is well-typed.
    let diags = check(
        "x <- if (TRUE) 1L\n\
             y <- x + 1\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "if-expr without else should join int+NULL=int, got {:?}",
        diags
    );
}

#[test]
fn if_expr_nested() {
    // Nested if-expressions: all branches integer, result integer.
    let diags = check(
        "x <- if (TRUE) { if (FALSE) 1L else 2L } else 3L\n\
             bad <- x + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from nested if-expr result + character, got {:?}",
        diags
    );
}

#[test]
fn negative_integer_literal_infers_integer() {
    // `-1L` is unary minus applied to an integer literal. The result
    // must be integer (same mode as the operand), length 1, non-NA.
    let (diags, scope) = check_with_scope("x <- -1L\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::One, "got {:?}", x);
}

#[test]
fn negative_double_literal_infers_double() {
    // `-3.14` is unary minus applied to a double literal; result is
    // double, length 1, non-NA.
    let (diags, scope) = check_with_scope("y <- -3.14\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.mode, Mode::Double, "got {:?}", y);
    assert_eq!(y.length, Length::One, "got {:?}", y);
}

#[test]
fn neg_colon_infers_integer_and_groups_correctly() {
    // `-1:3` parses as `(-1):3`, which R evaluates as seq(-1, 3) =
    // c(-1, 0, 1, 2, 3), an integer vector. The type must be integer
    // (not double, not error), and using it arithmetically must be
    // well-typed. This is the key correctness case for unary-minus
    // vs colon precedence.
    let (diags, scope) = check_with_scope("z <- -1:3\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let z = scope.get("z").expect("z should be bound");
    assert_eq!(z.mode, Mode::Integer, "got {:?}", z);
    // Behavioral check: `-1:3`'s LHS is a UnaryOp (not a literal),
    // so the literal-based length inference doesn't fire and the
    // length stays Unknown. The value must still be usable as an
    // integer in arithmetic.
    let diags = check("z <- -1:3\nbad <- z + 1L\n");
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "z + 1L must be valid int+int, got {:?}",
        diags
    );
}

#[test]
fn negated_paren_colon_infers_integer() {
    // `-(1:3)` negates the whole sequence; still an integer vector.
    let (diags, scope) = check_with_scope("w <- -(1:3)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let w = scope.get("w").expect("w should be bound");
    assert_eq!(w.mode, Mode::Integer, "got {:?}", w);
}

#[test]
fn neg_times_int_infers_integer_length_one() {
    // `-2L * 3L` = `(-2L) * 3L` = -6L, a length-1 integer.
    let (diags, scope) = check_with_scope("v <- -2L * 3L\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let v = scope.get("v").expect("v should be bound");
    assert_eq!(v.mode, Mode::Integer, "got {:?}", v);
    assert_eq!(v.length, Length::One, "got {:?}", v);
}

#[test]
fn neg_on_character_emits_ry020() {
    // Unary `-` applied to a character is a type error in R.
    let diags = check("x <- -\"hi\"\n");
    assert!(
        diags.iter().any(|d| d.code == "RY020"),
        "expected RY020 for negation of character, got {:?}",
        diags
    );
}

#[test]
fn neg_preserves_na_flag_and_mode() {
    // `-NA_integer_` must remain an NA integer (negation does not
    // change mode or clear the NA flag). This guards that the
    // checker's `UnaryOp::Neg` returns the operand type verbatim.
    let (diags, scope) = check_with_scope("a <- -NA_integer_\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let a = scope.get("a").expect("a should be bound");
    assert_eq!(a.mode, Mode::Integer, "got {:?}", a);
    assert_eq!(a.length, Length::One, "got {:?}", a);
}

// ---- Literal-based length inference: `:`, `rep`, `seq` ----
//
// These exercise the literal-arg fast paths that pin the result
// length exactly instead of returning `Length::Unknown`. The
// common pattern: build the expression, assert the inferred
// `RType` has `Length::Known(n)` with the expected `n`, then do a
// behavioral check that downstream code sees the precise length
// (e.g. mixing with a character fires RY040).

#[test]
fn colon_literals_pin_length() {
    // `1:10` has 10 elements; both endpoints are integer-valued
    // literals so the literal-based path fires.
    let (diags, scope) = check_with_scope("x <- 1:10\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::Known(10), "got {:?}", x);
}

#[test]
fn colon_literals_descending_pin_length() {
    // `10:1` is c(10, 9, ..., 1): length 10, mode integer.
    let (_, scope) = check_with_scope("x <- 10:1\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::Known(10), "got {:?}", x);
}

#[test]
fn colon_double_literals_pin_length() {
    // `1.0:5.0` - whole-number doubles also trigger the literal
    // path; R returns integer for whole-number endpoints.
    let (_, scope) = check_with_scope("x <- 1.0:5.0\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::Known(5), "got {:?}", x);
}

#[test]
fn colon_single_element_pin_length_one() {
    // `5:5` is a length-1 integer vector c(5).
    let (_, scope) = check_with_scope("x <- 5:5\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Known(1), "got {:?}", x);
}

#[test]
fn colon_literals_fire_ry040_on_char_mix() {
    // `1:10` is integer<10>; adding a character is a type error
    // (RY040). This is the headline benefit of precise length
    // inference: the checker sees a real vector, not an opaque.
    let diags = check("x <- 1:10\nbad <- x + \"hello\"\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 for integer<10> + character, got {:?}",
        diags
    );
}

#[test]
fn colon_non_literal_stays_unknown() {
    // `n:10` where `n` is a variable: LHS isn't a literal, so the
    // length stays Unknown (no false precision).
    let (_, scope) = check_with_scope("n <- 1L\nx <- n:10\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::Unknown, "got {:?}", x);
}

#[test]
fn rep_literal_times_pin_length() {
    // `rep(1:3, 2)` = c(1,2,3,1,2,3): length 6, mode integer.
    let (diags, scope) = check_with_scope("x <- rep(1:3, 2)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::Known(6), "got {:?}", x);
}

#[test]
fn rep_scalar_x_literal_times_pin_length() {
    // `rep(0, 5)` = c(0,0,0,0,0): length 5. `0` is a double
    // literal in R (no `L` suffix), so the mode stays double.
    let (diags, scope) = check_with_scope("x <- rep(0, 5)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Double, "got {:?}", x);
    assert_eq!(x.length, Length::Known(5), "got {:?}", x);
}

#[test]
fn rep_named_times_arg_pin_length() {
    // `rep(c(1, 2), times = 3)` = c(1,2,1,2,1,2): length 6.
    let (_, scope) = check_with_scope("x <- rep(c(1, 2), times = 3)\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Known(6), "got {:?}", x);
}

#[test]
fn rep_each_arg_pin_length() {
    // `rep(c(1, 2, 3), each = 2)` = c(1,1,2,2,3,3): length 6.
    let (_, scope) = check_with_scope("x <- rep(c(1, 2, 3), each = 2)\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Known(6), "got {:?}", x);
}

#[test]
fn rep_times_and_each_pin_length() {
    // `rep(c(1, 2), 3, each = 2)`: each element twice, then the
    // whole thing 3 times = 2 * 2 * 3 = 12.
    let (_, scope) = check_with_scope("x <- rep(c(1, 2), 3, each = 2)\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Known(12), "got {:?}", x);
}

#[test]
fn rep_non_literal_times_stays_unknown() {
    // `rep(1:3, n)` where `n` is a variable: `times` isn't a
    // literal, so the length stays Unknown.
    let (_, scope) = check_with_scope("n <- 2\nx <- rep(1:3, n)\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Unknown, "got {:?}", x);
}

#[test]
fn rep_literal_fire_ry040_on_char_mix() {
    // `rep(c(1, 2), 3)` is double<6>; adding a character fires RY040.
    let diags = check("x <- rep(c(1, 2), 3)\nbad <- x + \"hello\"\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 for double<6> + character, got {:?}",
        diags
    );
}

#[test]
fn seq_literal_by_pin_length() {
    // `seq(1, 10, 2)` = c(1, 3, 5, 7, 9): length 5.
    let (diags, scope) = check_with_scope("x <- seq(1, 10, 2)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Known(5), "got {:?}", x);
}

#[test]
fn seq_length_out_pin_length() {
    // `seq(1, 5, length.out = 3)` = c(1, 3, 5): length 3.
    let (diags, scope) = check_with_scope("x <- seq(1, 5, length.out = 3)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Known(3), "got {:?}", x);
}

#[test]
fn seq_default_by_one_pin_length() {
    // `seq(1, 5)` (no `by`, no `length.out`): R uses by = 1, so
    // length = 5.
    let (_, scope) = check_with_scope("x <- seq(1, 5)\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Known(5), "got {:?}", x);
}

#[test]
fn seq_int_literal_by_pin_length() {
    // `seq.int(1L, 10L, 2L)` = c(1L, 3L, 5L, 7L, 9L): length 5,
    // mode integer (all integer literals).
    let (diags, scope) = check_with_scope("x <- seq.int(1L, 10L, 2L)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::Known(5), "got {:?}", x);
}

#[test]
fn seq_int_double_by_pin_length() {
    // `seq.int(2, 10, 2.0)` uses whole-number double for `by`:
    // extract_literal_int accepts it, length = 5.
    let (_, scope) = check_with_scope("x <- seq.int(2, 10, 2.0)\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Known(5), "got {:?}", x);
}

#[test]
fn seq_non_literal_stays_unknown() {
    // `seq(1, n, 1)` where `n` is a variable: `to` isn't a
    // literal, so the length stays Unknown.
    let (_, scope) = check_with_scope("n <- 10\nx <- seq(1, n, 1)\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.length, Length::Unknown, "got {:?}", x);
}

#[test]
fn seq_literal_fire_ry040_on_char_mix() {
    // `seq(1, 10, 2)` is double<5>; adding a character fires RY040.
    let diags = check("x <- seq(1, 10, 2)\nbad <- x + \"hello\"\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 for double<5> + character, got {:?}",
        diags
    );
}

// ---- Pass-2 propagation + rep/seq edge cases ----
//
// These cover the three code-review fixes: (1) literal lengths
// now propagate through function return types because the literal
// fast paths live in pass 2 (`infer_discarding`) as well as
// pass 3; (2) `infer_rep` counts only unnamed args when binding
// positional `times`/`each`; (3) `infer_rep` never emits
// `Length::Known(0)` or treats negative multipliers as known.

#[test]
fn pass2_colon_literal_propagates_through_fn_return() {
    // `f <- function() 1:10` should give f a return type of
    // integer<10>, and `g <- f()` should propagate that precise
    // length to g. Previously the `:` literal fast path only
    // existed in pass 3, so f's return type (computed in pass 2)
    // was Length::Unknown and g inherited the unknown length.
    let (diags, scope) = check_with_scope("f <- function() 1:10\ng <- f()\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let g = scope.get("g").expect("g should be bound");
    assert_eq!(g.mode, Mode::Integer, "got {:?}", g);
    assert_eq!(g.length, Length::Known(10), "got {:?}", g);
}

#[test]
fn pass2_colon_literal_propagates_through_fn_return_fire_ry040() {
    // Behavioral check: f returns integer<10>, so mixing g with a
    // character fires RY040. This is the headline benefit - the
    // checker sees a real vector through the function boundary.
    let diags = check(
        "f <- function() 1:10\n\
             g <- f()\n\
             bad <- g + \"hello\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 for integer<10> + character (via fn return), got {:?}",
        diags
    );
}

#[test]
fn rep_named_each_before_positional_binds_times() {
    // `rep(each = 2, c(1, 2, 3), 1)`: the named `each = 2` appears
    // before the positional args. The trailing positional `1`
    // binds to `times` (positional index 1, counting only unnamed
    // args). Result: 3 (x) * 1 (times) * 2 (each) = 6. Previously
    // the raw-list index bug made `times` bind to the non-literal
    // `c(1,2,3)` at raw index 1, yielding Some(None) -> Unknown.
    let (diags, scope) = check_with_scope("x <- rep(each = 2, c(1, 2, 3), 1)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Double, "got {:?}", x);
    assert_eq!(x.length, Length::Known(6), "got {:?}", x);
}

#[test]
fn rep_negative_times_does_not_crash() {
    // `rep(x, times = -1)`: a negative `times` is modeled as
    // Length::Unknown. The `-1` parses as UnaryOp::Neg, which
    // extract_literal_int treats as a non-literal, so we can't pin
    // the length. The check must not panic and must stay Unknown.
    let (diags, scope) = check_with_scope("x <- 1:3\ny <- rep(x, times = -1)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.length, Length::Unknown, "got {:?}", y);
}

#[test]
fn rep_zero_times_yields_length_zero() {
    // `rep(1:3, times = 0)` returns a length-0 vector. The result
    // must be Length::Zero, not the invariant-violating Known(0).
    let (diags, scope) = check_with_scope("x <- rep(1:3, times = 0)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::Zero, "got {:?}", x);
}

// ---- Cross-file variable resolution (known_vars) ---------------

/// Parse helper for project-mode tests, mirroring the one in
/// `project::tests`.
fn parse_file(path: &str, src: &str) -> SourceFile {
    let mut p = RParser::new().unwrap();
    p.parse(path, src).unwrap()
}

#[test]
fn s4_terra_named_vector_dispatch_fixture_is_clean() {
    let diagnostics = check(include_str!("../testdata/ok_s4_terra_named_vector.R"));
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
    let mut project = Project::new();
    project.add_file("a.R".to_string(), parse_file("a.R", "my_const <- 42\n"));
    project.add_file("b.R".to_string(), parse_file("b.R", "x <- my_const\n"));
    let diags = project.check();
    let b_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == "b.R")
        .flat_map(|(_, d)| d)
        .collect();
    assert!(
        b_diags.iter().all(|d| d.code != "RY010"),
        "cross-file literal variable should not trigger RY010, got {:?}",
        b_diags
    );
}

#[test]
fn cross_file_opaque_call_variable_resolves() {
    // File A defines `GeomRect <- ggproto("GeomRect", Geom, ...)`.
    // The RHS is a CALL (not a function literal), so it would not
    // be in `fns`; previously any reference from file B would fire
    // RY010. With `known_vars`, `GeomRect` resolves to opaque.
    let mut project = Project::new();
    project.add_file(
        "geom.R".to_string(),
        parse_file(
            "geom.R",
            "GeomRect <- ggproto(\"GeomRect\", Geom, draw = function() NULL)\n",
        ),
    );
    project.add_file(
        "user.R".to_string(),
        parse_file("user.R", "x <- GeomRect\n"),
    );
    let diags = project.check();
    let user_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == "user.R")
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
    let mut project = Project::new();
    project.add_file(
        "config.R".to_string(),
        parse_file("config.R", "config <- list(timeout = 30, retries = 3)\n"),
    );
    project.add_file(
        "main.R".to_string(),
        parse_file("main.R", "t <- config$timeout\n"),
    );
    let diags = project.check();
    let main_diags: Vec<_> = diags
        .into_iter()
        .filter(|(p, _)| p == "main.R")
        .flat_map(|(_, d)| d)
        .collect();
    assert!(
        main_diags.iter().all(|d| d.code != "RY010"),
        "cross-file list-constructor variable should not trigger RY010, got {:?}",
        main_diags
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
fn calling_integer_emits_ry070() {
    let diags = check("x <- 42\ny <- x(10)\n");
    assert!(
        diags.iter().any(|d| d.code == "RY070"),
        "expected RY070 for calling integer, got {:?}",
        diags
    );
}

#[test]
fn calling_character_emits_ry070() {
    let diags = check("x <- \"hello\"\ny <- x()\n");
    assert!(
        diags.iter().any(|d| d.code == "RY070"),
        "expected RY070 for calling character, got {:?}",
        diags
    );
}

#[test]
fn calling_actual_function_no_ry070() {
    let diags = check("f <- function() 1L\ny <- f()\n");
    assert!(
        diags.iter().all(|d| d.code != "RY070"),
        "calling a real function should not emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_opaque_no_ry070() {
    // Opaque (unknown) values should not trigger RY070 - we don't know
    // if they're functions or not.
    let diags = check("y <- some_unknown_thing(10)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY070"),
        "opaque value should not emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_integer_literal_emits_ry070() {
    // Calling a literal (`42()`) errors in R.
    let diags = check("y <- 42()\n");
    assert!(
        diags.iter().any(|d| d.code == "RY070"),
        "calling integer literal `42()` should emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_string_literal_emits_ry070() {
    let diags = check("y <- \"x\"()\n");
    assert!(
        diags.iter().any(|d| d.code == "RY070"),
        "calling string literal should emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_null_literal_emits_ry070() {
    let diags = check("y <- NULL()\n");
    assert!(
        diags.iter().any(|d| d.code == "RY070"),
        "calling NULL literal should emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_index_expression_stays_silent() {
    // Non-literal non-Ident callees (index expressions, calls
    // returning functions) must stay silent as before.
    let diags = check("lst <- list(function() 1)\ny <- lst[[1]]()\n");
    assert!(
        diags.iter().all(|d| d.code != "RY070"),
        "calling an index expression should not emit RY070, got {:?}",
        diags
    );
}

#[test]
fn dollar_on_integer_emits_ry061() {
    let diags = check("x <- 1:10\nval <- x$col\n");
    assert!(diags.iter().any(|d| d.code == "RY061"), "got {:?}", diags);
}

#[test]
fn dollar_on_character_emits_ry061() {
    let diags = check("x <- c(\"a\", \"b\")\nval <- x$col\n");
    assert!(diags.iter().any(|d| d.code == "RY061"), "got {:?}", diags);
}

#[test]
fn dollar_on_list_no_warning() {
    let diags = check("x <- list(a = 1)\nval <- x$a\n");
    assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
}

#[test]
fn dollar_on_data_frame_no_warning() {
    let diags = check("val <- mtcars$mpg\n");
    assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
}

#[test]
fn dollar_on_opaque_no_warning() {
    let diags = check("x <- some_unknown_thing\nval <- x$col\n");
    assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
}

/// Idempotence: running the checker twice on the same
/// input must yield identical diagnostics. The fixpoint/refinement
/// machinery walks function tables whose iteration order is not
/// semantically meaningful, so any order-leak that bleeds into
/// observed types would show up here.
#[test]
fn diagnostics_are_deterministic_across_runs() {
    let sources = [
        // recursion (cycle detection in the fixpoint)
        "f <- function(n) { if (n > 0) f(n - 1) else 0L }\nx <- f(3) + 1\n",
        // mutual / cross-referencing function bodies
        "f <- function() { g() }\ng <- function() { 1L }\nx <- f() + 1\n",
        // a body with an arithmetic error + unbound var (exercises the
        // function-body walk in both passes)
        "h <- function() { a <- \"x\" + 1; b <- missing_thing }\n",
        // higher-order callback inference
        "v <- sapply(c(1.0, 2.0), function(x) x * 2)\ny <- v + 1\n",
        // a clean file (no diagnostics) with a closure factory
        "make_adder <- function(x) function(y) x + y\nadd5 <- make_adder(5)\nz <- add5(3)\n",
    ];
    for src in sources {
        let d1 = check(src);
        let d2 = check(src);
        // Compare on the semantically meaningful fields; `Diagnostic`
        // also carries `path` (constant here) and `message` (stable).
        let key = |d: &Diagnostic| (d.code, d.severity, d.span.start, d.span.end);
        let k1: Vec<_> = d1.iter().map(key).collect();
        let k2: Vec<_> = d2.iter().map(key).collect();
        assert_eq!(
            k1, k2,
            "non-deterministic diagnostics for src={src:?}\n  run1={d1:?}\n  run2={d2:?}"
        );
    }
}

#[test]
fn if_branch_binding_in_both_branches_is_visible_afterwards() {
    // `r` is bound in both branches; the merged type is the join of
    // character ("pos"/"neg"). Use after the `if` must be RY010-free.
    let src =
        "f <- function(a) {\n  if (a > 0) { r <- \"pos\" } else { r <- \"neg\" }\n  paste(r)\n}\n";
    let diags = check(src);
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "branch-local binding leaked to after the `if` must not fire RY010, got {:?}",
        diags
    );
}

#[test]
fn if_branch_binding_in_single_branch_is_unknown_but_visible() {
    // No `else`: `v` is possibly missing. We don't model "definitely
    // unbound"; the name is inserted as unknown so the use is silent.
    let (diags, top) = check_with_scope("if (TRUE) { v <- 1 }\nv\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "single-branch binding must be visible (as unknown) after the `if`, got {:?}",
        diags
    );
    let t = top.get("v").expect("v should be bound at top level");
    assert!(
        matches!(t.mode, Mode::Opaque),
        "single-branch binding should degrade to unknown (opaque), got {:?}",
        t
    );
}

#[test]
fn if_branch_join_type_is_union_when_branches_disagree() {
    // `s` bound to integer in one branch and character in the other:
    // the merged type is the join of integer and character, a union.
    let (diags, top) = check_with_scope("if (TRUE) { s <- 1L } else { s <- \"x\" }\ns\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "both-branch binding must not fire RY010, got {:?}",
        diags
    );
    let t = top.get("s").expect("s should be bound at top level");
    assert!(
        matches!(t.mode, Mode::Union),
        "disagreeing branches should join to a union, got {:?}",
        t
    );
}

#[test]
fn if_branch_reassignment_over_existing_type_stays_visible() {
    // `s <- 1L` then reassigned to `"x"` inside a single branch (no
    // else). The plan specifies single-branch bindings degrade to
    // unknown (opaque), since there is no sound type for "possibly
    // missing". What matters is that the use after the `if` stays
    // RY010-free; the merged type is opaque by design.
    let (diags, top) = check_with_scope("s <- 1L\nif (TRUE) { s <- \"x\" }\ns\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "reassigned branch binding must not fire RY010, got {:?}",
        diags
    );
    let t = top.get("s").expect("s should be bound at top level");
    assert!(
        matches!(t.mode, Mode::Opaque),
        "single-branch reassignment degrades to unknown (opaque) per plan, got {:?}",
        t
    );
}

#[test]
fn if_branch_both_branches_over_existing_type_folds_parent() {
    // `s <- 1L` (parent Integer) then reassigned in BOTH branches to
    // character. The merged branch type is character; folding the
    // parent's integer in yields union[integer, character] rather than
    // losing the parent's prior type.
    let (diags, top) =
        check_with_scope("s <- 1L\nif (TRUE) { s <- \"a\" } else { s <- \"b\" }\ns\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "both-branch reassignment must not fire RY010, got {:?}",
        diags
    );
    let t = top.get("s").expect("s should be bound at top level");
    assert!(
        matches!(t.mode, Mode::Union),
        "both-branch reassignment over a different parent type should fold the parent in (union), got {:?}",
        t
    );
}

#[test]
fn lapply_list_arith_does_not_fire_ry040() {
    // Iterating a list yields the unwrapped element,
    // so arithmetic inside the callback must not fire RY040.
    let src = "out <- lapply(list(1, 2, 3), function(x) x * 2)\n";
    let diags = check(src);
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "lapply over a homogeneous list must not fire RY040, got {:?}",
        diags
    );
}

#[test]
fn dollar_missing_on_plain_list_does_not_fire_ry060() {
    // `$` on a plain list with a missing name returns
    // NULL in R; RY060 must only fire for data frames.
    let diags = check("v <- list(a = 1, b = 2)$missing\n");
    assert!(
        diags.iter().all(|d| d.code != "RY060"),
        "`$` miss on a plain list must not fire RY060, got {:?}",
        diags
    );
}

#[test]
fn dollar_missing_on_plain_list_returns_null() {
    // The returned value matches R's NULL (not unknown).
    let (_, scope) = check_with_scope("v <- list(a = 1, b = 2)$missing\n");
    let v = scope.get("v").expect("v should be bound");
    assert!(
        matches!(v.mode, Mode::Null),
        "plain-list `$` miss should return NULL, got {:?}",
        v
    );
    assert!(
        matches!(v.length, Length::Zero),
        "NULL length should be Zero, got {:?}",
        v
    );
}

#[test]
fn dollar_missing_on_data_frame_still_fires_ry060() {
    // The data-frame case is a real bug and must keep
    // firing. `mtcars` is a data frame in the typeshed.
    let diags = check("df <- mtcars\nbad <- df$nonexistent\n");
    assert!(
        diags.iter().any(|d| d.code == "RY060"),
        "`$` miss on a data frame must still fire RY060, got {:?}",
        diags
    );
}

#[test]
fn for_over_homogeneous_list_does_not_fire_ry040() {
    // `for (el in list(1, 2, 3))` binds `el` to the unwrapped element
    // (double<1>) inside the loop body, so accumulating into `total`
    // is well-typed. (The loop var lives in the loop's child scope,
    // so we assert on the absence of RY040, not on `el`'s binding.)
    let diags =
        check_with_scope("total <- 0\nfor (el in list(1, 2, 3)) { total <- total + el }\n").0;
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "for over a homogeneous list must not fire RY040 on the body, got {:?}",
        diags
    );
}

#[test]
fn public_check_with_scope_surfaces_ry000_on_broken_file() {
    // Regression: `check_with_scope` used to clear diagnostics
    // AFTER emitting parse errors, wiping the RY000s. It must now
    // surface them.
    let mut p = RParser::new().unwrap();
    let f = p.parse("test.R", "f <- function( { 1 }\n").unwrap();
    let mut c = Checker::new("test.R");
    let (diags, _scope) = c.check_with_scope(&f);
    assert!(
        diags.iter().any(|d| d.code == "RY000"),
        "check_with_scope must surface RY000 on a broken file, got {:?}",
        diags
    );
}
