use super::*;

#[test]
fn detects_char_plus_int() {
    let diags = check(r#""a" + 1L"#);
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040, got {:?}",
        diags
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
        (
            "invalid-union `if` condition",
            "x <- if (runif(1) > 0.5) 1L else \"a\"\nif (x) print(1)",
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

#[test]
fn detects_unbound_var() {
    let diags = check("y <- undefined_thing\n");
    assert!(diags.iter().any(|d| d.code == "RY010"));
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
