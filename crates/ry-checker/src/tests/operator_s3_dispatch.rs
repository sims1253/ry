use super::*;

/// `check_with_scope` plus stub typeshed files (file name, raw JSON)
/// installed from one temp directory, for tests that assert both the
/// diagnostics and the inferred result type of dispatch through
/// stub-declared methods. A `base.json` replaces the embedded base
/// typeshed wholesale; other files become package typesheds. The
/// un-stubbed behavior of the same source is compared with plain
/// [`check`].
fn check_with_stubs(src: &str, stub_files: &[(&str, &str)]) -> (Vec<Diagnostic>, Scope) {
    let dir = tempfile::tempdir().unwrap();
    for (name, json) in stub_files {
        std::fs::write(dir.path().join(name), json).unwrap();
    }
    let stubs = Arc::new(ry_typeshed::load_stub_dir(dir.path()).unwrap());
    let mut c = Checker::new("test.R");
    c.set_user_stubs(stubs);
    c.check_with_scope(&parse_snippet("test.R", src))
}

#[test]
fn operator_dispatches_stub_typeshed_specific_method() {
    // A `+.widget` method that exists only in a stub typeshed must be
    // visible to the operator path, exactly as it is to normal call
    // dispatch (#165): with the stub, `w1 + w2` uses the stub's return
    // shape; without it, the same program reaches the arithmetic rules
    // and reports the list-mode mismatch.
    let stub = r#"{
        "schema_version": "1",
        "package": "base",
        "version": "test",
        "functions": {},
        "s3_methods": [
            {"generic": "+", "class": "widget", "params": ["e1", "e2"], "return": {"mode": "double", "length": "1"}}
        ]
    }"#;
    let src = "w1 <- structure(list(a = 1), class = \"widget\")\n\
               w2 <- structure(list(b = 2), class = \"widget\")\n\
               total <- w1 + w2\n";
    let (with, scope) = check_with_stubs(src, &[("base.json", stub)]);
    assert!(
        with.iter().all(|d| d.code != "RY040"),
        "the stubbed `+.widget` method must satisfy operator dispatch: {with:?}"
    );
    assert!(
        check(src).iter().any(|d| d.code == "RY040"),
        "without the stub the same operands must reach the arithmetic rules"
    );
    let total = scope.get("total").expect("total should be bound");
    assert_eq!(
        (total.mode, total.length),
        (Mode::Double, Length::One),
        "the stub's return shape must win over the opaque dispatch default: {total:?}"
    );
}

#[test]
fn operator_dispatches_stub_typeshed_group_method_shape() {
    // An `Ops.gadget` group-generic stub with a usable shape is
    // dispatched with that shape (the group rung shares the ladder;
    // only an *opaque* group stub defers to the base-type rules).
    let stub = r#"{
        "schema_version": "1",
        "package": "base",
        "version": "test",
        "functions": {},
        "s3_methods": [
            {"generic": "Ops", "class": "gadget", "params": ["e1", "e2"], "return": {"mode": "logical", "length": "1"}}
        ]
    }"#;
    let src = "g1 <- structure(c(\"a\"), class = \"gadget\")\n\
               g2 <- structure(c(\"b\"), class = \"gadget\")\n\
               merged <- g1 + g2\n";
    let (with, scope) = check_with_stubs(src, &[("base.json", stub)]);
    assert!(
        with.is_empty(),
        "the stubbed `Ops.gadget` method must satisfy operator dispatch: {with:?}"
    );
    assert!(
        check(src).iter().any(|d| d.code == "RY040"),
        "without the stub, character arithmetic must be rejected"
    );
    let merged = scope.get("merged").expect("merged should be bound");
    assert_eq!(
        (merged.mode, merged.length),
        (Mode::Logical, Length::One),
        "the group stub's declared shape must be applied: {merged:?}"
    );
}

#[test]
fn stub_default_method_does_not_hijack_operator_dispatch() {
    // Real R never consults `+.default` for an operator whose class has
    // no method: the primitive computes the result (and errors for a
    // list operand) regardless of any `.default` method defined
    // anywhere. So a stub `+.default` must not suppress the primitive's
    // own rules -- unlike the call path, where `.default` is a real
    // fallback (#165, corrected against verified R semantics).
    let stub = r#"{
        "schema_version": "1",
        "package": "base",
        "version": "test",
        "functions": {},
        "s3_methods": [
            {"generic": "+", "class": "default", "params": ["e1", "e2"], "return": {"mode": "opaque", "length": "unknown"}}
        ]
    }"#;
    let src = "x <- structure(list(), class = \"unhandled\")\ny <- x + 1\n";
    let (with, _) = check_with_stubs(src, &[("base.json", stub)]);
    assert!(
        with.iter().any(|d| d.code == "RY040") && with.iter().all(|d| d.code != "RY050"),
        "a stub `+.default` must neither satisfy nor report operator dispatch: {with:?}"
    );
}

#[test]
fn operator_dispatch_miss_falls_back_to_the_primitive() {
    // Real R silently computes `bar + 1` with the primitive even when
    // `+.foo` or `Ops.foo` exists for another class: unlike ordinary
    // generics, a primitive operator is its own fallback. A miss must
    // not report RY050, must not conclude opaque through an unrelated
    // method, and must keep the primitive's own checks (#165's RY050
    // criterion, corrected against verified R semantics).
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
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(
        y.mode,
        Mode::Double,
        "numeric-classed arithmetic keeps the primitive result: {y:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "the list-mode operand must stay arithmetic-checked: {diags:?}"
    );
}

#[test]
fn short_circuit_operators_never_dispatch_through_ops() {
    // `&&`/`||` are strictly logical short-circuit primitives in R: no
    // `Ops` group dispatch, so an `Ops.flagged` method cannot intercept
    // them, and the ordinary length/type diagnostics keep firing
    // (#165, pinned against verified R semantics).
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
        let t = scope.get(name).expect("result should be bound");
        assert_eq!(
            (t.mode, t.length),
            (Mode::Logical, Length::One),
            "`{name}` must be the primitive logical(1) result, not a dispatch: {t:?}"
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
    // Every embedded `Ops.<class>` stub is opaque, but that must not
    // silence the storage-mode rules: base R's own `Ops.factor` warns
    // for *any* factor arithmetic (`factor + list` included), and
    // `Date + character` is the primitive's own error. Falling through
    // keeps both diagnostics while preserving the useful shaping for
    // `factor + 1` and `Date` arithmetic (#165).
    let (diags, scope) = check_with_scope(
        "f <- factor(c(\"a\", \"b\"))\n\
         l <- list(1)\n\
         z <- f + l\n\
         w <- f + 1\n\
         d <- structure(19000, class = \"Date\")\n\
         y <- d + \"x\"\n\
         ok <- d + 1\n",
    );
    let factor_lines = diags
        .iter()
        .filter(|d| d.code == "RY042")
        .map(|d| d.span.line)
        .collect::<Vec<_>>();
    assert_eq!(
        factor_lines,
        vec![2, 3],
        "factor arithmetic keeps RY042 for both the list and numeric counterpart: {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code != "RY040" || d.span.line == 5),
        "the Date+character primitive error stays, factor arithmetic must not become RY040: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040" && d.span.line == 5),
        "Date + character must retain the primitive-mode error: {diags:?}"
    );
    let ok = scope.get("ok").expect("ok should be bound");
    assert_eq!(
        ok.mode,
        Mode::Double,
        "Date arithmetic keeps the lattice result: {ok:?}"
    );
    // Unary `-` on a factor is deliberately unpinned: R warns and
    // returns NA, which the checker's silent shaping does not model.
}

#[test]
fn factor_arithmetic_does_not_warn_about_recycling() {
    // `Ops.factor` preempts the primitive for *all* arithmetic: real R
    // warns "'+' not meaningful for factors" and returns
    // `rep.int(NA, max(length(e1), length(e2)))` without ever recycling,
    // so no factor path may claim "R will recycle with a warning"
    // (verified against R 4.6: neither `f + list(1, 2)` nor `f + 1:2`
    // emits the `longer object length ...` warning). The list line is
    // the review corner where the lattice cannot combine the operands
    // at all; the `1:2` line is where it can, so gating on `arith`
    // succeeding alone would not suffice. RY042 stays on both, and the
    // ordinary non-factor recycling case keeps its warning.
    let (diags, _) = check_with_scope(
        "f <- structure(1:3, class = \"factor\")\n\
         l <- list(1, 2)\n\
         a <- f + l\n\
         b <- f + 1:2\n\
         plain <- c(1, 2, 3) + c(10, 20)\n",
    );
    let ry042_lines = diags
        .iter()
        .filter(|d| d.code == "RY042")
        .map(|d| d.span.line)
        .collect::<Vec<_>>();
    assert_eq!(
        ry042_lines,
        vec![2, 3],
        "factor arithmetic keeps RY042 for both the list and numeric counterpart: {diags:?}"
    );
    let ry041_lines = diags
        .iter()
        .filter(|d| d.code == "RY041")
        .map(|d| d.span.line)
        .collect::<Vec<_>>();
    assert_eq!(
        ry041_lines,
        vec![4],
        "only the non-factor line may warn about recycling: {diags:?}"
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
    for (name, expected) in [("from_rhs", "R"), ("from_order", "F")] {
        let t = scope.get(name).unwrap_or_else(|| panic!("{name} bound"));
        assert_eq!(
            t.mode,
            Mode::Character,
            "`{name}` must use the `{expected}` method's return: {t:?}"
        );
    }
}

#[test]
fn operator_dispatches_package_typeshed_method() {
    // The ladder's last rung: an operator method declared in a package
    // typeshed (not the project fn table, not base) is visible to the
    // operator path, mirroring the call path's lookup.
    let package = r#"{
        "schema_version": "1",
        "package": "acme",
        "version": "test",
        "functions": {},
        "s3_methods": [
            {"generic": "+", "class": "widget", "params": ["e1", "e2"], "return": {"mode": "integer", "length": "1"}}
        ]
    }"#;
    let src = "w <- structure(1, class = \"widget\")\nout <- w + w\n";
    let (with, scope) = check_with_stubs(src, &[("acme.json", package)]);
    assert!(
        with.is_empty(),
        "the package stub's `+.widget` must satisfy operator dispatch: {with:?}"
    );
    let out = scope.get("out").expect("out should be bound");
    assert_eq!(
        (out.mode, out.length),
        (Mode::Integer, Length::One),
        "the package stub's declared shape must be applied: {out:?}"
    );
}

#[test]
fn operator_method_absence_still_reaches_arithmetic_checks() {
    // Without any dispatchable method, operator inference keeps using
    // the base-type rules: compatible storage modes produce their
    // arithmetic result silently, incompatible ones still report RY040.
    let (diags, scope) = check_with_scope(
        "ok <- structure(1, class = \"plain\") + 1\n\
         bad <- structure(list(a = 1), class = \"plain\") + 1\n",
    );
    let bad = diags.iter().filter(|d| d.code == "RY040").count();
    assert_eq!(
        bad, 1,
        "the list-mode operand must stay arithmetic-checked: {diags:?}"
    );
    let ok = scope.get("ok").expect("ok should be bound");
    assert_eq!(
        ok.mode,
        Mode::Double,
        "numeric-classed arithmetic keeps the lattice result: {ok:?}"
    );
}
