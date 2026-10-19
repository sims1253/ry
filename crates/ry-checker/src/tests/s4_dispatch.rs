//! S4 selection through inferred call results.

use super::*;

const GENERIC: &str = "setClass(\"A\", representation(v = \"numeric\"))\n\
    setGeneric(\"g\", function(x, y) standardGeneric(\"g\"))\n";

/// The diagnostic codes and the type bound to `r`.
fn selected(methods: &str, call: &str) -> (Vec<&'static str>, Mode) {
    let source = format!("{GENERIC}{methods}r <- {call}\n");
    let (diagnostics, scope) = check_with_scope(&source);
    let codes = diagnostics.iter().map(|d| d.code).collect();
    (codes, scope.get("r").expect("r is bound").mode)
}

const CHR_NUM: &str = "setMethod(\"g\", signature(\"A\", \"character\"), function(x, y) \"chr\")\n\
    setMethod(\"g\", signature(\"A\", \"numeric\"), function(x, y) 1L)\n";

#[test]
fn an_exact_full_signature_selects_its_method_in_either_order() {
    assert_eq!(selected(CHR_NUM, "g(new(\"A\"), \"t\")").1, Mode::Character);
    assert_eq!(selected(CHR_NUM, "g(new(\"A\"), 2)").1, Mode::Integer);
    let reversed = "setMethod(\"g\", signature(y = \"numeric\", x = \"A\"), function(x, y) 1L)\n\
        setMethod(\"g\", signature(x = \"A\", y = \"character\"), function(x, y) \"chr\")\n";
    assert_eq!(selected(reversed, "g(new(\"A\"), 2)").1, Mode::Integer);
    assert_eq!(
        selected(reversed, "g(y = \"t\", new(\"A\"))").1,
        Mode::Character
    );
    let missing = "setMethod(\"g\", signature(\"A\", \"missing\"), function(x, y) 1L)\n\
        setMethod(\"g\", signature(\"A\", \"character\"), function(x, y) \"chr\")\n";
    assert_eq!(selected(missing, "g(new(\"A\"))").1, Mode::Integer);
    let (codes, mode) = selected(CHR_NUM, "g(new(\"A\"), \"t\") + 1L");
    assert_eq!((codes, mode), (vec!["RY040"], Mode::Opaque));
}

#[test]
fn a_later_identical_signature_replaces_the_earlier_method() {
    let replaced = "setMethod(\"g\", signature(\"A\", \"character\"), function(x, y) \"chr\")\n\
        setMethod(\"g\", signature(x = \"A\", y = \"character\"), function(x, y) 1L)\n";
    assert_eq!(selected(replaced, "g(new(\"A\"), \"t\")").1, Mode::Integer);
}

#[test]
fn unproven_positions_and_inventories_give_an_unknown_result() {
    for (methods, call) in [
        // An integer selects `double` before `numeric` or `ANY`.
        (CHR_NUM, "g(new(\"A\"), 2L)"),
        // Only a plain literal has an exact implicit class.
        (CHR_NUM, "g(new(\"A\"), structure(1, dim = c(1L, 1L)))"),
        (CHR_NUM, "g(new(\"A\"), readRDS(\"f\"))"),
        // A constructor class need not be the dispatched one.
        (
            "setMethod(\"g\", signature(\"A\", \"factor\"), function(x, y) \"text\")\n",
            "g(new(\"A\"), factor(\"a\", ordered = TRUE))",
        ),
        // A dispatched formal never takes the method's default.
        (
            "setMethod(\"g\", signature(\"A\", \"numeric\"), function(x, y = \"text\") y)\n",
            "g(new(\"A\"), 1)",
        ),
        // A class test bounds the class from below.
        (
            &format!("{CHR_NUM}v <- readRDS(\"f\")\nstopifnot(inherits(v, \"A\"))\n"),
            "g(v, \"t\")",
        ),
        // Mixed named and positional entries.
        (
            "setMethod(\"g\", signature(y = \"character\", \"A\"), function(x, y) \"chr\")\n",
            "g(new(\"A\"), \"t\")",
        ),
        // An uninterpreted signature or a non-literal definition.
        (
            &format!("{CHR_NUM}setMethod(\"g\", c(\"A\", \"logical\"), function(x, y) TRUE)\n"),
            "g(new(\"A\"), \"t\")",
        ),
        (
            &format!(
                "{CHR_NUM}h <- function(x, y) 1L\nsetMethod(\"g\", signature(\"A\", \"character\"), h)\n"
            ),
            "g(new(\"A\"), \"t\")",
        ),
    ] {
        let (codes, mode) = selected(methods, call);
        assert!(codes.is_empty(), "{call}: {codes:?}");
        assert_eq!(mode, Mode::Opaque, "{methods}{call}");
    }
}

#[test]
fn a_generic_that_does_not_only_forward_is_not_selected() {
    let source = "setClass(\"A\", representation(v = \"numeric\"))\n\
        setGeneric(\"g\", function(x, y) { standardGeneric(\"g\"); 1L })\n\
        setMethod(\"g\", signature(\"A\", \"character\"), function(x, y) \"chr\")\n\
        r <- g(new(\"A\"), \"t\")\n";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("r").unwrap().mode, Mode::Opaque);
}

#[test]
fn a_restricted_signature_dispatches_on_its_own_formals() {
    let source = "setClass(\"A\", representation(v = \"numeric\"))\n\
        setClass(\"B\", representation(v = \"numeric\"))\n\
        setGeneric(\"g\", function(z, x) standardGeneric(\"g\"), signature = \"x\")\n\
        setMethod(\"g\", \"A\", function(z, x) \"a\")\n\
        setMethod(\"g\", \"B\", function(z, x) 1L)\n\
        r <- g(new(\"A\"), new(\"B\"))\n\
        setGeneric(\"k\", function(x, ..., y) { standardGeneric(\"k\") })\n\
        setMethod(\"k\", signature(\"A\", \"B\"), function(x, ..., y) \"ab\")\n\
        setMethod(\"k\", signature(\"A\", \"missing\"), function(x, ..., y) 1L)\n\
        s <- k(new(\"A\"), new(\"B\"))\n";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("r").unwrap().mode, Mode::Integer);
    assert_eq!(scope.get("s").unwrap().mode, Mode::Integer);
}

#[test]
fn a_single_dispatch_method_on_a_base_function_keeps_its_result() {
    let source = "setClass(\"E\", representation(v = \"numeric\"))\n\
        setMethod(\"as.vector\", signature(x = \"E\"), function(x, mode = \"any\") c(xmin = 1, xmax = 2))\n\
        setMethod(\"length\", \"E\", function(x) 4L)\n\
        v <- as.vector(new(\"E\"))\n\
        n <- length(new(\"E\"))\n\
        q <- base::as.vector(new(\"E\"))\n";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let v = scope.get("v").unwrap();
    assert_eq!((v.mode, v.length), (Mode::Double, Length::Known(2)));
    assert!(v.columns.is_some(), "the method's names are kept: {v:?}");
    assert_eq!(scope.get("n").unwrap().mode, Mode::Integer);
    // `base::as.vector` is the original function, not the generic.
    assert_eq!(scope.get("q").unwrap().mode, Mode::Opaque);
}

#[test]
fn a_method_formal_is_no_exact_evidence() {
    // The formal may hold a subclass with its own `h` method.
    let source = "setClass(\"A\", representation(v = \"numeric\"))\n\
        setGeneric(\"g\", function(x, y) standardGeneric(\"g\"))\n\
        setGeneric(\"h\", function(x) standardGeneric(\"h\"))\n\
        setMethod(\"h\", \"A\", function(x) 1L)\n\
        setMethod(\"g\", signature(y = \"character\", x = \"A\"), function(x, y) h(x))\n\
        r <- g(new(\"A\"), \"t\")\n";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("r").unwrap().mode, Mode::Opaque);
}

#[test]
fn only_registrations_ry_cannot_read_leave_the_result_unknown() {
    let source = "setClass(\"A\", representation(v = \"numeric\"))\n\
        setMethod(\"length\", c(x = \"A\"), function(x) list(k = 1L))\n\
        n <- length(new(\"A\"))\n\
        m <- length(1:3)\n";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("n").unwrap().mode, Mode::Opaque);
    // An unclassed actual cannot reach a class-named registration.
    assert_eq!(scope.get("m").unwrap().mode, Mode::Integer);
}

#[test]
fn only_a_proven_new_call_is_an_exact_instance() {
    // A binding may be replaced or modified after `new()`, and `t()` of an
    // `A` keeps its class names but need not be that instance.
    let methods = "setMethod(\"g\", signature(\"A\", \"character\"), function(x, y) 1L)\n\
        a <- new(\"A\")\nb <- a\n";
    for (call, mode) in [
        ("g(new(\"A\"), \"t\")", Mode::Integer),
        ("g(methods::new(\"A\"), \"t\")", Mode::Integer),
        ("g(a, \"t\")", Mode::Opaque),
        ("g(b, \"t\")", Mode::Opaque),
        ("g(t(new(\"A\")), \"t\")", Mode::Opaque),
    ] {
        assert_eq!(selected(methods, call).1, mode, "{call}");
    }
    // A masked `new` is not `methods::new`, whatever class it returns.
    let masked = "new <- function(Class) structure(list(), class = \"A\")\n";
    assert_eq!(
        selected(&format!("{methods}{masked}"), "g(new(\"A\"), \"t\")").1,
        Mode::Opaque
    );
    // An `initialize` method may return any object.
    let initialize = "setMethod(\"initialize\", \"A\", function(.Object, ...) .Object)\n";
    assert_eq!(
        selected(&format!("{methods}{initialize}"), "g(new(\"A\"), \"t\")").1,
        Mode::Opaque
    );
}

#[test]
fn a_masked_registration_helper_leaves_every_generic_unknown() {
    for masking in [
        "signature <- function(...) methods::signature(\"A\", \"character\")\n",
        "local({ setMethod <- function(...) NULL })\n",
        "assign(\"signature\", function(...) NULL)\n",
    ] {
        let methods = format!("{masking}{CHR_NUM}");
        assert_eq!(
            selected(&methods, "g(new(\"A\"), 2)").1,
            Mode::Opaque,
            "{masking}"
        );
    }
    assert_eq!(selected(CHR_NUM, "g(new(\"A\"), 2)").1, Mode::Integer);
}

#[test]
fn a_masked_helper_leaves_a_registered_generic_unknown_for_any_class() {
    // The masked `signature("Q")` registers `length` for `A` in R.
    let source = "setClass(\"A\", representation(v = \"numeric\"))\n\
        signature <- function(...) methods::signature(\"A\")\n\
        setMethod(\"length\", signature(\"Q\"), function(x) \"chr\")\n\
        n <- length(new(\"A\"))\n";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("n").unwrap().mode, Mode::Opaque);
}

#[test]
fn a_computed_generic_name_leaves_calls_on_new_instances_unknown() {
    let source = "setClass(\"A\", representation(v = \"numeric\"))\n\
        f <- function(x) \"four\"\n\
        for (g in c(\"dim\", \"length\")) setMethod(g, \"A\", f)\n\
        n <- length(new(\"A\"))\n\
        a <- methods::new(\"A\")\n\
        b <- length(a)\n\
        m <- length(1:3)\n";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("n").unwrap().mode, Mode::Opaque);
    // A bound instance is no exact evidence, but it is classed.
    assert_eq!(scope.get("b").unwrap().mode, Mode::Opaque);
    assert_eq!(scope.get("m").unwrap().mode, Mode::Integer);
}
