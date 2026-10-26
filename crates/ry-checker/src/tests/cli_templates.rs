use super::*;
use std::collections::HashMap;

// RY125 (#570): a definite function used as dimensional data, plain and
// inside recognized literal cli templates (the broom case).

#[test]
fn dimensional_call_on_definite_function_fires_plain() {
    for source in [
        "f <- function(x) ncol(data)\n",
        "f <- function(x) nrow(data)\n",
        "f <- function(x) dim(data)\n",
        "f <- function(x) base::ncol(utils::data)\n",
        "g <- function() 1L\nf <- function(x) ncol(g)\n",
        "f <- function(x) ncol(function() 1L)\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().any(|d| d.code == "RY125"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn dimensional_call_on_definite_function_silent_controls() {
    for source in [
        // Correct variable.
        "f <- function(x) ncol(x)\n",
        // A formal spelled like the search-path function shadows it.
        "f <- function(data) ncol(data)\n",
        // A lexical data binding shadows it.
        "f <- function(x) { data <- x; ncol(data) }\n",
        // A top-level data binding in the same file shadows it.
        "data <- data.frame(a = 1)\nf <- function(x) ncol(data)\n",
        // `length()` of a function is 1, not NULL: not dimensional misuse.
        "f <- function(x) length(data)\n",
        // Masked dimensional callee.
        "ncol <- function(x) 1L\nf <- function(x) ncol(data)\n",
        // Another package's same-named function.
        "f <- function(x) otherpkg::ncol(data)\n",
        // Unknown free name: no function proof.
        "f <- function(x) ncol(some_unknown_frame)\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|d| d.code != "RY125"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn broom_cli_template_case_fires() {
    let source = "as_tidy_tibble <- function(x, new_names = NULL) {\n  if (!is.null(new_names) && length(new_names) != ncol(x)) {\n    cli::cli_abort(\"number of columns ({ncol(data)})\")\n  }\n}\n";
    let diags = check(source);
    let hits: Vec<_> = diags.iter().filter(|d| d.code == "RY125").collect();
    assert_eq!(hits.len(), 1, "{diags:?}");
    assert_eq!(diags.len(), 1, "only RY125 expected: {diags:?}");
}

#[test]
fn cli_template_controls_stay_silent() {
    for source in [
        // Correct variable inside the template.
        "f <- function(x) cli::cli_abort(\"cols: {ncol(x)}\")\n",
        // Legitimate interpolation of a function value itself.
        "f <- function(x) cli::cli_abort(\"loader: {data}\")\n",
        "f <- function(fun) cli::cli_inform(\"using {fun}\")\n",
        // A formal spelled like the search-path function.
        "f <- function(data) cli::cli_abort(\"{ncol(data)}\")\n",
        // `.envir` override: expressions evaluate elsewhere.
        "f <- function(x, e) cli::cli_abort(\"{ncol(data)}\", .envir = e)\n",
        // Dynamically computed (non-literal) template.
        "f <- function(x) { msg <- \"{ncol(data)}\"; cli::cli_abort(msg) }\n",
        "f <- function(x) cli::cli_abort(paste0(\"{ncol(data)}\"))\n",
        // Escaped braces are literal text.
        "f <- function(x) cli::cli_abort(\"{{ncol(data)}}\")\n",
        // Markup span content is literal text, not code.
        "f <- function(x) cli::cli_abort(\"{.arg ncol(data)} is wrong\")\n",
        "f <- function(x) cli::cli_abort(\"{.code ncol(data)}\")\n",
        // Pluralization marker is not an expression.
        "f <- function(x) cli::cli_inform(\"{ncol(x)} column{?s}\")\n",
        // Unbalanced brace: no region, no diagnostic, no panic.
        "f <- function(x) cli::cli_abort(\"{ncol(data)\")\n",
        // Masked cli function (bare name bound locally).
        "cli_abort <- function(...) NULL\nf <- function(x) cli_abort(\"{ncol(data)}\")\n",
        // Another package's same-named function.
        "f <- function(x) otherpkg::cli_abort(\"{ncol(data)}\")\n",
        // Not a recognized cli message function.
        "f <- function(x) cli::cli_code(\"{ncol(data)}\")\n",
        "f <- function(x) message(\"{ncol(data)}\")\n",
        // Masked dimensional callee inside the template.
        "f <- function(x) { ncol <- length; cli::cli_abort(\"{ncol(data)}\") }\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|d| d.code != "RY125"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn cli_template_nested_and_markup_interpolations_fire() {
    for source in [
        // Interpolation nested inside a markup span is evaluated.
        "f <- function(x) cli::cli_abort(\"{.strong {ncol(data)}} cols\")\n",
        // Block expression with nested braces.
        "f <- function(x) cli::cli_abort(\"{ { ncol(data) } }\")\n",
        // Bullet vector built with c().
        "f <- function(x) cli::cli_abort(c(\"bad input\", i = \"cols: {ncol(data)}\"))\n",
        // Other recognized message functions and dimensional calls.
        "f <- function(x) cli::cli_warn(\"rows: {nrow(data)}\")\n",
        "f <- function(x) cli::cli_inform(\"dims: {dim(data)}\")\n",
        "f <- function(x) cli::cli_alert_success(\"cols: {ncol(data)}\")\n",
        "f <- function(x) cli::cli_text(\"cols: {ncol(data)}\")\n",
        // Bare cli name with an explicit cli import is proven below in
        // the import test; qualified form here.
        "f <- function(x) cli:::cli_abort(\"cols: {ncol(data)}\")\n",
        // Local function value used as data inside the template.
        "g <- function() 1L\nf <- function(x) cli::cli_abort(\"{ncol(g)}\")\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().any(|d| d.code == "RY125"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn bare_cli_name_fires_with_explicit_cli_import() {
    let diags = check_with(
        "f <- function(x) cli_abort(\"{ncol(data)}\")\n",
        |checker: &mut Checker| {
            checker.set_imported_from(HashMap::from([(
                "cli_abort".to_string(),
                "cli".to_string(),
            )]));
        },
    );
    assert!(diags.iter().any(|d| d.code == "RY125"), "{diags:?}");
    // The same bare spelling imported from another package is not cli.
    let diags = check_with(
        "f <- function(x) cli_abort(\"{ncol(data)}\")\n",
        |checker: &mut Checker| {
            checker.set_imported_from(HashMap::from([(
                "cli_abort".to_string(),
                "otherpkg".to_string(),
            )]));
        },
    );
    assert!(diags.iter().all(|d| d.code != "RY125"), "{diags:?}");
}

#[test]
fn unknown_class_function_values_stay_silent() {
    // A class chosen at runtime could carry a `dim` method, so the
    // always-NULL claim needs a known-empty class.
    let diags = check("f <- function(cl) { g <- structure(function() 1L, class = cl); ncol(g) }\n");
    assert!(diags.iter().all(|d| d.code != "RY125"), "{diags:?}");
}

#[test]
fn quoted_and_masked_template_code_stays_silent() {
    for source in [
        // Quoted code never runs.
        "f <- function(x) cli::cli_text(\"{deparse(quote(ncol(data)))}\")\n",
        // A data mask resolves `data` against the mask first.
        "f <- function(df) cli::cli_text(\"{with(df, ncol(data))}\")\n",
        // An earlier region's rebinding shadows later regions.
        "f <- function(x) cli::cli_abort(\"{dim <- 'x'; 'y'} {ncol(dim)}\")\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|d| d.code != "RY125"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn markup_punctuation_does_not_hide_interpolations() {
    for source in [
        "f <- function(x) cli::cli_abort(\"{.strong Don't use {ncol(data)}}\")\n",
        "f <- function(x) cli::cli_abort(\"{.strong #1 issue: {ncol(data)}}\")\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().any(|d| d.code == "RY125"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn template_hit_span_covers_the_interpolation() {
    let source = "f <- function(x) cli::cli_abort(\"cols: {ncol(data)} done\")\n";
    let diags = check(source);
    let hit = diags.iter().find(|d| d.code == "RY125").unwrap();
    let slice = source
        .get(hit.span.start..hit.span.end)
        .unwrap_or_else(|| panic!("span {:?} not on char boundaries", hit.span));
    assert_eq!(slice, "{ncol(data)}");
}

#[test]
fn sequential_region_shadowing_is_parse_based() {
    // Prose containing `<-` in an earlier region is not a rebinding:
    // the later region still reports.
    let diags =
        check("f <- function(warn) cli::cli_abort(\"{if (warn) 'use x <- 1'} {ncol(data)}\")\n");
    assert!(diags.iter().any(|d| d.code == "RY125"), "{diags:?}");
    // A hit evaluated before an in-region rebinding still reports.
    let diags = check("f <- function(x) cli::cli_abort(\"{ncol(data); x <- 1}\")\n");
    assert!(diags.iter().any(|d| d.code == "RY125"), "{diags:?}");
}

#[test]
fn equals_and_assign_rebindings_shadow_later_regions() {
    for source in [
        "f <- function(x) cli::cli_abort(\"{dim = 'x'} {ncol(dim)}\")\n",
        "f <- function(x) cli::cli_abort(\"{assign('dim', 'x')} {ncol(dim)}\")\n",
        "f <- function(x) cli::cli_abort(\"{for (dim in 1) NULL} {ncol(dim)}\")\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|d| d.code != "RY125"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn non_template_cli_arguments_stay_silent() {
    for source in [
        // `cli_par` has no text argument; a positional string is `id`.
        "f <- function(x) cli::cli_par(\"{ncol(data)}\")\n",
        // `id` is passed through raw, never interpolated.
        "f <- function(x) cli::cli_h1(\"t\", id = \"{ncol(data)}\")\n",
        // A second positional string on cli_abort binds `class`.
        "f <- function(x) cli::cli_abort(\"msg\", \"{ncol(data)}\")\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|d| d.code != "RY125"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn package_data_mask_verbs_inside_templates_stay_silent() {
    // Inside dplyr::mutate's mask, `data` may be a column; the
    // non-template equivalent is silent for the same reason.
    let diags = check("f <- function(df) cli::cli_text(\"{dplyr::mutate(df, n = ncol(data))}\")\n");
    assert!(diags.iter().all(|d| d.code != "RY125"), "{diags:?}");
}

#[test]
fn multiline_template_hit_span_maps_line_and_column() {
    let source = "f <- function(x) cli::cli_abort(\"first line\nsecond {ncol(data)} end\")\n";
    let diags = check(source);
    let hit = diags.iter().find(|d| d.code == "RY125").unwrap();
    let slice = source
        .get(hit.span.start..hit.span.end)
        .unwrap_or_else(|| panic!("span {:?} not on char boundaries", hit.span));
    assert_eq!(slice, "{ncol(data)}");
    assert_eq!((hit.span.line, hit.span.col), (1, 7));
}

#[test]
fn rightward_and_eval_rebindings_shadow_later_regions() {
    for source in [
        "f <- function(x) cli::cli_abort(\"{'v' -> dim} {ncol(dim)}\")\n",
        "f <- function(x) cli::cli_abort(\"{evalq(dim <- 'v')} {ncol(dim)}\")\n",
        // Second positional on cli_h1 binds `id`, not a template.
        "f <- function(x) cli::cli_h1(\"t\", \"{ncol(data)}\")\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|d| d.code != "RY125"),
            "{source}: {diags:?}"
        );
    }
    // cli_text is variadic: later positional strings interpolate.
    let diags = check("f <- function(x) cli::cli_text(\"a\", \"{ncol(data)}\")\n");
    assert!(diags.iter().any(|d| d.code == "RY125"), "{diags:?}");
}
