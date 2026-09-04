use super::*;

#[test]
fn confidence_defaults_follow_rule_precision_and_info_severity() {
    assert_eq!(
        crate::diagnostics::default_confidence_for("RY096"),
        Confidence::High
    );
    assert_eq!(
        crate::diagnostics::default_confidence_for("RY010"),
        Confidence::Medium
    );
    assert_eq!(
        crate::diagnostics::default_confidence_for("RY097"),
        Confidence::Low
    );
    let info = Diagnostic::new(
        Severity::Info,
        Span::new(0, 1, 0, 0),
        "test.R",
        "RY010",
        "info",
    );
    assert_eq!(info.confidence, Confidence::Low);
}

#[test]
fn ambient_function_used_as_value_resolves_silently() {
    // Higher-order and value uses of ambient base functions are legitimate
    // R idioms and must not fire RY010 (`lapply(exprs, all.vars)` was a
    // documented FP cluster). The typo class is caught downstream when the
    // function type flows into comparisons (see the RY030 test below).
    for src in [
        "lapply(letters, enc2utf8)\n",
        "x <- col\n",
        "if (identical(oldClass, \"zoo\")) x <- 1L\n",
    ] {
        let diagnostics = check(src);
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "ambient function value use must not fire RY010: {diagnostics:?}"
        );
    }
}

#[test]
fn ambient_function_in_comparison_still_diagnosed() {
    let diagnostics = check("if (oldClass > 3) x <- 1L\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY030" || diagnostic.code == "RY033"),
        "function value in comparison must stay diagnosed: {diagnostics:?}"
    );
}

#[test]
fn unknown_custom_infix_quotes_its_operands() {
    let diagnostics = check(
        "fib(n) %::% numeric : numeric\n\
         fib(0) %as% 1\n\
         hof_map_zip_with(func = .(k, v1, v2) %->% (CONCAT(k, \"_\", v1, \"_\", v2)))\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "unknown infix DSL operands must be quoted: {diagnostics:?}"
    );
}

#[test]
fn known_custom_infix_evaluates_its_operands() {
    let diagnostics = check(
        "`%myop%` <- function(a, b) a\n\
         f <- function() { a %myop% b }\n",
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "RY010" && diagnostic.message.contains("`b`") }),
        "known custom infix operands must remain evaluated: {diagnostics:?}"
    );
}

#[test]
fn user_quoting_functions_discard_all_diagnostics_for_quoted_parameters() {
    let diagnostics = check(
        "`%as%` <- function(lhs, rhs) match.call()\n\
         fib(n) %::% numeric : numeric\n\
         fib(0) %as% 1\n\
         myfn <- function(x) deparse(substitute(x))\n\
         myfn(A +-+ B)\n\
         myfn(D - E - F)\n\
         myfn(unbound_name)\n\
         myfn(1 + \"a\")\n\
         myfn2 <- function(x) x + 1\n\
         myfn2(still_unbound)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            !["n", "numeric", "A", "B", "D", "E", "F", "unbound_name"]
                .iter()
                .any(|name| diagnostic.message.contains(&format!("`{name}`")))
        }),
        "quoted user-function arguments must emit no diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`still_unbound`")
        }),
        "normally evaluated user-function arguments must retain RY010: {diagnostics:?}"
    );
}

#[test]
fn normal_arguments_still_emit_type_diagnostics() {
    let diagnostics = check("plain <- function(x) x\nplain(1 + \"a\")\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040"),
        "normally evaluated arguments must retain type diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ry100_supersedes_condition_type_diagnostic() {
    let diagnostics = check("if (abs(x > 1)) NULL\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY100")
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code, "RY001" | "RY003")),
        "RY100 must be the only condition diagnostic: {diagnostics:?}"
    );
}

#[test]
fn formula_data_mask_arguments_use_the_named_data_source() {
    let lm = check("d <- data.frame()\nlm(y ~ x, data = d, weights = w, subset = grp == \"a\")\n");
    assert!(
        lm.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "lm formula, weights, and subset must be quiet under data: {lm:?}"
    );

    let survival = check(
        "tdata <- data.frame()\nsurvival::survfit(survival::Surv(t1, t2, s) ~ 1, id = id, weights = wt, data = tdata)\n",
    );
    assert!(
        survival.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "survival formula extras must use its named data argument: {survival:?}"
    );
}

#[test]
fn formula_data_mask_arguments_remain_normal_without_data() {
    let diagnostics = check("lm(y ~ x, weights = w)\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "RY010" && diagnostic.message.contains("`w`") }),
        "without data, formula extras must be checked in the caller scope: {diagnostics:?}"
    );
}

#[test]
fn match_call_quotes_every_user_function_argument() {
    let diagnostics = check(
        "all_quoted <- function(x, y) { call <- match.call(); NULL }\n\
         all_quoted(first_unbound, second_unbound)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "match.call() must quote every formal: {diagnostics:?}"
    );
}

#[test]
fn bquote_dot_marks_its_parameter_as_quoting() {
    let diagnostics = check(
        "quoted <- function(x) bquote(list(.(x)))\n\
         quoted(unbound_name)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "bquote(.(x)) must quote x: {diagnostics:?}"
    );
}

/// An unquote inside a braced body still quotes: the walker must recurse
/// through `Expr::Block` statements, not treat the block as opaque.
#[test]
fn bquote_dot_in_a_braced_body_still_quotes() {
    let diagnostics = check(
        "quoted <- function(x) bquote({ 1 == .(x) })\n\
         quoted(unbound_name)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "bquote({{ .(x) }}) must quote x: {diagnostics:?}"
    );
}

#[test]
fn rlang_capture_functions_mark_formals_as_quoting() {
    let diagnostics = check(
        "f <- function(x) rlang::enexpr(x)\n\
         f(unbound)\n\
         h <- function(...) rlang::enquos(...)\n\
         h(unbound1, unbound2)\n\
         q <- function(...) rlang::quos(...)\n\
         q(unbound3, unbound4)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "rlang capture helpers must quote their captured promises: {diagnostics:?}"
    );
}

#[test]
fn rlang_defusing_helpers_suppress_ry010_through_stub_metadata() {
    // enexpr, ensym, enquo, enquos, ensyms, and quos are promise-capture
    // stubs. A resolved signature evaluates their arguments in
    // captures_promise mode. No hardcoded fallback entry is involved.
    let loaded = check(
        "library(rlang)\n\
         enexpr(unbound_one)\n\
         ensym(unbound_two)\n\
         enquo(unbound_three)\n\
         enquos(unbound_four, unbound_five)\n\
         ensyms(unbound_six, unbound_seven)\n\
         quos(named = unbound_eight)\n",
    );
    assert!(
        loaded.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "loaded rlang metadata must capture defused arguments: {loaded:?}"
    );

    let qualified = check(
        "rlang::enexpr(unbound_one)\n\
         rlang::ensym(unbound_two)\n\
         rlang::enquo(unbound_three)\n\
         rlang::enquos(unbound_four, unbound_five)\n\
         rlang::ensyms(unbound_six, unbound_seven)\n\
         rlang::quos(named = unbound_eight)\n",
    );
    assert!(
        qualified
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "a qualified call resolves the stub without library(): {qualified:?}"
    );
}

#[test]
fn unloaded_rlang_helpers_evaluate_their_arguments() {
    // Without library(rlang) and without a package qualifier, no stub
    // resolves a defusing helper. Its argument is an ordinary read, so
    // an unbound name reports RY010. Package metadata requires the
    // package; the former hardcoded entries hid that boundary.
    let diagnostics = check("enquo(unbound_name)\n");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`unbound_name`")
        }),
        "an unresolved defusing call must evaluate its argument: {diagnostics:?}"
    );
}

#[test]
fn all_vars_goes_through_dplyr_data_mask_metadata() {
    // dplyr's stub declares all_vars' `expr` parameter as data_mask. The
    // stub path must resolve that metadata for qualified and loaded
    // calls; no hardcoded fallback entry shadows it.
    let qualified = check("dplyr::all_vars(unbound_col == other_col)\n");
    assert!(
        qualified
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "a data-masked expression must not resolve columns lexically: {qualified:?}"
    );

    let loaded = check("library(dplyr)\nall_vars(unbound_col == other_col)\n");
    assert!(
        loaded.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "a loaded dplyr call must reach the same mask metadata: {loaded:?}"
    );

    // Data-mask inference still runs on the argument: a literal type
    // error inside the masked expression fires, where the removed
    // fallback skipped the argument entirely.
    let checked = check("dplyr::all_vars(\"a\" + 1L)\n");
    assert!(
        checked.iter().any(|diagnostic| diagnostic.code == "RY040"),
        "the masked expression must still be inferred: {checked:?}"
    );
}

#[test]
fn unloaded_all_vars_evaluates_its_argument() {
    // Without library(dplyr) and without a package qualifier, no stub
    // resolves all_vars. Its argument is an ordinary read, so an
    // unbound name reports RY010. dplyr metadata requires dplyr; the
    // removed fallback entry hid that boundary.
    let diagnostics = check("all_vars(unbound_name)\n");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`unbound_name`")
        }),
        "an unresolved all_vars call must evaluate its argument: {diagnostics:?}"
    );
}

#[test]
fn alist_stub_metadata_quotes_dots_and_keeps_the_list_type() {
    // The base stub declares alist's dots as quoted_expression. The
    // stub path must suppress RY010 and keep the list result that the
    // removed hardcoded entry used to produce.
    let (diagnostics, scope) = check_with_scope("args <- alist(a = unbound_name, unbound_sym)\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "alist's quoted dots must not resolve captured names: {diagnostics:?}"
    );
    assert_eq!(
        scope.get("args").map(|ty| ty.mode),
        Some(Mode::List),
        "alist must keep its stub return type: {:?}",
        scope.get("args")
    );
}

#[test]
fn a_literal_tidyselect_call_evaluates_its_arguments() {
    // tidyselect is a package name, not a function. A call spelled
    // tidyselect(...) is not a real call target. The bogus fallback
    // entry suppressed RY010 on it; the arguments are ordinary reads.
    let diagnostics = check("tidyselect(unbound_name)\n");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`unbound_name`")
        }),
        "a package-name call must not gain NSE suppression: {diagnostics:?}"
    );
}

#[test]
fn s3_generic_inherits_quoting_from_its_methods() {
    let diagnostics = check(
        "tabyl <- function(d, ...) UseMethod(\"tabyl\")\n\
         tabyl.data.frame <- function(d, ...) rlang::ensyms(...)\n\
         df <- data.frame()\n\
         tabyl(df, colA)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`colA`")
        }),
        "a quoting S3 method must make the generic's dots quoting: {diagnostics:?}"
    );
}

#[test]
fn s3_generic_dots_inherit_named_method_formal_quoting() {
    let diagnostics = check(
        "tabyl <- function(d, ...) UseMethod(\"tabyl\")\n\
         tabyl.data.frame <- function(d, var1, var2, var3, show_na = TRUE, ...) {\n\
           rlang::enquo(var1)\n\
           rlang::enquo(var2)\n\
           rlang::enquo(var3)\n\
         }\n\
         d <- data.frame()\n\
         tabyl(d, am, cyl)\n\
         d %>% tabyl(am, cyl)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("`am`") && !diagnostic.message.contains("`cyl`"))
        }),
        "named method quoting must make generic dots opaque: {diagnostics:?}"
    );
}

#[test]
fn project_s3_generic_dots_inherit_named_method_formal_quoting() {
    let mut project = Project::new();
    project.add_file(
        "generic.R".to_string(),
        parse_file(
            "generic.R",
            "tabyl <- function(dat, ...) UseMethod(\"tabyl\")\n",
        ),
    );
    project.add_file(
        "method.R".to_string(),
        parse_file(
            "method.R",
            "tabyl.default <- function(dat, show_na = TRUE, ...) dat\n\
             tabyl.data.frame <- function(dat, var1, var2, var3, ...) {\n\
             if (missing(var1) && missing(var2) && missing(var3)) NULL\n\
             rlang::enquo(var1)\n\
             rlang::enquo(var2)\n\
             rlang::enquo(var3)\n\
             }\n",
        ),
    );
    project.add_file(
        "call.R".to_string(),
        parse_file("call.R", "d <- data.frame()\nd %>% tabyl(am, cyl)\n"),
    );
    let diagnostics: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("`am`") && !diagnostic.message.contains("`cyl`"))
        }),
        "project generic dots must inherit named method quoting: {diagnostics:?}"
    );
}

#[test]
fn s3_generic_without_quoting_methods_keeps_arguments_eager() {
    let diagnostics = check(
        "plain <- function(d, ...) UseMethod(\"plain\")\n\
         plain.data.frame <- function(d, ...) print(...)\n\
         df <- data.frame()\n\
         plain(df, unbound_column)\n",
    );
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`unbound_column`")
        }),
        "a non-quoting S3 method must not make the generic's arguments opaque: {diagnostics:?}"
    );
}

#[test]
fn project_s3_generic_inherits_quoting_from_methods_in_another_file() {
    let mut project = Project::new();
    project.add_file(
        "generic.R".to_string(),
        parse_file(
            "generic.R",
            "tabyl <- function(d, ...) UseMethod(\"tabyl\")\n",
        ),
    );
    project.add_file(
        "method.R".to_string(),
        parse_file(
            "method.R",
            "tabyl.data.frame <- function(d, ...) rlang::ensyms(...)\n",
        ),
    );
    project.add_file(
        "call.R".to_string(),
        parse_file("call.R", "df <- data.frame()\ntabyl(df, colA)\n"),
    );
    let diagnostics: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`colA`")
        }),
        "cross-file S3 quoting must reach generic call sites: {diagnostics:?}"
    );
}

#[test]
fn on_exit_sees_locals_assigned_later_but_not_unbound_names() {
    let later_assigned = check(
        "f <- function() {\n\
           on.exit(print(later))\n\
           later <- 1L\n\
         }\n",
    );
    assert!(
        later_assigned.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`later`")
        }),
        "on.exit must see locals assigned later in its enclosing body: {later_assigned:?}"
    );

    let unbound = check("f <- function() on.exit(print(never_assigned))\n");
    assert!(
        unbound.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`never_assigned`")
        }),
        "on.exit must retain RY010 for names never assigned in its body: {unbound:?}"
    );
}

#[test]
fn direct_forwarding_inherits_quoting_but_expressions_do_not() {
    let diagnostics = check(
        "q <- function(a) substitute(a)\n\
         w <- function(b) q(b)\n\
         w(unbound)\n\
         w2 <- function(b) q(b + 1)\n\
         w2(still_unbound)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`unbound`")
        }),
        "direct forwarding must inherit quoting: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`still_unbound`")
        }),
        "non-direct forwarding must remain evaluated: {diagnostics:?}"
    );
}

#[test]
fn direct_dots_forwarding_inherits_dots_quoting() {
    let diagnostics = check(
        "capture <- function(...) rlang::enexprs(...)\n\
         forward <- function(...) capture(...)\n\
         forward(unbound)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "direct dots forwarding must inherit dots quoting: {diagnostics:?}"
    );
}

#[test]
fn forwarding_to_quoted_stub_marks_the_user_formal_as_quoting() {
    let diagnostics = check(
        "forward <- function(...) dbplyr::translate_sql(...)\n\
         forward(unbound)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "quoted stub dots must make forwarded dots quoting: {diagnostics:?}"
    );
}

#[test]
fn qualified_quoted_stub_forwarding_ignores_a_same_named_user_function() {
    let diagnostics = check(
        "translate_sql <- function(x) x\n\
         forward <- function(...) dbplyr::translate_sql(...)\n\
         forward(unbound)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "a qualified stub call must not be masked by a user function: {diagnostics:?}"
    );
}

#[test]
fn forwarding_to_plain_stub_does_not_mark_the_user_formal_as_quoting() {
    let diagnostics = check("forward <- function(x) paste(x)\nforward(unbound)\n");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY010"),
        "normally evaluated stub arguments must remain evaluated: {diagnostics:?}"
    );
}

#[test]
fn quoted_stub_forwarding_suppresses_ry010_through_a_custom_infix_operator() {
    let diagnostics = check(
        "`%myop%` <- function(...) dbplyr::translate_sql(...)\n\
         x %myop% (unbound_ident + 1)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "the custom infix's forwarded dots must quote its operands: {diagnostics:?}"
    );
}

#[test]
fn quoted_operator_name_with_substitute_quotes_its_operands() {
    let diagnostics = check(
        "'%::%' <- function(signature, types) {\n\
           s <- deparse(substitute(signature))\n\
           t <- deparse(substitute(types))\n\
         }\n\
         fib(n) %::% numeric : numeric\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "quoted operator definitions must retain their NSE metadata: {diagnostics:?}"
    );
}

#[test]
fn unknown_dot_helper_quotes_its_operands_but_known_dot_evaluates_them() {
    let unknown = check(".(alcgp)\n");
    assert!(
        unknown.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "unknown .() must quote its operands: {unknown:?}"
    );

    let known = check("`.` <- function(x) x\n.(alcgp)\n");
    assert!(
        known.iter().any(|diagnostic| {
            diagnostic.code == "RY010" && diagnostic.message.contains("`alcgp`")
        }),
        "user-defined .() operands must remain evaluated: {known:?}"
    );
}

#[test]
fn runtime_stub_defines_package_function_for_checker() {
    let file = parse_file("runtime.R", "library(foo)\nx <- bar() + 1L\n");

    let mut without = Checker::new("runtime.R");
    let (without_diags, without_scope) = without.check_with_scope(&file);
    assert!(
        without_diags.is_empty(),
        "preserve current opaque-call behavior"
    );
    assert_eq!(
        without_scope.get("x").map(|ty| &ty.mode),
        Some(&Mode::Opaque),
        "without a user stub, bar() must remain opaque"
    );

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("foo.json"),
        r#"{
            "schema_version": "1",
            "package": "foo",
            "version": "test",
            "functions": {
                "bar": {
                    "params": [],
                    "return": {"mode": "integer", "length": "1"}
                }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("base.json"),
        r#"{
            "schema_version": "1",
            "package": "base",
            "version": "test",
            "functions": {
                "custom_base": {
                    "params": [],
                    "return": {"mode": "integer", "length": "1"}
                }
            }
        }"#,
    )
    .unwrap();
    let stubs = Arc::new(ry_typeshed::load_stub_dir(dir.path()).unwrap());
    let mut with = Checker::new("runtime.R");
    with.set_user_stubs(Arc::clone(&stubs));
    let (with_diags, with_scope) = with.check_with_scope(&file);
    assert!(
        with_diags.is_empty(),
        "user stub should type bar(): {with_diags:?}"
    );
    assert_eq!(with_scope.get("x").map(|ty| &ty.mode), Some(&Mode::Integer));

    let base_file = parse_file("base.R", "x <- custom_base() + 1L\n");
    let mut base_checker = Checker::new("base.R");
    base_checker.set_user_stubs(stubs);
    let (base_diags, base_scope) = base_checker.check_with_scope(&base_file);
    assert!(
        base_diags.is_empty(),
        "user base stub must replace embedded base for this checker: {base_diags:?}"
    );
    assert_eq!(base_scope.get("x").map(|ty| &ty.mode), Some(&Mode::Integer));
}

#[test]
fn runtime_stub_schema_effect_extends_data_mask_semantics() {
    let file = parse_file(
        "runtime_nse.R",
        "library(fakepkg)\ndf <- data.frame(x = 1L)\nout <- enrich(df, y = x + 1L)\nz <- out$y + 1L\n",
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("fakepkg.json"),
        r#"{
            "schema_version": "1",
            "package": "fakepkg",
            "version": "test",
            "functions": {
                "enrich": {
                    "params": [".data", "..."],
                    "return": "arg0",
                    "eval": {"...": "data_mask"},
                    "schema_effect": "add_named_args"
                }
            }
        }"#,
    )
    .unwrap();
    let stubs = Arc::new(ry_typeshed::load_stub_dir(dir.path()).unwrap());
    let mut checker = Checker::new("runtime_nse.R");
    checker.set_user_stubs(stubs);
    let (diagnostics, scope) = checker.check_with_scope(&file);
    assert!(
        diagnostics.is_empty(),
        "stub-defined data mask must resolve x and add y: {diagnostics:?}"
    );
    assert_eq!(scope.get("z").map(|ty| &ty.mode), Some(&Mode::Integer));
}

#[test]
fn qualified_base_schema_effect_is_applied() {
    let (diagnostics, scope) =
        check_with_scope("df <- data.frame(x = 1L)\ny <- base::with(df, x + 1L)\nz <- y + 1L\n");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("z").map(|ty| &ty.mode), Some(&Mode::Integer));
}

#[test]
fn discarded_pure_expression_in_non_tail_if_branch_warns() {
    let diagnostics = check(
        "f <- function(z, text) {\n\
           if (z == 0) z + 0.001\n\
           if (!grepl(\"\\n$\", text)) paste0(text, \"\\n\")\n\
           z\n\
         }\n",
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY099")
            .count(),
        2,
        "both branch results are discarded: {diagnostics:?}"
    );
}

#[test]
fn intentional_side_effect_and_tail_expressions_remain_silent() {
    let diagnostics = check(
        "f <- function(x) {\n\
           if (x) message(\"side effect\")\n\
           if (x) x + 1 else x - 1\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY099"),
        "side effects and returned branch values are not discarded: {diagnostics:?}"
    );
}

#[test]
fn single_bracket_list_compared_with_scalar_by_identical_warns() {
    // Table-driven over constructors whose stubs declare a `mode: list`
    // return — the former hardcoded list/lapply/Map set plus everything
    // the stubs record the same way (strsplit, split, ...). The
    // `names(args) <-` replacement degrades the binding's type, so the
    // warning rides on the list-origin marker, not the resolved mode.
    for (note, construct) in [
        ("list literal", "args <- list(font = \"monospace\")"),
        ("lapply result", "args <- lapply(c(\"a\"), paste0)"),
        ("Map result", "args <- Map(paste0, c(\"a\"))"),
        ("strsplit result", "args <- strsplit(\"a b\", \" \")"),
        ("split result", "args <- split(c(1), c(\"a\"))"),
        ("block-wrapped result", "args <- { strsplit(\"a b\", \" \") }"),
        (
            "if-expression result",
            "args <- if (TRUE) strsplit(\"a b\", \" \") else list()",
        ),
    ] {
        let src = format!("{construct}\nnames(args) <- \"x\"\nidentical(args[1], \"s\")\n");
        let diagnostics = check(&src);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "RY101"),
            "{note}: identical(list[...], scalar) is always false: {diagnostics:?}"
        );
    }
}

#[test]
fn double_bracket_list_compared_with_scalar_by_identical_is_valid() {
    let diagnostics = check(
        "args <- list(font = \"monospace\")\n\
         identical(args[[\"font\"]], \"monospace\")\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY101"),
        "double-bracket extraction returns the scalar element: {diagnostics:?}"
    );
}

#[test]
fn magrittr_braced_rhs_binds_dot_pronoun() {
    let diagnostics = check(
        "library(magrittr)\n\
         data.frame(value = 1L) %>% { .$value == 1L } %>% all()\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "the braced RHS is a magrittr dot lambda: {diagnostics:?}"
    );
}

#[test]
fn unknown_bare_parameter_short_circuit_operands_remain_silent() {
    let diagnostics = check("f <- function(x, y) x && y\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY032"),
        "unknown bare parameters are not proof of vector misuse: {diagnostics:?}"
    );
}

#[test]
fn vectorized_predicates_over_parameters_warn_in_short_circuit_ops() {
    let diagnostics = check(
        "guarded <- function(x) {\n\
           if (is.null(x) || x == \"\") return(NULL)\n\
           paste(x, collapse = \"\\n\")\n\
         }\n\
         non_missing <- function(x) {\n\
           if (length(x) > 0 && !is.na(x)) NULL\n\
           paste(x, collapse = \",\")\n\
         }\n",
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY032")
            .count(),
        2,
        "parameter predicates may be vectors at runtime: {diagnostics:?}"
    );
}

#[test]
fn intersect_only_preserves_its_exact_zero_fact() {
    let (_, scope) = check_with_scope(
        "empty <- intersect(NULL, 1:3)\n\
         bounded <- intersect(1:3, 1L)\n",
    );
    assert_eq!(scope.get("empty").map(|ty| ty.length), Some(Length::Zero));
    let bounded = scope.get("bounded").expect("bounded should stay bound");
    assert_eq!(bounded.mode, Mode::Integer);
    assert_eq!(
        bounded.length,
        Length::Unknown,
        "without an exact-zero fact, intersect length must remain unknown"
    );
}

#[test]
fn paste_consumes_recycled_value_and_control_bindings() {
    let (diagnostics, scope) = check_with_scope(
        "empty <- paste(NULL, NULL)\n\
         collapsed <- paste(NULL, collapse = \"\")\n\
         reordered <- paste(collapse = \"\", NULL)\n\
         recycled <- paste(recycle0 = TRUE, NULL, \"x\")\n",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(scope.get("empty").map(|ty| ty.length), Some(Length::Zero));
    assert_eq!(
        scope.get("collapsed").map(|ty| ty.length),
        Some(Length::One)
    );
    assert_eq!(
        scope.get("reordered").map(|ty| ty.length),
        Some(Length::One),
        "named controls after `...` must be excluded from recycled values"
    );
    assert_eq!(
        scope.get("recycled").map(|ty| ty.length),
        Some(Length::Zero),
        "recycle0 must use its declared, formally bound control"
    );
}

#[test]
fn callback_recycled_length_does_not_look_argumentless() {
    let (_, scope) = check_with_scope("result <- sapply(letters, paste0)\n");
    assert_eq!(
        scope.get("result").map(|ty| ty.length),
        Some(Length::Known(26)),
        "callback types without source arguments must not imply an empty call"
    );
}

#[test]
fn join_normal_arguments_use_the_ordinary_scope() {
    let diagnostics = check(
        "library(dplyr)\nx <- unknown_source()\ny <- data.frame(id = 1L)\nleft_join(x, y, by = missing_name)\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("missing_name")
    }));
}

#[test]
fn typeshed_parameter_modes_drive_data_mask_evaluation() {
    let diagnostics = check_with(
        "x <- as_draws_df(source)\ny <- mutate_variables(x, tau2 = tau^2)\n",
        |c| c.set_loaded(HashSet::from(["posterior".to_string()])),
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("tau")
        }),
        "data-mask metadata should make tau an opaque masked binding: {:?}",
        diagnostics
    );
}

#[test]
fn embedded_package_eval_metadata_drives_data_mask_evaluation() {
    let diagnostics = check("library(rlist)\nr <- list.map(some_list(), . + score)\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "loaded rlist metadata should mask both `.` and `score`: {diagnostics:?}"
    );
}

#[test]
fn user_s3_method_inherits_generic_eval_metadata() {
    let diagnostics = check(
        "library(dplyr)\ncount.mystep <- function(.data, ...) 1\nobj <- structure(list(internal = 1L), class = \"mystep\")\ncount(obj, some_col)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("some_col")
        }),
        "the user method must inherit dplyr count's data mask: {diagnostics:?}"
    );
}

#[test]
fn dynamically_registered_s3_method_inherits_generic_eval_metadata() {
    let diagnostics = check(
        "complete.custom <- function(data, ...) data\nobj <- structure(list(), class = \"custom\")\ncomplete(obj, missing_column)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("missing_column")
        }),
        "the method should inherit tidyr complete's data mask: {diagnostics:?}"
    );
}

#[test]
fn s3_method_inherits_schema_generic_eval_metadata() {
    let diagnostics = check(
        "library(dplyr)\ngroup_by.custom <- function(.data, ...) .data\nobj <- structure(data.frame(known = 1L), class = c(\"custom\", \"data.frame\"))\ngroup_by(obj, missing_column)\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("missing_column")
        }),
        "the method should inherit group_by's data mask: {diagnostics:?}"
    );
}

#[test]
fn data_mask_metadata_without_a_data_frame_is_still_opaque() {
    let diagnostics =
        check("library(patrick)\nwith_parameters_test_that(\"case\", n2 + n3, n2 = 1L, n3 = 2L)\n");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "patrick's masked code should not require a data-frame first argument: {diagnostics:?}"
    );
}

#[test]
fn data_mask_binds_dot_inside_do_with_native_pipe() {
    let diagnostics = check("library(dplyr)\ndf <- data.frame(x = 1L)\ndf |> do(head(., 1))\n");
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`.`")
        }),
        "the current-group dot should be bound in do(): {diagnostics:?}"
    );
}

#[test]
fn user_function_defused_parameters_are_opaque_at_call_sites() {
    let diagnostics = check(
        "capture <- function(expr, other) {\n  expr <- rlang::enquo(expr)\n  other\n}\ncapture(.input + missing, other = 1L)\ncapture(other = 1L, expr = named_missing)\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010"),
        "positionally and named defused arguments should be opaque: {diagnostics:?}"
    );
}

#[test]
fn lexical_types_are_opaque_under_unknown_data_masks_only() {
    let masked = check("library(dplyr)\ndf <- get(\"df\")\ny <- \"a\"\nmutate(df, x = x / y)\n");
    assert!(
        masked.iter().all(|diagnostic| diagnostic.code != "RY040"),
        "a lexical type must not drive arithmetic diagnostics under an unknown mask: {masked:?}"
    );

    let outside = check("y <- \"a\"\ny / 1L\n");
    assert!(
        outside.iter().any(|diagnostic| diagnostic.code == "RY040"),
        "the same lexical type must still be checked outside a mask: {outside:?}"
    );
}

#[test]
fn exclusively_defused_dots_are_opaque_at_call_sites() {
    let source = "f <- function(...) enquos(...)\ny <- \"a\"\nf(not_a_binding == 1, y / 1L)\n";
    let file = parse_file("test.R", source);
    let mut checker = Checker::new("test.R");
    checker.collect_fns(&file.stmts);
    assert!(checker.fn_table.fns["f"].params[0].defused);

    let diagnostics = check(source);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY010" && diagnostic.code != "RY040"),
        "arguments absorbed by defused dots should be opaque: {diagnostics:?}"
    );
}

#[test]
fn propagated_quoting_generic_keeps_lazy_default_silent() {
    // `g`'s `x` turns quoting only through S3 propagation from its method,
    // and the propagated flag must hold through the nested lazy default.
    let source = "g <- function(x) UseMethod(\"g\")\n\
                  g.foo <- function(x) enquo(x)\n\
                  wrapper <- function() {\n\
                    inner <- function(a = a) {\n\
                      a <- g(a)\n\
                      a\n\
                    }\n\
                    inner\n\
                  }\n\
                  wrapper()\n";
    let file = parse_file("test.R", source);
    let mut checker = Checker::new("test.R");
    checker.collect_fns(&file.stmts);
    checker.run_fixpoint();
    assert!(
        checker.fn_table.fns["g"].params[0].quoting,
        "S3 propagation must mark the generic's formal quoting"
    );

    let diagnostics = check(source);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY098"),
        "a propagated-quoting generic defuses its argument, so the lazy default stays safe: {diagnostics:?}"
    );
}

#[test]
fn normally_used_dots_remain_eager_at_call_sites() {
    let diagnostics = check("g <- function(...) sum(...)\ng(not_a_binding)\n");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("not_a_binding")
    }));

    let mixed =
        check("h <- function(...) { captured <- enquos(...); list(...) }\nh(still_not_bound)\n");
    assert!(mixed.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("still_not_bound")
    }));
}

#[test]
fn embraced_parameters_are_defused_at_call_sites() {
    let source = "library(dplyr)\nwrapper <- function(df, var) select(df, {{ var }})\nwrapper(data.frame(a = 1L), a)\n";
    let file = parse_file("test.R", source);
    let mut checker = Checker::new("test.R");
    checker.collect_fns(&file.stmts);
    assert!(checker.fn_table.fns["wrapper"].params[1].defused);

    let diagnostics = check(source);
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010" || !diagnostic.message.contains("`a`")
        }),
        "an embraced parameter should forward its call-site expression: {diagnostics:?}"
    );
}

#[test]
fn normal_use_before_defusing_keeps_parameter_eager() {
    let diagnostics = check(
        "capture <- function(expr) {\n  print(expr)\n  expr <- enquo(expr)\n}\ncapture(still_missing)\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("still_missing")
    }));
}

#[test]
fn normal_first_use_in_any_branch_keeps_parameter_eager() {
    let diagnostics = check(
        "capture <- function(expr, flag) {\n  if (flag) enquo(expr) else print(expr)\n}\ncapture(still_missing, TRUE)\n",
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("still_missing")
    }));
}

#[test]
fn foreach_user_infix_binds_named_iteration_variables() {
    let diagnostics = check(
        "foreach(iter = seq_along(xs), parm = values, .errorhandling = \"stop\") %op% {\n  iter + parm + genuinely_missing\n}\nforeach(outer = xs) %:% foreach(inner = ys) %dopar% { outer + inner }\n",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "RY010"
                || (!diagnostic.message.contains("iter")
                    && !diagnostic.message.contains("parm")
                    && !diagnostic.message.contains("outer")
                    && !diagnostic.message.contains("inner"))
        }),
        "foreach iteration bindings should be scoped over the RHS: {diagnostics:?}"
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "RY010" && diagnostic.message.contains("genuinely_missing")
    }));
}
