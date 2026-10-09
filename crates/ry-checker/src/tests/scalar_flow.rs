//! Scalar facts from `stopifnot()` and loop-carried vector paths (RY032).

use super::*;

/// Oracle fixtures owned by this feature. Each one's `# oracle:` header pins
/// RY032: `must-warn RY032` requires it, `must-pass` forbids it.
fn scalar_flow_fixture(name: &str) -> bool {
    let prefixes = [
        "assertion_",
        "branch_subject_",
        "loop_mirrored_",
        "loop_vector_",
        "masked_parenthesis_",
        "masked_short_circuit_",
        "modelbased_option_",
        "stopifnot_",
    ];
    name.ends_with(".R") && prefixes.iter().any(|prefix| name.starts_with(prefix))
}

fn warns_ry032(source: &str) -> bool {
    check(source).iter().any(|d| d.code == "RY032")
}

/// A function body that asserts `x` is scalar, runs `between`, then reads
/// `x` in a later `||`.
fn asserted_then(between: &str) -> String {
    format!(
        "f <- function(x = 1L) {{ stopifnot(x > 0 && TRUE); {between}; if (is.null(x) || x == 1L) TRUE else FALSE }}"
    )
}

#[test]
fn scalar_flow_fixtures_match_their_oracle_tags() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/oracle");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("read oracle fixtures")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| scalar_flow_fixture(name))
        .collect();
    names.sort();
    assert!(names.len() > 30, "fixture discovery broke: {names:?}");
    let failures: Vec<_> = names
        .iter()
        .filter_map(|name| {
            let source = std::fs::read_to_string(dir.join(name)).expect("read fixture");
            let expected = match source.lines().next() {
                Some("# oracle: must-warn RY032") => true,
                Some("# oracle: must-pass") => false,
                other => return Some(format!("{name}: unexpected tag {other:?}")),
            };
            // A must-warn fixture with several `is.null(x) || x == 1L`
            // consumers needs RY032 on each of them, not just one.
            let lines: Vec<_> = check(&source)
                .into_iter()
                .filter(|d| d.code == "RY032")
                .map(|d| d.span.line)
                .collect();
            let consumers: Vec<_> = source
                .lines()
                .enumerate()
                .filter(|(_, line)| line.contains("is.null(x) || x == 1L"))
                .map(|(index, _)| index)
                .collect();
            let satisfied = if !expected {
                lines.is_empty()
            } else if consumers.is_empty() {
                !lines.is_empty()
            } else {
                consumers.iter().all(|line| lines.contains(line))
            };
            (!satisfied)
                .then(|| format!("{name}: expected RY032 = {expected}, got lines {lines:?}"))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn stopifnot_scalar_guard_carries_to_a_later_short_circuit() {
    for source in [
        "prepend <- function(x, values, before = NULL) { n <- length(x); stopifnot(is.null(before) || (before > 0 && before <= n)); if (is.null(before) || before == 1) c(values, x) else c(x, values) }",
        "f <- function(x) { stopifnot(is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
        "f <- function(x) { stopifnot(is.null(x) || (x > 0 && x <= 3)); if (is.null(x) || x == 1L) TRUE else FALSE }",
        "f <- function(x) { n <- 3L; stopifnot(is.null(x) || (x > 0 && x <= n)); if (is.null(x) || x == 1L) TRUE else FALSE }",
        "f <- function(x = NULL) { stopifnot(is.null(x) || (x > 0 && x <= 3L)); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        "f <- function(x) { stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f(1L)",
    ] {
        assert!(!warns_ry032(source), "{source}");
    }
    for source in [
        // stopifnot accepts a vector of TRUE values.
        "f <- function(x) { stopifnot(is.null(x) || x > 0); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // A local stopifnot proves nothing.
        "stopifnot <- function(...) TRUE; f <- function(x) { stopifnot(is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // An S3 length or comparison method may lie about the length.
        "length.foo <- function(x) 1L; f <- function(x) { stopifnot(is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
        "`>.foo` <- function(e1, e2) TRUE; f <- function(x) { stopifnot(is.null(x) || (x > 0 && x <= 3)); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // A later assertion argument replaces the checked value.
        "f <- function(x) { stopifnot(is.null(x) || length(x) == 1L, { x <- c(1L, 2L); TRUE }); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // The assertion RHS may run arbitrary code.
        "f <- function(x, mutate) { stopifnot(is.null(x) || (x > 0 && mutate())); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // Another formal's default may rebind x when forced.
        "f <- function(x, n = { x <- c(1L, 2L); 3L }) { stopifnot(is.null(x) || (x > 0 && x <= n)); if (is.null(x) || x == 1L) TRUE else FALSE }; f(1L)",
        // The subject's own default may rebind it when forced.
        "f <- function(x = { x <- c(1L, 2L); 1L }) { stopifnot(length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
    ] {
        assert!(warns_ry032(source), "{source}");
    }
}

#[test]
fn pinned_purrr_prepend_keeps_its_scalar_fact() {
    let source = "prepend <- function(x, values, before = NULL) {\n  lifecycle::deprecate_warn(\"1.0.0\", \"prepend()\", I(\"append(after = 0)\"))\n\n  n <- length(x)\n  stopifnot(is.null(before) || (before > 0 && before <= n))\n\n  if (is.null(before) || before == 1) {\n    c(values, x)\n  } else {\n    c(x[1:(before - 1)], values, x[before:n])\n  }\n}\n";
    assert!(!warns_ry032(source));
}

#[test]
fn scalar_fact_needs_a_known_search_path_and_predicate_arguments() {
    let body = "f <- function(x = NULL) { stopifnot(is.null(x) || (x > 0 && x <= 2L)); if (is.null(x) || x == 1L) TRUE else FALSE }; f()";
    assert!(!warns_ry032(body));
    for ambient in ["library(stats)", "attach(list())"] {
        assert!(warns_ry032(&format!("{ambient}\n{body}")), "{ambient}");
    }
    let guard = "is.null(x) || length(x) == 1L";
    let consumer = "if (is.null(x) || x == 1L) TRUE else FALSE";
    assert!(warns_ry032(&format!(
        "f <- function(x) {{ stopifnot(local = {guard}); {consumer} }}"
    )));
    assert!(!warns_ry032(&format!(
        "f <- function(x) {{ stopifnot(named_condition = {guard}); {consumer} }}"
    )));
    assert!(warns_ry032(
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { stopifnot(local = length(x) == 1L); if (x == 1L && TRUE) i; x <- 1L } }"
    ));
}

#[test]
fn only_safe_base_calls_keep_the_scalar_fact() {
    for between in [
        "length(x)",
        "is.na(x)",
        "identical(x, 1L)",
        "g <- function() assign('x', c(1L, 2L), envir = parent.frame())",
    ] {
        assert!(!warns_ry032(&asserted_then(between)), "{between}");
    }
    // Every other call may install a binding, even when it is harmless.
    for between in [
        "assign('x', c(1L, 2L))",
        "message('checked')",
        "print(x)",
        "as.character(x)",
        "do.call(identity, list(1L))",
        "(base::identity(base::identity))(1L)",
        "g <- function() NULL; g()",
        "tryCatch(1L, error = function(e) NULL)",
        "local(NULL)",
        "length <- function(x) 1L; length(x)",
        "x[2L] <- 2L",
    ] {
        assert!(warns_ry032(&asserted_then(between)), "{between}");
    }
    let helper = "helper <- function(...) NULL\n";
    assert!(warns_ry032(&format!(
        "{helper}{}",
        asserted_then("helper()")
    )));
    assert!(warns_ry032(
        "f <- function(g, x = 1L) { stopifnot(x > 0 && TRUE); g(); if (is.null(x) || x == 1L) TRUE else FALSE }"
    ));
    // An unsafe call before the assertion keeps it from proving anything,
    // unless it is base `I()` or a call into another package's namespace.
    let guard = "stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }";
    for before in [
        "message('start')",
        "helper()",
        "own::helper()",
        "pkg::run(function() NULL)",
        "base::message('start')",
    ] {
        let source = format!("{helper}f <- function(x = 1L) {{ {before}; {guard}");
        assert!(warns_ry032(&source), "{before}");
    }
    for before in [
        "I('a')",
        "lifecycle::deprecate_warn('1.0.0', 'f()', I('g()'))",
    ] {
        let source = format!("{helper}f <- function(x = 1L) {{ {before}; {guard}");
        assert!(!warns_ry032(&source), "{before}");
    }
    // After the assertion, even those calls end the fact.
    assert!(warns_ry032(&asserted_then("pkg::run()")));
    assert!(warns_ry032(&asserted_then("I(x)")));
    // An unsafe call inside an `&&` operand reaches the continuation.
    assert!(warns_ry032(
        "f <- function(g, x = 1L) { stopifnot(x > 0 && TRUE); if (TRUE && g()) NULL; if (is.null(x) || x == 1L) TRUE else FALSE }"
    ));
    // A non-literal default on any formal blocks the fact.
    assert!(warns_ry032(
        "f <- function(x = 1L, n = 2L + 1L) { stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }"
    ));
}

#[test]
fn cross_file_helpers_drop_the_scalar_fact() {
    let mut project = Project::new();
    project.add_file(
        "helper.R".into(),
        parse_file("helper.R", "install <- function(env) invisible(NULL)\n"),
    );
    project.add_file(
        "consumer.R".into(),
        parse_file(
            "consumer.R",
            "f <- function(x = 1L) { stopifnot(x > 0 && TRUE); install(environment()); if (is.null(x) || x == 1L) TRUE else FALSE }\n",
        ),
    );
    let warned = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .any(|d| d.code == "RY032");
    assert!(warned, "a project helper may install a caller binding");
}

#[test]
fn s4_length_methods_gate_only_scalar_facts() {
    // testthat's own tests define an S4 `length` method; that must not turn
    // its `length(x) == 1 && is.na(x)` guard into a new RY032.
    let s4 = "setMethod(\"length\", \"Thing\", function(x) 5L)\n";
    let guard = format!("{s4}is_na <- function(x) length(x) == 1 && is.na(x)\n");
    assert!(!warns_ry032(&guard));
    assert!(warns_ry032(&format!("{s4}{}\n", asserted_then("NULL"))));
}

#[test]
fn stopifnot_narrows_every_argument_not_rebound_later() {
    let (_, scope) = check_with_scope(
        "x <- if (runif(1) > 0.5) 1L else c(\"a\", \"b\")\n\
         stopifnot(is.character(x), length(x) > 0)\n",
    );
    assert_eq!(scope.get("x").map(|ty| ty.mode), Some(Mode::Character));
    let (_, scope) = check_with_scope(
        "x <- if (runif(1) > 0.5) 1L else c(\"a\", \"b\")\n\
         stopifnot(is.character(x), { x <- 1L; TRUE })\n",
    );
    assert_eq!(scope.get("x").map(|ty| ty.mode), Some(Mode::Integer));
}

#[test]
fn superassignment_keeps_its_main_binding_behaviour() {
    let diagnostics = check("f <- function() { flag <<- TRUE; if (flag) 1 }\n");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY010"),
        "{diagnostics:?}"
    );
    assert!(warns_ry032("x <<- c(1L, 2L)\nx && TRUE\n"));
}

#[test]
fn loop_vector_path_survives_aliases_and_joins() {
    for source in [
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) x <- 1L; if (x == 1L && TRUE) x }",
        "f <- function(xs, flag) { x <- c(1L, 2L); for (i in xs) { if (flag) x <- 1L; if (x == 1L && TRUE) i } }",
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { y <- x; if (y == 1L && TRUE) i; x <- 1L } }",
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { y <- x; z <- y; if (z == 1L && TRUE) i; x <- 1L } }",
        "`(` <- function(x) TRUE; f <- function(xs) { x <- c(1L, 2L); for (i in xs) { if ((length(x) == 1L)) { if (x == 1L && TRUE) i }; x <- 1L } }",
        "`(` <- function(x) TRUE; f <- function(xs) { x <- c(1L, 2L); for (i in xs) { stopifnot((length(x) == 1L)); if (x == 1L && TRUE) i; x <- 1L } }",
    ] {
        assert!(warns_ry032(source), "{source}");
    }
    for source in [
        "f <- function(xs) { original <- 1L; alias <- original; for (i in xs) { if (alias == 1L && TRUE) i; alias <- 1L } }",
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { x <- 1L; if (x == 1L && TRUE) i } }",
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { if (length(x) == 1L) { if (x == 1L && TRUE) i }; x <- 1L } }",
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { if ((length(x) == 1L)) { if (x == 1L && TRUE) i }; x <- 1L } }",
        "f <- function(xs, flag) { x <- c(1L, 2L); for (i in xs) { if (flag) x <- 1L else x <- 1L; if (x == 1L && TRUE) i } }",
        "f <- function() { x <- c(1L, 2L); for (i in 1L) x <- 1L; if (x == 1L && TRUE) x }",
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { stopifnot(length(x) == 1L); if (x == 1L && TRUE) i; x <- 1L } }",
        "`||` <- function(x, y) TRUE; f <- function(xs) { x <- c(1L, 2L); for (i in xs) { y <- x; if (y == 1L || FALSE) i; x <- 1L } }",
        "`-` <- function(y) 1L; f <- function() { x <- c(1L, 2L); for (i in integer()) x <- 1L; x < -1L && TRUE }",
        "f <- function() { x <- structure(c(1L, 2L), class = 'foo'); for (i in integer()) x <- 1L; x < -1L && TRUE }",
    ] {
        assert!(!warns_ry032(source), "{source}");
    }
}

#[test]
fn deferred_and_child_scopes_hold_no_stale_scalar_facts() {
    let mut scope = Scope::default();
    scope.insert("x", RType::scalar(Mode::Integer));
    scope.mark_scalar_asserted("x");
    scope.mark_loop_vector("x");
    let child = scope.independent_execution_scope();
    assert!(child.scalar_asserted_bindings.is_empty() && child.loop_vector_bindings.is_empty());
    // Exit code neither keeps nor proves a fact while the body is unwalked.
    assert!(warns_ry032(
        "f <- function(x) { on.exit({ stopifnot(length(x) == 1L); if (is.null(x) || x == 1L) TRUE }); x[2L] <- 2L }"
    ));
    // A reachable child scope that lost a fact loses it for the parent.
    for between in [
        "if (TRUE && pkg::run()) NULL",
        "y <- if (x > 0) pkg::run() else NULL",
    ] {
        assert!(warns_ry032(&asserted_then(between)), "{between}");
    }
}

#[test]
fn pre_assertion_exemption_needs_exported_calls_with_plain_values() {
    let guard = "stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }";
    for (formals, before) in [
        ("g, x = 1L", "pkg::run(g)"),
        ("x = 1L", "pkg:::run()"),
        ("x = 1L", "h <- function() NULL; pkg::run(h)"),
    ] {
        let source = format!("f <- function({formals}) {{ {before}; {guard}");
        assert!(warns_ry032(&source), "{before}");
    }
    let source = format!("f <- function(x = 1L) {{ n <- 1L; pkg::run(n); {guard}");
    assert!(!warns_ry032(&source));
}

#[test]
fn stopifnot_controls_and_comparison_methods() {
    let consumer = "if (is.null(x) || x == 1L) TRUE else FALSE }";
    // A trailing `local` control leaves the last predicate in charge.
    assert!(!warns_ry032(&format!(
        "f <- function(x) {{ stopifnot(is.null(x) || length(x) == 1L, local = TRUE); {consumer}"
    )));
    // `exprs` evaluates quoted code, so it proves nothing.
    assert!(warns_ry032(&format!(
        "f <- function(x) {{ stopifnot(exprs = {{ is.null(x) || length(x) == 1L }}); {consumer}"
    )));
    // A project comparison method may make the loop operand scalar.
    assert!(!warns_ry032(
        "`==.foo` <- function(e1, e2) TRUE\nf <- function(xs) { x <- c(1L, 2L); for (i in xs) x <- 1L; if (x == 1L && TRUE) x }"
    ));
}
