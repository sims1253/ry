//! Issue #374: `<<-` modeled as a type update to the enclosing binding.
//!
//! A closure that superassigns (`x <<- v` / `v ->> x`) may run any time
//! after its definition, and the checker has no call-graph evidence that
//! it did or did not run before a given read. The conservative model:
//! every `<<-` target anywhere in a function body (nested closure bodies
//! included) becomes unknown-typed in the definition scope once the
//! definition is walked -- except a plain-name write whose name an
//! intervening function frame binds as a formal: `<<-` searches that
//! frame before the definition scope, so the write lands there and
//! never reaches the outer binding. Complex targets (subscripted or
//! call-form) are never excepted: they fetch the root through that
//! frame and modify it, so a reference-typed root shared with the
//! definition scope is mutated in place and the outer binding does
//! observe the write. Reads before the definition keep the prior type,
//! so a provably-invalid condition still fires there.
//!
//! The silent shapes split by what R 4.6 actually does at runtime (each
//! verdict reproduced with Rscript): where the writing closure is
//! invoked before the condition, the `<<-` writes have executed when it
//! evaluates, so the loop or branch really does run. The remaining
//! shapes deliberately never call the writer (`writer` defined, no call
//! before the read): in R the binding keeps its stale value and the
//! condition errors -- `if (done)` on uncalled `writer <- function() 1
//! ->> done` fails with "argument is of length zero" -- but the checker
//! has no call-graph evidence the closure did not run either, so the
//! conservative unknown-typed update keeps those silent too.

use super::*;

fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code).collect()
}

/// The corpus loop-condition shapes from the issue: a binding
/// initialized to `NULL` (or never bound in the file) and mutated only
/// through `<<-` in a nested closure must not keep its stale type at a
/// later condition.
#[test]
fn superassignment_targets_are_unknown_after_the_definition() {
    for source in [
        // pak/remotes vendored json parser: the issue's minimal shape.
        "token <- NULL\nread <- function(v) token <<- v\nread(TRUE)\nwhile (token) break",
        // The binding may not exist in the file at all until `<<-`
        // creates it (the RY010 family): the call runs first at runtime.
        "setup <- function() flag <<- TRUE\nsetup()\nwhile (flag) break",
        // flexdashboard: the stale NULL flowed through a base stub
        // (`file.exists(NULL)` is logical(0)).
        "source_file <- NULL\npre_knit <- function(input) source_file <<- input\npre_knit('a.R')\nif (!file.exists(source_file)) stop('missing')",
        // pak's actual wrapper: definitions inside local({...}) and the
        // condition in a closure defined after the writer.
        "json <- local({\n  token <- NULL\n  read <- function(v) token <<- v\n  parse <- function() { read('x'); while (token != '}') break; TRUE }\n})",
        // Doubly-nested closure: the writer is defined one level deeper
        // than the reader's enclosing scope.
        "done <- NULL\nouter <- function() { inner <- function() done <<- TRUE; inner }\nouter()()\nwhile (done) break",
        // fastmap R6 / curl vector roots: a subscripted target rebinds
        // its root, whose members are then unprovable.
        "state <- list(key = NULL)\nset <- function(v) state$key <<- v\nset(TRUE)\nif (state$key) 1L",
        "success <- rep(FALSE, 2)\nlapply(seq_along(success), function(i) success[i] <<- TRUE)\nif (all(success)) 1L",
        // jsonlite stream_in: the writer is the value of an if
        // expression's branch, and the read is later in the same
        // function or after the definition at file level.
        "make <- function(handler) {\n  cb <- if (is.null(handler)) {\n    out <- new.env()\n    function(x) out[[as.character(length(x))]] <<- x\n  } else {\n    function(x) x\n  }\n  length(out)\n}",
        "make <- function(handler) {\n  cb <- if (is.null(handler)) function(x) out[[as.character(length(x))]] <<- x else function(x) x\n}\nmake(NULL)\nlength(out)",
        // Both spellings and expression position.
        "done <- NULL\nwriter <- function() 1 ->> done\nif (done) 1L",
        "done <- NULL\nwriter <- function() { other <- (done <<- TRUE) }\nif (done) 1L",
    ] {
        let diagnostics = check(source);
        assert!(
            !diagnostics
                .iter()
                .any(|d| matches!(d.code, "RY001" | "RY010" | "RY070")),
            "{source}: {:?}",
            codes(&diagnostics)
        );
    }
}

/// The update lands at the definition's position in the sequential
/// walk: a read that precedes the `<<-`-writing definition cannot have
/// seen the write at runtime either, so its stale type still fires.
#[test]
fn reads_before_the_definition_keep_the_prior_type() {
    let source = "x <- NULL\nwhile (x) break\nwriter <- function() x <<- TRUE";
    let diagnostics = check(source);
    assert!(
        diagnostics.iter().any(|d| d.code == "RY001"),
        "{diagnostics:?}"
    );

    let source = "while (late) break\nwriter <- function() late <<- TRUE";
    let diagnostics = check(source);
    assert!(
        diagnostics.iter().any(|d| d.code == "RY010"),
        "{diagnostics:?}"
    );
}

/// Plain assignment inside a nested closure binds only the closure's
/// own frame (#350's write-side territory): the enclosing read stays
/// unbound and the model here must not absorb it.
#[test]
fn plain_assignment_in_a_closure_does_not_publish() {
    let source = "f <- function() { g <- function() { v <- 1L; v } }\nif (v) 1L";
    let diagnostics = check(source);
    assert!(
        diagnostics.iter().any(|d| d.code == "RY010"),
        "{diagnostics:?}"
    );
}

/// A target whose rebound root cannot be named (`f()$a <<- v`) discards
/// all value facts in the definition scope: even an unrelated binding's
/// known-NULL type is no longer provable after the definition.
#[test]
fn unnameable_targets_invalidate_value_facts() {
    let before = "x <- NULL\nwhile (x) break";
    assert!(
        check(before).iter().any(|d| d.code == "RY001"),
        "{before:?}"
    );
    let after = "x <- NULL\nf <- function() make()$key <<- 1\nwhile (x) break";
    assert!(!check(after).iter().any(|d| d.code == "RY001"), "{after:?}");
}

/// `<<-` never writes the writing closure's own frame (verified in R:
/// `f <- function(x) { x <<- 10; x }` leaves the formal untouched), so
/// a defaulted formal that stays NULL inside the closure keeps its
/// provable type there even while the same name is being superassigned.
#[test]
fn superassignment_does_not_touch_the_writing_frame() {
    // The forwarded_superassignment oracle fixture pins the RY001 side
    // of this (the formal stays NULL, so the callee errors); here the
    // check is that the file-level binding, not the formal, updates.
    let source = "bins <- 0L\nstatement <- function(bins = NULL) { bins <<- 1L; bins }\nr <- statement()\nwhile (bins) break";
    let diagnostics = check(source);
    assert!(
        !diagnostics.iter().any(|d| d.code == "RY001"),
        "{diagnostics:?}"
    );
}

/// A `<<-` nested inside a closure whose intervening frame binds the
/// name as a formal lands in that frame, never the definition scope:
/// verified in R (`outer <- function(x) { inner <- function() x <<-
/// TRUE; inner }; outer(NULL)()` rebinds `outer`'s formal, the
/// file-level `x` stays NULL, and `while (x)` errors with "argument is
/// of length zero"), so the definition-scope binding keeps its provable
/// type and RY001 still fires.
#[test]
fn intervening_formals_keep_the_definition_scope_diagnosable() {
    let source = "x <- NULL\nouter <- function(x) { inner <- function() x <<- TRUE; inner }\nouter(NULL)()\nwhile (x) break";
    let diagnostics = check(source);
    assert!(
        diagnostics.iter().any(|d| d.code == "RY001"),
        "{source}: {:?}",
        codes(&diagnostics)
    );
}

/// The flip side, one nesting level shallower: the writing closure's own
/// formals never intercept because `<<-` skips the writing frame, so the
/// outer binding is still marked. R agrees that the write reaches it
/// (`inner2 <- function(x) { x <<- TRUE }; inner2(NULL)` leaves the
/// file-level `x` TRUE), the loop runs, and ry stays silent.
#[test]
fn writing_frame_formals_do_not_intercept() {
    let source = "x <- NULL\ninner2 <- function(x) { x <<- TRUE }\ninner2(NULL)\nwhile (x) break";
    let diagnostics = check(source);
    assert!(
        !diagnostics
            .iter()
            .any(|d| matches!(d.code, "RY001" | "RY010" | "RY070")),
        "{source}: {:?}",
        codes(&diagnostics)
    );
}

/// The interception prune is reserved for plain-name rebinding: a
/// complex target through an intervening formal fetches the root object
/// through that formal and modifies it, so when the root is an
/// environment shared between the formal and the definition scope the
/// member write happens in place and the definition scope observes it.
/// Verified in R (`env <- new.env(); env$key <- NULL; outer <-
/// function(env) { inner <- function() env$key <<- TRUE; inner };
/// outer(env)()` leaves the file-level `env$key` TRUE, so the loop
/// runs): pruning the root would keep the stale `key: NULL` member
/// fact and fire RY001 on code that runs fine, the false-positive
/// family this model exists to silence. The plain-name control one
/// test up keeps its finding.
#[test]
fn complex_targets_through_intervening_formals_stay_silent() {
    let source = "env <- new.env()\nenv$key <- NULL\nouter <- function(env) { inner <- function() env$key <<- TRUE; inner }\nouter(env)()\nwhile (env$key) break";
    let diagnostics = check(source);
    assert!(
        !diagnostics
            .iter()
            .any(|d| matches!(d.code, "RY001" | "RY010" | "RY070")),
        "{source}: {:?}",
        codes(&diagnostics)
    );

    // Call-form root, same reference semantics: `class(e2) <<- v`
    // through a formal retags the shared environment (probe: the
    // file-level `class(e2)` reads back "foo").
    let source = "e2 <- new.env()\nouter3 <- function(e2) { inner3 <- function() class(e2) <<- 'foo'; inner3 }\nouter3(e2)()\nif (class(e2) == 'foo') 1L";
    let diagnostics = check(source);
    assert!(
        !diagnostics
            .iter()
            .any(|d| matches!(d.code, "RY001" | "RY010" | "RY070")),
        "{source}: {:?}",
        codes(&diagnostics)
    );

    // Plain-name control: the rebind lands in the formal, the
    // definition-scope binding keeps its provable type (probe: the
    // file-level `x` stays NULL and `while (x)` errors), so the
    // finding must survive next to the silent complex shapes.
    let source = "x <- NULL\nouter2 <- function(x) { inner2 <- function() x <<- TRUE; inner2 }\nouter2(x)()\nwhile (x) break";
    let diagnostics = check(source);
    assert!(
        diagnostics.iter().any(|d| d.code == "RY001"),
        "{source}: {:?}",
        codes(&diagnostics)
    );
}
