use super::*;

/// One `s3_methods` stub entry: an operator method over `e1`/`e2`
/// returning one value of `mode`.
fn op_method(generic: &str, class: &str, mode: &str) -> String {
    format!(
        r#"{{"generic":"{generic}","class":"{class}","params":["e1","e2"],"return":{{"mode":"{mode}","length":"1"}}}}"#
    )
}

/// A minimal stub file for [`check_with_stubs`]; the file's stem is the
/// package key.
fn stub_file(methods: &[String]) -> String {
    format!(
        r#"{{"version":"t","functions":{{}},"s3_methods":[{}]}}"#,
        methods.join(",")
    )
}

/// The source lines carrying `code`, in emission order.
fn code_lines(diags: &[Diagnostic], code: &str) -> Vec<usize> {
    diags
        .iter()
        .filter(|d| d.code == code)
        .map(|d| d.span.line)
        .collect()
}

#[test]
fn operator_dispatches_stub_typeshed_methods() {
    // Operators share the call path's method-source ladder (#165), so a
    // method declared only in a typeshed is visible to `w1 + w2` and its
    // declared shape wins. Only the list/character rows make `is_empty`
    // dispatch-sensitive, so each carries a without-stub control that
    // must report RY040; the package row's double operands stay silent
    // either way, so its integer shape assertion pins the stub where
    // the primitive would say double.
    let cases = [
        (
            "base.json",
            stub_file(&[op_method("+", "widget", "double")]),
            "w1 <- structure(list(a = 1), class = \"widget\")\n\
             w2 <- structure(list(b = 2), class = \"widget\")\n\
             total <- w1 + w2\n",
            "total",
            Mode::Double,
            true,
        ),
        (
            "acme.json",
            stub_file(&[op_method("+", "widget", "integer")]),
            "w <- structure(1, class = \"widget\")\nout <- w + w\n",
            "out",
            Mode::Integer,
            false,
        ),
        (
            "base.json",
            stub_file(&[op_method("Ops", "gadget", "logical")]),
            "g1 <- structure(\"a\", class = \"gadget\")\n\
             g2 <- structure(\"b\", class = \"gadget\")\n\
             merged <- g1 + g2\n",
            "merged",
            Mode::Logical,
            true,
        ),
    ];
    for (file, json, src, binding, mode, dispatch_sensitive) in cases {
        let (with, scope) = check_with_stubs(src, &[(file, &json)]);
        assert!(
            with.is_empty(),
            "the stubbed method must satisfy operator dispatch: {with:?}"
        );
        assert_eq!(
            scope.get(binding).map(|t| (t.mode, t.length)),
            Some((mode, Length::One)),
            "the stub's declared shape must be applied for `{binding}`"
        );
        if dispatch_sensitive {
            let without = check(src);
            assert!(
                without.iter().any(|d| d.code == "RY040"),
                "without the stub the arithmetic rules must flag `{binding}`: {without:?}"
            );
        }
    }
}

#[test]
fn stub_default_method_does_not_hijack_operator_dispatch() {
    // R's operators have no implicit `.default` fallback: the primitive
    // itself is the fallback, so a `.default` stub must neither satisfy
    // dispatch nor report a missing method (#165, corrected against
    // real R -- unlike the call path, where `.default` is a real
    // fallback). The second case queries the guarded rung itself: an
    // operand whose class vector is literally `"default"` skips the
    // `.default` stub and falls back to ry's primitive. Divergence: R
    // walks the operand's class vector without special-casing, so it
    // would dispatch the literal `+.default` for a class named
    // `"default"` (verified against R 4.6.1, which returns the method's
    // value in either operand order); ry deliberately skips the
    // `"default"` rung so operator dispatch stays default-free -- an
    // accepted divergence for a pathological class name.
    let json = stub_file(&[op_method("+", "default", "opaque")]);
    let (with, _) = check_with_stubs(
        "x <- structure(list(), class = \"unhandled\")\ny <- x + 1\n",
        &[("base.json", &json)],
    );
    assert!(
        with.iter().any(|d| d.code == "RY040") && with.iter().all(|d| d.code != "RY050"),
        "a stub `+.default` must neither satisfy nor report operator dispatch: {with:?}"
    );
    let (guarded, scope) = check_with_stubs(
        "d <- structure(1, class = \"default\")\nout <- d + 1\n",
        &[("base.json", &json)],
    );
    assert!(
        guarded.is_empty(),
        "the guarded rung must fall back to the primitive silently: {guarded:?}"
    );
    assert_eq!(
        scope.get("out").map(|t| (t.mode, t.length)),
        Some((Mode::Double, Length::One)),
        "the primitive fallback keeps the numeric result"
    );
}

#[test]
fn operator_dispatch_miss_falls_back_to_the_primitive() {
    // Real R silently computes `bar + 1` with the primitive even when
    // `+.foo` exists for another class: no RY050, and the primitive's
    // own checks still apply (#165, corrected against real R).
    let (diags, scope) = check_with_scope(
        "`+.foo` <- function(e1, e2) 1L\n\
         x <- structure(1, class = \"bar\")\n\
         y <- x + 1\n\
         bad <- structure(list(a = 1), class = \"bar\") + 1\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "an operator miss is R's silent primitive fallback: {diags:?}"
    );
    assert_eq!(
        scope.get("y").map(|t| t.mode),
        Some(Mode::Double),
        "numeric-classed arithmetic keeps the primitive result"
    );
    assert_eq!(
        code_lines(&diags, "RY040"),
        vec![3],
        "only the list-mode operand stays arithmetic-checked: {diags:?}"
    );
}

#[test]
fn short_circuit_operators_never_dispatch_through_ops() {
    // `&&`/`||` are strictly logical short-circuit primitives in R: no
    // `Ops` dispatch can intercept them, and the ordinary length/type
    // diagnostics keep firing (#165).
    let (diags, scope) = check_with_scope(
        "`Ops.flagged` <- function(e1, e2) 1L\n\
         a <- structure(TRUE, class = \"flagged\")\n\
         both <- a && a\n\
         either <- a || a\n\
         v <- structure(c(TRUE, FALSE), class = \"flagged\")\n\
         long <- v && a\n\
         s <- structure(\"x\", class = \"flagged\")\n\
         typed <- s || a\n",
    );
    for name in ["both", "either"] {
        assert_eq!(
            scope.get(name).map(|t| (t.mode, t.length)),
            Some((Mode::Logical, Length::One)),
            "`{name}` must be the primitive logical(1) result"
        );
    }
    assert!(
        diags.iter().any(|d| d.code == "RY032"),
        "the vector `&&` operand must still report RY032: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == "RY031"),
        "the character `||` operand must still report RY031: {diags:?}"
    );
}

#[test]
fn opaque_group_stubs_keep_the_base_operator_diagnostics() {
    // Every embedded `Ops.<class>` stub is opaque; falling through on
    // an opaque group stub keeps the storage-mode rules modeling those
    // base classes: `Ops.factor` warns for *any* factor arithmetic
    // (`factor + list` included), and `Date + character` stays the
    // primitive's own RY040 (#165).
    let (diags, scope) = check_with_scope(
        "f <- factor(c(\"a\", \"b\"))\n\
         l <- list(1)\n\
         z <- f + l\n\
         w <- f + 1\n\
         d <- structure(19000, class = \"Date\")\n\
         y <- d + \"x\"\n\
         ok <- d + 1\n",
    );
    assert_eq!(
        code_lines(&diags, "RY042"),
        vec![2, 3],
        "factor arithmetic keeps RY042 for both the list and numeric counterpart: {diags:?}"
    );
    assert_eq!(
        code_lines(&diags, "RY040"),
        vec![5],
        "only Date + character is the primitive-mode error: {diags:?}"
    );
    assert_eq!(
        code_lines(&diags, "RY041"),
        Vec::<usize>::new(),
        "no operand pair recycles unevenly on this source: {diags:?}"
    );
    assert_eq!(
        scope.get("ok").map(|t| t.mode),
        Some(Mode::Double),
        "Date arithmetic keeps the lattice result"
    );
}

#[test]
fn factor_arithmetic_does_not_warn_about_recycling() {
    // `Ops.factor` preempts the primitive for all factor arithmetic and
    // never recycles (verified against R 4.6): RY042 only, never RY041
    // -- including where the modes would arith-combine (`f + 1:2`) and
    // where they cannot (`f + list`). The plain line keeps RY041.
    let (diags, _) = check_with_scope(
        "f <- structure(1:3, class = \"factor\")\n\
         l <- list(1, 2)\n\
         a <- f + l\n\
         b <- f + 1:2\n\
         plain <- c(1, 2, 3) + c(10, 20)\n",
    );
    assert_eq!(
        code_lines(&diags, "RY042"),
        vec![2, 3],
        "factor arithmetic keeps RY042 for both the list and numeric counterpart: {diags:?}"
    );
    assert_eq!(
        code_lines(&diags, "RY041"),
        vec![4],
        "only the non-factor line may warn about recycling: {diags:?}"
    );
    assert_eq!(
        code_lines(&diags, "RY040"),
        Vec::<usize>::new(),
        "factor arithmetic never reports the primitive-mode error on this source: {diags:?}"
    );
}

#[test]
fn operator_dispatch_tries_rhs_and_class_vector_order() {
    // R tries the LHS operand's classes, then the RHS's, and within one
    // operand the class vector in order: `1 + y` dispatches on `y`, and
    // a `c("second", "first")` value finds `+.first` only because
    // "second" has no method.
    let (diags, scope) = check_with_scope(
        "`+.rhs` <- function(e1, e2) \"R\"\n\
         `+.first` <- function(e1, e2) \"F\"\n\
         y <- structure(1, class = \"rhs\")\n\
         from_rhs <- 1 + y\n\
         z <- structure(1, class = c(\"second\", \"first\"))\n\
         from_order <- z + 1\n",
    );
    assert!(
        diags.is_empty(),
        "both dispatched operators must be silent: {diags:?}"
    );
    for name in ["from_rhs", "from_order"] {
        assert_eq!(
            scope.get(name).map(|t| t.mode),
            Some(Mode::Character),
            "`{name}` must use the dispatched method's return"
        );
    }
}
