use super::*;

/// The recorded scope-exit snapshot of each named function in `src`.
fn function_exits(src: &str) -> Vec<(String, Scope)> {
    let mut c = Checker::new("test.R");
    c.enable_scope_capture();
    c.check(&parse_file("test.R", src));
    c.take_scope_records()
        .into_iter()
        .filter_map(|record| Some((record.name?, record.scope)))
        .collect()
}

fn exit_type(exits: &[(String, Scope)], function: &str, binding: &str) -> RType {
    let (_, scope) = exits
        .iter()
        .find(|(name, _)| name == function)
        .unwrap_or_else(|| panic!("no record for {function}"));
    scope
        .get(binding)
        .cloned()
        .unwrap_or_else(|| panic!("{function}: no binding {binding}"))
}

/// Whether `ty` leaves room for a value of `mode`.
fn admits(ty: &RType, mode: Mode) -> bool {
    match &ty.members {
        Some(members) => members.iter().any(|member| admits(member, mode)),
        None => ty.mode == mode || ty.mode == Mode::Opaque,
    }
}

// R 4.6.1: `f(TRUE)` is "integer", `f(FALSE)` is "character" (#859).
#[test]
fn scope_exit_joins_states_that_leave_through_return() {
    let exits = function_exits(
        "f <- function(c) {\n  x <- 1L\n  if (c) return(typeof(x))\n  x <- \"a\"\n  typeof(x)\n}\n\
         g <- function(c) {\n  x <- 1L\n  if (c) {\n    return(x)\n  }\n  x <- \"a\"\n  x\n}\n",
    );
    for function in ["f", "g"] {
        let x = exit_type(&exits, function, "x");
        assert!(admits(&x, Mode::Integer), "{function}: {x:?}");
        assert!(admits(&x, Mode::Character), "{function}: {x:?}");
    }
}

// A `return()` inside a callback leaves the callback, so the enclosing
// function's exit keeps its precise final type.
#[test]
fn callback_returns_do_not_reach_the_enclosing_scope_exit() {
    let exits = function_exits(
        "f <- function(xs) {\n  x <- 1L\n  vapply(xs, function(v) return(v), 1)\n  x <- \"a\"\n  x\n}\n",
    );
    let x = exit_type(&exits, "f", "x");
    assert_eq!(x.mode, Mode::Character, "{x:?}");
}

// R 4.6.1: `f(0)` and `g(0)` leave `x` integer; `h(0)` and `h2(0)` leave
// `i` NULL, replacing an earlier binding (#860). ry makes no claim about
// `i` then, rather than a NULL union that would drive length-0 findings.
#[test]
fn loops_that_may_not_run_keep_their_entry_state() {
    let exits = function_exits(
        "f <- function(n) { x <- 1L; for (i in seq_len(n)) x <- \"a\"; x }\n\
         g <- function(n) { x <- 1L; while (n > 0) { x <- \"a\"; n <- 0 }; x }\n\
         h <- function(n) { for (i in seq_len(n)) NULL; i }\n\
         h2 <- function(n) { i <- \"z\"; for (i in seq_len(n)) NULL; i }\n\
         b <- function(n) { i <- \"z\"; for (i in seq_len(n)) if (i > 1) break; i }\n",
    );
    for function in ["f", "g"] {
        let x = exit_type(&exits, function, "x");
        assert!(admits(&x, Mode::Integer), "{function}: {x:?}");
        assert!(admits(&x, Mode::Character), "{function}: {x:?}");
    }
    for function in ["h", "h2", "b"] {
        let i = exit_type(&exits, function, "i");
        assert_eq!(i.mode, Mode::Opaque, "{function}: {i:?}");
    }
}

// R 4.6.1: `for (i in 1:3)` always runs, leaving `x` "character" and `i`
// "integer". So does a loop over `c(1L, y)`, whose length is at least one.
#[test]
fn loops_proven_to_run_keep_precise_exit_types() {
    let exits = function_exits(
        "k <- function() { x <- 1L; for (i in 1:3) x <- \"a\"; x }\n\
         k2 <- function(y) { x <- 1L; for (i in c(1L, y)) x <- \"a\"; x }\n",
    );
    for function in ["k", "k2"] {
        let x = exit_type(&exits, function, "x");
        assert_eq!(x.mode, Mode::Character, "{function}: {x:?}");
    }
    let i = exit_type(&exits, "k", "i");
    assert_eq!(i.mode, Mode::Integer, "{i:?}");
}

// R 4.6.2: a `return()` inside `local()`, `with()` or `within()` leaves
// that evaluation, not `f`, so each `f*` below exits with `x` logical.
// One forced as a promise in the caller (`suppressWarnings()`) does leave
// the function: `g()` returns 99 with `x` character.
#[test]
fn child_frame_returns_do_not_reach_the_enclosing_scope_exit() {
    let exits = function_exits(
        "f <- function() { x <- 1L; local({ x <- \"a\"; return(x); NULL }); x <- TRUE; x }\n\
         f2 <- function() { x <- 1L; with(list(a = 1), { x <- \"a\"; return(99); NULL }); x <- TRUE; x }\n\
         f3 <- function() { x <- 1L; within(list(a = 1), { x <- \"a\"; return(99); NULL }); x <- TRUE; x }\n\
         f4 <- function(d) { x <- 1L; with(d, { x <- \"a\"; return(99); NULL }); x <- TRUE; x }\n\
         f5 <- function(d) { x <- 1L; within(d, { x <- \"a\"; return(99); NULL }); x <- TRUE; x }\n\
         g <- function() { x <- 1L; suppressWarnings({ x <- \"a\"; return(99); NULL }); x <- TRUE; x }\n",
    );
    // `f4` and `f5` mask with an unknown schema, which shares the caller's
    // lookup frame.
    for function in ["f", "f2", "f3", "f4", "f5"] {
        let x = exit_type(&exits, function, "x");
        assert_eq!(x.mode, Mode::Logical, "{function}: {x:?}");
    }
    let x = exit_type(&exits, "g", "x");
    assert!(admits(&x, Mode::Character), "g: {x:?}");
}
