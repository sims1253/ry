use super::*;

#[test]
fn type_narrowing_predicate_then_branch_stays_well_typed() {
    // `x <- <NULL or opaque>; if (<type predicate over x>) { <use> }`:
    // the `then` branch sees `x` narrowed. The refinement only
    // removes NULL or installs the predicate's mode -- it never
    // fabricates precision -- so the branch-local use stays
    // well-typed and must not fire RY040.
    for (binding, guard, use_expr) in [
        ("x <- NULL\n", "!is.null(x)", "y <- x + 1\n"),
        ("x <- some_opaque_thing\n", "is.numeric(x)", "y <- x + 1\n"),
        (
            "x <- some_opaque_thing\n",
            "is.character(x)",
            "n <- nchar(x)\n",
        ),
    ] {
        let src = format!("{binding}if ({guard}) {{\n  {use_expr}}}\n");
        let diags = check(&src);
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "`{guard}` then branch should not fire RY040, got {diags:?}"
        );
    }
}

#[test]
fn underscored_null_predicate_narrows_default_parameter() {
    let diagnostics = check(
        "bind_rows <- function(..., .id = NULL) {\n\
           if (!is_null(.id)) {\n\
             check_string(.id)\n\
           }\n\
         }\n\
         bind_rows(value = 1L)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY092"),
        "is_null() must narrow like is.null(): {diagnostics:?}"
    );
}

#[test]
fn diverging_guards_narrow_the_guarded_continuation() {
    // A guard whose true branch provably diverges -- stop(), a final
    // return(), or a block ending in stop() -- leaves only the false
    // path for the continuation, where the guarded parameter's NULL
    // default has been narrowed away. The continuation use must
    // therefore not fire the diagnostic the unnarrowed default would.
    let guarded = |params: &str, guard: &str, diverge: &str, tail: &str, call: &str| {
        format!("f <- function({params}) {{\n  if ({guard}) {diverge}\n  {tail}\n}}\n{call}\n")
    };
    for (label, src, suppressed) in [
        (
            "a stop guard makes a default function callable",
            guarded(
                "x, fun = NULL",
                "is.null(fun)",
                "stop(\"fun required\")",
                "fun(x)",
                "f(1)",
            ),
            "RY070",
        ),
        (
            "a stop guard keeps a default value record-like",
            guarded("x = NULL", "is.null(x)", "stop(\"x required\")", "x$field", "f()"),
            "RY061",
        ),
        (
            "a negated predicate stop guard narrows the mode",
            guarded(
                "x = NULL",
                "!is.character(x)",
                "stop(\"x must be character\")",
                "x == \"1\"",
                "f()",
            ),
            "RY033",
        ),
        (
            "a return guard narrows like a stop guard",
            guarded("x = NULL", "is.null(x)", "return(NULL)", "x$field", "f()"),
            "RY061",
        ),
        (
            "a guard block ending in stop diverges",
            guarded(
                "x = NULL",
                "is.null(x)",
                "{ log(\"x required\"); stop(\"x required\") }",
                "x$field",
                "f()",
            ),
            "RY061",
        ),
        (
            "a compound null guard narrows its continuation",
            guarded(
                "x = NULL",
                "is.null(x) || is.na(x)",
                "stop(\"x required\")",
                "x$field",
                "f()",
            ),
            "RY061",
        ),
        (
            "a guard inside a function defined in a loop narrows independently",
            "for (i in 1:2) {\n  read_field <- function(x = NULL) {\n    if (is.null(x)) stop(\"x required\")\n    x$field\n  }\n}\n".to_string(),
            "RY061",
        ),
    ] {
        let diagnostics = check(&src);
        assert!(
            diagnostics.iter().all(|d| d.code != suppressed),
            "{label}: {diagnostics:?}"
        );
    }
}

#[test]
fn non_diverging_guard_does_not_narrow_continuation() {
    let diagnostics = check(
        "apply_fun <- function(x, fun = NULL) {\n\
           if (is.null(fun)) warning(\"fun required\")\n\
           fun(x)\n\
         }\n\
         apply_fun(1)\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY070"),
        "a warning guard must not narrow its continuation: {diagnostics:?}"
    );
}

#[test]
fn project_function_named_abort_does_not_diverge() {
    let diagnostics = check(
        "abort <- function(message) warning(message)\n\
         apply_fun <- function(x, fun = NULL) {\n\
           if (is.null(fun)) abort(\"fun required\")\n\
           fun(x)\n\
         }\n\
         apply_fun(1)\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY070"),
        "a project abort() must not be treated as a terminator: {diagnostics:?}"
    );
}

#[test]
fn diverging_compound_length_guard_makes_continuation_scalar() {
    let diagnostics = check(
        "Primes <- function(n1 = 1, n2 = NULL) {\n\
           if (is.null(n2)) return(1L)\n\
           if (!is.numeric(n2) || length(n2) != 1) stop(\"x\")\n\
           if (n2 > 0) return(2L)\n\
           3L\n\
         }\n\
         omitted <- function() Primes(5)\n\
         explicit_null <- function() Primes(5, NULL)\n\
         explicit_number <- function() Primes(5, 10)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY001"),
        "the false path of length(n2) != 1 proves n2 is scalar: {diagnostics:?}"
    );
}

#[test]
fn reversed_length_guard_and_long_or_chain_make_continuation_scalar() {
    let diagnostics = check(
        "f <- function(x = NULL) {\n\
           if (!is.character(x) || other_check(x) || 1 != length(x)) stop(\"x\")\n\
           if (x == \"ok\") TRUE else FALSE\n\
         }\n\
         f()\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY001"),
        "all false || operands prove scalar character x: {diagnostics:?}"
    );
}

#[test]
fn non_diverging_compound_length_guard_does_not_narrow_continuation() {
    let diagnostics = check(
        "f <- function(x = NULL) {\n\
           if (!is.numeric(x) || length(x) != 1) warning(\"x\")\n\
           if (x > 0) NULL\n\
         }\n\
         f()\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY001"),
        "a warning does not reject the non-scalar path: {diagnostics:?}"
    );
}

#[test]
fn null_return_guard_alone_does_not_prove_non_empty() {
    let diagnostics = check(
        "f <- function(x = NULL) {\n\
           if (is.null(x)) return(NULL)\n\
           if (x > 0) NULL\n\
         }\n\
         f()\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY001"),
        "a non-NULL value may still be a zero-length vector: {diagnostics:?}"
    );
}

#[test]
fn missing_guard_is_ignored_for_type_narrowing() {
    let diagnostics = check(
        "apply_fun <- function(x, fun = NULL) {\n\
           if (missing(fun)) stop(\"fun required\")\n\
           fun(x)\n\
         }\n\
         apply_fun(1)\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY070"),
        "missing(x) proves nothing about x's type: {diagnostics:?}"
    );
}

#[test]
fn known_never_returning_helper_narrows_continuation() {
    let diagnostics = check(
        "fail <- function() stop(\"required\")\n\
         use_fun <- function(fun = NULL) {\n\
           if (is.null(fun)) fail()\n\
           fun()\n\
         }\n\
         use_fun()\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "a helper ending in stop must be recognized as never returning: {diagnostics:?}"
    );
}

#[test]
fn break_guard_narrows_the_rest_of_a_loop_body() {
    let diagnostics = check(
        "for (i in 1:2) {\n\
           fun <- NULL\n\
           if (is.null(fun)) break\n\
           fun()\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "break must make the false guard path the loop-body continuation: {diagnostics:?}"
    );
}

#[test]
fn diverging_else_branch_promotes_then_refinement() {
    let diagnostics = check(
        "use_fun <- function(fun = NULL) {\n\
           if (!is.null(fun)) fun else stop(\"required\")\n\
           fun()\n\
         }\n\
         use_fun()\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "a diverging else branch must promote then-branch refinements: {diagnostics:?}"
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
fn default_parameter_is_list_guard_replaces_incompatible_default_only_in_then_branch() {
    let diagnostics = check(
        "f <- function(x = FALSE) {\n\
           if (is.list(x)) x$enabled else x$enabled\n\
         }\n\
         f()\n",
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY061")
            .count(),
        1,
        "only the unguarded else branch should reject `$`: {diagnostics:?}"
    );
}

#[test]
fn null_default_parameter_access_is_unknown_but_assigned_null_is_preserved() {
    let (_, default_scope) =
        check_with_scope("f <- function(x = NULL) { value <- x$field; value }\nout <- f()\n");
    assert_eq!(
        default_scope.get("out").map(|ty| ty.mode),
        Some(Mode::Opaque),
        "access through a default-null parameter must be unknown"
    );

    let (_, assigned_scope) = check_with_scope("x <- NULL\nvalue <- x$field\n");
    assert_eq!(
        assigned_scope.get("value").map(|ty| ty.mode),
        Some(Mode::Null),
        "directly assigned NULL must retain its existing access result"
    );
}

#[test]
fn null_default_parameter_double_bracket_access_is_unknown() {
    let (_, scope) = check_with_scope(
        "f <- function(x = NULL) { value <- x[[\"field\"]]; value }\nout <- f()\n",
    );
    assert_eq!(scope.get("out").map(|ty| ty.mode), Some(Mode::Opaque));
}

#[test]
fn default_parameter_is_function_guard_replaces_incompatible_default() {
    let diagnostics = check("f <- function(x = FALSE) { if (is.function(x)) x() }\nf()\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "function guard must make a default-logical parameter callable: {diagnostics:?}"
    );
}

#[test]
fn nested_call_argument_assignment_in_if_condition_binds_after_short_circuit() {
    let diagnostics = check(
        "if (TRUE && grepl(\"x\", value <- \"x\")) {\n\
           nchar(value)\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "assignment nested in a condition call argument must bind: {diagnostics:?}"
    );
}

#[test]
fn nested_call_argument_assignment_in_while_condition_binds_after_short_circuit() {
    let diagnostics = check(
        "while (FALSE || grepl(\"x\", value <- \"x\")) {\n\
           nchar(value)\n\
           break\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "assignment nested in a while condition call argument must bind: {diagnostics:?}"
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
fn standalone_check_string_narrows_name_trusted_calls() {
    let diagnostics = check(
        "h2 <- function() {\n\
           choice <- c(\"foo\", \"bar\")\n\
           check_string(choice)\n\
           if (choice == \"foo\") 1 else 2\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY002"),
        "the standalone guard must prove a scalar string: {diagnostics:?}"
    );
}

#[test]
fn standalone_check_string_on_known_incompatible_value_flags_guard() {
    let diagnostics = check(
        "choice <- c(\"foo\", \"bar\")\n\
         check_string(choice)\n",
    );
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY092")
        .expect("a guard that must throw should be flagged");
    assert!(
        diagnostic.message.contains("check_string")
            && diagnostic.message.contains("character<len=2>")
            && diagnostic.message.contains("character"),
        "the diagnostic should describe the impossible guard: {diagnostic:?}"
    );
}

#[test]
fn standalone_check_string_impossible_guard_makes_continuation_unreachable() {
    let diagnostics = check(
        "choice <- c(\"foo\", \"bar\")\n\
         check_string(choice)\n\
         missing_after_throw\n\
         choice + 1\n",
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY092")
            .count(),
        1,
        "the guard should be the only type-mismatch diagnostic: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, "RY010" | "RY040")),
        "the continuation after an impossible guard is unreachable: {diagnostics:?}"
    );
}

#[test]
fn impossible_guard_does_not_hide_diagnostics_in_later_function_bodies() {
    let diagnostics = check(
        "value <- 1L\n\
         check_string(value)\n\
         later <- function() missing_in_function\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY010"),
        "independent function bodies must still be checked: {diagnostics:?}"
    );
}

#[test]
fn standalone_check_string_incompatibility_respects_allowances_and_uncertainty() {
    for source in [
        "value <- NULL\ncheck_string(value, allow_null = TRUE)\n",
        "value <- TRUE\ncheck_string(value, allow_na = TRUE)\n",
        "value <- if (runif(1) > 0.5) \"ok\" else 1L\ncheck_string(value)\n",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY092"),
            "a guard that may succeed must not be flagged: {diagnostics:?}"
        );
    }

    for source in [
        "value <- NULL\ncheck_string(value)\n",
        "value <- list()\ncheck_string(value)\n",
        "value <- if (runif(1) > 0.5) 1L else list()\ncheck_string(value)\n",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY092"),
            "a guard with no compatible runtime value must be flagged: {diagnostics:?}"
        );
    }
}

#[test]
fn standalone_checks_do_not_reject_incompatible_parameter_defaults() {
    let diagnostics = check(
        "bind <- function(.id = NULL, .trace_bottom = NULL) {\n\
           check_string(.id)\n\
           check_environment(.trace_bottom)\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY092"),
        "a default is one call shape, not the parameter's exhaustive type: {diagnostics:?}"
    );
}

#[test]
fn continuation_narrowing_preserves_parameter_default_uncertainty() {
    let diagnostics = check(
        "validate <- function(value = NULL) {\n\
           if (is.null(value)) abort(\"missing\")\n\
           check_string(value)\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY092"),
        "continuation narrowing must retain default-parameter provenance: {diagnostics:?}"
    );
}

#[test]
fn reassigned_parameter_can_make_standalone_check_impossible() {
    let diagnostics = check(
        "validate <- function(value = NULL) {\n\
           value <- NULL\n\
           check_string(value)\n\
           missing_after_guard\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY092"),
        "assignment clears the parameter-default uncertainty: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "the impossible guard makes its continuation unreachable: {diagnostics:?}"
    );
}

#[test]
fn project_standalone_checks_do_not_reject_incompatible_parameter_defaults() {
    let mut parser = RParser::new().unwrap();
    let mut project = Project::new();
    project.add_file(
        "R/check.R".to_string(),
        parser
            .parse(
                "R/check.R",
                "check_string <- function(x, what = NULL, ..., allow_null = FALSE, allow_na = FALSE, arg = caller_arg(x), call = caller_env()) invisible(NULL)\n",
            )
            .unwrap(),
    );
    project.add_file(
        "R/bind.R".to_string(),
        parser
            .parse(
                "R/bind.R",
                "bind <- function(.id = NULL) { check_string(.id) }\n",
            )
            .unwrap(),
    );
    let diagnostics: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY092"),
        "project checking must retain parameter-default provenance: {diagnostics:?}"
    );
}

#[test]
fn standalone_check_string_local_value_collision_is_not_a_guard() {
    let diagnostics = check(
        "check_string <- 1L\n\
         value <- 1L\n\
         check_string(value)\n\
         missing_after_call\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY092"),
        "a local non-function binding is not a standalone guard: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY010"),
        "the rejected call must not make its continuation unreachable: {diagnostics:?}"
    );
}

#[test]
fn standalone_check_data_frame_accepts_multiple_columns() {
    let diagnostics = check(
        "value <- data.frame(x = 1L, y = 2L)\n\
         check_data_frame(value)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY092"),
        "a data frame's column count is not a failed scalar check: {diagnostics:?}"
    );
}

#[test]
fn impossible_guards_in_all_if_arms_make_continuation_unreachable() {
    let diagnostics = check(
        "if (runif(1) > 0.5) {\n\
           left <- c(\"a\", \"b\")\n\
           check_string(left)\n\
         } else {\n\
           right <- c(\"c\", \"d\")\n\
           check_string(right)\n\
         }\n\
         missing_after_if\n",
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY092")
            .count(),
        2,
        "both impossible guards should be diagnosed: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "no path reaches the statement after the if: {diagnostics:?}"
    );
}

#[test]
fn impossible_guard_in_repeat_makes_continuation_unreachable() {
    let diagnostics = check(
        "repeat {\n\
           value <- c(\"a\", \"b\")\n\
           check_string(value)\n\
         }\n\
         missing_after_repeat\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY092"),
        "the impossible guard should be diagnosed: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "the repeat loop cannot reach its continuation: {diagnostics:?}"
    );
}

#[test]
fn standalone_check_string_name_collision_does_not_flag_impossible_guard() {
    let diagnostics = check(
        "check_string <- function(x) nchar(x) > 0\n\
         value <- 1L\n\
         check_string(value)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY092"),
        "an ordinary same-named function is not a standalone guard: {diagnostics:?}"
    );
}

#[test]
fn standalone_check_string_narrows_fingerprinted_user_function() {
    let diagnostics = check(
        "check_string <- function(x, ..., arg = caller_arg(x), call = caller_env()) invisible(NULL)\n\
         choice <- c(\"foo\", \"bar\")\n\
         check_string(choice)\n\
         if (choice == \"foo\") 1 else 2\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY002"),
        "an inlined standalone checker must retain guard semantics: {diagnostics:?}"
    );
}

#[test]
fn standalone_check_string_does_not_narrow_name_collision() {
    let (diagnostics, scope) = check_with_scope(
        "check_string <- function(x) nchar(x) > 0\n\
         choice <- c(\"foo\", \"bar\")\n\
         guard_result <- check_string(choice)\n\
         if (choice == \"foo\") 1 else 2\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY002"),
        "a same-named ordinary user function must not narrow: {diagnostics:?}"
    );
    assert_ne!(
        scope.get("guard_result").map(|ty| ty.mode),
        Some(Mode::Null),
        "a rejected name collision must use the user function's return type"
    );
}

#[test]
fn standalone_check_string_named_subject_and_control_use_formal_matching() {
    let (_, scope) = check_with_scope(
        "choice <- unknown_string_or_null()\n\
         check_string(allow_null = TRUE, x = choice)\n\
         field <- choice$field\n",
    );
    let choice = scope.get("choice").expect("choice should stay bound");
    assert_eq!(choice.mode, Mode::Union, "{choice:?}");
    let members = choice
        .members
        .as_ref()
        .expect("allow_null should produce a populated union");
    assert!(
        members
            .iter()
            .any(|member| member.mode == Mode::Character && member.length == Length::One),
        "the scalar character member must be preserved: {choice:?}"
    );
    assert!(
        members.iter().any(|member| member.mode == Mode::Null),
        "the weakened guard must retain NULL: {choice:?}"
    );
}

#[test]
fn typeshed_predicate_uses_exact_callee_provenance_and_formal_binding() {
    let diagnostics = check("x <- NULL\nif (rlang::is_null(x = x)) stop(\"missing\")\nx()\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "a qualified predicate must narrow its named subject: {diagnostics:?}"
    );

    let diagnostics = check(
        "run <- function(action = NULL) {\n\
           if (!rlang::is_null(action)) action()\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "a negated schema predicate must narrow its true branch: {diagnostics:?}"
    );

    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "test.R",
            "x <- NULL\nif (is_null(x)) stop(\"missing\")\nx()\n",
        )
        .unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_loaded(HashSet::from(["rlang".to_string()]));
    checker.check(&file);
    assert!(
        checker
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "an attached package predicate must not count the same origin twice: {:?}",
        checker.diagnostics
    );

    let mut checker = Checker::new("test.R");
    checker.check(&file);
    assert!(
        checker
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY070"),
        "an unattached package predicate must not establish bare-call provenance: {:?}",
        checker.diagnostics
    );

    for source in [
        "x <- NULL\nif (base::is_null(x)) stop(\"missing\")\nx()\n",
        "x <- NULL\nif (unrelated::is_null(x)) stop(\"missing\")\nx()\n",
        "is_null <- function(x) TRUE\nx <- NULL\nif (is_null(x)) stop(\"missing\")\nx()\n",
        "is_null <- TRUE\nx <- NULL\nif (is_null(x)) stop(\"missing\")\nx()\n",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY070"),
            "unproven predicate provenance must stay silent: {diagnostics:?}"
        );
    }
}

#[test]
fn qualified_standalone_assertion_uses_exact_package_provenance() {
    let diagnostics = check(
        "value <- if (runif(1) > 0.5) \"a\" else c(\"a\", \"b\")\n\
         rlang::check_string(value)\n\
         if (value == \"a\") 1 else 2\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, "RY002" | "RY092")),
        "a qualified assertion with exact provenance must narrow: {diagnostics:?}"
    );
}

#[test]
fn standalone_check_number_whole_narrows_to_scalar_numeric_union() {
    let (diagnostics, scope) = check_with_scope(
        "n <- unknown_number()\n\
         check_number_whole(n)\n\
         if (n > 1) 1 else 2\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, "RY002" | "RY033")),
        "the numeric guard must make the condition scalar numeric: {diagnostics:?}"
    );
    let n = scope.get("n").expect("n should stay bound");
    assert_eq!(n.mode, Mode::Union, "{n:?}");
    let members = n
        .members
        .as_ref()
        .expect("numeric target should be a union");
    assert!(
        [Mode::Integer, Mode::Double].into_iter().all(|mode| members
            .iter()
            .any(|member| member.mode == mode && member.length == Length::One)),
        "the target must be scalar integer-or-double: {n:?}"
    );
}

#[test]
fn stopifnot_installs_positive_predicate_narrowing() {
    let (diagnostics, scope) = check_with_scope(
        "x <- if (runif(1) > 0.5) 1L else c(\"a\", \"b\")\n\
         stopifnot(is.character(x))\n\
         width <- nchar(x)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY092"),
        "character-only use after stopifnot must be accepted: {diagnostics:?}"
    );
    assert_eq!(scope.get("x").map(|ty| ty.mode), Some(Mode::Character));
}

#[test]
fn assert_that_installs_positive_predicate_narrowing() {
    let (diagnostics, scope) = check_with_scope(
        "x <- if (runif(1) > 0.5) \"a\" else 1L\n\
         assert_that(is.numeric(x))\n\
         value <- x + 1\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY040"),
        "numeric use after assert_that must be accepted: {diagnostics:?}"
    );
    let x = scope.get("x").expect("x should stay bound");
    assert_eq!(x.mode, Mode::Union, "{x:?}");
    let members = x
        .members
        .as_ref()
        .expect("numeric target should be a union");
    assert!(
        [Mode::Integer, Mode::Double]
            .into_iter()
            .all(|mode| members.iter().any(|member| member.mode == mode)),
        "assert_that must retain the full numeric target: {x:?}"
    );
}

#[test]
fn namespaced_assert_that_narrows_predicates_but_not_msg() {
    let (_, scope) = check_with_scope(
        "x <- if (runif(1) > 0.5) \"a\" else 1L\n\
         y <- if (runif(1) > 0.5) \"b\" else 2L\n\
         assertthat::assert_that(is.numeric(x), msg = is.character(y))\n",
    );
    let x = scope.get("x").expect("x should stay bound");
    let x_members = x
        .members
        .as_ref()
        .expect("numeric target should be a union");
    assert!(
        x_members
            .iter()
            .all(|member| matches!(member.mode, Mode::Integer | Mode::Double)),
        "the predicate argument must narrow x: {x:?}"
    );
    let y = scope.get("y").expect("y should stay bound");
    let y_members = y.members.as_ref().expect("y should remain a union");
    assert!(
        y_members.iter().any(|member| member.mode == Mode::Integer),
        "the msg expression must not narrow y: {y:?}"
    );
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
    // else). `s` is definitely bound on every path (it exists in the
    // parent), so the merged type is the union of the branch's
    // character and the parent's integer -- NOT opaque. That keeps the
    // use after the `if` RY010-free while preserving the precise type.
    let (diags, top) = check_with_scope("s <- 1L\nif (TRUE) { s <- \"x\" }\ns\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "reassigned branch binding must not fire RY010, got {:?}",
        diags
    );
    let t = top.get("s").expect("s should be bound at top level");
    assert!(
        matches!(t.mode, Mode::Union),
        "parent-defined single-branch reassignment should be a union of parent and branch types, got {:?}",
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
fn diverging_length_guards_narrow_null_defaults_in_the_continuation() {
    for guard in ["!length(x)", "length(x) == 0"] {
        let diagnostics = check(&format!(
            "f <- function(x = NULL) {{ if ({guard}) return(0L); x + 1L }}\n"
        ));
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY040"),
            "a diverging {guard} guard must narrow x away from NULL: {diagnostics:?}"
        );
    }
}
