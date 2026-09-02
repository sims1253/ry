use super::*;

#[test]
fn package_loading_calls_have_distinct_return_types() {
    let (diags, scope) = check_with_scope(
        "attached <- library(stats)\navailable <- require(stats)\nnamespaced <- requireNamespace(\"stats\")\n",
    );
    assert!(diags.is_empty(), "{diags:?}");

    let attached = scope.get("attached").expect("attached should be bound");
    assert_eq!(attached.mode, Mode::Null);
    assert_eq!(attached.length, Length::Zero);

    let available = scope.get("available").expect("available should be bound");
    assert_eq!(available.mode, Mode::Logical);
    assert_eq!(available.length, Length::One);

    let namespaced = scope.get("namespaced").expect("namespaced should be bound");
    assert_eq!(namespaced.mode, Mode::Logical);
    assert_eq!(namespaced.length, Length::One);
}

#[test]
fn user_function_argument_rules_wait_for_callable_provenance() {
    let file = parse_file(
        "project.R",
        "f <- function(required) required\nf()\nc <- function(x) x\nc(unrelated = 1L)\n",
    );
    let mut project = Project::new();
    project.add_file("project.R".to_string(), file);
    let diags: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        diags
            .iter()
            .all(|diagnostic| diagnostic.code != "RY090" && diagnostic.code != "RY091"),
        "project-wide function names are not sufficient to validate a call: {diags:?}"
    );
}

#[test]
fn typeshed_required_arguments_are_still_checked() {
    let diags = check("Filter()\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY091"),
        "explicit typeshed required metadata should remain authoritative: {diags:?}"
    );
}

#[test]
fn classed_and_null_generic_arguments_do_not_report_type_mismatches() {
    let diags =
        check("x <- structure(list(value = 1L), class = \"custom\")\nround(x)\nlog(NULL)\n");
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY092"),
        "classed values may dispatch and numeric generics accept NULL: {diags:?}"
    );
}

#[test]
fn plain_character_numeric_generic_argument_still_reports_mismatch() {
    let diags = check("log(\"not numeric\")\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY092"),
        "a plain character value cannot use numeric generic dispatch: {diags:?}"
    );
}

#[test]
fn quoted_dsl_metadata_suppresses_only_captured_symbols() {
    let diags = check(
        "library(dplyr)\nspec <- join_by(left_id == right_id)\nmissing_after\nlibrary(igraph)\ng <- graph_from_literal(A - B, B - C)\n",
    );
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("left_id")
                    && !diagnostic.message.contains("right_id")
                    && !diagnostic.message.contains("`A`")
                    && !diagnostic.message.contains("`B`")
                    && !diagnostic.message.contains("`C`"))
        }),
        "quoted DSL symbols must not be resolved lexically: {diags:?}"
    );
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("missing_after")
        }),
        "ordinary lexical reads must remain checked: {diags:?}"
    );
}

#[test]
fn expanded_dplyr_metadata_resolves_masks_and_selectors() {
    let diags = check(
        "library(dplyr)\ndf <- data.frame(a = 1L, b = 2L)\ndistinct(df, a)\npull(df, b)\nrelocate(df, b, .before = a)\nslice_min(df, order_by = b)\nmutate(df, picked = pick(a, b))\n",
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "dplyr masks and selectors should resolve known columns: {diags:?}"
    );
}

#[test]
fn expanded_tidyr_metadata_resolves_captured_columns() {
    let diags = check(
        "library(tidyr)\ndf <- data.frame(a = 1L, b = 2L)\ngather(df, key, value, a, b)\nchop(df, a)\ncomplete(df, a)\nnest(df, nested = c(a, b))\nunnest(df, nested)\nunite(df, combined, a, b)\n",
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "tidyr captured column arguments should not be resolved lexically: {diags:?}"
    );
}

#[test]
fn recipes_metadata_resolves_selectors_and_masked_expressions() {
    let diags = check(
        "library(recipes)\nr <- data.frame(a = 1L, b = 2L, outcome = 3L)\nstep_center(r, a, b)\nstep_pls(r, a, outcome = outcome)\nstep_mutate(r, total = a + b)\nimp_vars(quoted_predictor)\nmissing_after\n",
    );
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("`a`")
                    && !diagnostic.message.contains("`b`")
                    && !diagnostic.message.contains("`outcome`")
                    && !diagnostic.message.contains("quoted_predictor"))
        }),
        "recipes selectors and expressions are captured, not lexical reads: {diags:?}"
    );
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("missing_after")
        }),
        "ordinary reads outside recipes calls must remain checked: {diags:?}"
    );
}

#[test]
fn standard_r_inventory_resolves_default_package_symbols() {
    let diags = check(
        "family <- binomial\ndataset <- WWWusage\nhandler <- conditionMessage\nconverter <- as.name\nmaximum <- which.max\n",
    );
    assert!(
        diags.is_empty(),
        "standard inventory symbols (functions and datasets) resolve silently: {diags:?}"
    );
}

#[test]
fn standard_inventory_does_not_override_precise_types() {
    let (diags, scope) = check_with_scope("callback <- sqrt\ndf <- mtcars\nbad <- df$missing\n");
    let callback = scope.get("callback").expect("callback should be bound");
    assert_eq!(callback.mode, Mode::Function);
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY060"),
        "typed dataset schemas must win over existence-only inventory: {diags:?}"
    );
}

#[test]
fn standard_inventory_does_not_hide_unknown_names() {
    let diags = check("definitely_not_a_standard_r_symbol\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY010"),
        "unknown neighboring names must still be diagnosed: {diags:?}"
    );
}

#[test]
fn call_position_skips_local_values_for_standard_functions() {
    let diags = check(
        "dimnames <- list(rows = \"r\")\nx <- matrix(1L, 1L, 1L)\ny <- dimnames(x)\ndimnames(x) <- dimnames\nserialize <- TRUE\nserialize(1L, NULL)\n",
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY070"),
        "R call lookup skips same-named non-function bindings: {diags:?}"
    );
}

#[test]
fn standard_non_function_values_do_not_suppress_call_errors() {
    let diags = check("WWWusage <- 1L\nWWWusage()\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY070"),
        "standard datasets are values, not call-position candidates: {diags:?}"
    );
}

#[test]
fn withr_tempfile_injects_literal_names_into_code_scope() {
    let diags = check("withr::with_tempfile(c(\"first\", \"second\"), code = { first; second })\n");
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "with_tempfile string names should be bound inside code: {diags:?}"
    );
}

#[test]
fn withr_tempfile_bindings_do_not_leak() {
    let diags = check("withr::with_tempfile(\"path\", code = path)\npath\n");
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`path`")
        }),
        "with_tempfile bindings are local to the code expression: {diags:?}"
    );
}

#[test]
fn withr_tempfile_keeps_checking_other_code_names() {
    let diags = check("withr::with_tempfile(\"path\", code = { path; missing_inside })\n");
    assert!(
        diags.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("missing_inside")
        }),
        "only explicitly injected names should be suppressed: {diags:?}"
    );
}

#[test]
fn dbplyr_translation_helpers_capture_sql_expressions() {
    // `translate_sql` is the exported quoting entry point; the test-local
    // `expect_translation` helpers were removed from the stub because they
    // are not part of dbplyr's namespace (the audit enforces that).
    let diags = check("library(dbplyr)\ntranslate_sql(x + y)\nmissing_after\n");
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("`x`") && !diagnostic.message.contains("`y`"))
        }),
        "translation expressions are captured rather than evaluated lexically: {diags:?}"
    );
    assert!(diags.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("missing_after")
    }));
}

#[test]
fn lazy_defaults_can_reference_body_local_bindings() {
    let diags = check("f <- function(value = generated) {\n  generated <- 1L\n  value\n}\nf()\n");
    assert!(
        diags.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("generated")
        }),
        "R defaults are promises evaluated in the function environment: {diags:?}"
    );
}

#[test]
fn conditional_lazy_default_force_stays_silent() {
    let diags = check(include_str!(
        "../../testdata/ry098_default_forced_before_assignment.R"
    ));
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY098"),
        "a conditional force is not guaranteed: {diags:?}"
    );
}

#[test]
fn lazy_default_reachability_precision_cases_stay_silent() {
    let diags = check(include_str!(
        "../../testdata/ok_lazy_default_reachability.R"
    ));
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY098"),
        "conservative negative cases must remain silent: {diags:?}"
    );
}

#[test]
fn nse_function_alias_quotes_cli_time_ago_expressions() {
    let diags = check(include_str!("../../testdata/ok_nse_function_alias.R"));
    assert!(
        diags.is_empty(),
        "an alias of expression() must preserve quoted-call semantics: {diags:?}"
    );
}

#[test]
fn quote_and_printf_semantics_follow_function_aliases() {
    let diags = check("q <- quote\nq(undefined_sym)\ns <- sprintf\ns(\"%d %d\", 1)\n");
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "quote() through an alias must not resolve its captured symbol: {diags:?}"
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY094")
            .count(),
        1,
        "sprintf() format validation must run through an alias: {diags:?}"
    );
}

#[test]
fn function_alias_semantics_are_cleared_by_reassignment() {
    let diags = check("q <- quote\nq <- function(x) x\nq(undefined_sym)\n");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "RY010"),
        "overwriting an alias with a local function must clear quote semantics: {diags:?}"
    );
}

#[test]
fn nse_symbol_fallback_does_not_overlap_stub_eval_modes() {
    // `is_nse_symbol_fn` is the hardcoded half of the NSE knowledge. The
    // stub `eval` metadata is the source of truth. The fallback
    // intercepts before signature resolution, so a member listed in both
    // shadows its stub silently. Delete the member so the stubs stay
    // authoritative (issue #41).
    //
    // `quoted_expression`, `captures_promise`, and `quoted_symbol` skip
    // ordinary argument inference; `data_mask` and `tidy_select` still
    // infer arguments under a mask. Both kinds overlap the fallback. A
    // `data_mask`/`tidy_select` member may stay only when listed in
    // `data_mask_exemptions` with a reason. The list is a local test
    // fixture, not checker knowledge, so it stays out of the semantic
    // list registry.
    let data_mask_exemptions: &[&str] = &[
        // Empty today. dplyr's stub declares all_vars' `expr` as
        // data_mask, so all_vars left the fallback list.
    ];
    use ry_typeshed::EvalMode;
    let mut stub_eval = std::collections::HashMap::new();
    let mut add_typeshed = |typeshed: &ry_typeshed::Typeshed| {
        for (name, signature) in &typeshed.functions {
            for mode in signature.eval.values() {
                if *mode != EvalMode::Normal {
                    stub_eval.insert(name.clone(), *mode);
                }
            }
        }
    };
    if let Ok(base) = ry_typeshed::load_base_cached() {
        add_typeshed(base);
    }
    for package in ry_typeshed::known_packages() {
        if let Some(typeshed) = ry_typeshed::load_package(package) {
            add_typeshed(typeshed);
        }
    }
    let overlap: Vec<String> = crate::infer::NSE_SYMBOL_FNS
        .iter()
        .filter_map(|name| {
            let mode = stub_eval.get(*name)?;
            let exempted = matches!(mode, EvalMode::DataMask | EvalMode::TidySelect)
                && data_mask_exemptions.contains(name);
            (!exempted).then(|| format!("{name} ({mode:?})"))
        })
        .collect();
    assert!(
        overlap.is_empty(),
        "is_nse_symbol_fn members already covered by stub eval metadata; delete them or add a documented exemption: {overlap:?}"
    );
}
