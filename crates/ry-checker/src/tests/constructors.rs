use super::*;

#[test]
fn vector_sizes_use_values_and_r_argument_matching() {
    for (name, mode) in [
        ("integer", Mode::Integer),
        ("numeric", Mode::Double),
        ("double", Mode::Double),
        ("logical", Mode::Logical),
        ("character", Mode::Character),
        ("raw", Mode::Raw),
    ] {
        for (argument, length) in [
            ("", Length::Zero),
            ("0", Length::Zero),
            ("1", Length::One),
            ("10", Length::Known(10)),
            ("len = 3", Length::Known(3)),
            ("length = 2.9", Length::Known(2)),
            ("-0.2", Length::Zero),
            ("-2", Length::Unknown),
            ("NA_real_", Length::Unknown),
            ("Inf", Length::Unknown),
            ("size", Length::Unknown),
        ] {
            for callee in [name.to_string(), format!("base::{name}")] {
                let (_, scope) =
                    check_with_scope(&format!("size <- readLines()\nx <- {callee}({argument})"));
                assert_eq!(
                    scope.get("x"),
                    Some(&RType::new(mode, length)),
                    "{callee}({argument})"
                );
            }
        }
    }
    for (arguments, mode, length) in [
        ("", Mode::Logical, Length::Zero),
        ("length = 3", Mode::Logical, Length::Known(3)),
        ("length = 2, 'integer'", Mode::Integer, Length::Known(2)),
        ("len = 2, mo = 'double'", Mode::Double, Length::Known(2)),
    ] {
        let (diags, scope) = check_with_scope(&format!("x <- vector({arguments})"));
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(scope.get("x"), Some(&RType::new(mode, length)));
    }
}

#[test]
fn local_constructors_override_base_metadata() {
    let (diags, scope) = check_with_scope("numeric <- function(length) 'local'\nx <- numeric(10)");
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(scope.get("x"), Some(&RType::scalar(Mode::Character)));
}

#[test]
fn lengths_overlap_respects_lattice_order_for_assertions() {
    // Nonempty (e.g. after `length(x) > 0` or `unique(...)`) overlaps `One`:
    // standalone rlang scalar assertions must neither emit RY092 nor mark
    // following code unreachable (#923).
    let src = "check_string <- function(x, arg = NULL, call = NULL) invisible(NULL)\n\
               f <- function(x) {\n\
                 if (is.character(x) && length(x) > 0L) {\n\
                   check_string(x)\n\
                   y <- 1 + 'a'\n\
                 }\n\
               }\n";
    let stub = r#"{
        "schema_version": "2",
        "package": "rlang",
        "version": "0.0.1",
        "functions": {
            "check_string": {
                "params": ["x", "arg", "call"],
                "return": {"mode": "null", "length": "0"},
                "assertion": {
                    "subject_param": "x",
                    "target": {"mode": "character", "length": "1"},
                    "provenance": {"kind": "standalone_types_check", "fingerprint_params": ["arg", "call"]}
                }
            }
        }
    }"#;
    let (diags, _) = check_with_stubs(
        "f <- function(x) {\n\
           if (is.character(x) && length(x) > 0L) {\n\
             rlang::check_string(x)\n\
             y <- 1 + 'a'\n\
           }\n\
         }\n",
        &[("rlang.json", stub)],
    );
    assert!(
        !diags.iter().any(|d| d.code == "RY092"),
        "Nonempty must overlap One: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "code after check_string(nonempty) must remain reachable: {diags:?}"
    );

    // Control: proven length >= 2 still does NOT overlap One (RY092 stays).
    let (bad_diags, _) = check_with_stubs(
        "f <- function(x) {\n\
           if (is.character(x) && length(x) > 1L) {\n\
             rlang::check_string(x)\n\
           }\n\
         }\n",
        &[("rlang.json", stub)],
    );
    assert!(
        bad_diags.iter().any(|d| d.code == "RY092"),
        "AtLeast(2) must not overlap One: {bad_diags:?}"
    );
    let _ = src;
}

#[test]
fn longest_arg_length_preserves_zero_and_lower_bounds() {
    use crate::infer::longest_arg_length;

    let zero = RType::new(Mode::Integer, Length::Zero);
    let one = RType::new(Mode::Integer, Length::One);
    let three = RType::new(Mode::Integer, Length::Known(3));
    let nonempty = RType::new(Mode::Integer, Length::Nonempty);
    let unknown = RType::new(Mode::Integer, Length::Unknown);

    assert_eq!(
        longest_arg_length(std::slice::from_ref(&zero)),
        Length::Zero
    );
    assert_eq!(
        longest_arg_length(&[zero.clone(), three.clone()]),
        Length::Known(3)
    );
    assert_eq!(
        longest_arg_length(&[three.clone(), unknown.clone()]),
        Length::AtLeast(3)
    );
    assert_eq!(
        longest_arg_length(&[three.clone(), nonempty.clone()]),
        Length::AtLeast(3)
    );
    assert_eq!(
        longest_arg_length(&[one.clone(), unknown.clone()]),
        Length::Nonempty
    );
    assert_eq!(longest_arg_length(&[zero, unknown]), Length::Unknown);

    let (diags, scope) = check_with_scope("df <- data.frame(a = integer(0))\nx <- df$a\n");
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(scope.get("x").unwrap().length, Length::Zero);
}

#[test]
fn transpose_data_frame_returns_unclassed_matrix_not_data_frame() {
    let (diags, scope) = check_with_scope(
        "df <- data.frame(a = 1, b = 2L)\n\
         m <- t(df)\n\
         ok <- m + 1\n",
    );
    assert!(
        diags.is_empty(),
        "t(df) + 1 must not trigger RY040: {diags:?}"
    );
    let m = scope.get("m").unwrap();
    assert_eq!(m.mode, Mode::Double);
    assert!(m.class.is_classless());
    assert!(m.columns.is_none());

    // `$` on t(df) must report RY061 (atomic vector).
    let dollar_diags = check("df <- data.frame(a = 1)\nt(df)$a\n");
    assert!(
        dollar_diags.iter().any(|d| d.code == "RY061"),
        "t(df)$a must report RY061: {dollar_diags:?}"
    );
}

#[test]
fn data_frame_named_atomic_and_incomplete_schema_lengths() {
    // 1-arg named atomic vector creates a 1-column data frame, not 2 columns (#923).
    let (diags, scope) = check_with_scope("df <- data.frame(c(a = 1, b = 2))\n");
    assert!(diags.is_empty(), "{diags:?}");
    let df = scope.get("df").unwrap();
    assert_eq!(df.length, Length::Known(1));
    assert!(
        df.columns
            .as_ref()
            .is_none_or(|cols| cols.get("a").is_none()),
        "named atomic vector must not become columns a/b: {df:?}"
    );

    // Incomplete schema passed to data.frame()/as.data.frame() keeps lower bound,
    // not an exact column count.
    let (diags2, scope2) = check_with_scope(
        "f <- function(extra) {\n\
           l <- list(a = 1)\n\
           l[[extra]] <- 2\n\
           d1 <- data.frame(l)\n\
           d2 <- as.data.frame(l)\n\
           list(d1, d2)\n\
         }\n\
         l <- list(a = 1)\n\
         k <- readLines()\n\
         l[[k]] <- 2\n\
         d1 <- data.frame(l)\n\
         d2 <- as.data.frame(l)\n",
    );
    assert!(diags2.is_empty(), "{diags2:?}");
    assert_ne!(scope2.get("d1").unwrap().length, Length::Known(1));
    assert_ne!(scope2.get("d2").unwrap().length, Length::Known(1));
}

#[test]
fn constructor_and_trycatch_provenance_and_user_stub_override_precedence() {
    // Qualified external package `otherpkg::t(x)` / `otherpkg::list(x)` must
    // not be hijacked as base constructors (#924).
    let other_stub = r#"{
        "schema_version": "2",
        "package": "otherpkg",
        "version": "0.0.1",
        "functions": {
            "t": {"params": ["x"], "return": {"mode": "character", "length": "1"}},
            "list": {"params": ["..."], "return": {"mode": "integer", "length": "1"}}
        }
    }"#;
    let (diags, scope) = check_with_stubs(
        "a <- otherpkg::t(1L)\nb <- otherpkg::list('x', 'y')\n",
        &[("otherpkg.json", other_stub)],
    );
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(scope.get("a").unwrap().mode, Mode::Character);
    assert_eq!(scope.get("b").unwrap().mode, Mode::Integer);

    // S3 method `t.foo` must win over base `t` (#924).
    let (diags_s3, scope_s3) = check_with_scope(
        "t.foo <- function(x) 'transposed'\n\
         x <- structure(1L, class = 'foo')\n\
         y <- t(x)\n",
    );
    assert!(diags_s3.is_empty(), "{diags_s3:?}");
    assert_eq!(scope_s3.get("y").unwrap().mode, Mode::Character);

    // Project-level `t <- function(...)` must win over base `t` (#924).
    let (diags_fn, scope_fn) = check_with_scope("t <- function(x) 'helper'\ny <- t(1L)\n");
    assert!(diags_fn.is_empty(), "{diags_fn:?}");
    assert_eq!(scope_fn.get("y").unwrap().mode, Mode::Character);

    // `base::tryCatch` infers the join, while shadowed `tryCatch` uses the user fn (#924).
    let (diags_tc, scope_tc) =
        check_with_scope("x <- base::tryCatch(1L, error = function(e) 2L)\n");
    assert!(diags_tc.is_empty(), "{diags_tc:?}");
    assert_eq!(scope_tc.get("x").unwrap().mode, Mode::Integer);

    let (diags_tc_shadow, scope_tc_shadow) = check_with_scope(
        "tryCatch <- function(expr, ...) 'shadowed'\n\
         x <- tryCatch(1L, error = function(e) 2L)\n",
    );
    assert!(diags_tc_shadow.is_empty(), "{diags_tc_shadow:?}");
    assert_eq!(scope_tc_shadow.get("x").unwrap().mode, Mode::Character);

    // User stub for a standard package (`stats`) wins over embedded base DB
    // on qualified calls (`stats::median`) (#924).
    let stats_stub = r#"{
        "schema_version": "2",
        "package": "stats",
        "version": "0.0.1",
        "functions": {
            "median": {"params": ["x"], "return": {"mode": "character", "length": "1"}}
        }
    }"#;
    let (diags_stats, scope_stats) =
        check_with_stubs("x <- stats::median(1:3)\n", &[("stats.json", stats_stub)]);
    assert!(diags_stats.is_empty(), "{diags_stats:?}");
    assert_eq!(scope_stats.get("x").unwrap().mode, Mode::Character);
}
