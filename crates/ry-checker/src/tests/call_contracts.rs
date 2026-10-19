//! Call contracts of one-operation project bodies (#646).

use super::*;

fn contract_errors(source: &str) -> Vec<String> {
    check(source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "RY092")
        .map(|diagnostic| diagnostic.message)
        .collect()
}

#[test]
fn a_one_operation_body_demands_its_formal_at_call_sites() {
    for (source, operator) in [
        ("f <- function(x) x + 1L; f('oops')", "an operand of `+`"),
        ("f <- function(x) 2 * x; f(list(1))", "an operand of `*`"),
        (
            "f <- function(x) x %/% 2L; f(identity)",
            "an operand of `%/%`",
        ),
        ("f <- function(x) x ^ TRUE; f(x = 'a')", "an operand of `^`"),
        (
            "f <- function(df) df$value * 2; f(1:3)",
            "the receiver of `$`",
        ),
        ("f <- function(df) df$value; f('a')", "the receiver of `$`"),
        (
            "f <- function(x) x + 1L; g <- function() f('oops')",
            "an operand of `+`",
        ),
        (
            "g <- function() f('a'); f <- function(x) x + 1L; g()",
            "an operand of `+`",
        ),
        // Frame readers and fresh environments install nothing elsewhere.
        (
            "p <- function(x) UseMethod('p'); q <- function() get('p', envir = environment()); r <- local({ n <- 1L; n }); f <- function(x) x + 1L; f('oops')",
            "an operand of `+`",
        ),
    ] {
        let errors = contract_errors(source);
        assert!(
            errors.len() == 1 && errors[0].contains(operator),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn only_a_proven_base_operation_on_a_supplied_unclassed_actual_is_a_demand() {
    for source in [
        // Values the operation accepts, or dispatches on.
        "f <- function(x) x + 1L; f(2L); f(TRUE); f(NULL)",
        "f <- function(x) x + 1L; f(structure('a', class = 'myop'))",
        "f <- function(df) df$value * 2; f(list(value = 1)); f(NULL)",
        "f <- function(x = 'a') x + 1L; f()",
        "f <- function(x) x + 1L; g <- function(...) f(...); g('a')",
        // S1: an unforced actual's effect never reaches the bound one.
        "f <- function(x, ...) x + 1L; v <- 1L; f({v <- 'a'}, x = v)",
        "id <- function(x, ...) x; v <- 1L; id({v <- 'a'}, x = v) + 1L",
        // Bodies that evaluate more than one base operation on the formal.
        "f <- function(x) { x + 1L }; f('a')",
        "f <- function(x) (x + 1L); f('a')",
        "f <- function(x) x + (1L); f('a')",
        "f <- function(x) -x + 1L; f('a')",
        "f <- function(x) return(x + 1L); f('a')",
        "f <- function(x) x + offset; f('a')",
        "f <- function(x, y) x + y; f('a', 1L)",
        "f <- function(x) x$a$b; f(1L)",
        "f <- function(x) x == 1L; f('a')",
        // S9: a default may replace the formal.
        "f <- function(x, y = {x <- 1L; NULL}) x + 1L; f('a')",
        // The operator is not base where the function is defined.
        "f <- function(x, `+`) x + 1L; f('a', paste)",
        "f <- function(x) x + 1L; `+` <- function(e1, e2) paste(e1, e2); f('a')",
        "f <- function(df) df$value; `$` <- function(x, name) 1; f('a')",
        // The call may reach a different literal.
        "f <- function(x) x + 1L; g <- function() f('a'); f <- function(x) paste(x)",
        "f <- function(x) x + 1L; f <- function(x, y) paste(x); g <- function() f('a')",
        // An eager call before the definition reaches base, and a quoted
        // local spelling binds the same name.
        "y <- sort('b'); sort <- function(x) x + 1L",
        // An enclosing frame may bind the name: a forced default (S6), a
        // later write (S7), or an installer.
        "f <- function(x) x + 1L; h <- function(y = { f <- paste; NULL }) { y; f('a') }",
        "f <- function(x) x + 1L; h <- function() { g <- function() f('a'); f <- paste; g() }",
        "f <- function(x) x + 1L; h <- function() { r <- f('a'); assign('f', paste); r }",
        // Environment-targeting binders install `f` in the calling frame.
        "f <- function(x) x + 1L; h <- function() { list2env(list(f = paste), envir = environment()); f('a') }",
        "f <- function(x) x + 1L; h <- function(p) { sys.source(p, envir = environment()); f('a') }",
        "f <- function(x) x + 1L; h <- function(p) { source(p, local = TRUE); f('a') }",
        "f <- function(x) x + 1L; h <- function(p) { load(p); f('a') }",
        // Anywhere in the project, these may install a global `f`.
        "f <- function(x) x + 1L; load('p.RData'); f('a')",
        "f <- function(x) x + 1L; source('d.R'); f('a')",
        "f <- function(x) x + 1L; list2env(list(f = paste), envir = .GlobalEnv); f('a')",
        "f <- function(x) x + 1L; k <- function() eval.parent(quote(f <- paste)); k(); f('a')",
        "f <- function(x) x + 1L; do.call('load', list('p.RData')); f('a')",
        "f <- function(x) x + 1L; k <- function(n) assign(n, paste, envir = globalenv()); k('f'); f('a')",
        "f <- function(x) x + 1L; e <- globalenv(); e$f <- paste; f('a')",
        // S8: a default runs in the frame another default rebinds.
        "f <- function(x) x + 1L; h <- function(y = { z; f('a') }, z = { f <- paste; NULL }) y",
        "f <- function(x) x + 1L; h <- function(y = function() f('a'), z = { f <- paste; NULL }) y",
        "f <- function(x) x + 1L; h <- function() { `f` <- function(x) paste(x); f('a') }",
    ] {
        assert_eq!(contract_errors(source), Vec::<String>::new(), "{source}");
    }
}

#[test]
fn a_bare_formal_body_returns_its_actual() {
    let (diagnostics, scope) =
        check_with_scope("id <- function(x, ...) x; r <- id('a'); s <- id(x = 1L, 'b')");
    assert_eq!(scope.get("r").unwrap().mode, Mode::Character);
    assert_eq!(scope.get("s").unwrap().mode, Mode::Integer);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    // S10: the identity return reaches the caller's own arithmetic.
    let codes: Vec<_> = check("id <- function(x) x; id('a') + 1")
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert_eq!(codes, ["RY040"]);
    let diagnostics =
        check("id <- function(x) x; h <- function() { `id` <- function(x) 1L; id('a') + 1L }");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let diagnostics = check(
        "id <- function(x) x; h <- function() { list2env(list(id = function(x) 1L), envir = environment()); id('a') + 1L }",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let diagnostics = check("id <- function(x) x; load('p.RData'); id('a') + 1L");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    // S14 and braces: `return` and `{` are ordinary bindings in R.
    for source in [
        "id <- function(x) return(x); r <- id('a')",
        "id <- function(x) { x }; r <- id('a')",
        "id <- function(x) (x); r <- id('a')",
        "r <- length('ab'); length <- function(x) x",
        "id <- function(x) substitute(x); r <- id('a')",
    ] {
        let (_, scope) = check_with_scope(source);
        assert_ne!(scope.get("r").unwrap().mode, Mode::Character, "{source}");
    }
}
