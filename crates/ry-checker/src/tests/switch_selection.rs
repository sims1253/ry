use super::*;

#[test]
fn literal_switch_selects_only_the_executed_alternative() {
    for call in [
        "switch('a', a=1L, b='bad'+1)",
        "base::switch('a', a=, b=1L, c='bad'+1)",
        "switch(2.9, 'bad'+1, 1L)",
        "switch(TRUE, 1L, 'bad'+1)",
        "switch('a', a=1L, a='bad'+1)",
        "switch(E=2L, a='bad'+1, EXPR=1L)",
        "switch('absent', a='bad'+1, 1L)",
    ] {
        let (diagnostics, scope) = check_with_scope(&format!("out <- {call}"));
        assert_eq!(scope.get("out").unwrap().mode, Mode::Integer, "{call}");
        assert!(
            !diagnostics.iter().any(|d| d.code == "RY040"),
            "{call}: {diagnostics:?}"
        );
    }
    let (diagnostics, _) = check_with_scope("out <- switch(1L, 'bad'+1, 1L)");
    assert!(diagnostics.iter().any(|d| d.code == "RY040"));
}

#[test]
fn literal_switch_null_and_invalid_selections_do_not_evaluate_alternatives() {
    for call in [
        "switch(FALSE, 'bad'+1)",
        "switch(0, 'bad'+1)",
        "switch(-1, 'bad'+1)",
        "switch(3L, 'bad'+1)",
        "switch(Inf, 'bad'+1)",
        "switch('absent', a='bad'+1)",
        "switch('a', a=)",
    ] {
        let (diagnostics, scope) = check_with_scope(&format!("out <- {call}"));
        assert_eq!(scope.get("out").unwrap().mode, Mode::Null, "{call}");
        assert!(!diagnostics.iter().any(|d| d.code == "RY040"), "{call}");
    }
    for call in [
        "switch(a=1L, EXPR='a', b='bad'+1)",
        "switch('a', a=1L, 'bad'+1, 2L)",
        "switch(1L, , 'bad'+1)",
    ] {
        let (diagnostics, scope) = check_with_scope(&format!("out <- {call}"));
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{call}");
        assert!(!diagnostics.iter().any(|d| d.code == "RY040"), "{call}");
    }
}

#[test]
fn switch_selected_writes_reach_the_caller() {
    let (_, scope) = check_with_scope(
        "x <- 1L; out <- switch('a', a={x <- 'selected'; 1L}, b={x <- FALSE; 2L})",
    );
    assert_eq!(scope.get("x").unwrap().mode, Mode::Character);
}

#[test]
fn switch_masks_and_namespace_guards_do_not_borrow_selection() {
    let (_, scope) = check_with_scope("switch <- function(...) 'custom'; out <- switch(1L, 1L)");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Character);
    for source in [
        "switch <- unknown; out <- switch(1L, 1L)",
        "`::` <- function(...) unknown; out <- base::switch(1L, 1L)",
        "f <- switch; out <- f(1L, 1L)",
    ] {
        let (_, scope) = check_with_scope(source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{source}");
    }
    let (_, scope) = check_with_stubs(
        "out <- base::switch(1L, 1L)",
        &[(
            "base.json",
            r#"{"version":"t","functions":{"switch":{"params":["..."],"return":{"mode":"character","length":"1"}}}}"#,
        )],
    );
    assert_eq!(scope.get("out").unwrap().mode, Mode::Character);
}

#[test]
fn uncertain_switch_calls_invalidate_caller_facts_without_forcing_actuals() {
    for source in [
        "switch <- unknown; x <- 1L; out <- switch(1L, {x <- 'changed'; 'bad'+1}); after <- x+1L",
        "out <- switch('a', a=1L, ...)",
        "out <- switch('a', `\\x61`=1L, 2L)",
        "`(` <- function(...) 2L; out <- switch((1L), 1L, FALSE)",
    ] {
        let (diagnostics, scope) = check_with_scope(source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{source}");
        assert!(
            !diagnostics.iter().any(|d| d.code == "RY040"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn selected_alternatives_preserve_return_and_stop_behavior() {
    let (diagnostics, stopped) = check_with_scope("out <- switch(1L, stop('selected'), 'bad'+1)");
    assert_eq!(stopped.get("out").unwrap().mode, Mode::Opaque);
    assert!(!diagnostics.iter().any(|d| d.code == "RY040"));
    let (_, returned) = check_with_scope("switch(1L, {return(1L); 2L}, 3L)");
    assert!(returned.unreachable);
    let (_, continuing) = check_with_scope("switch(2L, stop('unselected'), 1L)");
    assert!(!continuing.unreachable);
    let (diagnostics, _) = check_with_scope("f <- function(switch) switch(1L, 'bad'+1)");
    assert!(
        !diagnostics.iter().any(|d| d.code == "RY040"),
        "{diagnostics:?}"
    );
}

#[test]
fn decoded_ascii_selectors_follow_r_escape_values() {
    for call in [
        r#"switch('\141', a=1L, 'wrong')"#,
        r#"switch('\u{61}', a=1L, 'wrong')"#,
        r#"switch('\x61', a=1L, 'wrong')"#,
        "switch('a\\\nb', ab='wrong', 1L)",
    ] {
        let (_, scope) = check_with_scope(&format!("out <- {call}"));
        assert_eq!(scope.get("out").unwrap().mode, Mode::Integer, "{call}");
    }
    for call in [r#"switch('\u{e9}', a=1L, 2L)"#, r#"switch('\\', a=1L, 2L)"#] {
        let (_, scope) = check_with_scope(&format!("out <- {call}"));
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{call}");
    }
}

#[test]
fn prior_calls_cannot_lend_base_switch_identity() {
    for source in [
        "replace <- function() assign(paste0('swi', 'tch'), function(...) 1L, envir=.GlobalEnv); replace(); out <- switch(1L, 'bad', 1L); after <- out+1L",
        "replace <- function() assign(paste0(':', ':'), function(...) function(...) 1L, envir=.GlobalEnv); replace(); out <- base::switch(1L, 'bad', 1L); after <- out+1L",
    ] {
        let (diagnostics, scope) = check_with_scope(source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
        assert!(
            !diagnostics.iter().any(|d| d.code == "RY040"),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn dynamic_negative_selectors_keep_the_legacy_join() {
    for selector in ["-x", "-f()"] {
        let (_, scope) = check_with_scope(&format!("unknown(); out <- switch({selector}, 1L, 2L)"));
        assert_eq!(scope.get("out").unwrap().mode, Mode::Integer, "{selector}");
    }
}

#[test]
fn qualified_dynamic_selectors_do_not_gain_an_all_alternative_model() {
    let (_, scope) = check_with_scope("out <- base::switch(x, 1L, 2L)");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
    let (_, scope) = check_with_scope("out <- switch(x, 1L, 2L)");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Integer);
}

#[test]
fn rebound_assignment_cannot_hide_a_callee_replacement() {
    let diagnostics = check(
        "`<-` <- function(...) assign(paste0('swi', 'tch'), function(...) 1L, envir=.GlobalEnv); x <- 1L; switch(1L, 'bad', 1L)+1L",
    );
    assert!(
        !diagnostics.iter().any(|d| d.code == "RY040"),
        "{diagnostics:?}"
    );
}

#[test]
fn uncertain_selected_effects_discard_result_and_caller_facts() {
    for selected in ["{mutate(); x}", "{object[1L]; x}"] {
        let source = format!(
            "x <- 'bad'; object <- base::structure(1L, class='foo'); mutate <- function() assign('x', 1L, envir=.GlobalEnv); `[.foo` <- function(x, ...) {{assign('x', 1L, envir=.GlobalEnv); 1L}}; out <- switch(1L, {selected}, 1L); after <- out+1L; caller <- x+1L"
        );
        let (diagnostics, scope) = check_with_scope(&source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
        assert_eq!(scope.get("caller").unwrap().mode, Mode::Opaque);
        assert!(
            !diagnostics.iter().any(|d| d.code == "RY040"),
            "{selected}: {diagnostics:?}"
        );
    }
}

#[test]
fn pure_selected_list_retains_required_error_but_masks_do_not() {
    for call in ["list(1L)", "base::list(1L)"] {
        let diagnostics = check(&format!("out <- switch(1L, {call}, 1L); out+1L"));
        assert!(
            diagnostics.iter().any(|d| d.code == "RY040"),
            "{call}: {diagnostics:?}"
        );
    }
    let (diagnostics, scope) =
        check_with_scope("list <- function(...) 1L; out <- switch(1L, list(1L), 1L); out+1L");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
    assert!(!diagnostics.iter().any(|d| d.code == "RY040"));
}
