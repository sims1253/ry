use super::*;

#[test]
fn custom_deferred_names_keep_ordinary_calls() {
    let (diags, scope) = check_with_scope(
        "hasArg <- function(...) list(value=1L); out <- hasArg('x'); value <- out$value",
    );
    assert_eq!(scope.get("out").unwrap().mode, Mode::List);
    assert!(
        diags.iter().all(|d| !matches!(d.code, "RY061" | "RY096")),
        "{diags:?}"
    );
    let (diags, scope) = check_with_scope("on.exit <- function(expr) 'actual'; out <- on.exit(1L)");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Character);
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn supplied_deferred_names_cannot_borrow_quoting_or_return_facts() {
    for source in [
        "f <- function(hasArg) hasArg('not_formal'); out <- f(function(x) TRUE)",
        "f <- function(on.exit) { x <- 'before'; on.exit({x <- 1L}); x+1L }; out <- f(function(expr) expr)",
        "hasArg <- function(...) list(value=1L); probe <- hasArg; hasArg <- NULL; out <- probe('value')$value",
    ] {
        let diags = check(source);
        assert!(
            diags
                .iter()
                .all(|d| !matches!(d.code, "RY040" | "RY061" | "RY096")),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn proven_deferred_calls_keep_the_existing_models() {
    for callee in ["hasArg", "methods::hasArg", "methods:::hasArg"] {
        let diags = check(&format!("f <- function(x) {callee}(absent)"));
        assert!(
            diags.iter().any(|d| d.code == "RY096"),
            "{callee}: {diags:?}"
        );
        assert!(diags.iter().all(|d| d.code != "RY010"), "{diags:?}");
    }
    for callee in ["on.exit", "base::on.exit", "base:::on.exit"] {
        let diags = check(&format!(
            "f <- function() {{ {callee}(later); later <- 1L }}"
        ));
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "{callee}: {diags:?}"
        );
        let diags = check(&format!("f <- function() {callee}(never_bound)"));
        assert!(
            diags.iter().any(|d| d.code == "RY010"),
            "{callee}: {diags:?}"
        );
    }
}

#[test]
fn deferred_provenance_handles_packages_aliases_and_namespace_operators() {
    for source in [
        "out <- base::hasArg(absent)",
        "out <- methods::on.exit(absent)",
        "out <- otherpkg::hasArg(absent)",
        "out <- otherpkg::on.exit(absent)",
        "probe <- hasArg; out <- probe(absent)",
        "probe <- on.exit; out <- probe(absent)",
        "`::` <- function(pkg,name) function(...) list(value=1L); out <- methods::hasArg(absent)",
        "`::` <- function(pkg,name) function(...) list(value=1L); out <- base::on.exit(absent)",
        "`methods::hasArg` <- function(...) 1L; out <- 'methods::hasArg'(absent)",
        "`base::on.exit` <- function(...) 1L; out <- 'base::on.exit'(absent)",
    ] {
        let (diags, scope) = check_with_scope(source);
        assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque, "{source}");
        assert!(
            diags.iter().all(|d| !matches!(d.code, "RY010" | "RY096")),
            "{source}: {diags:?}"
        );
    }
    for (name, package) in [("hasArg", "methods"), ("on.exit", "base")] {
        let mut checker = Checker::new("test.R");
        checker.set_imported_from(HashMap::from([(name.into(), package.into())]));
        let (_, scope) = checker.check_with_scope(&parse_file(
            "test.R",
            &format!("detach('package:methods'); out <- {name}('x')"),
        ));
        assert_eq!(
            scope.get("out").unwrap().mode,
            if name == "hasArg" {
                Mode::Logical
            } else {
                Mode::Null
            }
        );
    }
}

#[test]
fn deferred_stub_overrides_keep_their_contracts() {
    for (package, name) in [
        ("methods", "hasArg"),
        ("base", "on.exit"),
        ("otherpkg", "hasArg"),
        ("otherpkg", "on.exit"),
    ] {
        let json = format!(
            r#"{{"version":"t","functions":{{"{name}":{{"params":["..."],"return":{{"mode":"character","length":"1"}}}}}}}}"#
        );
        let (_, scope) = check_with_stubs(
            &format!("out <- {package}::{name}('x')"),
            &[(&format!("{package}.json"), &json)],
        );
        assert_eq!(
            scope.get("out").unwrap().mode,
            Mode::Character,
            "{package}::{name}"
        );
    }
}

#[test]
fn opaque_deferred_calls_discard_caller_effects() {
    for source in [
        "f <- function(on.exit) { x <- 'before'; on.exit(assign('x',1L)); x+1L }; out <- f(function(expr)expr)",
        "f <- function(hasArg) { x <- 'before'; hasArg(); x+1L }; out <- f(function()assign('x',1L,parent.frame()))",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn ambient_uncertainty_keeps_deferred_compatibility_without_hasarg_proof() {
    for prefix in ["library(unknownpkg)", "detach('package:methods')"] {
        let (diags, scope) = check_with_scope(&format!(
            "{prefix}; out <- hasArg(absent); f <- function() hasArg(absent)"
        ));
        assert_eq!(scope.get("out").unwrap().mode, Mode::Logical);
        assert!(
            diags.iter().all(|d| !matches!(d.code, "RY010" | "RY096")),
            "{diags:?}"
        );
        let diags = check(&format!(
            "{prefix}; f <- function() {{ on.exit(later); later <- 1L }}"
        ));
        assert!(diags.iter().all(|d| d.code != "RY010"), "{diags:?}");
    }
}
