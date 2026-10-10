use super::*;
use ry_core::types::NewNaProvenance;

fn range_warnings(source: &str) -> usize {
    check(source).iter().filter(|d| d.code == "RY119").count()
}

#[test]
fn exact_integer_range_and_truncation() {
    for value in ["2147483648", "-2147483648", "1e10", "Inf", "-Inf"] {
        assert_eq!(
            range_warnings(&format!("as.integer({value})")),
            1,
            "{value}"
        );
    }
    for value in [
        "2147483647",
        "-2147483647",
        "2147483647.9",
        "-2147483647.9",
        "NaN",
        "NA_real_",
    ] {
        assert_eq!(
            range_warnings(&format!("as.integer({value})")),
            0,
            "{value}"
        );
    }
}

#[test]
fn assignment_and_vector_preserve_new_na_provenance() {
    let (diags, scope) = check_with_scope("x <- c(0, 1e10)\ny <- as.integer(x)");
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY119").count(),
        1,
        "{diags:?}"
    );
    assert_eq!(
        scope.get("y").unwrap().coercion_new_na(),
        NewNaProvenance::ProvenContains
    );
    let (diags, scope) = check_with_scope("x <- as.integer(1e10)");
    assert_eq!(diags.iter().filter(|d| d.code == "RY119").count(), 1);
    assert_eq!(
        scope.get("x").unwrap().coercion_new_na(),
        NewNaProvenance::ProvenOnly
    );
    let (diags, scope) = check_with_scope("x <- as.integer(1e10)\ny <- as.integer(x)");
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY119").count(),
        1,
        "{diags:?}"
    );
    assert_eq!(
        scope.get("y").unwrap().coercion_new_na(),
        NewNaProvenance::ProvenOnly
    );
    let (_, scope) = check_with_scope("f <- function() as.integer(1e10)\ny <- f()");
    assert_eq!(
        scope.get("y").unwrap().coercion_new_na(),
        NewNaProvenance::ProvenOnly
    );
    assert_eq!(
        range_warnings("limits <- c(0, 1e10)\nlimits[is.na(limits)] <- -1L\nas.integer(limits)"),
        1
    );
    assert_eq!(range_warnings("x <- 1e10\nx[1] <- 0\nas.integer(x)"), 0);
    assert_eq!(range_warnings("x <- c(NA_real_, 1e10)\nas.integer(x)"), 1);
}

#[test]
fn unknown_values_and_shadowed_casts_stay_quiet() {
    for source in [
        "x <- scan(); as.integer(x)",
        "as.integer <- function(x) x; as.integer(1e10)",
        "as.integer(structure(1e10, class = 'special'))",
        "c.special <- function(...) 1; x <- structure(1e10, class = 'special'); as.integer(c(x))",
    ] {
        assert_eq!(range_warnings(source), 0, "{source}");
    }
    let (_, scope) = check_with_scope("x <- scan()\ny <- as.integer(x)");
    assert_eq!(
        scope.get("y").unwrap().coercion_new_na(),
        NewNaProvenance::Possible
    );
    let (_, scope) = check_with_scope("y <- as.integer(NA_real_)");
    assert_eq!(
        scope.get("y").unwrap().coercion_new_na(),
        NewNaProvenance::None
    );
    assert!(scope.get("y").unwrap().value_facts.prior_na);
}

#[test]
fn immediate_new_na_repair_suppresses_cast_warning_and_clears_fact() {
    let (diags, scope) = check_with_scope("x <- as.integer(1e10)\nx[is.na(x)] <- 0L");
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY119").count(),
        0,
        "{diags:?}"
    );
    assert_eq!(
        scope.get("x").unwrap().coercion_new_na(),
        NewNaProvenance::None
    );
    assert_eq!(
        range_warnings(
            "x <- as.integer(1e10) # observed warning\n# repair before use\nx[is.na(x)] <- 0L"
        ),
        0
    );
    assert_eq!(
        range_warnings("x <- as.integer(1e10)\nprint(x)\nx[is.na(x)] <- 0L"),
        1
    );
    assert_eq!(
        range_warnings("x <- as.integer(1e10)\ny <- x\ny[is.na(y)] <- 0L"),
        1
    );
    assert_eq!(
        range_warnings("x <- as.integer(1e10)\nis.na <- function(x) FALSE\nx[is.na(x)] <- 0L"),
        1
    );
}

#[test]
fn control_flow_join_keeps_only_supported_na_claims() {
    let (_, scope) =
        check_with_scope("flag <- scan()\ny <- if (flag) as.integer(1e10) else as.integer(2e10)");
    assert_eq!(
        scope.get("y").unwrap().coercion_new_na(),
        NewNaProvenance::ProvenOnly
    );
    let (_, scope) =
        check_with_scope("flag <- scan()\ny <- if (flag) as.integer(1e10) else as.integer(1)");
    assert_eq!(
        scope.get("y").unwrap().coercion_new_na(),
        NewNaProvenance::Possible
    );
    let (_, scope) =
        check_with_scope("flag <- scan()\ny <- if (flag) as.integer(1e10) else NA_real_");
    assert_eq!(
        scope.get("y").unwrap().coercion_new_na(),
        NewNaProvenance::Possible
    );
}

#[test]
fn numeric_replacement_infers_index_expressions() {
    for values in ["c(1, 2)", "c(1L, 2L)", "c(TRUE, FALSE)"] {
        for target in ["x[yy]", "x[[yy]]", "x[1, yy]"] {
            let source = format!("x <- {values}; {target} <- 1");
            let diags = check(&source);
            assert!(
                diags.iter().any(|d| d.code == "RY010"),
                "{source}: {diags:?}"
            );
        }
    }
}

#[test]
fn missing_replacement_updates_bounds_and_numeric_mode() {
    for source in [
        "x <- c(5, NA_real_); x[is.na(x)] <- 1e10; as.integer(x)",
        "x <- as.integer(1e10)\nx[is.na(x)] <- 1e10\nas.integer(x)",
        "x <- c(5L, NA_integer_); x[is.na(x)] <- 1e10; as.integer(x)",
        "x <- 1e10; x[is.na(x)] <- numeric(1); as.integer(x)",
    ] {
        assert_eq!(range_warnings(source), 1, "{source}");
    }
    for source in [
        "x <- c(5, 6); x[is.na(x)] <- 1e10; as.integer(x)",
        "x <- as.integer(structure(5, class = 'Date')); x[is.na(x)] <- 1e10; as.integer(x)",
        "x <- numeric(0); x[is.na(x)] <- 1e10; as.integer(x)",
        "x <- c(5, NA_real_); x[is.na(x)] <- 0L; as.integer(x)",
        "x <- c(5, NA_real_); x[is.na(x)] <- scan(); as.integer(x)",
        "x <- c(5, NA_real_); x[is.na(x)] <- c(0, 1e10); as.integer(x)",
        "flag <- scan(); x <- if (flag) c(5, NA_real_) else c(5, 5); x[is.na(x)] <- 1e10; as.integer(x)",
    ] {
        assert_eq!(range_warnings(source), 0, "{source}");
    }
    let (_, scope) = check_with_scope("x <- as.integer(1e10); x[is.na(x)] <- 1e10");
    assert_eq!(scope.get("x").unwrap().mode, Mode::Double);
    let (_, scope) = check_with_scope("x <- c(5L, NA_integer_); x[is.na(x)] <- numeric(1)");
    assert_eq!(scope.get("x").unwrap().mode, Mode::Double);
}

#[test]
fn list_homogeneity_ignores_values_and_merges_their_facts() {
    for values in ["list(list(1), list(2))", "list(list(1), list(1))"] {
        let source = format!("for (x in {values}) if (x) 1");
        let diags = check(&source);
        assert!(
            diags.iter().any(|d| d.code == "RY001"),
            "{source}: {diags:?}"
        );
    }
    assert_eq!(range_warnings("for (x in list(1, 1e10)) as.integer(x)"), 0);
}

#[test]
fn missing_replacement_requires_base_operator_and_nonempty_plain_value() {
    for source in [
        "`[<-` <- function(x, i, value) x\nx <- as.integer(1e10)\nx[is.na(x)] <- 0L",
        "x <- as.integer(1e10)\nx[is.na(x)] <- numeric(0)",
        "x <- as.integer(1e10)\nx[is.na(x)] <- structure(0L, class = 'special')",
        "x <- as.integer(1e10)\nx[is.na(wrong = x)] <- 0L",
    ] {
        assert_eq!(range_warnings(source), 1, "{source}");
    }
    assert_eq!(
        range_warnings("x <- as.integer(1e10); x[is.na(x)] <- 1e10; as.integer(x)"),
        2
    );
    assert_eq!(
        range_warnings("x <- as.integer(1e10)\nx[is.na(x)] <- c(0L, 1L)"),
        0
    );
}

#[test]
fn immediate_repair_suppresses_only_the_direct_cast() {
    let source = "x <- as.integer({\n  as.integer(1e10)\n  1e10\n})\nx[is.na(x)] <- 0L";
    let diags = check(source);
    let warnings: Vec<_> = diags.iter().filter(|d| d.code == "RY119").collect();
    assert_eq!(warnings.len(), 1, "{diags:?}");
    assert_eq!(
        source.get(warnings[0].span.start..warnings[0].span.end),
        Some("as.integer(1e10)")
    );
    for source in [
        "x <- as.integer(1e10)\nx[is.na(x)] <- 0L",
        "x <- (as.integer(1e10))\nx[is.na(x)] <- 0L",
        "x <- (as.integer(1e10) # handled\n)\nx[is.na(x)] <- 0L",
    ] {
        assert_eq!(range_warnings(source), 0, "{source}");
    }
    assert_eq!(
        range_warnings("y <- as.integer(1e10)\nx <- c(y, 1e10)\nas.integer(x)"),
        2
    );
}

#[test]
fn subsets_do_not_keep_whole_vector_extremes() {
    // `x[2]` is 1, which `as.integer()` keeps, although `x` holds 1e10.
    for source in [
        "x <- c(1e10, 1)\nas.integer(x[2])\n",
        "x <- c(1e10, 1)\nas.integer(x[-1])\n",
        "x <- c(1e10, 1)\nas.integer(x[[2]])\n",
        "d <- data.frame(x = c(1e10, 1))\nas.integer(d[2, 1])\n",
        "d <- data.frame(x = c(1e10, 1))\nas.integer(d[2, , drop = FALSE]$x)\n",
    ] {
        assert_eq!(range_warnings(source), 0, "{source}");
    }
    assert_eq!(range_warnings("x <- c(1e10, 1e11)\nas.integer(x)\n"), 1);
}
