use super::*;

fn columns(src: &str, binding: &str) -> (Vec<String>, bool, Vec<Diagnostic>) {
    let (diagnostics, scope) = check_with_scope(src);
    let schema = scope
        .get(binding)
        .unwrap_or_else(|| panic!("missing binding {binding}"))
        .columns
        .as_ref()
        .unwrap_or_else(|| panic!("missing schema for {binding}"))
        .clone();
    (
        schema
            .columns
            .iter()
            .map(|(name, _)| name.clone())
            .collect(),
        schema.complete,
        diagnostics,
    )
}

#[test]
fn pinned_group_by_computation_creates_a_column_before_access() {
    // tidyverse/dplyr d5e94e7, tests/testthat/test-group-by.R:17-44.
    let src = "d <- data.frame(x = 1:4, g = rep(1:2, each = 2))\n\
               out <- d |> dplyr::group_by(g) |> dplyr::group_by(big = x > mean(x), .add = TRUE)\n\
               valid <- out$big\ninvalid <- out$missing\n";
    let (names, complete, diagnostics) = columns(src, "out");
    assert_eq!(names, ["x", "g", "big"]);
    assert!(complete);
    assert!(
        diagnostics
            .iter()
            .all(|d| { d.code != "RY060" || !d.message.contains("`big`") }),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| { d.code == "RY060" && d.message.contains("`missing`") }),
        "{diagnostics:?}"
    );
}

#[test]
fn literal_verb_effects_create_drop_and_rename_columns() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for (call, expected) in [
        ("dplyr::mutate(d, new = x + 1L)", vec!["x", "g", "new"]),
        ("dplyr::mutate(d, x = NULL)", vec!["g"]),
        (
            "dplyr::mutate(d, new = x + 1L, .keep = 'none')",
            vec!["new"],
        ),
        ("dplyr::transmute(d, new = x + 1L)", vec!["new"]),
        ("dplyr::transmute(d, x)", vec!["x"]),
        ("dplyr::rename(d, new = x)", vec!["new", "g"]),
        ("dplyr::relocate(d, new = x)", vec!["new", "g"]),
        ("dplyr::select(d, new = x)", vec!["new"]),
    ] {
        let (names, complete, diagnostics) = columns(&format!("{prefix}out <- {call}\n"), "out");
        assert_eq!(names, expected, "{call}: {diagnostics:?}");
        assert!(complete, "{call}: {diagnostics:?}");
    }
}

#[test]
fn summarise_by_keeps_only_literal_group_and_summary_columns() {
    let src = "d <- data.frame(x = 1:4, g = c(1L, 1L, 2L, 2L))\n\
               out <- dplyr::summarise(d, n = dplyr::n(), .by = g)\n\
               group <- out$g\nsummary <- out$n\nmissing <- out$x\n";
    let (names, complete, diagnostics) = columns(src, "out");
    assert_eq!(names, ["g", "n"]);
    assert!(complete);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "RY060" && d.message.contains("`x`")),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code != "RY060" || d.message.contains("`x`")),
        "{diagnostics:?}"
    );
}

#[test]
fn aggregate_after_group_by_does_not_prove_group_keys_absent() {
    // dplyr d5e94e7, tests/testthat/test-reframe.R:79,312. Group keys
    // survive reframe, but the current type lattice does not store their
    // exact names; keep the result schema incomplete rather than flagging g.
    let src = "d <- tibble::tibble(g = 1:2, x = 1:2)\n\
               grouped <- dplyr::group_by(d, g)\n\
               out <- dplyr::reframe(grouped, y = mean(x))\n\
               value <- out$g\n";
    let (diagnostics, scope) = check_with_scope(src);
    assert!(scope.get("grouped").unwrap().class.contains("grouped_df"));
    let schema = scope.get("out").unwrap().columns.as_ref().unwrap().clone();
    assert!(!schema.complete);
    assert!(
        diagnostics.iter().all(|d| d.code != "RY060"),
        "{diagnostics:?}"
    );
}

#[test]
fn dynamic_writes_and_selections_do_not_claim_complete_schemas() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for call in [
        "dplyr::mutate(d, dplyr::across(dplyr::everything(), identity))",
        "dplyr::transmute(d, dplyr::across(dplyr::everything(), identity))",
        "dplyr::select(d, dplyr::starts_with('x'))",
        "dplyr::rename(d, new = tidyselect::all_of('x'))",
        "dplyr::relocate(d, dplyr::starts_with('x'))",
        "dplyr::mutate(d, new = x + 1L, .keep = 'used')",
    ] {
        let src = format!("{prefix}out <- {call}\nvalue <- out$not_proven_missing\n");
        let (_, complete, diagnostics) = columns(&src, "out");
        assert!(!complete, "{call}: {diagnostics:?}");
        assert!(
            diagnostics.iter().all(|d| d.code != "RY060"),
            "{call}: {diagnostics:?}"
        );
    }
}

#[test]
fn unknown_and_custom_s3_receivers_do_not_prove_missing_result_fields() {
    for source in [
        "d <- unknown_frame()",
        "d <- structure(data.frame(x = 1L), class = c('lazy_dt', 'data.frame'))",
    ] {
        for verb in [
            "dplyr::summarise(d, n = 1L)",
            "dplyr::mutate(d, n = 1L)",
            "dplyr::group_by(d, n = 1L)",
        ] {
            let src = format!("{source}\nout <- {verb}\nvalue <- out$method_field\n");
            let (_, complete, diagnostics) = columns(&src, "out");
            assert!(!complete, "{source}; {verb}: {diagnostics:?}");
            assert!(
                diagnostics.iter().all(|d| d.code != "RY060"),
                "{source}; {verb}: {diagnostics:?}"
            );
        }
    }
}

#[test]
fn forwarded_ellipsis_does_not_prove_aggregate_columns() {
    let src =
        "d <- data.frame(x = 1L)\nout <- dplyr::reframe(d, ...)\nvalue <- out$dynamic_field\n";
    let (_, complete, diagnostics) = columns(src, "out");
    assert!(!complete, "{diagnostics:?}");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY060"),
        "{diagnostics:?}"
    );
}

#[test]
fn qualified_and_attached_dplyr_keep_schema_resolution_distinct() {
    for call in [
        "dplyr::group_by(d, big = x > 0)",
        "group_by(d, big = x > 0)",
    ] {
        let attach = if call.starts_with("dplyr::") {
            ""
        } else {
            "library(dplyr)\n"
        };
        let (names, complete, diagnostics) = columns(
            &format!("{attach}d <- data.frame(x = 1L)\nout <- {call}\n"),
            "out",
        );
        assert_eq!(names, ["x", "big"], "{call}: {diagnostics:?}");
        assert!(complete);
    }
    let (names, complete, _) = columns(
        "d <- data.frame(x = 1L)\nout <- base::transform(d, y = x + 1L)\n",
        "out",
    );
    assert_eq!(names, ["x", "y"]);
    assert!(complete);
    let (names, complete, _) = columns(
        "d <- data.frame(x = 1L)\nout <- base::transform(d, .add = x + 1L)\n",
        "out",
    );
    assert_eq!(names, ["x", ".add"]);
    assert!(complete);
}
