use super::*;

#[test]
fn primitive_arithmetic_uses_operator_specific_result_modes() {
    let (diagnostics, scope) = check_with_scope(
        "division <- 1L / 2L\npower <- 2L ^ 3L\nalternate <- 2L ** 3L\nlogical <- TRUE / FALSE\nquotient <- 3L %/% 2L\nremainder <- 3L %% 2L\nf <- function() 1L / 2L\nreturned <- f()\n",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    for name in ["division", "power", "alternate", "logical", "returned"] {
        assert_eq!(scope.get(name).unwrap().mode, Mode::Double, "{name}");
    }
    for name in ["quotient", "remainder"] {
        assert_eq!(scope.get(name).unwrap().mode, Mode::Integer, "{name}");
    }
}

#[test]
fn primitive_coercion_does_not_override_s3_arithmetic_returns() {
    let (diagnostics, scope) = check_with_scope(
        "`/.widget` <- function(e1, e2) 1L\n`^.widget` <- function(e1, e2) 'power'\nx <- structure(2L, class = 'widget')\ndivision <- x / 2L\npower <- x ^ 2L\n",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("division").unwrap().mode, Mode::Integer);
    assert_eq!(scope.get("power").unwrap().mode, Mode::Character);
}

#[test]
fn complex_remainder_errors_only_for_known_nonempty_values() {
    let (diagnostics, scope) = check_with_stubs(
        "z <- values::scalar_complex()\nbad_mod <- z %% 2L\nbad_div <- 2L %/% z\nempty <- z %% values::empty_integer()\nuncertain <- z %% values::unknown_integer()\n",
        &[(
            "values.json",
            r#"{"version":"test","functions":{
            "scalar_complex":{"params":[],"return":{"mode":"complex","length":"1"}},
            "empty_integer":{"params":[],"return":{"mode":"integer","length":"0"}},
            "unknown_integer":{"params":[],"return":{"mode":"integer","length":"?"}}
        }}"#,
        )],
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| d.code == "RY040")
            .map(|d| d.span.line)
            .collect::<Vec<_>>(),
        [1, 2],
        "{diagnostics:?}"
    );
    assert_eq!(scope.get("empty").unwrap().mode, Mode::Complex);
    assert_eq!(scope.get("empty").unwrap().length, Length::Zero);
    assert_eq!(scope.get("uncertain").unwrap().mode, Mode::Opaque);
}

#[test]
fn detects_char_plus_int() {
    let diags = check(r#""a" + 1L"#);
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040, got {:?}",
        diags
    );
}

// Forwarded-default typing (#342). `forwarded_default_type` asserts that a
// caller's parameter default reaches the callee's formal when the argument
// is the bare parameter name and some caller call site omits it. That
// assertion is only sound while the caller never rebinds the name before
// the call: once the body assigns it (statement, loop variable, or
// expression-position `<-`), the forwarded value is the rebinding's
// result, not the literal default. ggplot2's `compute_bins` reassigns and
// standalone-checks `bins` before forwarding it, which manufactured a
// `logical<len=0>` condition at `bin_breaks_bins`'s `bins == 1`.
#[test]
fn reassigned_forwarded_default_does_not_reach_the_callee() {
    let diags = check(
        "callee <- function(x_range, bins = 30) {\n\
           if (bins == 1) 1 else 2\n\
         }\n\
         caller <- function(x, bins = NULL) {\n\
           bins <- allow_lambda(bins)\n\
           callee(range, bins)\n\
         }\n\
         z <- caller(c(0, 1))\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY001"),
        "a rebinding replaces the caller default before the call: {diags:?}"
    );
}

#[test]
fn rebinding_forms_invalidate_wrapped_and_backticked_forwarding() {
    for formal in ["bins", "`bins`"] {
        for body in [
            "`bins` <- 1L; callee(`bins`)",
            "bins[1L] <- 1L; result <- callee(bins)",
            "bins[[1L]] <- 1L; print(callee(bins))",
            "length(bins) <- 1L; return(callee(bins))",
            "assign('bins', 1L); callee(`bins` = bins)",
            "base::assign(value = 1L, x = 'bins'); callee(bins)",
            "delayedAssign('bins', 1L); callee(bins)",
            "for (`bins` in list(1L)) callee(bins)",
            "if ((`bins` <- 1L) > 0) print(callee(bins))",
            "bins <<- (bins <- 1L); print(callee(bins))",
            "(bins <- 1L) ->> bins; print(callee(bins))",
            "bins <- (bins <<- 1L); print(callee(bins))",
            "bins <- (1L ->> bins); print(callee(bins))",
            "1L -> bins; print(callee(bins))",
            "print({ bins <- 1L; callee(bins) })",
        ] {
            let source = format!(
                "callee <- function({formal} = 30) {{ if ({formal} == 1) 1L else 2L }}\ncaller <- function(bins = NULL) {{ {body} }}\nz <- caller()\n"
            );
            let diagnostics = check(&source);
            assert!(
                diagnostics.iter().all(|d| d.code != "RY001"),
                "{formal}: {body}: {diagnostics:?}"
            );
        }
    }
}

#[test]
fn pre_call_conditional_rebinding_invalidates_the_root_anchored_default() {
    let diags = check(
        "callee <- function(x_range, bins = 30) {\n\
           if (bins == 1) 1 else 2\n\
         }\n\
         caller <- function(x, bins = NULL) {\n\
           if (is.null(bins)) bins <- 30L\n\
           callee(range, bins)\n\
         }\n\
         z <- caller(c(0, 1))\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY001"),
        "a conditional rebinding before the anchored call may replace the default: {diags:?}"
    );
}

#[test]
fn loop_variable_rebinding_precedes_the_forwarding_call() {
    let diags = check(
        "callee <- function(x_range, bins = 30) {\n\
           if (bins == 1) 1 else 2\n\
         }\n\
         caller <- function(x, bins = NULL) {\n\
           for (bins in list(1)) callee(range, bins)\n\
         }\n\
         z <- caller(c(0, 1))\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY001"),
        "the loop variable replaces the default: {diags:?}"
    );
}

#[test]
fn forwarded_default_survives_without_a_prior_local_write() {
    for formal in ["bins", "`bins`"] {
        for body in [
            "callee(bins)",
            "callee(bins); bins <- 30L",
            "print(callee(bins)); bins <- 30L",
            "callee(bins); for (i in 1:2) bins <- 30L",
            "callee(bins); if (TRUE) bins <- 30L",
            "callee(bins); bins <- 30L; callee(bins)",
            "rebinder <- function() bins <- 5L; callee(bins)",
            "bins <- callee(bins)",
            "for (bins in callee(bins)) NULL",
            "bins <<- 1L; print(callee(bins))",
            "if ((`bins` <<- 1L) > 0) print(callee(bins))",
            "bins[1L] <<- 1L; print(callee(bins))",
            "length(bins) <<- 1L; print(callee(bins))",
            "1L ->> bins; print(callee(bins))",
            "if ((1L ->> `bins`) > 0) print(callee(bins))",
            "1L ->> bins[1L]; print(callee(bins))",
        ] {
            let source = format!(
                "callee <- function({formal} = 30) {{ if ({formal} == 1) 1L else 2L }}\ncaller <- function(bins = NULL) {{ {body} }}\nz <- caller()\n"
            );
            let diagnostics = check(&source);
            assert!(
                diagnostics.iter().any(|d| d.code == "RY001"),
                "{formal}: {body}: {diagnostics:?}"
            );
        }
    }
    let diagnostics = check(include_str!(
        "../../testdata/oracle/forwarded_rebinding_after_call.R"
    ));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "RY001");
}

#[test]
fn superassignment_oracle_preserves_forwarded_default_diagnostic() {
    let diagnostics = check(include_str!(
        "../../testdata/oracle/forwarded_superassignment.R"
    ));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "RY001");
}

#[test]
fn condition_assignment_counts_as_a_rebinding() {
    // Bounded delta from the earlier blanket invalidation:
    // may_rebind_source_before descends control-flow tests, unlike
    // assigned_names_in_body, so a `while ((bins <- f()) > 0)` condition
    // assignment counts. It genuinely rebinds before any later call, so
    // the forwarded default is dropped here.
    let diags = check(
        "callee <- function(x_range, bins = 30) {\n\
           if (bins == 1) 1 else 2\n\
         }\n\
         caller <- function(x, bins = NULL) {\n\
           while ((bins <- f(x)) > 0) break\n\
           callee(range, bins)\n\
         }\n\
         z <- caller(c(0, 1))\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY001"),
        "an assignment in a control test rebinds before the call: {diags:?}"
    );
}

#[test]
fn proven_zero_length_conditions_still_fire() {
    // Insurance for the #342 family: genuinely zero-length conditions keep
    // their diagnostics; only the manufactured forwarded-default zero goes.
    let diags = check(
        "a <- if (character(0) == \"a\") 1 else 2\n\
         b <- if (numeric(0) > 1) 1 else 2\n",
    );
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY001").count(),
        2,
        "real zero-length conditions stay diagnosed: {diags:?}"
    );
}

#[test]
fn allows_int_plus_double() {
    let diags = check("1L + 2.0\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

// Table-driven RY001/RY002/RY003 condition family: each row pins which
// family code one condition source fires, that its sibling codes stay
// silent, and that RY003 keeps its info-level severity. Absorbs the
// former single-case `detects_if_on_character` and
// `detects_long_condition_warning` tests.
#[test]
fn condition_rules_fire_their_family_code() {
    for (note, src, expected) in [
        ("integer `if` condition", "if (1L) print(1)", "RY003"),
        (
            "numeric-union `if` condition",
            "x <- if (runif(1) > 0.5) 1L else 2.0\nif (x) print(1)",
            "RY003",
        ),
        // A scalar character member no longer proves a union invalid
        // (#373); a list member still does.
        (
            "invalid-union `if` condition",
            "x <- if (runif(1) > 0.5) 1L else list(\"a\")\nif (x) print(1)",
            "RY001",
        ),
        ("NULL `if` condition", "if (NULL) print(1)", "RY001"),
        ("character `if` condition", r#"if ("x") print(1)"#, "RY001"),
        (
            "integer `while` condition",
            "n <- 1L\nwhile (n) n <- 0L",
            "RY003",
        ),
        (
            "length-2 logical `if` condition",
            "if (c(TRUE, FALSE)) print(1)\n",
            "RY002",
        ),
    ] {
        let diags = check(src);
        assert!(
            diags.iter().any(|d| d.code == expected),
            "{note}: expected {expected}, got {diags:?}"
        );
        for silent in ["RY001", "RY002", "RY003"] {
            assert!(
                silent == expected || diags.iter().all(|d| d.code != silent),
                "{note}: {silent} must stay silent, got {diags:?}"
            );
        }
        assert!(
            expected != "RY003"
                || diags
                    .iter()
                    .any(|d| d.code == "RY003" && d.severity == Severity::Info),
            "{note}: RY003 is an info-level nudge, got {diags:?}"
        );
    }
}

// #373: R coerces `if`/`while` conditions beyond logical scalars. Scalar
// raw values coerce numerically, scalar complex values coerce by OR over
// their real and imaginary components (0+0i is FALSE, 0+1i is TRUE), and
// a scalar character value coerces only when its text is one of the eight
// accepted literals. The value of a computed scalar string (Sys.getenv
// flags) is unknowable statically, so those conditions stay silent
// instead of claiming R rejects them.
#[test]
fn coercible_scalar_conditions_stay_silent() {
    for (note, src) in [
        ("character literal TRUE", r#"if ("TRUE") print(1)"#),
        ("character literal T", r#"if ("T") print(1)"#),
        ("character literal true", r#"if ("true") print(1)"#),
        ("character literal True", r#"if ("True") print(1)"#),
        ("character literal F", r#"if ("F") print(1)"#),
        ("character literal false", r#"if ("false") print(1)"#),
        (
            "hex-escaped literal decodes to TRUE",
            r#"if ("\x54RUE") print(1)"#,
        ),
        (
            "octal-escaped literal decodes to FALSE",
            r#"if ("\106ALSE") print(1)"#,
        ),
        ("raw string literal", r#"if (r"(TRUE)") print(1)"#),
        ("parenthesized literal", r#"if ((("TRUE"))) print(1)"#),
        (
            "Sys.getenv flag in if",
            r#"if (Sys.getenv("FLAG")) print(1)"#,
        ),
        (
            "Sys.getenv flag binding in if",
            "flag <- Sys.getenv(\"OTHER\")\nif (flag) print(1)\n",
        ),
        (
            "Sys.getenv flag in while",
            "while (Sys.getenv(\"LOOP\")) {\n  break\n}\n",
        ),
        ("raw scalar condition", "if (as.raw(1)) print(1)\n"),
        ("raw zero scalar condition", "if (as.raw(0)) print(1)\n"),
        (
            "complex scalar condition",
            "z <- complex(real = 1)\nif (z) print(1)\n",
        ),
        (
            "complex scalar via vector",
            "if (vector(\"complex\", 1)) print(1)\n",
        ),
        (
            "complex scalar with imaginary part",
            "if (complex(real = 0, imaginary = 1)) print(1)\n",
        ),
        (
            "complex scalar zero components",
            "if (complex(real = 0, imaginary = 0)) print(1)\n",
        ),
        // Imaginary literals (2+3i, 0+1i, 0+0i) type as opaque today, so
        // these guard silence through that path too; the constructor rows
        // above are what exercise Mode::Complex.
        ("imaginary literal 2+3i", "if (2+3i) print(1)\n"),
        ("imaginary literal 0+1i", "if (0+1i) print(1)\n"),
        ("imaginary literal 0+0i", "if (0+0i) print(1)\n"),
        (
            // NA_complex_ is complex<len=1>, so this exercises the same
            // silence arm. R ERRORS at runtime ("argument is not
            // interpretable as logical"): the VALUE, not the mode,
            // decides. Silence is the uncertainty policy for untracked
            // values (NA boundary stays with #354), not a validity claim.
            "NA complex constant stays silent (uncertainty boundary)",
            "if (NA_complex_) print(1)\n",
        ),
    ] {
        let diags = check(src);
        assert!(
            diags
                .iter()
                .all(|d| !matches!(d.code, "RY001" | "RY002" | "RY003")),
            "{note}: a condition R may coerce must stay silent, got {diags:?}"
        );
    }
}

// The complex/raw silence must flow through the real constructor paths
// (Mode::Complex via the `complex` stub, Mode::Raw via the `as.raw`
// stub), not through an opaque fallback that would be silent anyway.
#[test]
fn coercible_condition_silence_uses_real_complex_and_raw_modes() {
    let (diags, scope) = check_with_scope("z <- complex(real = 1)\nw <- as.raw(1)\n");
    assert!(
        matches!(scope.get("z").map(|t| t.mode), Some(Mode::Complex)),
        "complex(real = 1) must infer Mode::Complex"
    );
    assert!(
        matches!(scope.get("w").map(|t| t.mode), Some(Mode::Raw)),
        "as.raw(1) must infer Mode::Raw"
    );
    assert!(diags.is_empty(), "got {diags:?}");
    let (diags, _) = check_with_scope("z <- complex(real = 1)\nwhile (z) {\n  break\n}\n");
    assert!(
        diags
            .iter()
            .all(|d| !matches!(d.code, "RY001" | "RY002" | "RY003")),
        "complex while condition must stay silent, got {diags:?}"
    );
}

// The retained error side of #373: shapes R provably rejects keep RY001.
// String literals outside the eight accepted spellings (including the
// "NA" string, which the `if` coercion path rejects like any other
// text), multi-element character, zero-length, NULL, and list values
// all error at runtime.
#[test]
fn known_multi_value_conditions_are_rejected_in_if_and_while() {
    for (source, expected) in [
        ("if (c(TRUE, FALSE)) print(1)", "RY002"),
        ("if (c(1L, 2L)) print(1)", "RY001"),
        ("if (c(1, 2)) print(1)", "RY001"),
        ("while (c(TRUE, FALSE)) { break }", "RY001"),
        ("while (c(1L, 2L)) { break }", "RY001"),
        ("while (c(1, 2)) { break }", "RY001"),
    ] {
        let diags = check(source);
        let condition_codes: Vec<_> = diags
            .iter()
            .filter(|d| matches!(d.code, "RY001" | "RY002" | "RY003"))
            .map(|d| d.code)
            .collect();
        assert_eq!(condition_codes, vec![expected], "{source}: {diags:?}");
    }
}

#[test]
fn proven_invalid_conditions_keep_ry001() {
    for (note, src) in [
        ("non-coercible literal", r#"if ("x") print(1)"#),
        ("mixed-case literal", r#"if ("tRue") print(1)"#),
        ("numeric-looking literal", r#"if ("1") print(1)"#),
        ("empty literal", r#"if ("") print(1)"#),
        ("NA string literal", r#"if ("NA") print(1)"#),
        (
            "two-element character",
            r#"if (c("TRUE", "FALSE")) print(1)"#,
        ),
        ("zero-length character", "if (character(0)) print(1)\n"),
        (
            "zero-length complex",
            "if (vector(\"complex\", 0)) print(1)\n",
        ),
        (
            "two-element complex",
            "if (vector(\"complex\", 2)) print(1)\n",
        ),
        ("list condition", "if (list(TRUE)) print(1)\n"),
        ("NULL condition", "if (NULL) print(1)\n"),
    ] {
        let diags = check(src);
        assert!(
            diags.iter().any(|d| d.code == "RY001"),
            "{note}: a condition R rejects keeps RY001, got {diags:?}"
        );
    }
}

// Union conditions: a coercible scalar character member no longer marks
// the union invalid, but members with proven-wrong shapes (zero length,
// known length above one, list) still do. A logical or opaque member
// keeps its existing whole-union silence.
// Retain the existing uncertainty contract from scope_resolution:
// a valid logical alternative prevents a definitely-invalid RY001 claim.
// The runtime oracle pins both the passing and failing branch outcomes.
#[test]
fn logical_union_members_preserve_possibly_valid_conditions() {
    for (left, right) in [("TRUE", "list(TRUE)"), ("list(TRUE)", "TRUE")] {
        for condition in ["if (x) print(1)", "while (x) { break }"] {
            let source = format!("x <- if (runif(1) > 0.5) {left} else {right}\n{condition}\n");
            let (diags, scope) = check_with_scope(&source);
            assert_eq!(scope.get("x").expect("union binding").mode, Mode::Union);
            assert!(
                diags.iter().all(|d| d.code != "RY001"),
                "{source}: {diags:?}"
            );
            assert!(diags.iter().all(|d| !matches!(d.code, "RY002" | "RY003")));
        }
    }
    for (left, right) in [("TRUE", "1L"), ("1L", "TRUE")] {
        let source = format!("x <- if (runif(1) > 0.5) {left} else {right}\nif (x) print(1)\n");
        let (diags, scope) = check_with_scope(&source);
        assert_eq!(scope.get("x").expect("union binding").mode, Mode::Union);
        assert!(
            diags
                .iter()
                .all(|d| !matches!(d.code, "RY001" | "RY002" | "RY003")),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn union_condition_members_keep_proven_invalidity() {
    for (note, src, wants_ry001, wants_union) in [
        (
            "zero-length member stays invalid",
            "x <- if (runif(1) > 0.5) \"TRUE\" else character(0)\nif (x) print(1)\n",
            true,
            true,
        ),
        (
            "known-length-two member stays invalid",
            "x <- if (runif(1) > 0.5) \"TRUE\" else c(\"T\", \"F\")\nif (x) print(1)\n",
            true,
            true,
        ),
        (
            "list member stays invalid",
            "x <- if (runif(1) > 0.5) \"TRUE\" else list(TRUE)\nif (x) print(1)\n",
            true,
            true,
        ),
        (
            "scalar character members stay silent",
            "x <- if (runif(1) > 0.5) \"TRUE\" else Sys.getenv(\"F\")\nif (x) print(1)\n",
            false,
            false,
        ),
    ] {
        let (diags, scope) = check_with_scope(src);
        assert_eq!(
            scope.get("x").expect("condition binding").mode,
            if wants_union {
                Mode::Union
            } else {
                Mode::Character
            },
            "{note}: inferred condition shape"
        );
        assert_eq!(
            diags.iter().any(|d| d.code == "RY001"),
            wants_ry001,
            "{note}: got {diags:?}"
        );
    }
}

// Mixed unions of a numeric member and any coercible scalar member
// (character, complex, raw): the coercible member's runtime story may
// differ from numeric truthiness (character literal gate, complex
// component-OR, raw byte truthiness), so the RY003 "R coerces nonzero to
// TRUE" message would misdescribe it. The union stays silent unless a
// member is PROVEN invalid, which still dominates. Pure numeric unions
// keep RY003.
#[test]
fn mixed_numeric_coercible_union_condition_is_silent() {
    for (note, other_branch) in [
        ("character member", "\"a\""),
        ("raw member", "as.raw(1)"),
        ("complex member", "complex(real = 1)"),
    ] {
        let (diags, _) = check_with_scope(&format!(
            "x <- if (runif(1) > 0.5) 1L else {other_branch}\nif (x) print(1)\n"
        ));
        assert!(
            diags
                .iter()
                .all(|d| !matches!(d.code, "RY001" | "RY002" | "RY003")),
            "mixed numeric|{note} union must stay silent, got {diags:?}"
        );
    }
    let (diags, _) = check_with_scope("x <- if (runif(1) > 0.5) 1L else \"a\"\nif (x) print(1)\n");
    assert!(
        diags
            .iter()
            .all(|d| !matches!(d.code, "RY001" | "RY002" | "RY003")),
        "mixed numeric|character union must stay silent, got {diags:?}"
    );
    // An invalid member still dominates the mixed union.
    let (diags, _) = check_with_scope(
        "x <- if (runif(1) > 0.5) 1L else \"a\"\ny <- if (runif(1) > 0.5) x else list(TRUE)\nif (y) print(1)\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY001"),
        "a proven-invalid member dominates the mixed union, got {diags:?}"
    );
    // Pure integer|double unions keep the numeric info nudge.
    let (diags, _) = check_with_scope("x <- if (runif(1) > 0.5) 1L else 2.0\nif (x) print(1)\n");
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY003" && d.severity == Severity::Info),
        "pure numeric union keeps RY003 info, got {diags:?}"
    );
}

#[test]
fn detects_unbound_var() {
    let diags = check("y <- undefined_thing\n");
    assert!(diags.iter().any(|d| d.code == "RY010"));
}

// The numeric-truthiness idiom credits the stub's declared integer-1
// return; a local binding of the same name replaces the callee, so its
// — still integer-1 — return is no longer the counted idiom and the
// coercion nudge fires again.
#[test]
fn shadowed_length_keeps_the_coercion_nudge() {
    let shadowed = check("length <- function(x) 1L\nif (length(c(1, 2))) 1\n");
    assert!(
        shadowed.iter().any(|d| d.code == "RY003"),
        "a shadowed length() must not inherit the stub idiom: {shadowed:?}"
    );
    let unshadowed = check("if (length(c(1, 2))) 1\n");
    assert!(
        unshadowed.iter().all(|d| d.code != "RY003"),
        "the stub idiom suppression must stand: {unshadowed:?}"
    );
}

// The sum arm recognizes `is.*` predicates by name; a locally defined
// `is.*` callee is a different function, while an alias keeps pointing
// at the base predicate.
#[test]
fn shadowed_is_predicate_keeps_the_coercion_nudge() {
    let shadowed = check("is.value <- function(x) 1L\nif (sum(is.value(x))) 1\n");
    assert!(
        shadowed.iter().any(|d| d.code == "RY003"),
        "a shadowed is.* predicate must not feed the sum idiom: {shadowed:?}"
    );
    let aliased = check("predicate <- is.na\nx <- c(1, NA)\nif (sum(predicate(x))) 1\n");
    assert!(
        aliased.iter().all(|d| d.code != "RY003"),
        "an aliased base predicate keeps the sum idiom: {aliased:?}"
    );
    let plain = check("x <- c(1, NA)\nif (sum(is.na(x))) 1\n");
    assert!(
        plain.iter().all(|d| d.code != "RY003"),
        "the base predicate idiom must stand: {plain:?}"
    );
}

// The sum arm matches argument shape rather than a stub return, so it
// must pass the same shadow gate as the stub-backed arms: a locally
// defined `sum` is a different function even when its argument is the
// `is.*` count shape.
#[test]
fn shadowed_sum_keeps_the_coercion_nudge() {
    let shadowed = check("sum <- function(x) 1L\nx <- c(1, NA)\nif (sum(is.na(x))) 1\n");
    assert!(
        shadowed.iter().any(|d| d.code == "RY003"),
        "a shadowed sum() must not inherit the argument-shape idiom: {shadowed:?}"
    );
    let base = check("x <- c(1, NA)\nif (sum(is.na(x))) 1\n");
    assert!(
        base.iter().all(|d| d.code != "RY003"),
        "the base sum() idiom suppression must stand: {base:?}"
    );
}

// A missing match is NA_integer_, so its scalar integer must not receive the
// never-NA numeric-truthiness suppression. Position is no longer a suitable
// control because its arbitrary nomatch value requires an opaque result.
#[test]
fn na_capable_integer_count_keeps_the_coercion_nudge() {
    let missing_match = check("if (match(1L, 2L)) 1\n");
    assert!(
        missing_match.iter().any(|d| d.code == "RY003"),
        "A missing match must not feed the non-empty idiom: {missing_match:?}"
    );
    for suppressed in [
        "x <- c(1, 2, 3)\nif (length(x)) 1\n",
        "d <- data.frame(a = 1)\nif (nrow(d)) 1\n",
        "d <- data.frame(a = 1)\nif (ncol(d)) 1\n",
        "l <- list(1)\nif (NROW(l)) 1\n",
        "l <- list(1)\nif (NCOL(l)) 1\n",
        "fit <- NULL\nif (nobs(fit)) 1\n",
        "library(vctrs)\nx <- c(1, 2, 3)\nif (vec_size(x)) 1\n",
    ] {
        let diags = check(suppressed);
        assert!(
            diags.iter().all(|d| d.code != "RY003"),
            "a never-NA integer count keeps the idiom suppression: {suppressed} -> {diags:?}"
        );
    }
}

#[test]
fn loop_carried_bindings_are_available_at_the_start_of_each_iteration() {
    for src in [
        "n <- function() {\n  for (i in 1:3) {\n    if (i > 1) print(acc)\n    acc <- i\n  }\n}\n",
        "x <- 1:3\ntotal <- 0L\nfor (i in x) { total <- total + i }\n",
        "x <- 1:3\nfor (i in x) { total <- total + i }\n",
        "keep_going <- TRUE\nwhile (keep_going) {\n  print(acc)\n  acc <- 1L\n}\n",
        "repeat {\n  print(acc)\n  acc <- 1L\n  break\n}\n",
    ] {
        let diags = check(src);
        assert!(
            diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
            "loop-carried binding should not be unbound: {diags:?}"
        );
    }
}

// The T7b mutually-exclusive-branch refinement was reverted after repeated
// corpus regressions; opposite-arm reads inside loops are prebound like any
// other loop-carried name (accepted recall loss: FactoMineR MFA.R:310).
#[test]
fn loop_prebinding_suppresses_opposite_arm_reads() {
    for src in [
        "for (i in 1:3) {\n  if (is.null(tab.comp)) {\n    QuantiAct <- i\n  } else {\n    print(QuantiAct)\n  }\n}\n",
        "while (keep_going) {\n  if (flag == TRUE) {\n    value <- 1L\n  } else {\n    print(value)\n  }\n}\n",
    ] {
        let diags = check(src);
        assert!(
            diags.iter().all(|diagnostic| {
                diagnostic.code != "RY010"
                    || (!diagnostic.message.contains("QuantiAct")
                        && !diagnostic.message.contains("`value`"))
            }),
            "loop-assigned names are prebound in every arm: {diags:?}"
        );
    }
}

#[test]
fn loop_prebinding_remains_for_variant_branch_conditions() {
    for src in [
        "for (i in 1:3) {\n  if (i > 1) {\n    acc <- i\n  } else {\n    print(acc)\n  }\n}\n",
        "while (keep_going) {\n  if (flag) {\n    value <- 1L\n    flag <- FALSE\n  } else {\n    print(value)\n  }\n}\n",
    ] {
        let diags = check(src);
        assert!(
            diags.iter().all(|diagnostic| {
                diagnostic.code != "RY010"
                    || (!diagnostic.message.contains("acc")
                        && !diagnostic.message.contains("value"))
            }),
            "variant condition must retain loop prebinding: {diags:?}"
        );
    }
}

#[test]
fn loop_prebinding_clears_nested_branch_exclusions_after_assignment() {
    let diags = check(
        r"for (g in groups) {
  if (nlevels > 1L) {
    if (conditional.x) {
      COV <- matrix
      COV[is.na(COV)] <- 0
      diag(COV)
    } else {
      COV <- matrix
    }
  } else {
    COV <- matrix
  }
}
",
    );
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`COV`")
        }),
        "a real assignment in a nested branch must clear inherited loop exclusions: {diags:?}"
    );
}

#[test]
fn straight_line_function_use_before_assignment_still_emits_ry010() {
    let diags = check("f <- function() {\n  print(n)\n  n <- 1L\n}\n");
    assert!(
        diags
            .iter()
            .any(|diagnostic| { diagnostic.code == "RY010" && diagnostic.message.contains("n") }),
        "non-loop use-before-assignment must remain diagnosed: {diags:?}"
    );
}

#[test]
fn scalar_logical_warns_on_vector_operand() {
    let diags = check("x <- c(TRUE, FALSE)\nbad <- x && TRUE\n");
    assert!(
        diags.iter().any(|d| d.code == "RY032"),
        "expected RY032 for && with vector, got {:?}",
        diags
    );
}

#[test]
fn vectorized_logical_no_warning() {
    let diags = check("x <- c(TRUE, FALSE)\nok <- x & TRUE\n");
    assert!(
        diags.iter().all(|d| d.code != "RY032"),
        "vectorized & should not warn, got {:?}",
        diags
    );
}

#[test]
fn scalar_logical_with_scalars_no_warning() {
    let diags = check("a <- TRUE\nb <- FALSE\nx <- a && b\n");
    assert!(
        diags.iter().all(|d| d.code != "RY032"),
        "&& with scalars should not warn, got {:?}",
        diags
    );
}

#[test]
fn compare_char_numeric_warns() {
    let diags = check(r#"bad <- "hello" < 42"#);
    assert!(
        diags.iter().any(|d| d.code == "RY033"),
        "expected RY033 for character vs numeric, got {:?}",
        diags
    );
}

#[test]
fn compare_same_mode_no_warning() {
    let diags = check("bad <- 1 < 2\n");
    assert!(
        diags.iter().all(|d| d.code != "RY033"),
        "numeric vs numeric should not warn, got {:?}",
        diags
    );
}

#[test]
fn compare_char_char_no_warning() {
    let diags = check(r#"x <- "abc" < "xyz""#);
    assert!(
        diags.iter().all(|d| d.code != "RY033"),
        "character vs character should not warn, got {:?}",
        diags
    );
}

#[test]
fn compare_eq_char_numeric_warns() {
    let diags = check(r#"bad <- "hello" == 1"#);
    assert!(
        diags.iter().any(|d| d.code == "RY033"),
        "expected RY033 for character == numeric, got {:?}",
        diags
    );
}

#[test]
fn in_operator_uses_lhs_length() {
    // `x %in% table` returns a logical vector of length(x); the RHS
    // length is irrelevant. A length-1 `x` matched against a length-2
    // literal must stay length-1 logical -- not length-2 (which would
    // drive RY002/RY032 false positives downstream).
    let (_diags, scope) = check_with_scope("x <- \"a\"\nr <- x %in% c(\"a\", \"b\")\n");
    let r = scope.get("r").expect("binding r");
    assert_eq!(r.mode, Mode::Logical, "got {:?}", r);
    assert_eq!(r.length, Length::One, "got {:?}", r);
}

#[test]
fn in_operator_condition_no_ry002_ry032() {
    // The end-to-end shape from the purrr net: a length-1 `%in%` result
    // used as an `if` condition and inside `&&` must not fire RY002 or
    // RY032.
    let diags = check(
        "x <- \"a\"\nif (x %in% c(\"a\", \"b\")) print(1)\nif (is.character(x) && x %in% c(\"a\", \"b\")) print(2)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY002" && d.code != "RY032"),
        "expected no RY002/RY032 for length-1 %in%, got {:?}",
        diags
    );
}

#[test]
fn function_param_inference_no_diag() {
    // `f` has a default-typed param `x = 1L` (integer), so `x + 1`
    // is integer + double = double. Well-typed; no diagnostics.
    let diags = check("f <- function(x = 1L) { x + 1 }\ng <- f(2L)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "got false positive: {:?}",
        diags
    );
}

#[test]
fn user_fn_return_type_inferred() {
    // `text` returns a string literal, so `text()` is character and
    // the arithmetic use must error.
    let diags = check("text <- function() { \"hello\" }\ny <- text() + 1L\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from character-returning fn used arithmetically, got {:?}",
        diags
    );
}

#[test]
fn user_fn_return_explicit_return() {
    let diags = check("f <- function(x = 1L) { return(x * 2) }\ny <- f() + \"bad\"\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from integer-returning fn + character, got {:?}",
        diags
    );
}

#[test]
fn recursive_fn_terminates() {
    // The fixpoint must converge on fact()'s return type (integer)
    // without infinite descent. We don't assert any specific diag,
    // just that the checker terminates and doesn't crash.
    let diags = check(
        "fact <- function(n = 1L) { if (n <= 1L) return(1L); n * fact(n - 1L) }\ny <- fact(5)\n",
    );
    // The result is integer; arithmetic with another integer is fine.
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "false positive on recursive fn: {:?}",
        diags
    );
}

#[test]
fn seq_operator_produces_integer() {
    // `1:10` is integer, so `i` in the loop is integer, so `i + 1L`
    // is well-typed.
    let diags = check("total <- 0L\nfor (i in 1:10) { total <- total + i }\n");
    assert!(diags.is_empty(), "got {:?}", diags);
}

#[test]
fn for_loop_var_is_element_type() {
    // Iterating over a character vector makes the loop variable
    // character; using it arithmetically should error.
    let diags = check("for (s in c(\"a\", \"b\")) { total <- s + 1 }\n");
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from character loop var + int, got {:?}",
        diags
    );
}

#[test]
fn pipe_forms_desugar_to_well_typed_calls() {
    // Every supported pipe form desugars to an ordinary call that
    // type-checks cleanly: magrittr `%>%` (call rhs, bare function
    // name, `.` placeholder argument, `%T>%` tee) and the native
    // base-R `|>`.
    for (src, note) in [
        ("result <- c(1, 2, 3) %>% mean()\n", "call rhs"),
        (
            "a <- c(1, 2, 3) %>% mean() %>% round(2)\n",
            "two-step chain",
        ),
        ("a <- c(1, 2, 3) |> mean()\n", "native |>"),
        ("x <- 1L\ny <- x %>% abs\n", "bare function name rhs"),
        (
            "result <- c(1, 2, 3) %>% round(., digits = 2)\n",
            "placeholder argument",
        ),
        (
            "result <- c(1, 2, 3) %T>% print()\n",
            "tee returns the lhs type",
        ),
    ] {
        let diags = check(src);
        assert!(
            diags.is_empty(),
            "{note} (`{}`): got {:?}",
            src.trim(),
            diags
        );
    }
}

#[test]
fn long_pipe_chain_infers_expected_type() {
    let mut src = String::from("piped <- data.frame(a = 1:3)");
    for i in 0..30 {
        src.push_str(&format!(" |> transform(b{i} = a + {i})"));
    }
    src.push_str("\nresult <- piped$a + 1L\n");

    let (diagnostics, scope) = check_with_scope(&src);
    assert!(diagnostics.is_empty(), "got {diagnostics:?}");
    assert_eq!(
        scope.get("result").map(|ty| (&ty.mode, ty.length)),
        Some((&Mode::Integer, Length::Known(3)))
    );
}

#[test]
fn long_else_if_force_flow_completes() {
    let mut src = String::from(r#"f <- function(what) { if (what == "a0") { 0 }"#);
    for i in 1..60 {
        src.push_str(&format!(r#" else if (what == "a{i}") {{ {i} }}"#));
    }
    src.push_str(r#" else { stop("nope") } }"#);
    src.push('\n');

    let diags = check(&src);
    assert!(diags.is_empty(), "got {diags:?}");
}

#[test]
fn pipe_dot_pronoun_extracts_typed_column() {
    // `df %>% .$mpg` and `df %>% .[["mpg"]]` resolve `.` to the
    // piped LHS (`mtcars`) and index by column name -- `[[` with a
    // string literal mirrors `$` semantics -- so `col` should be
    // `double<32>` (the type of `mtcars$mpg`). We assert the
    // inferred type directly via the test scope and also check that
    // no RY010 (unbound `.`) leaks out.
    for (label, access) in [("dollar", ".$mpg"), ("double-bracket", ".[[\"mpg\"]]")] {
        let src = format!("df <- mtcars\ncol <- df %>% {access}\n");
        let (diags, scope) = check_with_scope(&src);
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "{label}: dot pronoun should not emit RY010 (unbound `.`), got {:?}",
            diags
        );
        let col = scope
            .get("col")
            .unwrap_or_else(|| panic!("{label}: col should be bound"));
        assert_eq!(
            col.mode,
            Mode::Double,
            "{label}: must infer double, got {:?}",
            col
        );
        assert_eq!(col.length, Length::Known(32), "{label}: mpg has 32 rows");
    }
}

#[test]
fn pipe_underscore_placeholder_extraction() {
    // R >= 4.3 allows the native-pipe placeholder as the base of an
    // extraction: `mtcars |> _$mpg`. The `_` is the piped LHS, so it
    // must not be reported as an unbound variable (issue #27).
    let (diags, scope) = check_with_scope(
        "col <- mtcars |> _$mpg\nm <- mtcars |> _$mpg |> mean()\nalso <- mtcars |> _[[\"mpg\"]]\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "`_` extraction placeholder should not emit RY010, got {:?}",
        diags
    );
    let col = scope.get("col").expect("col should be bound");
    assert_eq!(col.mode, Mode::Double, "mtcars |> _$mpg must infer double");
    assert_eq!(col.length, Length::Known(32), "mpg has 32 rows");
    let m = scope.get("m").expect("m should be bound");
    assert_eq!(m.mode, Mode::Double, "mean() of a double column is double");
    assert_eq!(m.length, Length::One, "mean() returns a scalar");
    let also = scope.get("also").expect("also should be bound");
    assert_eq!(also.mode, Mode::Double, "mtcars |> _[[\"mpg\"]] is double");
    assert_eq!(
        also.length,
        Length::Known(32),
        "mtcars |> _[[\"mpg\"]] has 32 rows"
    );
}

#[test]
fn pipe_placeholder_extraction_chain() {
    // The placeholder may sit at the root of a longer extraction chain
    // (`mtcars |> _$mpg[1]` evaluates to 21 in R 4.6). Every link is
    // applied to the piped LHS, so no link may report an unbound `_`/`.`.
    let (diags, scope) = check_with_scope(
        "a <- mtcars |> _$mpg[1]\nb <- mtcars |> _[[\"mpg\"]][2]\nd <- mtcars %>% .$mpg[1]\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "placeholder-rooted extraction chains should not emit RY010, got {:?}",
        diags
    );
    for name in ["a", "b", "d"] {
        let t = scope.get(name).expect("binding should exist");
        assert_eq!(
            t.mode,
            Mode::Double,
            "{} indexes the double column mpg",
            name
        );
        assert_eq!(t.length, Length::One, "{} extracts a single element", name);
    }
}

#[test]
fn pipe_dot_substituted_at_every_placeholder_argument() {
    // magrittr replaces every `.` argument with the LHS, so the second
    // `.` in `paste(., ., sep = "-")` must not read as an unbound name.
    let (diags, scope) =
        check_with_scope("x <- c(\"a\", \"b\")\ny <- x %>% paste(., ., sep = \"-\")\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "repeated `.` arguments should not emit RY010, got {:?}",
        diags
    );
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.mode, Mode::Character, "paste() returns character");
    assert_eq!(
        y.length,
        Length::Known(2),
        "both `.` arguments have length 2"
    );
}

#[test]
fn pipe_dot_substituted_inside_nested_calls() {
    // magrittr binds `.` throughout the RHS, so a pronoun nested in an
    // inner call resolves too: `c(1, 2) %>% sum(rev(.))` is 6 in R.
    let (diags, scope) = check_with_scope("x <- c(1, 2)\ny <- x %>% sum(rev(.))\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "a nested `.` should not emit RY010, got {:?}",
        diags
    );
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.mode, Mode::Double, "sum() of doubles is double");
}

#[test]
fn pipe_dot_resolves_inside_subscripts() {
    // The magrittr filtering idiom subscripts the pronoun with a
    // predicate over the pronoun itself: `mtcars %>% .[.$mpg > 20, ]`
    // selects 14 rows in R. The inner `.` must resolve to the LHS.
    let diags = check("a <- mtcars %>% .[.$mpg > 20, ]\nb <- mtcars %>% .$mpg[.$cyl > 4]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "a `.` inside a subscript should not emit RY010, got {:?}",
        diags
    );
}

#[test]
fn pipe_placeholders_are_specific_to_their_pipe_form() {
    // Each pipe binds only its own placeholder: `.` is magrittr's, `_`
    // is the native pipe's. Used in the other form they are ordinary
    // identifier references, so they must still report RY010.
    for (src, placeholder) in [
        ("a <- mtcars |> .$mpg\n", "."),
        ("b <- mtcars %>% _$mpg\n", "_"),
    ] {
        let diags = check(src);
        assert!(
            diags
                .iter()
                .any(|d| d.code == "RY010" && d.message.contains(placeholder)),
            "`{}` is unbound in `{}`, got {:?}",
            placeholder,
            src.trim(),
            diags
        );
    }
}

#[test]
fn magrittr_leading_dot_pipe_builds_a_function() {
    // `. %>% f %>% g` is magrittr's functional sequence: the dot is the
    // chain's parameter (?magrittr::`%>%`, "Using the dot-place-holder as
    // lhs"), not an unbound name, and the chain's value is a function
    // (torch's `map(.x, . %>% get_args %>% parse_args)` corpus shape).
    let (diags, scope) = check_with_scope("g <- . %>% identity %>% class\nh <- . %T>% print\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "the leading dot is a chain parameter, got {diags:?}"
    );
    for name in ["g", "h"] {
        let t = scope.get(name).unwrap_or_else(|| panic!("{name} bound"));
        assert_eq!(t.mode, Mode::Function, "{name}: {t:?}");
    }
    // The native pipe has no functional-sequence form: `_` on the left of
    // `|>` stays an ordinary (unbound) name.
    let native = check("k <- _ |> identity()\n");
    assert!(
        native.iter().any(|d| d.code == "RY010"),
        "native `_` has no lambda form: {native:?}"
    );
}

#[test]
fn pipe_dot_pronoun_single_bracket() {
    // `df %>% .[1]` preserves the base type (single-bracket
    // subsetting keeps the existing opaque behavior at v1), so the
    // result is the same data.frame-typed value as the LHS. The
    // important behavioral check is that no RY010 leaks on `.`.
    let (diags, scope) = check_with_scope("df <- mtcars\nsub <- df %>% .[1]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dot pronoun should not emit RY010, got {:?}",
        diags
    );
    let sub = scope.get("sub").expect("sub should be bound");
    assert_eq!(sub.mode, Mode::List, "df[1] preserves base mode");
    assert!(
        sub.class.contains("data.frame"),
        ".[1] preserves the data.frame class"
    );
}

#[test]
fn pipe_dot_pronoun_bare_returns_lhs() {
    // `x %>% .` returns the LHS value itself (the `.` refers to the
    // LHS). For a length-3 double vector, the result type matches.
    let (diags, scope) = check_with_scope("x <- c(1, 2, 3)\ny <- x %>% .\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.mode, Mode::Double, "x %>% . must infer double");
    assert_eq!(y.length, Length::Known(3), "length is preserved");
}

#[test]
fn pipe_dot_pronoun_undefined_column_emits_ry060() {
    // `df %>% .$nonexistent` resolves `.` to the LHS, then the
    // column lookup fails against `mtcars`'s schema, so RY060
    // (undefined-column) must fire - the pronoun path reuses the
    // same diagnostics as a direct `df$nonexistent`.
    let diags = check("df <- mtcars\nbad <- df %>% .$nonexistent\n");
    assert!(
        diags.iter().any(|d| d.code == "RY060"),
        "expected RY060 for undefined column via dot pronoun, got {:?}",
        diags
    );
}

#[test]
fn pipe_dot_pronoun_chains_into_arithmetic() {
    // End-to-end behavioral check: `df %>% .$mpg` produces a real
    // double type (not opaque), so subsequent arithmetic that would
    // fail on an opaque value type-checks cleanly. This is the
    // motivating use case from the task description.
    let diags = check("df <- mtcars\ncol <- df %>% .$mpg\nok <- col + 1L\n");
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "col + 1L should be valid (double + int), got {:?}",
        diags
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "no RY010 should leak from the dot pronoun, got {:?}",
        diags
    );
}

#[test]
fn if_expr_integer_branches_join_to_integer() {
    // `if (TRUE) 1L else 2L` joins to integer. Using the result
    // with a character must fire RY040, proving the type was
    // inferred (not opaque, which would be permissive).
    let diags = check(
        "x <- if (TRUE) 1L else 2L\n\
             bad <- x + \"hello\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from if-expr result + character, got {:?}",
        diags
    );
}

#[test]
fn if_expr_mismatched_branches_join() {
    // `if (TRUE) list(1) else function(){1}` joins to
    // union[list, function]. Using the result arithmetically fires
    // RY040 because EVERY member of the union errors against `+ 1`
    // (an op on a union errors only when ALL members error).
    let diags = check(
        "x <- if (TRUE) list(1) else function() { 1 }\n\
             bad <- x + 1\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from joined if-expr (all-invalid union) + int, got {:?}",
        diags
    );
}

#[test]
fn if_expr_no_else_joins_with_null() {
    // `if (TRUE) 1L` (no else) joins integer + NULL = integer.
    // Using the result arithmetically is well-typed.
    let diags = check(
        "x <- if (TRUE) 1L\n\
             y <- x + 1\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "if-expr without else should join int+NULL=int, got {:?}",
        diags
    );
}

#[test]
fn if_expr_nested() {
    // Nested if-expressions: all branches integer, result integer.
    let diags = check(
        "x <- if (TRUE) { if (FALSE) 1L else 2L } else 3L\n\
             bad <- x + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from nested if-expr result + character, got {:?}",
        diags
    );
}

#[test]
fn negative_literals_infer_operand_mode() {
    // Unary minus on a numeric literal preserves the operand's mode:
    // `-1L` stays integer, `-3.14` stays double; length is one.
    for (src, mode) in [
        ("x <- -1L\n", Mode::Integer),
        ("x <- -3.14\n", Mode::Double),
    ] {
        let (diags, scope) = check_with_scope(src);
        assert!(diags.is_empty(), "`{}`: got {:?}", src.trim(), diags);
        let x = scope
            .get("x")
            .unwrap_or_else(|| panic!("`{}`: x should be bound", src.trim()));
        assert_eq!(x.mode, mode, "`{}`: got {:?}", src.trim(), x);
        assert_eq!(x.length, Length::One, "`{}`: got {:?}", src.trim(), x);
    }
}

#[test]
fn neg_colon_infers_integer_and_groups_correctly() {
    // `-1:3` parses as `(-1):3`, which R evaluates as seq(-1, 3) =
    // c(-1, 0, 1, 2, 3), an integer vector. The type must be integer
    // (not double, not error), and using it arithmetically must be
    // well-typed. This is the key correctness case for unary-minus
    // vs colon precedence.
    let (diags, scope) = check_with_scope("z <- -1:3\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let z = scope.get("z").expect("z should be bound");
    assert_eq!(z.mode, Mode::Integer, "got {:?}", z);
    // Behavioral check: `-1:3`'s LHS is a UnaryOp (not a literal),
    // so the literal-based length inference doesn't fire and the
    // length stays Unknown. The value must still be usable as an
    // integer in arithmetic.
    let diags = check("z <- -1:3\nbad <- z + 1L\n");
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "z + 1L must be valid int+int, got {:?}",
        diags
    );
}

#[test]
fn negated_paren_colon_infers_integer() {
    // `-(1:3)` negates the whole sequence; still an integer vector.
    let (diags, scope) = check_with_scope("w <- -(1:3)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let w = scope.get("w").expect("w should be bound");
    assert_eq!(w.mode, Mode::Integer, "got {:?}", w);
}

#[test]
fn neg_times_int_infers_integer_length_one() {
    // `-2L * 3L` = `(-2L) * 3L` = -6L, a length-1 integer.
    let (diags, scope) = check_with_scope("v <- -2L * 3L\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let v = scope.get("v").expect("v should be bound");
    assert_eq!(v.mode, Mode::Integer, "got {:?}", v);
    assert_eq!(v.length, Length::One, "got {:?}", v);
}

#[test]
fn neg_on_character_emits_ry020() {
    // Unary `-` applied to a character is a type error in R.
    let diags = check("x <- -\"hi\"\n");
    assert!(
        diags.iter().any(|d| d.code == "RY020"),
        "expected RY020 for negation of character, got {:?}",
        diags
    );
}

// ---- data.table select subscripts (issue #367) ----
//
// `[.data.table` reads `-<character>` / `!<character>` in the `j` slot
// as column drops, and `!<character>` / `!<list>` in the `i` slot as key
// exclusion / not-join. data.table ships no stubs, so its receivers are
// opaque to inference (parameters, `data.table::` calls). Base R has no
// negative or negated character subscript, so provably base receivers
// and non-subscript positions keep the diagnostics. R semantics verified
// by the `testdata/oracle/` fixtures for this issue.

#[test]
fn negative_character_j_subscript_on_opaque_receiver_is_a_datatable_drop() {
    // The corpus shape from issue #367: the receiver is a parameter that
    // may be a data.table, so `-c(...)` in `j` is a documented drop.
    let diags = check("f <- function(design) design[, -c(\"dob\", \"eol\")]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY020"),
        "data.table column drop flagged, got {:?}",
        diags
    );
}

#[test]
fn negative_character_j_subscript_on_datatable_classed_receiver_is_quiet() {
    let diags = check(
        "dt <- structure(list(a = 1), class = c(\"data.table\", \"data.frame\"))\nu <- dt[, -c(\"a\")]\n",
    );
    assert!(diags.iter().all(|d| d.code != "RY020"), "got {:?}", diags);
}

#[test]
fn datatable_not_forms_on_opaque_receivers_stay_quiet() {
    for src in [
        "f <- function(dt) dt[!\"key\"]\n",
        "f <- function(dt) dt[!list(k = \"a\")]\n",
        "f <- function(dt) dt[, !c(\"a\")]\n",
    ] {
        let diags = check(src);
        assert!(
            diags.iter().all(|d| d.code != "RY021"),
            "data.table not-form flagged: {src}got {:?}",
            diags
        );
    }
}

#[test]
fn negative_character_subscript_in_package_mode_stays_quiet() {
    // The corpus sites live in packages importing data.table; the same
    // subscript shape must stay quiet there (importing data.table alone
    // is not what licenses the form -- the opaque receiver is).
    let diags = check_with(
        "drop_cols <- function(design) {\n  design[, -c(\"dob\", \"eol\")]\n}\n",
        |checker| checker.set_loaded(HashSet::from(["data.table".to_string()])),
    );
    assert!(diags.iter().all(|d| d.code != "RY020"), "got {:?}", diags);
}

#[test]
fn negative_character_subscript_on_base_vector_still_flags() {
    let diags = check("v <- c(\"x\", \"y\")\nu <- v[-c(\"x\")]\n");
    assert!(
        diags.iter().any(|d| d.code == "RY020"),
        "base vector has no negative character subscript, got {:?}",
        diags
    );
}

#[test]
fn negated_character_subscript_on_base_vector_still_flags() {
    let diags = check("v <- c(\"x\", \"y\")\nu <- v[!\"x\"]\n");
    assert!(
        diags.iter().any(|d| d.code == "RY021"),
        "base vector has no negated character subscript, got {:?}",
        diags
    );
}

#[test]
fn negative_character_subscript_on_base_data_frame_still_flags() {
    let diags = check("df <- data.frame(a = 1, b = 2)\nu <- df[, -c(\"a\")]\n");
    assert!(
        diags.iter().any(|d| d.code == "RY020"),
        "base data.frame requires integer/logical negatives, got {:?}",
        diags
    );
}

#[test]
fn negated_list_subscript_on_base_list_still_flags() {
    let diags = check("l <- list(a = 1, b = 2)\nu <- l[!list(a = 1)]\n");
    assert!(
        diags.iter().any(|d| d.code == "RY021"),
        "base list has no negated list subscript, got {:?}",
        diags
    );
}

#[test]
fn negative_character_subscript_in_i_slot_still_flags() {
    // `-<character>` is only a documented drop in the `j` selector slots;
    // data.table does not interpret it in the row-filter `i` slot.
    let diags = check("f <- function(dt) dt[-c(\"a\")]\n");
    assert!(diags.iter().any(|d| d.code == "RY020"), "got {:?}", diags);
}

#[test]
fn negative_character_double_subscript_still_flags() {
    // `[[` has no select semantics; the operand error stays.
    let diags = check("f <- function(dt) dt[[-c(\"a\")]]\n");
    assert!(diags.iter().any(|d| d.code == "RY020"), "got {:?}", diags);
}

#[test]
fn named_j_argument_selects_the_j_role() {
    // `j = -c("a")` at the first positional slot is still the column
    // selector: the tag, not the position, carries the role (R-verified:
    // `dt[j = -c("a")]` drops the column).
    let diags = check("f <- function(dt) dt[j = -c(\"a\")]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY020"),
        "named j drop flagged, got {:?}",
        diags
    );
}

#[test]
fn named_non_selector_argument_still_flags() {
    // `drop = -c("a")` passes data.table's unused `drop` formal, not the
    // column selector, so the operand keeps its ordinary eager-argument
    // diagnostic — the same stance as any unused lazy formal (R-verified:
    // data.table warns the ignored argument will become an error).
    let diags = check("f <- function(dt) dt[, drop = -c(\"a\")]\n");
    assert!(
        diags.iter().any(|d| d.code == "RY020"),
        "named drop argument treated as a selector, got {:?}",
        diags
    );
}

#[test]
fn sdcols_inversion_forms_stay_quiet() {
    // data.table documents `.SDcols = !cols` and `.SDcols = -cols` as
    // equivalent selection inversions, at any positional slot (R-verified).
    for src in [
        "f <- function(dt) dt[, .SD, .SDcols = !c(\"a\")]\n",
        "f <- function(dt) dt[, .SD, .SDcols = -c(\"a\")]\n",
    ] {
        let diags = check(src);
        assert!(
            diags.iter().all(|d| d.code != "RY020" && d.code != "RY021"),
            "`.SDcols` inversion flagged: {src}got {:?}",
            diags
        );
    }
}

#[test]
fn positional_third_subscript_argument_still_flags() {
    // Positional slot 2 binds `by`, where data.table gives `-`/`!` no
    // select meaning and both operators error (R-verified).
    for src in [
        "f <- function(dt) dt[, .N, -c(\"a\")]\n",
        "f <- function(dt) dt[, .N, !c(\"a\")]\n",
    ] {
        let diags = check(src);
        assert!(
            diags.iter().any(|d| d.code == "RY020" || d.code == "RY021"),
            "positional `by` negation not flagged: {src}got {:?}",
            diags
        );
    }
}

#[test]
fn subscript_context_does_not_leak_into_callback_bodies() {
    // A function literal executed inside a subscript argument runs in
    // its own frame; `-` on its parameter is an ordinary operand error
    // even when the literal appears in `j` (R-verified: errors at
    // runtime, unlike the syntactic `-c(...)` select form).
    let diags = check("f <- function(dt) dt[, sapply(c(\"a\"), function(s) -s)]\n");
    assert!(
        diags.iter().any(|d| d.code == "RY020"),
        "callback body inherited the subscript context, got {:?}",
        diags
    );
}

#[test]
fn subscript_context_does_not_leak_into_deferred_bodies() {
    // The same holds for deferred bodies that bypass function frames:
    // a foreach `%do%` body evaluates its expression eagerly in its own
    // iteration environment, so `-c("a")` there errors in R rather than
    // reading as a data.table select form.
    let diags = check("f <- function(dt) dt[, foreach(i = 1:2) %do% -c(\"a\")]\n");
    assert!(
        diags.iter().any(|d| d.code == "RY020"),
        "foreach body inherited the subscript context, got {:?}",
        diags
    );
}

#[test]
fn neg_preserves_na_flag_and_mode() {
    // `-NA_integer_` must remain an NA integer (negation does not
    // change mode or clear the NA flag). This guards that the
    // checker's `UnaryOp::Neg` returns the operand type verbatim.
    let (diags, scope) = check_with_scope("a <- -NA_integer_\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let a = scope.get("a").expect("a should be bound");
    assert_eq!(a.mode, Mode::Integer, "got {:?}", a);
    assert_eq!(a.length, Length::One, "got {:?}", a);
}

// ---- Literal-based length inference: `:`, `rep`, `seq` ----
//
// These exercise the literal-arg fast paths that pin the result
// length exactly instead of returning `Length::Unknown`, and their
// behavioral payoff: a precisely typed vector mixed with a character
// operand is a diagnosed type error (RY040), where an opaque result
// would stay silent. Non-literal operands must stay `Unknown` (no
// false precision). `mode` pins only what each case asserted.
#[test]
fn literal_constructors_pin_exact_lengths() {
    for (src, mode, length) in [
        // `:` with integer-valued literal endpoints (whole-number
        // doubles included) yields an integer vector; `5:5` is the
        // single-element case; a non-literal LHS stays Unknown.
        ("x <- 1:10\n", Some(Mode::Integer), Length::Known(10)),
        ("x <- 10:1\n", Some(Mode::Integer), Length::Known(10)),
        ("x <- 1.0:5.0\n", Some(Mode::Integer), Length::Known(5)),
        ("x <- 5:5\n", None, Length::Known(1)),
        ("n <- 1L\nx <- n:10\n", Some(Mode::Integer), Length::Unknown),
        // `rep`: positional, named (`times =`), and `each =`
        // multipliers; `rep(0, 5)` keeps double (`0` has no `L`);
        // a non-literal `times` stays Unknown.
        ("x <- rep(1:3, 2)\n", Some(Mode::Integer), Length::Known(6)),
        ("x <- rep(0, 5)\n", Some(Mode::Double), Length::Known(5)),
        ("x <- rep(c(1, 2), times = 3)\n", None, Length::Known(6)),
        ("x <- rep(c(1, 2, 3), each = 2)\n", None, Length::Known(6)),
        ("x <- rep(c(1, 2), 3, each = 2)\n", None, Length::Known(12)),
        ("n <- 2\nx <- rep(1:3, n)\n", None, Length::Unknown),
        // `seq`/`seq.int`: `by`, `length.out`, and the by-one
        // default; whole-number double `by` still pins; a
        // non-literal endpoint stays Unknown.
        ("x <- seq(1, 10, 2)\n", None, Length::Known(5)),
        ("x <- seq(1, 5, length.out = 3)\n", None, Length::Known(3)),
        ("x <- seq(1, 5)\n", None, Length::Known(5)),
        (
            "x <- seq.int(1L, 10L, 2L)\n",
            Some(Mode::Integer),
            Length::Known(5),
        ),
        ("x <- seq.int(2, 10, 2.0)\n", None, Length::Known(5)),
        ("n <- 10\nx <- seq(1, n, 1)\n", None, Length::Unknown),
    ] {
        let (diags, scope) = check_with_scope(src);
        assert!(diags.is_empty(), "`{src}`: got {diags:?}");
        let x = scope
            .get("x")
            .unwrap_or_else(|| panic!("`{src}`: x should be bound"));
        if let Some(mode) = mode {
            assert_eq!(x.mode, mode, "`{src}`: got {x:?}");
        }
        assert_eq!(x.length, length, "`{src}`: got {x:?}");
    }
}

#[test]
fn literal_constructors_fire_ry040_on_char_mix() {
    // The precise types are visible to downstream arithmetic:
    // `1:10` is integer<10>, `rep(c(1, 2), 3)` is double<6>, and
    // `seq(1, 10, 2)` is double<5>, so each mixed character
    // addition must fire RY040.
    for (src, vector) in [
        ("x <- 1:10\nbad <- x + \"hello\"\n", "integer<10>"),
        ("x <- rep(c(1, 2), 3)\nbad <- x + \"hello\"\n", "double<6>"),
        ("x <- seq(1, 10, 2)\nbad <- x + \"hello\"\n", "double<5>"),
    ] {
        let diags = check(src);
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 for {vector} + character in `{}`, got {:?}",
            src.trim(),
            diags
        );
    }
}

// ---- Pass-2 propagation + rep/seq edge cases ----
//
// These cover the three code-review fixes: (1) literal lengths
// now propagate through function return types because the literal
// fast paths live in pass 2 (`infer_discarding`) as well as
// pass 3; (2) `infer_rep` counts only unnamed args when binding
// positional `times`/`each`; (3) `infer_rep` never emits
// `Length::Known(0)` or treats negative multipliers as known.
#[test]
fn pass2_colon_literal_propagates_through_fn_return() {
    // `f <- function() 1:10` should give f a return type of
    // integer<10>, and `g <- f()` should propagate that precise
    // length to g. Previously the `:` literal fast path only
    // existed in pass 3, so f's return type (computed in pass 2)
    // was Length::Unknown and g inherited the unknown length.
    let (diags, scope) = check_with_scope("f <- function() 1:10\ng <- f()\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let g = scope.get("g").expect("g should be bound");
    assert_eq!(g.mode, Mode::Integer, "got {:?}", g);
    assert_eq!(g.length, Length::Known(10), "got {:?}", g);
}

#[test]
fn pass2_colon_literal_propagates_through_fn_return_fire_ry040() {
    // Behavioral check: f returns integer<10>, so mixing g with a
    // character fires RY040. This is the headline benefit - the
    // checker sees a real vector through the function boundary.
    let diags = check(
        "f <- function() 1:10\n\
             g <- f()\n\
             bad <- g + \"hello\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 for integer<10> + character (via fn return), got {:?}",
        diags
    );
}

#[test]
fn rep_named_each_before_positional_binds_times() {
    // `rep(each = 2, c(1, 2, 3), 1)`: the named `each = 2` appears
    // before the positional args. The trailing positional `1`
    // binds to `times` (positional index 1, counting only unnamed
    // args). Result: 3 (x) * 1 (times) * 2 (each) = 6. Previously
    // the raw-list index bug made `times` bind to the non-literal
    // `c(1,2,3)` at raw index 1, yielding Some(None) -> Unknown.
    let (diags, scope) = check_with_scope("x <- rep(each = 2, c(1, 2, 3), 1)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Double, "got {:?}", x);
    assert_eq!(x.length, Length::Known(6), "got {:?}", x);
}

#[test]
fn rep_negative_times_does_not_crash() {
    // `rep(x, times = -1)`: a negative `times` is modeled as
    // Length::Unknown. The `-1` parses as UnaryOp::Neg, which
    // extract_literal_int treats as a non-literal, so we can't pin
    // the length. The check must not panic and must stay Unknown.
    let (diags, scope) = check_with_scope("x <- 1:3\ny <- rep(x, times = -1)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let y = scope.get("y").expect("y should be bound");
    assert_eq!(y.length, Length::Unknown, "got {:?}", y);
}

#[test]
fn rep_zero_times_yields_length_zero() {
    // `rep(1:3, times = 0)` returns a length-0 vector. The result
    // must be Length::Zero, not the invariant-violating Known(0).
    let (diags, scope) = check_with_scope("x <- rep(1:3, times = 0)\n");
    assert!(diags.is_empty(), "got {:?}", diags);
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
    assert_eq!(x.length, Length::Zero, "got {:?}", x);
}

#[test]
fn calling_non_function_values_emits_ry070() {
    for (src, kind) in [
        ("x <- 42\ny <- x(10)\n", "integer"),
        ("x <- \"paste\"\ny <- x(1)\n", "character"),
    ] {
        let diags = check(src);
        assert!(
            diags.iter().any(|d| d.code == "RY070"),
            "expected RY070 for calling {kind}, got {:?}",
            diags
        );
    }
}

#[test]
fn calling_actual_function_no_ry070() {
    let diags = check("f <- function() 1L\ny <- f()\n");
    assert!(
        diags.iter().all(|d| d.code != "RY070"),
        "calling a real function should not emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_opaque_no_ry070() {
    // Opaque (unknown) values should not trigger RY070 - we don't know
    // if they're functions or not.
    let diags = check("y <- some_unknown_thing(10)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY070"),
        "opaque value should not emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_integer_literal_emits_ry070() {
    // Calling a literal (`42()`) errors in R.
    let diags = check("y <- 42()\n");
    assert!(
        diags.iter().any(|d| d.code == "RY070"),
        "calling integer literal `42()` should emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_string_literal_uses_function_lookup() {
    let (diags, scope) = check_with_scope("y <- \"paste\"(1, 2)\n");
    assert!(
        diags.is_empty(),
        "string-literal function lookup should be callable, got {:?}",
        diags
    );
    assert_eq!(scope.get("y").map(|ty| ty.mode), Some(Mode::Character));
}

#[test]
fn calling_null_literal_emits_ry070() {
    let diags = check("y <- NULL()\n");
    assert!(
        diags.iter().any(|d| d.code == "RY070"),
        "calling NULL literal should emit RY070, got {:?}",
        diags
    );
}

#[test]
fn calling_index_expression_stays_silent() {
    // Non-literal non-Ident callees (index expressions, calls
    // returning functions) must stay silent as before.
    let diags = check("lst <- list(function() 1)\ny <- lst[[1]]()\n");
    assert!(
        diags.iter().all(|d| d.code != "RY070"),
        "calling an index expression should not emit RY070, got {:?}",
        diags
    );
}

#[test]
fn dollar_on_atomic_vectors_emits_ry061() {
    // `$` subset assignment only exists for recursive (list-like)
    // objects; R raises "$ operator is invalid for atomic vectors".
    for src in [
        "x <- 1:10\nval <- x$col\n",
        "x <- c(\"a\", \"b\")\nval <- x$col\n",
    ] {
        let diags = check(src);
        assert!(
            diags.iter().any(|d| d.code == "RY061"),
            "`{}`: got {:?}",
            src.trim(),
            diags
        );
    }
}

#[test]
fn dollar_on_list_no_warning() {
    let diags = check("x <- list(a = 1)\nval <- x$a\n");
    assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
}

#[test]
fn dollar_on_classed_atomic_values_keeps_dispatch_opaque() {
    for value in [
        "structure(1L, class='widget')",
        "structure('payload', class='widget')",
        "if (runif(1) > 0.5) structure(1L, class='widget') else 'plain'",
    ] {
        let (diags, scope) = check_with_scope(&format!(
            "`$.widget` <- function(x, name) list(value=2L); x <- {value}; out <- x$field; out$value + 1L"
        ));
        assert!(
            diags.iter().all(|d| !matches!(d.code, "RY061" | "RY040")),
            "{value}: {diags:?}"
        );
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{value}");
    }
}

#[test]
fn dollar_on_classed_atomic_values_invalidates_caller_effects() {
    let diags = check(
        "`$.widget` <- function(x, name) { assign('marker', 1L, envir=parent.frame()); 2L }; f <- function() { marker <- 'before'; x <- structure(1L, class='widget'); out <- x$field; marker + 1L }; f()",
    );
    assert!(
        diags.iter().all(|d| !matches!(d.code, "RY061" | "RY040")),
        "{diags:?}"
    );
}

#[test]
fn dollar_on_classed_atomic_values_does_not_require_a_collected_method() {
    let (diags, scope) = check_with_scope("x <- structure(1L, class='external'); out <- x$field");
    assert!(diags.iter().all(|d| d.code != "RY061"), "{diags:?}");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
}

#[test]
fn dollar_on_union_with_outer_class_invalidates_caller_effects() {
    let diags = check(
        "`$.widget` <- function(x,name) { assign('marker',1L,envir=parent.frame()); 2L }; f <- function(flag) { marker <- 'before'; x <- structure(if(flag) 1L else 'payload',class='widget'); out <- x$field; marker+1L }; f(TRUE); f(FALSE)",
    );
    assert!(
        diags.iter().all(|d| !matches!(d.code, "RY061" | "RY040")),
        "{diags:?}"
    );
}

#[test]
fn dollar_assignment_on_classed_atomic_values_preserves_uncertainty() {
    for value in [
        "structure(1L,class='widget')",
        "structure(if(flag) 1L else 'payload',class='widget')",
    ] {
        let diags = check(&format!(
            "`$<-.widget` <- function(x,name,value) {{ assign('marker',1L,envir=parent.frame()); list(saved=value) }}; f <- function(flag) {{ marker <- 'before'; x <- {value}; x$field <- 3L; marker+1L; x$saved }}; f(TRUE); f(FALSE)"
        ));
        assert!(
            diags.iter().all(|d| !matches!(d.code, "RY061" | "RY040")),
            "{value}: {diags:?}"
        );
    }
    let diags = check("x <- 1L; x$field <- 3L");
    assert!(diags.iter().any(|d| d.code == "RY061"), "{diags:?}");
}

#[test]
fn dollar_on_data_frame_no_warning() {
    let diags = check("val <- mtcars$mpg\n");
    assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
}

#[test]
fn dollar_on_opaque_no_warning() {
    let diags = check("x <- some_unknown_thing\nval <- x$col\n");
    assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
}

#[test]
fn dollar_on_all_atomic_union_emits_ry061() {
    let diags = check("x <- if (runif(1) > 0.5) 1L else \"x\"\nx$field\n");
    assert!(diags.iter().any(|d| d.code == "RY061"), "got {diags:?}");

    let mixed = check("x <- if (runif(1) > 0.5) 1L else list(field = 1)\nx$field\n");
    assert!(mixed.iter().all(|d| d.code != "RY061"), "got {mixed:?}");
}

#[test]
fn early_return_joins_trailing_if_tail_type() {
    // An early `return()` must join, not replace, the trailing
    // `if`-expression type. When the union contains a non-atomic member
    // (here the opaque `fromJSON` result), `$` must not fire RY061.
    let diags = check(
        "process <- function(req, raw = FALSE) {\n  if (req == 204) return(TRUE)\n  if (raw) req else jsonlite::fromJSON(\"x\")\n}\nuse <- function(x) process(x)$config\n",
    );
    assert!(diags.iter().all(|d| d.code != "RY061"), "got {diags:?}");
}

#[test]
fn early_return_with_all_atomic_trailing_if_still_reports() {
    // Correctness guard: when every branch of the joined union IS
    // atomic, `$` is a real runtime error and RY061 must still fire.
    let diags = check(
        "a <- function(req, raw) {\n  if (req == 204) return(TRUE)\n  if (raw) 1 else 2\n}\nb <- function(x) a(x, FALSE)$k\n",
    );
    assert!(diags.iter().any(|d| d.code == "RY061"), "got {diags:?}");
}

#[test]
fn diverging_branch_and_unreachable_tail_do_not_pollute_return_join() {
    let (diags, scope) = check_with_scope(
        "f <- function(x) {\n  if (is.null(x)) return(list(field = 1L))\n  x <- list(field = 2L)\n  x\n}\ny <- f(NULL)$field\n",
    );
    assert!(diags.iter().all(|d| d.code != "RY061"), "got {diags:?}");
    assert_eq!(scope.get("y").map(|ty| ty.mode), Some(Mode::Integer));

    let tail = check("g <- function() { return(list(field = 1L)); 1L }\ng()$field\n");
    assert!(tail.iter().all(|d| d.code != "RY061"), "got {tail:?}");
}

#[test]
fn ry003_is_default_off_but_explicitly_selectable() {
    let mut diagnostics = check("if (1L) print(1)\n");
    assert!(diagnostics.iter().any(|d| d.code == "RY003"));

    apply_filter_to_diagnostics(&mut diagnostics, &SeverityFilter::default());
    assert!(diagnostics.iter().all(|d| d.code != "RY003"));

    let mut selected = check("if (1L) print(1)\n");
    let mut selection = SeverityFilter::default();
    selection.add_select("RY003");
    apply_filter_to_diagnostics(&mut selected, &selection);
    assert!(selected.iter().any(|d| d.code == "RY003"));

    let mut diagnostics = check("if (1L) print(1)\n");
    let mut filter = SeverityFilter::default();
    filter.add_warn("RY003");
    apply_filter_to_diagnostics(&mut diagnostics, &filter);
    assert!(diagnostics.iter().any(|d| d.code == "RY003"));
}

#[test]
fn guarded_unknown_parameter_vector_emits_ry032_without_other_vector_intent() {
    for source in [
        "f <- function(x) is.null(x) || is.na(x)\n",
        "f <- function(x) length(x) && x == 1L\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().any(|d| d.code == "RY032"),
            "{source}: {diags:?}"
        );
    }

    let reassigned = check("f <- function(x) { x <- TRUE; is.null(x) || is.na(x) }\n");
    assert!(
        reassigned.iter().all(|d| d.code != "RY032"),
        "got {reassigned:?}"
    );
}

#[test]
fn successful_stopifnot_scalar_guard_carries_to_later_short_circuit() {
    let pinned_purrr_shape = "prepend <- function(x, values, before = NULL) {\n\
        n <- length(x)\n\
        stopifnot(is.null(before) || (before > 0 && before <= n))\n\
        if (is.null(before) || before == 1) c(values, x) else c(x, values)\n\
        }\n";
    let diagnostics = check(pinned_purrr_shape);
    assert!(
        diagnostics.iter().all(|d| d.code != "RY032"),
        "the assertion rejects non-scalar before before its later use: {diagnostics:?}"
    );

    let exact_length = check(
        "f <- function(x) { stopifnot(is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
    );
    assert!(
        exact_length.iter().all(|d| d.code != "RY032"),
        "{exact_length:?}"
    );

    for source in [
        // stopifnot accepts a vector of TRUE values; this does not prove
        // that its subject has scalar length.
        "f <- function(x) { stopifnot(is.null(x) || x > 0); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // A local replacement of the assertion function cannot validate x.
        "stopifnot <- function(...) TRUE; f <- function(x) { stopifnot(is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // An S3 length method may report one for a longer vector.
        "length.foo <- function(x) 1L; f <- function(x) { stopifnot(is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // The validation comparison may dispatch independently of the
        // later equality comparison.
        "`>.foo` <- function(e1, e2) TRUE; f <- function(x) { stopifnot(is.null(x) || (x > 0 && x <= 3)); if (is.null(x) || x == 1L) TRUE else FALSE }",
        // A later assertion argument can replace the value validated by
        // an earlier one before the following condition executes.
        "f <- function(x) { stopifnot(is.null(x) || length(x) == 1L, { x <- c(1L, 2L); TRUE }); if (is.null(x) || x == 1L) TRUE else FALSE }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "unproved scalar path must retain the existing warning: {source}: {diagnostics:?}"
        );
    }
}

#[test]
fn scalar_assertion_rejects_effectful_rhs_before_carrying_a_fact() {
    for source in [
        "f <- function(x) { stopifnot(is.null(x) || (x > 0 && { assign('x', c(1L, 2L)); TRUE })); if (is.null(x) || x == 1L) TRUE else FALSE }",
        "f <- function(x, mutate) { stopifnot(is.null(x) || (x > 0 && mutate())); if (is.null(x) || x == 1L) TRUE else FALSE }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "an effectful assertion RHS cannot validate the continuation: {source}: {diagnostics:?}"
        );
    }
    let pure = check(
        "f <- function(x) { stopifnot(is.null(x) || (x > 0 && x <= 3)); if (is.null(x) || x == 1L) TRUE else FALSE }",
    );
    assert!(pure.iter().all(|d| d.code != "RY032"), "{pure:?}");

    // A helper that can write through the enclosing frame must not install
    // the new scalar assertion fact. Reporting its value at the later `if`
    // also requires the local-call effect model (#568).
    let (_, scope) = check_with_scope(
        "x <- 1L; mutate <- function() { x <<- c(1L, 2L); TRUE }; stopifnot(is.null(x) || (x > 0 && mutate())); if (is.null(x) || x == 1L) TRUE else FALSE",
    );
    assert!(!scope.scalar_asserted_bindings.contains("x"));
}

#[test]
fn scalar_assertion_does_not_force_another_formals_default_as_pure() {
    for source in [
        "f <- function(x, ok = { x <- c(1L, 2L); TRUE }) { stopifnot(is.null(x) || (x > 0 && ok)); if (is.null(x) || x == 1L) TRUE else FALSE }; f(1L)",
        "f <- function(x, n = { x <- c(1L, 2L); 3L }) { stopifnot(is.null(x) || (x > 0 && x <= n)); if (is.null(x) || x == 1L) TRUE else FALSE }; f(1L)",
        "f <- function(x) { delayedAssign('n', { x <- c(1L, 2L); 3L }); stopifnot(is.null(x) || (x > 0 && x <= n)); if (is.null(x) || x == 1L) TRUE else FALSE }; f(1L)",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "forcing a promise may replace the asserted value: {source}: {diagnostics:?}"
        );
    }
    let pure_local = check(
        "f <- function(x) { n <- 3L; stopifnot(is.null(x) || (x > 0 && x <= n)); if (is.null(x) || x == 1L) TRUE else FALSE }",
    );
    assert!(
        pure_local.iter().all(|d| d.code != "RY032"),
        "{pure_local:?}"
    );
}

#[test]
fn scalar_proof_rejects_a_subject_promise_that_rebinds_itself() {
    for source in [
        "f <- function(x = { x <- c(1L, 2L); 1L }) { stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        "f <- function(x = { x <- c(1L, 2L); 1L }) { stopifnot(length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        // A loop-vector fact for another binding activates ScalarThen
        // narrowing. It must use the same subject-promise provenance gate.
        "f <- function(x = { x <- c(1L, 2L); 1L }, xs) { y <- c(1L, 2L); for (i in xs) { if (x > 0 && TRUE) { if (is.null(x) || x == 1L) TRUE else FALSE }; y <- 1L } }; f(xs = 1L)",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "the asserted value can differ from its now-vector binding: {source}: {diagnostics:?}"
        );
    }
    for source in [
        "f <- function(x = 1L) { stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        "f <- function(x = NULL) { stopifnot(is.null(x) || (x > 0 && x <= 3L)); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        "f <- function(x) { stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f(1L)",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY032"),
            "a stable subject keeps the scalar proof: {source}: {diagnostics:?}"
        );
    }
}

#[test]
fn scalar_proof_rejects_a_replaced_literal_default_binding() {
    for (index, source) in [
        include_str!("../../testdata/oracle/assertion_subject_active_binding.R"),
        include_str!("../../testdata/oracle/assertion_subject_delayed_binding.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_binding.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_default_env.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_passed_env.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_transitive_local.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_alias_local.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_environment_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_transformed_env.R"),
        include_str!("../../testdata/oracle/assertion_subject_expression_environment_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_get_env.R"),
        include_str!("../../testdata/oracle/assertion_subject_iife_env.R"),
        include_str!("../../testdata/oracle/assertion_subject_masked_new_env.R"),
        include_str!("../../testdata/oracle/assertion_subject_masked_invisible.R"),
        include_str!("../../testdata/oracle/assertion_subject_installer_local_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_installer_global_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_installer_alias_chain.R"),
        include_str!("../../testdata/oracle/assertion_subject_installer_iife.R"),
        include_str!("../../testdata/oracle/assertion_subject_installer_computed_get.R"),
        include_str!("../../testdata/oracle/assertion_subject_installer_literal_selector.R"),
        include_str!("../../testdata/oracle/assertion_subject_primitive_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_global_primitive_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_wrapped_global_primitive_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_wrapped_global_alias_chain.R"),
        include_str!("../../testdata/oracle/assertion_subject_masked_global_extraction.R"),
        include_str!("../../testdata/oracle/assertion_subject_string_do_call_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_wrapped_string_do_call_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_block_global_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_callback_helper_hop.R"),
        include_str!("../../testdata/oracle/assertion_subject_callback_helper_alias_hop.R"),
        include_str!("../../testdata/oracle/assertion_subject_quoted_callback_formal.R"),
        include_str!("../../testdata/oracle/assertion_subject_quoted_callback_partial.R"),
        include_str!("../../testdata/oracle/assertion_subject_quoted_callback_actual.R"),
        include_str!("../../testdata/oracle/assertion_subject_direct_quoted_formal.R"),
        include_str!("../../testdata/oracle/assertion_subject_direct_quoted_actual.R"),
        include_str!("../../testdata/oracle/assertion_subject_direct_quoted_partial.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_value.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_string.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_wrapped.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_block.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_alias_chain.R"),
        include_str!(
            "../../testdata/oracle/assertion_subject_local_do_call_saved_before_overwrite.R"
        ),
        include_str!(
            "../../testdata/oracle/assertion_subject_local_do_call_saved_string_before_overwrite.R"
        ),
        include_str!(
            "../../testdata/oracle/assertion_subject_local_do_call_saved_wrapped_before_overwrite.R"
        ),
        include_str!(
            "../../testdata/oracle/assertion_subject_local_do_call_saved_block_before_overwrite.R"
        ),
        include_str!("../../testdata/oracle/assertion_subject_quoted_assign_env.R"),
        include_str!(
            "../../testdata/oracle/assertion_subject_qualified_source_literal_collision.R"
        ),
        include_str!(
            "../../testdata/oracle/assertion_subject_qualified_source_wrapped_literal_collision.R"
        ),
        include_str!(
            "../../testdata/oracle/assertion_subject_qualified_source_block_literal_collision.R"
        ),
        include_str!(
            "../../testdata/oracle/assertion_subject_qualified_source_internal_literal_collision.R"
        ),
        include_str!("../../testdata/oracle/assertion_subject_quoted_do_call_what.R"),
        include_str!("../../testdata/oracle/assertion_subject_loop_carried_do_call_for.R"),
        include_str!("../../testdata/oracle/assertion_subject_loop_carried_do_call_while.R"),
        include_str!("../../testdata/oracle/assertion_subject_loop_carried_do_call_repeat.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_local_installer_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_global_installer_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_parent_assign_helper.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_direct_assign.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_local_assign_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_global_assign_alias.R"),
        include_str!(
            "../../testdata/oracle/assertion_subject_same_frame_direct_base_environment.R"
        ),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_active_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_wrapped_assign_alias.R"),
        include_str!(
            "../../testdata/oracle/assertion_subject_same_frame_invisible_delayed_alias.R"
        ),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_block_assign_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_branch_installer_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_do_call_assign.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_supplied_assign.R"),
        include_str!("../../testdata/oracle/assertion_subject_named_dots_callback_used.R"),
        include_str!("../../testdata/oracle/assertion_subject_unknown_global_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_triple_namespace_primitive_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_quoted_primitive_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_supplied_installer.R"),
        include_str!("../../testdata/oracle/assertion_subject_supplied_installer_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_overridden_pure_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_forwarded_callback_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_passed_default_do_call.R"),
        include_str!("../../testdata/oracle/assertion_subject_wrapped_identity_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_wrapped_list_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_variadic_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_variadic_wrapped_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_second_variadic_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_variadic_do_call.R"),
        include_str!("../../testdata/oracle/assertion_subject_second_variadic_do_call.R"),
        include_str!("../../testdata/oracle/assertion_subject_variadic_do_call_wrapped_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_reordered_arguments.R"),
        include_str!("../../testdata/oracle/assertion_subject_called_function_default.R"),
        include_str!("../../testdata/oracle/assertion_subject_chained_called_defaults_earlier.R"),
        include_str!("../../testdata/oracle/assertion_subject_chained_called_defaults_later.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_forced_default.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_delayed_default.R"),
        include_str!("../../testdata/oracle/assertion_subject_helper_forced_default_call.R"),
        include_str!("../../testdata/oracle/assertion_subject_default_iife.R"),
        include_str!("../../testdata/oracle/assertion_subject_default_local_closure.R"),
        include_str!("../../testdata/oracle/assertion_subject_default_literal_get.R"),
        include_str!("../../testdata/oracle/assertion_subject_default_computed_get.R"),
    ]
    .into_iter()
    .enumerate()
    {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "case {index}: the assertion consumed a scalar but the binding can now be a vector: {diagnostics:?}"
        );
    }
    let after_assertion = check(
        "f <- function(x = 1L) { stopifnot(x > 0 && TRUE); delayedAssign('x', { x <- c(1L, 2L); 1L }, assign.env = environment()); if (is.null(x) || x == 1L) TRUE else FALSE }",
    );
    assert!(
        after_assertion.iter().any(|d| d.code == "RY032"),
        "a later binding installation revokes an earlier scalar proof: {after_assertion:?}"
    );

    for (control, source) in [
        "local_install <- function() { makeActiveBinding('y', function() 1L, base::environment()) }; f <- function(x = 1L) { local_install(); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        "read_parent <- function() parent.frame(); f <- function(x = 1L) { read_parent(); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        include_str!("../../testdata/oracle/assertion_subject_unused_installer_default.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_installer_env.R"),
        include_str!("../../testdata/oracle/assertion_subject_installer_argument_controls.R"),
        include_str!("../../testdata/oracle/assertion_subject_pure_supplied_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_unused_callable_formal.R"),
        include_str!("../../testdata/oracle/assertion_subject_omitted_pure_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_passed_default_value_only.R"),
        include_str!("../../testdata/oracle/assertion_subject_wrapped_value_only.R"),
        include_str!("../../testdata/oracle/assertion_subject_variadic_pure_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_variadic_unused_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_second_variadic_pure_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_variadic_do_call_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_variadic_do_call_value_only.R"),
        include_str!("../../testdata/oracle/assertion_subject_wrapped_global_pure_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_global_stored_primitive.R"),
        include_str!("../../testdata/oracle/assertion_subject_pure_callback_helper_hop.R"),
        include_str!("../../testdata/oracle/assertion_subject_stored_callback_helper_hop.R"),
        include_str!("../../testdata/oracle/assertion_subject_stored_string_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_pure_block_global_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_forwarded_pure_callback.R"),
        include_str!("../../testdata/oracle/assertion_subject_quoted_callback_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_direct_quoted_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_stored.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_pure_alias_chain.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_stored_string.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_nested_alias_shadow.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_overwrite_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_saved_after_overwrite_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_saved_block_after_overwrite_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_later_write_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_local_do_call_shared_pure_aliases.R"),
        include_str!("../../testdata/oracle/assertion_subject_quoted_assign_env_local.R"),
        include_str!("../../testdata/oracle/assertion_subject_literal_qualified_local_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_quoted_do_call_what_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_loop_do_call_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_loop_do_call_single_literal.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_pure_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_overwritten_installer_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_local_assign_helper.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_direct_assign_local.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_fresh_installer_target.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_fresh_assign_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_fresh_active_target.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_fresh_wrapped_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_unused_branch_alias.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_branch_overwrite_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_do_call_fresh_target.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_do_call_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_same_frame_supplied_overwritten_pure.R"),
        include_str!("../../testdata/oracle/assertion_subject_named_dots_callback_unused.R"),
        "run <- function(env, `action` = function(...) NULL) action('x', 1L, assign.env = env, eval.env = env); install <- function() run(env = parent.frame()); f <- function(x = 1L) { install(); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        "run <- function(env, `action`) action('x', 1L, assign.env = env, eval.env = env); install <- function() run(act = function(...) NULL, env = parent.frame()); f <- function(x = 1L) { install(); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        "run <- function(env, action) action('x', 1L, assign.env = env, eval.env = env); install <- function() run(`action` = function(...) NULL, env = parent.frame()); f <- function(x = 1L) { install(); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
        "install <- function(env) { target <- base::new.env(); makeActiveBinding('x', function() 1L, target) }; f <- function(x = 1L) { install(environment()); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()",
    ]
    .into_iter()
    .enumerate()
    {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY032"),
            "control {control}: a helper without a caller-frame binding install keeps the proof: {diagnostics:?}"
        );
    }
}

#[test]
fn shared_callback_aliases_have_bounded_purity_work() {
    let mut source = String::from("a0 <- function(...) NULL\nb0 <- function(...) NULL\n");
    for level in 1..=24 {
        let prior = level - 1;
        source.push_str(&format!(
            "a{level} <- if (TRUE) a{prior} else b{prior}\nb{level} <- if (TRUE) a{prior} else b{prior}\n"
        ));
    }
    source.push_str("run <- function(action) action()\ninstall <- function() run(a24)\nf <- function(x = 1L) { install(); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()\n");
    let file = parse_file("alias-diamond.R", &source);
    let mut checker = Checker::new("alias-diamond.R");
    checker.collect_fns(&file.stmts);
    let mut purity = CallerBindingPurity {
        table: &checker.fn_table,
        completed: HashMap::new(),
        visiting: HashSet::new(),
        remaining: 128,
    };
    assert!(purity.inert_source("a24"));
    assert!(purity.inert_source("b24"));
    assert!(
        128 - purity.remaining <= 52,
        "shared suffixes should consume work once per unique alias/function"
    );
    let diagnostics = check(&source);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY032"),
        "the pure shared aliases should retain the scalar proof: {diagnostics:?}"
    );

    let installer = source.replace(
        "a0 <- function(...) NULL\nb0 <- function(...) NULL",
        "a0 <- base::delayedAssign\nb0 <- function(...) NULL",
    );
    let diagnostics = check(&installer);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY032"),
        "one installing leaf invalidates the shared proof: {diagnostics:?}"
    );

    let unknown = source.replace(
        "a0 <- function(...) NULL\nb0 <- function(...) NULL",
        "a0 <- get('unknown_callback')\nb0 <- function(...) NULL",
    );
    let diagnostics = check(&unknown);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY032"),
        "an unresolved leaf cannot certify the shared aliases: {diagnostics:?}"
    );

    let cycle = "a <- b; b <- a; run <- function(action) action(); install <- function() run(a); f <- function(x = 1L) { install(); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }; f()";
    let diagnostics = check(cycle);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY032"),
        "a cyclic alias does not certify callback purity: {diagnostics:?}"
    );
}

#[test]
fn variadic_positional_references_resolve_only_with_enclosing_dots() {
    for source in [
        "f <- function(...) ..1\nf(1L)",
        "f <- function(...) { nested <- function() ..2; nested() }\nf(1L, 2L)",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "`..n` reads an enclosing dots promise: {source}: {diagnostics:?}"
        );
    }
    let unbound = check("value <- ..1");
    assert!(
        unbound.iter().any(|diagnostic| diagnostic.code == "RY010"),
        "without `...`, `..1` has no binding: {unbound:?}"
    );
}

#[test]
fn computed_call_heads_respect_binding_installers_and_inert_values() {
    for (name, source) in [
        (
            "assign identity",
            include_str!("../../testdata/oracle/assertion_subject_computed_assign_identity.R"),
        ),
        (
            "assign block",
            include_str!("../../testdata/oracle/assertion_subject_computed_assign_block.R"),
        ),
        (
            "assign list index",
            include_str!("../../testdata/oracle/assertion_subject_computed_assign_list_index.R"),
        ),
        (
            "assign returned",
            include_str!("../../testdata/oracle/assertion_subject_computed_assign_returned.R"),
        ),
        (
            "delayed invisible",
            include_str!("../../testdata/oracle/assertion_subject_computed_delayed_invisible.R"),
        ),
        (
            "active nested returned",
            include_str!(
                "../../testdata/oracle/assertion_subject_computed_active_nested_returned.R"
            ),
        ),
        (
            "nested head effect",
            include_str!("../../testdata/oracle/assertion_subject_computed_nested_head_effect.R"),
        ),
        (
            "branch effect",
            include_str!("../../testdata/oracle/assertion_subject_computed_branch_effect.R"),
        ),
        (
            "evaluating head",
            include_str!("../../testdata/oracle/assertion_subject_computed_eval_effect.R"),
        ),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "{name}: the later || reads a newly installed vector binding: {diagnostics:?}"
        );
    }
    for (name, source) in [
        (
            "assign fresh",
            include_str!("../../testdata/oracle/assertion_subject_computed_assign_fresh.R"),
        ),
        (
            "delayed fresh",
            include_str!("../../testdata/oracle/assertion_subject_computed_delayed_fresh.R"),
        ),
        (
            "active fresh",
            include_str!("../../testdata/oracle/assertion_subject_computed_active_fresh.R"),
        ),
        (
            "pure block",
            include_str!("../../testdata/oracle/assertion_subject_computed_pure_block.R"),
        ),
        (
            "pure returned",
            include_str!("../../testdata/oracle/assertion_subject_computed_pure_returned.R"),
        ),
        (
            "pure overwrite",
            include_str!("../../testdata/oracle/assertion_subject_computed_pure_overwrite.R"),
        ),
        (
            "branch pure",
            include_str!("../../testdata/oracle/assertion_subject_computed_branch_pure.R"),
        ),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY032"),
            "{name}: no binding can replace the asserted value: {diagnostics:?}"
        );
    }
}

#[test]
fn direct_loop_carried_callables_and_negative_numeric_operands() {
    for (name, source) in [
        (
            "for direct",
            include_str!("../../testdata/oracle/assertion_subject_r18_for_direct.R"),
        ),
        (
            "for do.call",
            include_str!("../../testdata/oracle/assertion_subject_r18_for_do_call.R"),
        ),
        (
            "for alias",
            include_str!("../../testdata/oracle/assertion_subject_r18_for_alias.R"),
        ),
        (
            "for wrapped",
            include_str!("../../testdata/oracle/assertion_subject_r18_for_wrapped.R"),
        ),
        (
            "while direct",
            include_str!("../../testdata/oracle/assertion_subject_r18_while_direct.R"),
        ),
        (
            "repeat do.call",
            include_str!("../../testdata/oracle/assertion_subject_r18_repeat_do_call.R"),
        ),
        (
            "negative for",
            include_str!("../../testdata/oracle/assertion_subject_r18_negative_for.R"),
        ),
        (
            "negative while",
            include_str!("../../testdata/oracle/assertion_subject_r18_negative_while.R"),
        ),
        (
            "negative mirror",
            include_str!("../../testdata/oracle/assertion_subject_r18_negative_mirror.R"),
        ),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "{name}: a repeated callable or unclassed vector can reach the scalar operator: {diagnostics:?}"
        );
    }
    for (name, source) in [
        (
            "one iteration",
            include_str!("../../testdata/oracle/assertion_subject_r18_literal_one.R"),
        ),
        (
            "fresh target",
            include_str!("../../testdata/oracle/assertion_subject_r18_fresh_target.R"),
        ),
        (
            "pure loop",
            include_str!("../../testdata/oracle/assertion_subject_r18_pure_loop.R"),
        ),
        (
            "pure overwrite",
            include_str!("../../testdata/oracle/assertion_subject_r18_pure_overwrite.R"),
        ),
        (
            "while false",
            include_str!("../../testdata/oracle/assertion_subject_r18_while_false.R"),
        ),
        (
            "negative scalar",
            include_str!("../../testdata/oracle/assertion_subject_r18_negative_scalar.R"),
        ),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY032"),
            "{name}: the guarded binding stays stable or the operand is scalar: {diagnostics:?}"
        );
    }
    for (name, source) in [
        (
            "masked unary minus",
            "`-` <- function(y) 1L; f <- function() { x <- c(1L, 2L); for(i in integer()) x <- 1L; x < -1L && TRUE }; f()",
        ),
        (
            "classed vector",
            "f <- function() { x <- structure(c(1L, 2L), class='foo'); for(i in integer()) x <- 1L; x < -1L && TRUE }; f()",
        ),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY032"),
            "{name}: no unclassed numeric-vector proof is available: {diagnostics:?}"
        );
    }
}

#[test]
fn named_head_selection_and_inside_loop_assertions_follow_r_evaluation_order() {
    for (name, source) in [
        (
            "named assign",
            include_str!("../../testdata/oracle/assertion_subject_r19_named_assign_selected.R"),
        ),
        (
            "named delayed",
            include_str!("../../testdata/oracle/assertion_subject_r19_named_delayed_selected.R"),
        ),
        (
            "named active",
            include_str!("../../testdata/oracle/assertion_subject_r19_named_active_selected.R"),
        ),
        (
            "qualified assign",
            include_str!("../../testdata/oracle/assertion_subject_r19_named_qualified.R"),
        ),
        (
            "selected formal",
            include_str!("../../testdata/oracle/assertion_subject_r19_formal_selected.R"),
        ),
        (
            "first iteration",
            include_str!("../../testdata/oracle/assertion_subject_r19_initial_installer.R"),
        ),
        (
            "inside for",
            include_str!("../../testdata/oracle/assertion_subject_r19_inside_for.R"),
        ),
        (
            "inside while",
            include_str!("../../testdata/oracle/assertion_subject_r19_inside_while.R"),
        ),
        (
            "inside repeat",
            include_str!("../../testdata/oracle/assertion_subject_r19_inside_repeat.R"),
        ),
        (
            "inside branch",
            include_str!("../../testdata/oracle/assertion_subject_r19_inside_branch.R"),
        ),
        (
            "carried delayed binding",
            include_str!("../../testdata/oracle/assertion_subject_r19_carried_delayed.R"),
        ),
        (
            "loop header installer",
            include_str!("../../testdata/oracle/assertion_subject_r19_header_installer.R"),
        ),
        (
            "loop header alias installer",
            include_str!("../../testdata/oracle/assertion_subject_r19_header_alias_installer.R"),
        ),
        (
            "installer after loop join",
            include_str!("../../testdata/oracle/assertion_subject_r19_postloop_installer_join.R"),
        ),
        (
            "rebound superassignment operator",
            include_str!("../../testdata/oracle/assertion_subject_r19_rebound_superassign.R"),
        ),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY032"),
            "{name}: a selected installer can replace the binding before the later ||: {diagnostics:?}"
        );
    }
    for (name, source) in [
        (
            "reverse pure",
            include_str!("../../testdata/oracle/assertion_subject_r19_named_reverse_pure.R"),
        ),
        (
            "fresh target",
            include_str!("../../testdata/oracle/assertion_subject_r19_named_fresh.R"),
        ),
        (
            "do.call target timing",
            include_str!("../../testdata/oracle/assertion_subject_r19_do_call_timing.R"),
        ),
        (
            "pure loop",
            include_str!("../../testdata/oracle/assertion_subject_r19_inside_pure.R"),
        ),
        (
            "fresh loop",
            include_str!("../../testdata/oracle/assertion_subject_r19_inside_fresh.R"),
        ),
        (
            "singleton",
            include_str!("../../testdata/oracle/assertion_subject_r19_inside_singleton.R"),
        ),
        (
            "scalar reassertion",
            include_str!("../../testdata/oracle/assertion_subject_r19_reassert_immediate.R"),
        ),
        (
            "vector rejected at reassertion",
            include_str!("../../testdata/oracle/assertion_subject_r19_reassert_rejects_vector.R"),
        ),
        (
            "pure loop header",
            include_str!("../../testdata/oracle/assertion_subject_r19_header_pure.R"),
        ),
        (
            "superassignment leaves local callable",
            include_str!("../../testdata/oracle/assertion_subject_r19_superassign_outer_pure.R"),
        ),
        (
            "pure callable survives loop join",
            include_str!("../../testdata/oracle/assertion_subject_r19_postloop_inert_join.R"),
        ),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY032"),
            "{name}: the selected call is harmless or a later assertion rejects the vector: {diagnostics:?}"
        );
    }
}

#[test]
fn scalar_proof_rejects_cross_file_caller_binding_helper() {
    let mut project = Project::new();
    project.add_file(
        "helper.R".into(),
        parse_file(
            "helper.R",
            "install <- function(env) makeActiveBinding('x', function() c(1L, 2L), env)\nbridge <- function(target) install(target)\n",
        ),
    );
    project.add_file(
        "consumer.R".into(),
        parse_file(
            "consumer.R",
            "f <- function(x = NULL) { bridge(environment()); stopifnot(x > 0 && TRUE); if (is.null(x) || x == 1L) TRUE else FALSE }\n",
        ),
    );
    let diagnostics: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        diagnostics.iter().any(|d| d.code == "RY032"),
        "cross-file helper can replace the binding: {diagnostics:?}"
    );
}

#[test]
fn scalar_assertion_requires_known_ambient_call_identity() {
    let body = "f <- function(x = NULL) { stopifnot(is.null(x) || (x > 0 && x <= 2L)); if (is.null(x) || x == 1L) TRUE else FALSE }; f()";
    assert!(
        check(body).iter().all(|d| d.code != "RY032"),
        "the unchanged base assertion is a valid scalar guard"
    );
    for ambient in ["library(stats)", "attach(list())"] {
        let source = format!("{ambient}\n{body}");
        let diagnostics = check(&source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "unknown search-path effects cannot certify the assertion after {ambient}: {diagnostics:?}"
        );
    }
}

#[test]
fn stopifnot_named_controls_do_not_validate_the_continuation() {
    let parameter = check(
        "f <- function(x) { stopifnot(local = is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
    );
    assert!(
        parameter.iter().any(|d| d.code == "RY032"),
        "a FALSE `local` control is not a failed assertion: {parameter:?}"
    );
    let loop_value = check(
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { stopifnot(local = length(x) == 1L); if (x == 1L && TRUE) i; x <- 1L } }",
    );
    assert!(
        loop_value.iter().any(|d| d.code == "RY032"),
        "a named control cannot clear the proven vector path: {loop_value:?}"
    );
    let positional = check(
        "f <- function(x) { stopifnot(is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
    );
    assert!(
        positional.iter().all(|d| d.code != "RY032"),
        "a genuine final positional assertion still proves scalar or NULL: {positional:?}"
    );
    let named_predicate = check(
        "f <- function(x) { stopifnot(named_condition = is.null(x) || length(x) == 1L); if (is.null(x) || x == 1L) TRUE else FALSE }",
    );
    assert!(
        named_predicate.iter().all(|d| d.code != "RY032"),
        "a named assertion in `...` still rejects vectors: {named_predicate:?}"
    );
}

#[test]
fn parameter_guards_respect_scalar_membership_and_exact_length() {
    for source in [
        "f <- function(x) is.null(x) || 'value' %in% x",
        "f <- function(x) is.null(x) || !('value' %in% x)",
        "f <- function(x) length(x) == 1L && is.na(x)",
        "f <- function(x) 1 == length(x) && x == ''",
        "f <- function(x) base::length(x) == 1 && x == ''",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY032"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn guarded_vector_membership_and_nonempty_lengths_still_warn() {
    for source in [
        "f <- function(x) is.null(x) || x %in% 'value'",
        "f <- function(x) length(x) > 0 && x == ''",
        "f <- function(x) length(x) == 2L && is.na(x)",
        "length <- function(x) 1L; f <- function(x) length(x) == 1L && is.na(x)",
        "f <- function(x, length) length(x) == 1L && is.na(x)",
        "f <- function(x) other::length(x) == 1L && is.na(x)",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn equality_length_guards_refuse_dispatch_risk_and_reassignment() {
    for source in [
        // A `length.<class>` method can report 1 for a longer value, so
        // the guard does not prove the operand scalar (docs/scalar-guards.md).
        "length.disguised <- function(x) 1L\nf <- function(x) length(x) == 1L && x == 1L\n",
        "length.disguised <- function(x) 1L\nf <- function(x) length(x) == 1L && is.na(x)\n",
        // A classed parameter default names the class a method could
        // attach to, registered or not.
        "length.disguised <- function(x) 1L\nf <- function(x = structure(c(1L, 2L), class = \"disguised\")) length(x) == 1L && x == 1L\n",
        // An unclassed default describes only the omitted-argument call
        // shape; callers can still pass a classed value, so it is not
        // proven unclassed either.
        "length.disguised <- function(x) 1L\nf <- function(x = 1L) length(x) == 1L && x == 1L\n",
        // Reassignment inside the guarded operand invalidates the guard
        // before the guarded use.
        "f <- function(x) length(x) == 1L && { x <- c(1, 2); x == 1L }\n",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "{source}: {diagnostics:?}"
        );
    }
    // Without a project `length.*` method and without reassignment the
    // guard still holds for an unknown-class parameter — with or without
    // a scalar default, under the same accepted-risk line.
    let clean = check("f <- function(x) length(x) == 1L && x == 1L\n");
    assert!(clean.iter().all(|d| d.code != "RY032"), "{clean:?}");
    let clean_default = check("f <- function(x = 1L) length(x) == 1L && x == 1L\n");
    assert!(
        clean_default.iter().all(|d| d.code != "RY032"),
        "{clean_default:?}"
    );
}

#[test]
fn loop_carried_values_do_not_keep_the_initial_empty_length() {
    let source = "quote <- raw()\nfor (x in as.raw(c(1, 2))) {\nif (length(quote)) { if (x == quote) print(x) }\nquote <- x\n}";
    assert!(check(source).is_empty(), "{:?}", check(source));
}

#[test]
fn loop_carried_alias_keeps_a_proven_vector_path_for_ry032() {
    let source = "f <- function(xs) {\n\
        original <- c(1L, 2L)\n\
        alias <- original\n\
        for (i in xs) {\n\
          if (alias == 1L && TRUE) i\n\
          alias <- 1L\n\
        }\n\
        }\n";
    let diagnostics = check(source);
    assert!(
        diagnostics.iter().any(|d| d.code == "RY032"),
        "the first iteration receives a proven vector through the alias: {diagnostics:?}"
    );

    let zero_iteration = check(
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) x <- 1L; if (x == 1L && TRUE) x }",
    );
    assert!(
        zero_iteration.iter().any(|d| d.code == "RY032"),
        "an empty iterator leaves the original vector at the later condition: {zero_iteration:?}"
    );

    let partial_rebind = check(
        "f <- function(xs, flag) { x <- c(1L, 2L); for (i in xs) { if (flag) x <- 1L; if (x == 1L && TRUE) i } }",
    );
    assert!(
        partial_rebind.iter().any(|d| d.code == "RY032"),
        "the branch that did not rebind x keeps the vector path: {partial_rebind:?}"
    );

    for source in [
        "f <- function(xs) { original <- 1L; alias <- original; for (i in xs) { if (alias == 1L && TRUE) i; alias <- 1L } }",
        "f <- function(xs) { original <- c(1L, 2L); alias <- original; for (i in xs) { alias <- 1L; if (alias == 1L && TRUE) i } }",
        "f <- function(xs) { original <- c(1L, 2L); alias <- original; for (i in xs) { if (length(alias) == 1L) { if (alias == 1L && TRUE) i }; alias <- 1L } }",
        "f <- function(xs, flag) { x <- c(1L, 2L); for (i in xs) { if (flag) x <- 1L else x <- 1L; if (x == 1L && TRUE) i } }",
        "f <- function() { x <- c(1L, 2L); for (i in 1L) x <- 1L; if (x == 1L && TRUE) x }",
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { stopifnot(length(x) == 1L); if (x == 1L && TRUE) i; x <- 1L } }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY032"),
            "a scalar path must not inherit the vector alternative: {source}: {diagnostics:?}"
        );
    }
}

#[test]
fn loop_vector_fact_flows_through_simple_aliases_but_not_safe_overwrites() {
    for source in [
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { y <- x; if (y == 1L && TRUE) i; x <- 1L } }",
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) x <- 1L; y <- x; if (y == 1L && TRUE) y }",
        include_str!("../../testdata/oracle/loop_mirrored_numeric_lt.R"),
        include_str!("../../testdata/oracle/loop_mirrored_numeric_eq.R"),
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { y <- x; z <- y; if (z == 1L && TRUE) i; x <- 1L } }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "a proven vector path reaches the aliased operand: {source}: {diagnostics:?}"
        );
    }
    for source in [
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { x <- 1L; y <- x; if (y == 1L && TRUE) i } }",
        "f <- function() { x <- c(1L, 2L); for (i in 1L) x <- 1L; y <- x; if (y == 1L && TRUE) y }",
        "`&&` <- function(x, y) TRUE; f <- function(xs) { x <- c(1L, 2L); for (i in xs) { y <- x; if (y == 1L && TRUE) i; x <- 1L } }",
        "`||` <- function(x, y) TRUE; f <- function(xs) { x <- c(1L, 2L); for (i in xs) { y <- x; if (y == 1L || FALSE) i; x <- 1L } }",
        include_str!("../../testdata/oracle/loop_mirrored_masked_lt.R"),
        include_str!("../../testdata/oracle/loop_mirrored_masked_eq.R"),
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY032"),
            "a safe overwrite or masked operator must stay quiet: {source}: {diagnostics:?}"
        );
    }
}

#[test]
fn scalar_then_refuses_shadowed_parentheses_in_guards_and_assertions() {
    for source in [
        "`(` <- function(x) TRUE; f <- function(xs) { x <- c(1L, 2L); for (i in xs) { if ((length(x) == 1L)) { if (x == 1L && TRUE) i }; x <- 1L } }",
        "`(` <- function(x) TRUE; f <- function(xs) { x <- c(1L, 2L); for (i in xs) { stopifnot((length(x) == 1L)); if (x == 1L && TRUE) i; x <- 1L } }",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY032"),
            "a masked parenthesis never proves scalar length: {source}: {diagnostics:?}"
        );
    }
    let base = check(
        "f <- function(xs) { x <- c(1L, 2L); for (i in xs) { if ((length(x) == 1L)) { if (x == 1L && TRUE) i }; x <- 1L } }",
    );
    assert!(base.iter().all(|d| d.code != "RY032"), "{base:?}");
}

#[test]
fn loop_break_retains_bindings_at_the_exit() {
    for loop_header in ["while (TRUE)", "repeat", "for (i in 1:2)"] {
        let source = format!(
            "f <- function(flag) {{ scale <- 1L; trial <- list(); {loop_header} {{ trial$score <- 1L; if (flag) break; trial <- list(alpha=2) }}; if (flag) scale <- 1 + abs(trial$score) else scale <- abs(trial$score); if (scale > 0) TRUE }}"
        );
        let diagnostics = check(&source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY001"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn loop_break_does_not_borrow_later_callable_assignment() {
    let (diagnostics, scope) =
        check_with_scope("x <- 1L; while (TRUE) { break; x <- function() 1L }; x$field ");
    assert_eq!(
        scope.get("x").map(|ty| ty.mode),
        Some(Mode::Integer),
        "{:?}",
        scope.get("x")
    );
    assert!(
        diagnostics.iter().any(|d| d.code == "RY061"),
        "{diagnostics:?}"
    );
}

#[test]
fn loop_transfers_preserve_nested_execution_frames() {
    for source in [
        "x <- 'old'; while (TRUE) { repeat { break }; x <- 1L; break }",
        "x <- 'old'; while (TRUE) { unused <- function() { break }; x <- 1L; break }",
        "x <- 'old'; while (TRUE) { lapply(integer(), function(value) { break }); x <- 1L; break }",
        "capture <- function(value) substitute(value); x <- 'old'; while (TRUE) { capture(break); x <- 1L; break }",
        "x <- 'old'; while (TRUE) { with(list(), if (FALSE) break); x <- 1L; break }",
        "x <- 'old'; for (i in 1:2) { x <- 1L; next; x <- function() 1L }",
        "x <- 'old'; for (i in 1:2) { { x <- 1L; break; x <- function() 1L } }",
        "`break` <- function() NULL; x <- 'old'; for (i in 1:2) { break; x <- 1L }",
        "`next` <- function() NULL; x <- 'old'; for (i in 1:2) { next; x <- 1L }",
        "x <- 'old'; while (TRUE) { ignored <- if (flag) { x <- 1L; break } else { x <- 2L; break }; x <- function() 1L }",
    ] {
        let (_, scope) = check_with_scope(source);
        assert_eq!(
            scope.get("x").map(|ty| ty.mode),
            Some(Mode::Integer),
            "{source}: {:?}",
            scope.get("x")
        );
    }
}

#[test]
fn loop_break_paths_join_without_using_non_exiting_tails() {
    let source = "x <- 'old'; while (TRUE) { if (flag) { x <- 1L; break }; x <- 2L; break; x <- function() 1L }; x$field";
    let (diagnostics, scope) = check_with_scope(source);
    assert_eq!(scope.get("x").map(|ty| ty.mode), Some(Mode::Integer));
    assert!(
        diagnostics.iter().any(|d| d.code == "RY061"),
        "{diagnostics:?}"
    );
}

#[test]
fn deferred_loop_transfers_do_not_change_the_function_body_exit() {
    let (_, scope) = check_with_scope(
        "f <- function() { x <- 'old'; while (TRUE) { on.exit(if (FALSE) break); x <- 1L; break }; x }; out <- f()",
    );
    assert_eq!(
        scope.get("out").map(|ty| ty.mode),
        Some(Mode::Integer),
        "{:?}",
        scope.get("out")
    );
}

#[test]
fn loop_exit_preserves_uncertainty_from_unmodelled_paths() {
    let source = r#"`+` <- function(e1, e2) { assign("x", list(field = 1L), envir = parent.frame()); NULL }
f <- function(flag) {
    x <- 1L
    while (TRUE) {
        if (flag) break
        1 + 2
        break
    }
    if (!flag) x$field
}
f(FALSE)
"#;
    let diagnostics = check(source);
    assert!(
        diagnostics.iter().all(|d| d.code != "RY061"),
        "{diagnostics:?}"
    );
}

#[test]
fn quoted_top_level_symbols_resolve_as_values_in_functions() {
    for source in [
        "`n1` <- 42; f <- function() n1 + 1",
        "`g` <- function(x) x + 1; f <- function() { z <- g; z(1) }",
        "`g` <- function() 1; `g` <- 42; f <- function() g + 1",
        "`n1` <- 1; n1 <- 2; f <- function() n1",
        "n1 <- 1; `n1` <- 2; f <- function() n1",
    ] {
        let diagnostics = check(source);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
    }
}

#[test]
fn quoted_symbol_existence_does_not_invent_distinct_bindings() {
    for source in [
        "`n1` <- 42; f <- function() n2",
        r#""`n1`" <- 42; f <- function() n1"#,
        r#"`n\x31` <- 42; f <- function() n2"#,
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY010"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn condition_severities_match_the_rule_registry() {
    for source in [
        "if (NULL) 1L",
        "if (1L) 1L",
        "if (c(TRUE, FALSE)) 1L",
        "while (list(TRUE)) break",
    ] {
        let diagnostics = check(source);
        let condition = diagnostics
            .iter()
            .find(|d| matches!(d.code, "RY001" | "RY002" | "RY003"))
            .unwrap();
        assert_eq!(
            condition.severity,
            crate::rules::find(condition.code).unwrap().default_severity,
            "{source}"
        );
    }
}

#[test]
fn logical_condition_length_survives_nested_math_warning() {
    let diagnostics = check("if (abs(c(1, 2) > 0) > 0) 1L");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY100"),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|d| d.code == "RY002"),
        "{diagnostics:?}"
    );
}
