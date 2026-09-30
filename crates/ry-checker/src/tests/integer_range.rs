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
