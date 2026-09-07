use super::*;

#[test]
fn printf_arity_requires_proven_base_callees() {
    for name in ["sprintf", "gettextf"] {
        for source in [
            format!("{name} <- function(...) 1L; out <- {name}('%d')"),
            format!("f <- function({name}) {name}('%d'); f(function(...) 1L)"),
            format!("probe <- {name}; out <- probe('%d')"),
            format!(
                "{name} <- local(function(...) 1L); probe <- {name}; {name} <- NULL; out <- probe('%d')"
            ),
            format!("out <- otherpkg::{name}('%d')"),
            format!("out <- methods::{name}('%d')"),
            format!("library(unknownpkg); out <- {name}('%d')"),
            format!("`::` <- function(pkg,name) function(...) 1L; out <- base::{name}('%d')"),
            format!("`base::{name}` <- function(...) 1L; out <- 'base::{name}'('%d')"),
        ] {
            let diags = check(&source);
            assert!(
                diags.iter().all(|d| d.code != "RY094"),
                "{source}: {diags:?}"
            );
        }
        let (_, scope) =
            check_with_scope(&format!("{name} <- function(...) 1L; out <- {name}('%d')"));
        assert_eq!(scope.get("out").unwrap().mode, Mode::Integer);
    }
}

#[test]
fn proven_printf_calls_retain_arity_diagnostics() {
    for name in ["sprintf", "gettextf"] {
        for callee in [
            name.to_string(),
            format!("base::{name}"),
            format!("base:::{name}"),
        ] {
            let diags = check(&format!("{callee}('%d')"));
            assert!(
                diags.iter().any(|d| d.code == "RY094"),
                "{callee}: {diags:?}"
            );
            let diags = check(&format!("{callee}('%d',1L)"));
            assert!(
                diags.iter().all(|d| d.code != "RY094"),
                "{callee}: {diags:?}"
            );
        }
    }
}

#[test]
fn printf_stub_overrides_do_not_borrow_base_arity_rules() {
    for package in ["base", "custom"] {
        for name in ["sprintf", "gettextf"] {
            let json = format!(
                r#"{{"version":"t","functions":{{"{name}":{{"params":["..."],"return":{{"mode":"integer","length":"1"}}}}}}}}"#
            );
            let (diags, scope) = check_with_stubs(
                &format!("out <- {package}::{name}('%d')"),
                &[(&format!("{package}.json"), &json)],
            );
            assert!(
                diags.iter().all(|d| d.code != "RY094"),
                "{package}::{name}: {diags:?}"
            );
            assert_eq!(scope.get("out").unwrap().mode, Mode::Integer);
        }
    }
}
