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
    // The tibble constructor is opaque to the current typeshed, so its
    // group_by result must not claim a precisely known class vector.
    assert!(scope.get("grouped").unwrap().class.is_unknown());
    let out = scope.get("out").unwrap();
    assert!(
        out.columns.as_ref().is_none_or(|schema| !schema.complete),
        "an unproved receiver cannot yield a complete result schema: {out:?}"
    );
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
            let (diagnostics, scope) = check_with_scope(&src);
            let out = scope.get("out").unwrap();
            assert!(out.columns.is_none(), "{source}; {verb}: {out:?}");
            assert!(out.class.is_unknown(), "{source}; {verb}: {out:?}");
            assert!(
                diagnostics.iter().all(|d| d.code != "RY060"),
                "{source}; {verb}: {diagnostics:?}"
            );
        }
    }
}

#[test]
fn custom_s3_rename_and_relocate_do_not_transfer_standard_column_types() {
    for verb in ["rename", "relocate"] {
        let src = format!(
            "{verb}.widget <- function(.data, ...) .data\n\
             d <- structure(data.frame(x = 'a', y = 1L), class = c('widget', 'data.frame'))\n\
             out <- dplyr::{verb}(d, y = x)\nvalue <- out$y + 1L\n"
        );
        let (diagnostics, scope) = check_with_scope(&src);
        let out = scope.get("out").unwrap();
        assert!(out.columns.is_none(), "{verb}: {out:?}");
        assert!(out.class.is_unknown(), "{verb}: {out:?}");
        assert!(
            diagnostics.iter().all(|d| d.code != "RY040"),
            "{verb}: {diagnostics:?}"
        );

        let standard = format!(
            "d <- data.frame(x = 'a', y = 1L)\n\
             out <- dplyr::{verb}(d, y = x)\nvalue <- out$y + 1L\n"
        );
        let (diagnostics, _) = check_with_scope(&standard);
        assert!(
            diagnostics.iter().any(|d| d.code == "RY040"),
            "standard {verb} still transfers the proved character type: {diagnostics:?}"
        );
    }
}

#[test]
fn custom_s3_join_does_not_claim_the_standard_join_schema() {
    let src = "left_join.widget <- function(x, y, ...) data.frame(y = 1L)\n\
               d <- structure(data.frame(x = 'a'), class = c('widget', 'data.frame'))\n\
               out <- dplyr::left_join(d, data.frame(x = 'a'), by = 'x')\n\
               value <- out$y + 1L\n";
    let (diagnostics, scope) = check_with_scope(src);
    let out = scope.get("out").unwrap();
    assert!(out.columns.is_none(), "{out:?}");
    assert!(out.class.is_unknown(), "{out:?}");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY060"),
        "{diagnostics:?}"
    );
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
fn unnamed_aggregate_outputs_are_retained_or_incomplete() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for verb in ["summarise", "reframe"] {
        let call = format!("dplyr::{verb}(d, x)");
        let (names, complete, diagnostics) =
            columns(&format!("{prefix}out <- {call}\nvalue <- out$x\n"), "out");
        assert_eq!(names, ["x"], "{call}: {diagnostics:?}");
        assert!(complete, "{call}: {diagnostics:?}");
        assert!(
            diagnostics.iter().all(|d| d.code != "RY060"),
            "{call}: {diagnostics:?}"
        );
    }
    for call in ["dplyr::summarise(d, extra)", "dplyr::reframe(d, extra)"] {
        let src = format!("{prefix}extra <- data.frame(y = 3L)\nout <- {call}\nvalue <- out$y\n");
        let (_, complete, diagnostics) = columns(&src, "out");
        assert!(!complete, "{call}: {diagnostics:?}");
        assert!(
            diagnostics.iter().all(|d| d.code != "RY060"),
            "{call}: {diagnostics:?}"
        );
    }
}

#[test]
fn mutate_keep_none_preserves_literal_by_columns() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for tag in [".by", "`.by`"] {
        let call = format!("dplyr::mutate(d, z = x + 1L, .keep = 'none', {tag} = g)");
        let (names, complete, diagnostics) =
            columns(&format!("{prefix}out <- {call}\nvalue <- out$g\n"), "out");
        assert_eq!(names, ["g", "z"], "{call}: {diagnostics:?}");
        assert!(complete, "{call}: {diagnostics:?}");
        assert!(
            diagnostics.iter().all(|d| d.code != "RY060"),
            "{call}: {diagnostics:?}"
        );
    }
    let (names, complete, _) = columns(
        &format!(
            "{prefix}out <- dplyr::mutate(d, z = x + 1L, .keep = 'none', .by = starts_with('g'))\n"
        ),
        "out",
    );
    assert_eq!(names, ["z"]);
    assert!(!complete);
}

#[test]
fn mutate_keep_none_retains_unnamed_source_columns() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for call in [
        "dplyr::mutate(d, x, .keep = 'none')",
        "dplyr::mutate(d, x, z = g + 1L, .keep = 'none')",
    ] {
        let src = format!("{prefix}out <- {call}\nvalue <- out$x\n");
        let (names, complete, diagnostics) = columns(&src, "out");
        assert!(names.contains(&"x".to_owned()), "{call}: {names:?}");
        assert!(complete, "{call}: {diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != "RY060" || !d.message.contains("`x`")),
            "{call}: {diagnostics:?}"
        );
    }
    let (names, complete, _) = columns(
        &format!("{prefix}out <- dplyr::mutate(d, x, x = NULL, .keep = 'none')\n"),
        "out",
    );
    assert!(names.is_empty());
    assert!(complete);
}

#[test]
fn unknown_add_keeps_prior_group_keys_uncertain() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n\
                  grouped <- dplyr::group_by(d, g)\n";
    for verb in [
        "dplyr::summarise(regrouped, n = dplyr::n())",
        "dplyr::transmute(regrouped, n = 1L)",
    ] {
        let src = format!(
            "{prefix}flag <- TRUE\nregrouped <- dplyr::group_by(grouped, .add = flag)\n\
             out <- {verb}\nvalue <- out$g\n"
        );
        let (diagnostics, scope) = check_with_scope(&src);
        let grouped = scope.get("regrouped").unwrap();
        let out = scope.get("out").unwrap();
        let grouped_complete = grouped
            .columns
            .as_ref()
            .is_some_and(|schema| schema.complete);
        let complete = out.columns.as_ref().is_some_and(|schema| schema.complete);
        assert!(!grouped_complete, "{verb}");
        assert!(!complete, "{verb}: {diagnostics:?}");
        assert!(grouped.class.is_unknown());
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code != "RY060" || !d.message.contains("`g`")),
            "{verb}: {diagnostics:?}"
        );
    }
    let literal = format!("{prefix}out <- dplyr::group_by(grouped, .add = FALSE)\n");
    let (_, complete, _) = columns(&literal, "out");
    assert!(complete, "a literal FALSE has a known grouping effect");
}

#[test]
fn by_selection_applies_order_and_empty_selection() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for (selection, expected) in [
        ("-x", vec!["g", "z"]),
        ("c(x, -x)", vec!["z"]),
        ("c(-x, x, -x)", vec!["g", "z"]),
    ] {
        let src = format!("{prefix}out <- dplyr::summarise(d, z = 1L, .by = {selection})\n");
        let (names, complete, diagnostics) = columns(&src, "out");
        assert_eq!(names, expected, "{selection}: {diagnostics:?}");
        assert!(complete, "{selection}: {diagnostics:?}");
    }
    for verb in ["summarise", "reframe"] {
        let src =
            format!("{prefix}out <- dplyr::{verb}(d, z = 1L, .by = c(-x, x))\nvalue <- out$x\n");
        let (names, complete, diagnostics) = columns(&src, "out");
        assert_eq!(names, ["g", "x", "z"], "{verb}: {diagnostics:?}");
        assert!(complete, "{verb}: {diagnostics:?}");
        let (_, scope) = check_with_scope(&src);
        assert_eq!(scope.get("value").unwrap().mode, Mode::Integer);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY060"),
            "{verb}: {diagnostics:?}"
        );
        let empty_src =
            format!("{prefix}out <- dplyr::{verb}(d, z = 1L, .by = c())\nvalue <- out$x\n");
        let (names, complete, diagnostics) = columns(&empty_src, "out");
        assert_eq!(names, ["z"], "{verb}");
        assert!(complete, "{verb}");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "RY060" && d.message.contains("`x`")),
            "{verb}: {diagnostics:?}"
        );
    }
    let (names, complete, _) = columns(
        &format!("{prefix}out <- dplyr::mutate(d, z = 1L, .keep = 'none', .by = c())\n"),
        "out",
    );
    assert_eq!(names, ["z"]);
    assert!(complete);
}

#[test]
fn nested_by_selection_keeps_result_schema_incomplete() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for selection in ["c(x, c(-x))", "c(c(), -x)"] {
        for call in [
            format!("dplyr::summarise(d, z = 1L, .by = {selection})"),
            format!("dplyr::mutate(d, z = 1L, .keep = 'none', .by = {selection})"),
        ] {
            let src = format!("{prefix}out <- {call}\nxread <- out$x\ngread <- out$g\n");
            let (_, complete, diagnostics) = columns(&src, "out");
            assert!(!complete, "{call}: {diagnostics:?}");
            assert!(
                diagnostics.iter().all(|d| d.code != "RY060"),
                "{call}: {diagnostics:?}"
            );
        }
    }
}

#[test]
fn control_tags_belong_to_the_specific_verb() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for (call, field) in [
        ("dplyr::summarise(d, .keep = 1L)", ".keep"),
        ("dplyr::reframe(d, .groups = 1L)", ".groups"),
        ("base::transform(d, .keep = 1L)", ".keep"),
    ] {
        let (names, complete, diagnostics) = columns(
            &format!("{prefix}out <- {call}\nvalue <- out${field}\n"),
            "out",
        );
        assert!(names.contains(&field.to_owned()), "{call}: {names:?}");
        assert!(complete, "{call}: {diagnostics:?}");
        assert!(
            diagnostics.iter().all(|d| d.code != "RY060"),
            "{call}: {diagnostics:?}"
        );
    }
}

#[test]
fn quoted_by_tag_is_a_control_in_aggregate() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for verb in ["summarise", "reframe"] {
        let call = format!("dplyr::{verb}(d, z = 1L, `.by` = g)");
        let (names, complete, diagnostics) =
            columns(&format!("{prefix}out <- {call}\nvalue <- out$g\n"), "out");
        assert_eq!(names, ["g", "z"], "{call}: {diagnostics:?}");
        assert!(complete, "{call}: {diagnostics:?}");
        assert!(
            diagnostics.iter().all(|d| d.code != "RY060"),
            "{call}: {diagnostics:?}"
        );
    }
    let (_, complete, diagnostics) = columns(
        r#"d <- data.frame(x = 1L, g = 2L)
out <- dplyr::summarise(d, z = 1L, `.\u0062y` = g)
value <- out$g
"#,
        "out",
    );
    assert!(
        !complete,
        "encoded tag must not prove absence: {diagnostics:?}"
    );
}

#[test]
fn transmute_null_deletes_previous_literal_outputs() {
    let prefix = "d <- data.frame(x = 1L, g = 2L)\n";
    for call in [
        "dplyr::transmute(d, x = 2L, x = NULL)",
        "dplyr::transmute(d, x, x = NULL)",
    ] {
        let (names, complete, diagnostics) =
            columns(&format!("{prefix}out <- {call}\nvalue <- out$x\n"), "out");
        assert!(names.is_empty(), "{call}: {names:?}");
        assert!(complete, "{call}: {diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == "RY060" && d.message.contains("`x`")),
            "{call}: {diagnostics:?}"
        );
    }
    let (_, complete, _) = columns(
        &format!("{prefix}out <- dplyr::transmute(d, .keep = 1L)\n"),
        "out",
    );
    assert!(!complete, "transmute rejects the mutate-only .keep control");
}

#[test]
fn simultaneous_renames_use_original_column_identities() {
    let prefix = "d <- data.frame(x = 1L, y = 'a', g = 2L)\n";
    for verb in ["rename", "relocate"] {
        let src = format!(
            "{prefix}out <- dplyr::{verb}(d, y = x, z = y)\ny_read <- out$y\nz_read <- out$z\n"
        );
        let (names, complete, diagnostics) = columns(&src, "out");
        assert_eq!(names, ["y", "z", "g"], "{verb}: {diagnostics:?}");
        assert!(complete, "{verb}: {diagnostics:?}");
        let (_, scope) = check_with_scope(&src);
        assert_eq!(scope.get("y_read").unwrap().mode, Mode::Integer, "{verb}");
        assert_eq!(scope.get("z_read").unwrap().mode, Mode::Character, "{verb}");
    }
    let (names, complete, _) = columns(&format!("{prefix}out <- dplyr::relocate(d, g)\n"), "out");
    assert_eq!(names, ["g", "x", "y"]);
    assert!(complete);
}

#[test]
fn group_by_class_is_full_when_receiver_is_proved() {
    let src = "d <- data.frame(x = 1L)\nout <- dplyr::group_by(d, x)\n";
    let (_, scope) = check_with_scope(src);
    assert_eq!(
        scope.get("out").unwrap().class,
        ClassVector::from_slice(&["grouped_df", "tbl_df", "tbl", "data.frame"])
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
