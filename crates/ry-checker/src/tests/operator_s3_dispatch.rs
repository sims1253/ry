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
fn data_frame_operator_overrides_precede_schema_inference() {
    let (diags, scope) = check_with_scope(
        "`+.left` <- function(e1, e2) \"left\"\n\
         `+.right` <- function(e1, e2) \"right\"\n\
         chooseOpsMethod.left <- function(x, y, mx, my, cl, reverse) TRUE\n\
         x <- structure(data.frame(a = 1), class = c(\"left\", \"data.frame\"))\n\
         y <- structure(2, class = \"right\")\n\
         one_sided <- x + 1\n\
         same_method <- x + x\n\
         conflict <- x + y\n\
         reversed <- y + x\n",
    );
    assert!(diags.is_empty(), "{diags:?}");
    for name in ["one_sided", "same_method"] {
        assert_eq!(scope.get(name).map(|ty| ty.mode), Some(Mode::Character));
    }
    for name in ["conflict", "reversed"] {
        let ty = scope.get(name).expect("operator result should be bound");
        assert_eq!(ty.mode, Mode::Opaque);
        assert!(ty.columns.is_none(), "{name}: {ty:?}");
    }
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
            "w1 <- list(a = 1); class(w1) <- \"widget\"\n\
             w2 <- list(b = 2); class(w2) <- \"widget\"\n\
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
            "g1 <- \"a\"; class(g1) <- \"gadget\"\n\
             g2 <- \"b\"; class(g2) <- \"gadget\"\n\
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
        "x <- list(); class(x) <- \"unhandled\"\ny <- x + 1\n",
        &[("base.json", &json)],
    );
    assert!(
        with.iter().any(|d| d.code == "RY040") && with.iter().all(|d| d.code != "RY050"),
        "a stub `+.default` must neither satisfy nor report operator dispatch: {with:?}"
    );
    let (guarded, scope) = check_with_stubs(
        "d <- 1; class(d) <- \"default\"\nout <- d + 1\n",
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
fn factor_arithmetic_returns_logical_missing_values() {
    for (other, length) in [
        ("NULL", Length::Known(2)),
        ("numeric(0)", Length::Known(2)),
        ("1", Length::Known(2)),
        ("list(1, 2, 3)", Length::Known(3)),
    ] {
        for expression in [format!("f + {other}"), format!("{other} + f")] {
            let (diags, scope) = check_with_scope(&format!(
                "f <- structure(1:2, class = \"factor\")\nx <- {expression}\n"
            ));
            assert_eq!(diags.len(), 1, "{expression}: {diags:?}");
            assert_eq!(diags[0].code, "RY042", "{expression}");
            assert_eq!(scope.get("x"), Some(&RType::new(Mode::Logical, length)));
        }
    }
    let (diags, scope) =
        check_with_scope("x <- structure(vector(\"integer\", 0), class = \"factor\") + NULL\n");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "RY042");
    assert_eq!(
        scope.get("x"),
        Some(&RType::new(Mode::Logical, Length::Zero))
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

#[test]
fn unary_factor_arithmetic_warns_and_returns_logical() {
    let (diags, scope) = check_with_scope("x <- -structure(1:3, class = 'factor')");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "RY042");
    assert_eq!(
        scope.get("x"),
        Some(&RType::new(Mode::Logical, Length::Known(3)))
    );
}

#[test]
fn opaque_custom_group_winner_keeps_result_unknown() {
    let json = stub_file(&[op_method("Ops", "custom", "opaque")]);
    for (file, attachment) in [("custom.json", "library(custom)\n"), ("base.json", "")] {
        let source = format!(
            "{attachment}`+.parent` <- function(e1, e2) 1L\n\
             `-.parent` <- function(e1, e2) 1L\n\
             x <- list(); class(x) <- c('custom', 'parent')\n\
             left <- x + 1\nright <- 1 + x\nnegated <- -x\n"
        );
        let (diagnostics, scope) = check_with_stubs(&source, &[(file, &json)]);
        assert!(diagnostics.is_empty(), "{file}: {diagnostics:?}");
        for name in ["left", "right", "negated"] {
            assert_eq!(scope.get(name).map(|ty| ty.mode), Some(Mode::Opaque));
        }
    }
}

#[test]
fn literal_ops_choosers_follow_scoped_values_and_reverse_order() {
    let prefix = "x <- structure(1, class = 'left')\ny <- structure(2, class = 'right')\n`+.left` <- function(e1, e2) 'left'\n`+.right` <- function(e1, e2) 1L\n";
    for (selectors, mode) in [
        (
            "chooseOpsMethod.left <- function(...) TRUE",
            Mode::Character,
        ),
        (
            "chooseOpsMethod.left <- function(x,y,mx,my,cl,reverse) FALSE\nchooseOpsMethod.right <- function(...) TRUE",
            Mode::Integer,
        ),
        (
            "chooseOpsMethod.left <- function(...) TRUE\nchooseOpsMethod.right <- function(...) TRUE",
            Mode::Character,
        ),
        (
            "select <- function(...) TRUE\nchooseOpsMethod.left <- select",
            Mode::Character,
        ),
        (
            "chooseOpsMethod.left <- function(...) FALSE\nchooseOpsMethod.right <- function(...) FALSE",
            Mode::Double,
        ),
        ("chooseOpsMethod.right <- function(...) TRUE", Mode::Opaque),
        (
            "chooseOpsMethod.left <- function(...) sample(c(TRUE,FALSE),1)",
            Mode::Opaque,
        ),
    ] {
        let source = format!("{prefix}{selectors}\nout <- x + y");
        let (_, scope) = check_with_scope(&source);
        assert_eq!(scope.get("out").unwrap().mode, mode, "{source}");
    }
    let source = format!(
        "{prefix}chooseOpsMethod.left <- function(...) TRUE\nchooseOpsMethod.right <- function(...) TRUE\nf <- function() {{ chooseOpsMethod.left <- function(...) FALSE; x + y }}\nout <- f()"
    );
    let (_, scope) = check_with_scope(&source);
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
}

#[test]
fn literal_ops_method_identity_is_assignment_identity() {
    let prefix = "x <- structure(1, class = 'left')\ny <- structure(2, class = 'right')\n";
    for (methods, expected) in [
        (
            "method <- function(e1,e2) 'same'\n`+.left` <- method\n`+.right` <- method",
            Mode::Character,
        ),
        (
            "`+.left` <- function(e1,e2) 'left'\n`+.right` <- function(e1,e2) 'right'",
            Mode::Opaque,
        ),
        (
            "factory <- function() function(e1,e2) 'same'\n`+.left` <- factory()\n`+.right` <- factory()",
            Mode::Opaque,
        ),
    ] {
        let source = format!("{prefix}{methods}\nout <- x + y");
        let (_, scope) = check_with_scope(&source);
        assert_eq!(scope.get("out").unwrap().mode, expected, "{source}");
    }
}

#[test]
fn literal_ops_chooser_evidence_does_not_survive_uncertain_writes() {
    let prefix = "x <- structure(1, class = 'left')\ny <- structure(2, class = 'right')\n`+.left` <- function(e1,e2) 'left'\n`+.right` <- function(e1,e2) 1L\nchooseOpsMethod.left <- function(...) TRUE\n";
    for mutation in [
        "chooseOpsMethod.left <- function(...) FALSE",
        "if (unknown) chooseOpsMethod.left <- function(...) FALSE",
        "for (i in 1:2) chooseOpsMethod.left <- function(...) FALSE",
        "while (unknown) chooseOpsMethod.left <- function(...) FALSE",
        "assign('chooseOpsMethod.left', function(...) FALSE)",
        "rm(chooseOpsMethod.left)",
        "change()",
        "`{` <- function(...) FALSE",
    ] {
        let source = format!("{prefix}{mutation}\nout <- x + y");
        let (_, scope) = check_with_scope(&source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{source}");
    }
    let (diags, _) = check_with_scope(&format!("{prefix}unknown((x + y) + 1L)"));
    assert!(!diags.iter().any(|diagnostic| diagnostic.code == "RY040"));
    let source = format!("{prefix}out <- x + {{ change(); y }}");
    let (_, scope) = check_with_scope(&source);
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
}

#[test]
fn literal_ops_proof_respects_syntax_and_call_barriers() {
    let prefix = "x <- structure(1, class = 'left')\ny <- structure(2, class = 'right')\n`+.left` <- function(e1,e2) 'left'\n`+.right` <- function(e1,e2) 1L\nchooseOpsMethod.left <- function(...) TRUE\n";
    for name in ["function", "<-", "=", "::", "(", "if", r"\x3a\x3a"] {
        let source = format!("`{name}` <- function(...) FALSE\n{prefix}out <- x + y");
        let (_, scope) = check_with_scope(&source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{source}");
    }
    for call in ["base::structure(x, class = 'left')", "unknown()"] {
        let source = format!("{prefix}x <- {call}");
        let (_, scope) = check_with_scope(&source);
        assert!(scope.literal_functions.is_empty(), "{source}");
    }
}

#[test]
fn literal_ops_proof_is_not_established_in_deferred_functions() {
    let source = "f <- function() { x <- structure(1, class = 'left'); y <- structure(2, class = 'right'); `+.left` <- function(e1,e2) 'left'; `+.right` <- function(e1,e2) 1L; chooseOpsMethod.left <- function(...) FALSE; chooseOpsMethod.right <- function(...) TRUE; x + y }\nout <- f()";
    let (_, scope) = check_with_scope(source);
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
}

#[test]
fn literal_ops_proof_does_not_trust_delayed_reads() {
    let source = "x <- structure(1L, class = 'left')\ny <- structure(1L, class = 'right')\ntrigger <- 1L\ndelayedAssign('trigger', { chooseOpsMethod.left <- function(...) FALSE; 1L })\n`+.left` <- function(e1,e2) 'left'\n`+.right` <- function(e1,e2) FALSE\nchooseOpsMethod.left <- function(...) TRUE\nchooseOpsMethod.right <- function(...) TRUE\ntrigger\nout <- x + y";
    let (_, scope) = check_with_scope(source);
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
}

#[test]
fn literal_ops_environment_uncertainty_survives_control_flow() {
    for effect in [
        "if (flag) delayedAssign('trigger', mutation)",
        "for (i in 1L) delayedAssign('trigger', mutation)",
        "while (flag) delayedAssign('trigger', mutation)",
        "flag && { delayedAssign('trigger', mutation); TRUE }",
        "ignored <- if (flag) delayedAssign('trigger', mutation) else NULL",
        "unknown_identifier",
    ] {
        let source = format!(
            "x <- structure(1L,class='left')\ny <- structure(2L,class='right')\nflag <- TRUE\ntrigger <- 1L\n{effect}\n`+.left` <- function(e1,e2) 'left'\n`+.right` <- function(e1,e2) 1L\nchooseOpsMethod.left <- function(...) TRUE\ntrigger\nout <- x + y"
        );
        let (_, scope) = check_with_scope(&source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{source}");
    }
}

#[test]
fn literal_ops_proof_does_not_freeze_future_function_environments() {
    let source = "f <- function() { `+.left` <- function(e1,e2) 'left'; `+.right` <- function(e1,e2) 1L; chooseOpsMethod.left <- function(...) TRUE; x <- structure(1L,class='left'); y <- structure(1L,class='right'); x + y }\nassign('structure', function(...) 1, envir=.GlobalEnv)\nf() + 1";
    let (diags, _) = check_with_scope(source);
    assert!(
        !diags.iter().any(|diagnostic| diagnostic.code == "RY040"),
        "{diags:?}"
    );
}

#[test]
fn literal_ops_selected_arithmetic_diagnostic() {
    let source = include_str!("../../testdata/err_s3_literal_ops_chooser.R");
    for source in [
        source.to_string(),
        source.replace("structure(", "base::structure("),
    ] {
        let (diags, _) = check_with_scope(&source);
        assert!(
            diags.iter().any(|diagnostic| diagnostic.code == "RY040"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn literal_ops_proof_rejects_imported_syntax_and_constructor_bindings() {
    let source = include_str!("../../testdata/err_s3_literal_ops_chooser.R");
    for name in ["function", "::", "+", "structure", r"\x3a\x3a"] {
        let diagnostics = check_with(source, |checker| {
            checker.set_external_bindings(HashSet::from([name.to_string()]));
            checker.set_imported_from(HashMap::from([(name.to_string(), "custom".to_string())]));
        });
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY040"),
            "{name}: {diagnostics:?}"
        );
    }
}

#[test]
fn literal_ops_proof_rejects_an_initial_attached_package() {
    let source = include_str!("../../testdata/err_s3_literal_ops_chooser.R");
    let diagnostics = check_with(source, |checker| {
        checker.set_bare_loaded(HashSet::from(["custom".to_string()]));
    });
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040")
    );
}

#[test]
fn literal_ops_proof_rejects_nonordinary_assignment_operators() {
    let prefix = "x <- structure(1L,class='left')\ny <- structure(2L,class='right')\n`+.left` <- function(e1,e2) 'left'\n`+.right` <- function(e1,e2) 1L\nchooseOpsMethod.left <- function(...) TRUE\nchooseOpsMethod.right <- function(...) TRUE\n";
    for effect in [
        "`:=` <- function(...) { chooseOpsMethod.left <<- function(...) FALSE }; ignored := 1L",
        "(function(...) FALSE) ->> chooseOpsMethod.left",
        "chooseOpsMethod.left <<- function(...) FALSE",
        "ignored <- ((function(...) FALSE) ->> chooseOpsMethod.left)",
    ] {
        let source = format!("{prefix}{effect}\nout <- x + y");
        let (_, scope) = check_with_scope(&source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{source}");
    }
}

#[test]
fn literal_ops_proof_is_cleared_before_unknown_operator_arguments() {
    let prefix = "x <- structure(1L,class='left')\ny <- structure(2L,class='right')\n`*.left` <- function(e1,e2) 'left'\n`*.right` <- function(e1,e2) 1L\nchooseOpsMethod.left <- function(...) TRUE\nchooseOpsMethod.right <- function(...) TRUE\n";
    for operation in [
        "`!` <- function(value) { chooseOpsMethod.left <<- function(...) FALSE; value }; !((x * y) + 1L)",
        "`[` <- function(value,index) { chooseOpsMethod.left <<- function(...) FALSE; index }; x[(x * y) + 1L]",
        "`^` <- function(value,power) { chooseOpsMethod.left <<- function(...) FALSE; value }; ((x * y) + 1L) ^ 2L",
    ] {
        let source = format!("{prefix}{operation}");
        let (diagnostics, _) = check_with_scope(&source);
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY040"),
            "{source}: {diagnostics:?}"
        );
    }
}
