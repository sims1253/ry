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
        // single-element case; a non-literal LHS has no exact count,
        // but `a:b` always has at least one element (`1:0` is `c(1, 0)`).
        ("x <- 1:10\n", Some(Mode::Integer), Length::Known(10)),
        ("x <- 10:1\n", Some(Mode::Integer), Length::Known(10)),
        ("x <- 1.0:5.0\n", Some(Mode::Integer), Length::Known(5)),
        ("x <- 5:5\n", None, Length::Known(1)),
        (
            "n <- 1L\nx <- n:10\n",
            Some(Mode::Integer),
            Length::Nonempty,
        ),
        // `rep`: positional, named (`times =`), and `each =`
        // multipliers; `rep(0, 5)` keeps double (`0` has no `L`);
        // a non-literal `times` stays Unknown.
        ("x <- rep(1:3, 2)\n", Some(Mode::Integer), Length::Known(6)),
        ("x <- rep(0, 5)\n", Some(Mode::Double), Length::Known(5)),
        ("x <- rep(c(1, 2), times = 3)\n", None, Length::Known(6)),
        ("x <- rep(c(1, 2, 3), each = 2)\n", None, Length::Known(6)),
        ("x <- rep(c(1, 2), 3, each = 2)\n", None, Length::Known(12)),
        ("n <- 2\nx <- rep(1:3, n)\n", None, Length::Unknown),
        // `length.out` (third positional, or a partial name) overrides
        // `times` and `each`; a zero-length `x` pads with `NA`.
        (
            "x <- rep(1:3, length.out = 1)\n",
            Some(Mode::Integer),
            Length::One,
        ),
        ("x <- rep(1:2, 2, 3)\n", None, Length::Known(3)),
        ("x <- rep(1:3, len = 2)\n", None, Length::Known(2)),
        ("x <- rep(1:3, each = 2, l = 4)\n", None, Length::Known(4)),
        ("x <- rep(1:3, length.out = 0)\n", None, Length::Zero),
        (
            "x <- rep(integer(0), length.out = 3)\n",
            None,
            Length::Known(3),
        ),
        (
            "n <- 2\nx <- rep(1:3, length.out = n)\n",
            None,
            Length::Unknown,
        ),
        ("x <- rep(1:2, ti = 2)\n", None, Length::Known(4)),
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
fn rep_length_out_caps_the_condition_length() {
    // Each call returns one element in R, so the condition is legal.
    for call in [
        "rep(c(TRUE, FALSE), length.out = 1)",
        "rep(c(TRUE, FALSE), 2, 1)",
        "rep(c(TRUE, FALSE), len = 1)",
        "rep(c(TRUE, FALSE), times = 3, each = 2, length.out = 1)",
    ] {
        let diags = check(&format!("y <- {call}\nif (y) 1L else 2L\n"));
        assert!(diags.iter().all(|d| d.code != "RY002"), "{call}: {diags:?}");
    }
    // A literal `length.out` still pins a length the condition rejects.
    let diags = check("y <- rep(TRUE, length.out = 2)\nif (y) 1L else 2L\n");
    assert!(diags.iter().any(|d| d.code == "RY002"), "{diags:?}");
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
fn dollar_on_classed_atomic_values_needs_some_method() {
    let diags = check("x <- structure(1L, class='external'); out <- x$field");
    assert!(diags.iter().any(|d| d.code == "RY061"), "{diags:?}");
    // An attached package without a stub may register `$.external`.
    let (diags, scope) = check_with_scope(
        "library(unknownpkg); x <- structure(1L, class='external'); out <- x$field",
    );
    assert!(diags.iter().all(|d| d.code != "RY061"), "{diags:?}");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
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

fn is_integer_or_double_union(t: &RType) -> bool {
    t.mode == Mode::Union
        && t.members.as_ref().is_some_and(|ms| {
            ms.len() == 2
                && ms.iter().any(|m| m.mode == Mode::Integer)
                && ms.iter().any(|m| m.mode == Mode::Double)
        })
}

/// `a:b` with a non-literal endpoint: R's `:` returns a plain integer or
/// double vector of length >= 1 unless both endpoints are factors (their
/// interaction). Issue #648.
#[test]
fn colon_with_unknown_endpoint_is_nonempty_numeric() {
    for src in [
        // R: typeof(1:5) is "integer" but typeof(1:3e9) is "double", so an
        // unproven `to` (or a non-literal `from`) leaves both possible.
        "n <- runif(1)\nx <- 1:n\n",
        "n <- runif(1)\nx <- n:1\n",
        // The literal fast path must not claim an integer vector past int
        // range.
        "x <- 1:3e9\n",
    ] {
        let (_, scope) = check_with_scope(src);
        let x = scope.get("x").expect("x should be bound");
        assert!(is_integer_or_double_union(x), "`{src}`: got {x:?}");
        assert_eq!(x.length, Length::Nonempty, "`{src}`: got {x:?}");
    }
    // R: typeof(1.5:n) is "double" whatever `n` is.
    let (_, scope) = check_with_scope("n <- runif(1)\nx <- 1.5:n\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Double, "got {x:?}");
    assert_eq!(x.length, Length::Nonempty, "got {x:?}");
    // R: `1:n` with any `n` is integer/double (a lone factor contributes its
    // first code), so arithmetic with character errors.
    let diags = check("f <- function(n) {\n  x <- 1:n\n  x + \"a\"\n}\n");
    assert!(diags.iter().any(|d| d.code == "RY040"), "got {diags:?}");
}

#[test]
fn colon_without_a_non_factor_endpoint_stays_unknown() {
    // R: factor(c("a", "b")):factor(c("x", "y")) is a factor interaction,
    // so two endpoints of unknown class prove nothing about the result.
    let diags = check("f <- function(a, b) {\n  x <- a:b\n  x + \"a\"\n}\n");
    assert!(diags.iter().all(|d| d.code != "RY040"), "got {diags:?}");
    // A user-defined `:` replaces the primitive.
    let diags = check(
        "g <- function(n) {\n  `:` <- function(a, b) \"s\"\n  x <- 1:n\n  paste0(x + \"a\")\n}\n",
    );
    assert!(diags.iter().all(|d| d.code != "RY040"), "got {diags:?}");
    let (_, scope) = check_with_scope("`:` <- function(a, b) \"s\"\nx <- 1:3\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Opaque, "got {x:?}");
    // Inferred plain classes are not proof that an endpoint is not a factor:
    // in R each of these `a` is a factor, so `a:g` is an interaction and
    // `+.factor` handles the addition.
    for setup in [
        "a <- c(factor(\"a\"))",
        "a <- structure(1L, levels = \"a\", class = c(\"c1\", \"c2\", \"c3\", \"c4\", \"factor\"))",
        "a <- 1L\n  class(a) <- c(\"c1\", \"c2\", \"c3\", \"c4\", \"factor\")\n  levels(a) <- \"a\"",
        "a <- 1L\n  attr(a, \"levels\") <- \"a\"\n  oldClass(a) <- \"factor\"",
        "a <- 1L\n  attributes(a) <- list(levels = \"a\", class = \"factor\")",
    ] {
        let src = format!(
            "`+.factor` <- function(a, b) \"ok\"\nf <- function(g) {{\n  {setup}\n  x <- a:g\n  x + \"a\"\n}}\n"
        );
        let diags = check(&src);
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "`{src}`: got {diags:?}"
        );
    }
    // A user-defined unary `-` can return a factor, so `(-1)` is no longer a
    // literal endpoint.
    let diags = check(
        "`-` <- function(x) factor(\"a\")\n`+.factor` <- function(a, b) \"ok\"\nf <- function(n) {\n  x <- (-1):n\n  x + \"a\"\n}\n",
    );
    assert!(diags.iter().all(|d| d.code != "RY040"), "got {diags:?}");
    // Without a mask, a negated literal is still a non-factor endpoint.
    let diags = check("f <- function(n) {\n  x <- (-1):n\n  x + \"a\"\n}\n");
    assert!(diags.iter().any(|d| d.code == "RY040"), "got {diags:?}");
}

#[test]
fn masked_unary_operator_result_is_unknown() {
    // A user-defined `-` replaces the primitive; its result need not be
    // numeric.
    let (_, scope) = check_with_scope("`-` <- function(x) \"s\"\nx <- -1\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Opaque, "got {x:?}");
}

#[test]
fn colon_literal_fast_path_stops_at_int_range() {
    // R: typeof(2147483647:2147483647) and typeof(-2147483647:0) are
    // "integer"; typeof(1:2147483648), typeof(-2147483648:0), and
    // typeof(2147483648:1) are "double".
    let (_, scope) = check_with_scope("x <- 2147483647:2147483647\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {x:?}");
    assert_eq!(x.length, Length::Known(1), "got {x:?}");
    let (_, scope) = check_with_scope("x <- -2147483647:0\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Integer, "got {x:?}");
    for src in ["x <- 1:2147483648\n", "x <- -2147483648:0\n"] {
        let (_, scope) = check_with_scope(src);
        let x = scope.get("x").expect("x should be bound");
        assert!(is_integer_or_double_union(x), "`{src}`: got {x:?}");
        assert_eq!(x.length, Length::Nonempty, "`{src}`: got {x:?}");
    }
    let (_, scope) = check_with_scope("x <- 2147483648:1\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(x.mode, Mode::Double, "got {x:?}");
}
