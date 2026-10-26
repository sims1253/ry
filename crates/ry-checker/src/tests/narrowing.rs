use super::*;

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
fn project_standalone_checks_do_not_reject_incompatible_parameter_defaults() {
    let mut project = Project::new();
    project.add_file(
        "R/check.R".to_string(),
        parse_file(
            "R/check.R",
            "check_string <- function(x, what = NULL, ..., allow_null = FALSE, allow_na = FALSE, arg = caller_arg(x), call = caller_env()) invisible(NULL)\n",
        ),
    );
    project.add_file(
        "R/bind.R".to_string(),
        parse_file(
            "R/bind.R",
            "bind <- function(.id = NULL) { check_string(.id) }\n",
        ),
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

    let diagnostics = check_with("x <- NULL\nif (is_null(x)) stop(\"missing\")\nx()\n", |c| {
        c.set_loaded(HashSet::from(["rlang".to_string()]))
    });
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY070"),
        "an attached package predicate must not count the same origin twice: {:?}",
        diagnostics
    );

    let diagnostics = check("x <- NULL\nif (is_null(x)) stop(\"missing\")\nx()\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY070"),
        "an unattached package predicate must not establish bare-call provenance: {:?}",
        diagnostics
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
fn if_branch_both_branches_over_existing_type_replace_parent() {
    // `s <- 1L` (parent Integer) then reassigned in BOTH branches to
    // character. Neither path can retain the parent's integer type.
    let (diags, top) =
        check_with_scope("s <- 1L\nif (TRUE) { s <- \"a\" } else { s <- \"b\" }\ns\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "both-branch reassignment must not fire RY010, got {:?}",
        diags
    );
    let t = top.get("s").expect("s should be bound at top level");
    assert!(
        matches!(t.mode, Mode::Character),
        "both-branch reassignment replaces the prior parent type, got {:?}",
        t
    );
}

#[test]
fn branch_narrowing_without_reassignment_keeps_parent_members() {
    let (_, scope) = check_with_scope(
        "x <- if (TRUE) 1L else 'text'\nif (is.character(x)) { nchar(x) } else { x + 1L }\n",
    );
    let members = scope.get("x").unwrap().members.as_ref().unwrap();
    assert!(members.iter().any(|ty| ty.mode == Mode::Integer));
    assert!(members.iter().any(|ty| ty.mode == Mode::Character));
}

#[test]
fn single_branch_function_install_keeps_the_null_member() {
    let (_, scope) = check_with_scope("g <- NULL\nif (flag) g <- function(x) x\n");
    let ty = scope.get("g").unwrap();
    let members = ty.members.as_ref().expect("g should be a union");
    assert!(members.iter().any(|ty| ty.mode == Mode::Null));
    assert!(members.iter().any(|ty| ty.mode == Mode::Function));
}

#[test]
fn zero_iteration_loop_retains_the_prebranch_nonfunction_path() {
    // Loop inference may make the binding opaque. It must never claim
    // every path installs a function when the loop can run zero times.
    let (_, scope) = check_with_scope(
        "g <- NULL\nif (TRUE) { for (i in integer(0)) g <- function(x) x } else g <- function(x) x\n",
    );
    let ty = scope.get("g").unwrap();
    assert!(
        ty.mode == Mode::Opaque
            || ty
                .members
                .as_ref()
                .is_some_and(|members| members.iter().any(|ty| ty.mode == Mode::Null))
    );
}

#[test]
fn compound_guard_refinements_cover_each_parameter_and_stay_in_the_branch() {
    let source = "f <- function(x = 'x', y = 'y') { if (is.character(x) || is.character(y)) 1L else { x$field; y$field }; x$field }; f()";
    let diagnostics = check(source);
    let dollars: Vec<_> = diagnostics.iter().filter(|d| d.code == "RY061").collect();
    assert_eq!(dollars.len(), 1, "{diagnostics:?}");
    assert_eq!(
        &source[dollars[0].span.start..dollars[0].span.end],
        "x$field"
    );
    assert_eq!(dollars[0].span.start, source.rfind("x$field").unwrap());
}

#[test]
fn compound_guard_oracle_has_no_atomic_access_errors() {
    let diagnostics = check(include_str!(
        "../../testdata/oracle/compound_default_guards.R"
    ));
    assert!(
        diagnostics.iter().all(|d| d.code != "RY061"),
        "{diagnostics:?}"
    );
}

#[test]
fn length_lower_bound_guard_reaches_condition_rules() {
    // Issue #649 founding case: the guard proves length >= 2, so the
    // inner condition is a proven length error in R.
    let source = "f <- function(x) {\n  if (is.logical(x) && length(x) > 1L) {\n    if (x) 1L else 0L\n  }\n}";
    let diagnostics = check(source);
    let hits: Vec<_> = diagnostics.iter().filter(|d| d.code == "RY001").collect();
    assert_eq!(hits.len(), 1, "{diagnostics:?}");
    assert_eq!(&source[hits[0].span.start..hits[0].span.end], "x");

    // Every spelling of the same bound: >=, mirrored, negated,
    // early-exit continuation, loop conditions, switch selectors.
    for source in [
        "f <- function(x) { if (length(x) >= 2L) { if (x) 1L } }",
        "f <- function(x) { if (1L < length(x)) { if (x) 1L } }",
        "f <- function(x) { if (!(length(x) < 2L)) { if (x) 1L } }",
        "f <- function(x) { if (length(x) <= 1L) stop('short'); if (x) 1L }",
        "f <- function(x) { if (length(x) > 1L) { while (x) 1L } }",
        "f <- function(x) { if (length(x) > 1L) switch(x, a = 1L) }",
    ] {
        let diagnostics = check(source);
        assert_eq!(
            diagnostics.iter().filter(|d| d.code == "RY001").count(),
            1,
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn length_lower_bound_guard_controls_stay_silent() {
    for source in [
        // > 0 proves nonempty, not above one.
        "f <- function(x) { if (length(x) > 0L) { if (x) 1L } }",
        // The element, not the vector, is the condition.
        "f <- function(x) { if (length(x) > 1L) { if (x[[1]]) 1L } }",
        // Rebinding inside the branch replaces the narrowed type.
        "f <- function(x) { if (length(x) > 1L) { x <- TRUE; if (x) 1L } }",
        "f <- function(x) { if (length(x) > 1L) { x <- x[1]; if (x) 1L } }",
        // The bound is about another binding.
        "f <- function(x, y) { if (length(y) > 1L) { if (x) 1L } }",
        // A computed or fractional bound proves nothing here.
        "f <- function(x, k) { if (length(x) > k) { if (x) 1L } }",
        "f <- function(x) { if (length(x) > 1.5) { if (x) 1L } }",
        // A rebound `length` is not base length.
        "length <- function(x) 5L\nf <- function(x) { if (length(x) > 1L) { if (x) 1L } }",
        "f <- function(x, length = function(y) 5L) { if (length(x) > 1L) { if (x) 1L } }",
        // `c()` controls matched after `...` are not elements, so
        // they add neither an exact element nor a lower bound.
        "a <- c(1L, use.names = FALSE)\nif (a) 1L",
        "a <- c(1L, recursive = FALSE)\nif (a) 1L",
        "f <- function(x) { if (length(x) > 0L) { y <- c(x, use.names = FALSE); if (y) 1L } }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn length_lower_bound_propagates_through_construction_and_joins() {
    // Choosing between length-2 and length-3 vectors keeps "at least
    // 2" through the join and the negation; a logical `if` condition
    // with a proven length above one is RY002.
    let diagnostics = check(
        "f <- function(flag) { x <- if (flag) c(TRUE, FALSE) else c(TRUE, FALSE, TRUE); if (!x) 1L }",
    );
    assert_eq!(
        diagnostics.iter().filter(|d| d.code == "RY002").count(),
        1,
        "{diagnostics:?}"
    );

    // c() sums lower bounds; rep() scales them.
    for source in [
        "f <- function(x) { if (length(x) > 1L) { y <- c(x, TRUE); if (y) 1L } }",
        "f <- function(x) { if (length(x) > 1L) { y <- rep(x, 2); if (y) 1L } }",
    ] {
        let diagnostics = check(source);
        assert_eq!(
            diagnostics.iter().filter(|d| d.code == "RY001").count(),
            1,
            "{source}: {diagnostics:?}"
        );
    }
}
