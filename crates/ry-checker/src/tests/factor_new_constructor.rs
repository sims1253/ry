use super::*;

#[test]
fn custom_factor_and_new_do_not_lend_builtin_constructor_facts() {
    let (diags, _) = check_with_scope(
        "f <- function(factor) { x <- factor(1L); x$a }; out <- f(function(...) list(a=1L))",
    );
    assert!(diags.iter().all(|d| d.code != "RY061"), "{diags:?}");
    let (diags, scope) = check_with_scope(
        "new <- function(...) 1L; `+.widget` <- function(e1,e2) 'wrong'; x <- new('widget'); y <- x+1L; z <- y+1L",
    );
    assert!(diags.iter().all(|d| d.code != "RY040"), "{diags:?}");
    assert!(!scope.get("x").unwrap().class.contains("widget"));
}

#[test]
fn proven_factor_and_methods_new_keep_classes() {
    for call in ["factor(1L)", "base::factor(1L)", "base:::factor(1L)"] {
        let (_, scope) = check_with_scope(&format!("out <- {call}"));
        let out = scope.get("out").unwrap();
        assert_eq!(out.mode, Mode::Integer, "{call}: {out:?}");
        assert!(out.class.contains("factor"));
    }
    for call in [
        "new('widget')",
        "methods::new('widget')",
        "methods:::new('widget')",
    ] {
        let (_, scope) = check_with_scope(&format!("out <- {call}"));
        assert!(scope.get("out").unwrap().class.contains("widget"), "{call}");
    }
    let mut checker = Checker::new("test.R");
    checker.set_imported_from(HashMap::from([("new".into(), "methods".into())]));
    let (_, scope) =
        checker.check_with_scope(&parse_file("test.R", "other(); out <- new('widget')"));
    assert!(scope.get("out").unwrap().class.contains("widget"));
}

#[test]
fn uncertain_constructor_provenance_stays_opaque() {
    for source in [
        "detach('package:methods'); out <- new('widget')",
        "library(unknownpkg); out <- new('widget')",
        "library(unknownpkg); out <- factor(1L)",
        "out <- base::new('widget')",
        "out <- otherpkg::new('widget')",
        "out <- otherpkg::factor(1L)",
        "maker <- new; out <- maker('widget')",
        "maker <- factor; out <- maker(1L)",
        "`::` <- function(pkg,name) function(...) 'custom'; out <- methods::new('widget')",
        "`::` <- function(pkg,name) function(...) 'custom'; out <- base::factor(1L)",
        "`methods::new` <- function(...) 'custom'; out <- \"methods::new\"('widget')",
        "`base::factor` <- function(...) 'custom'; out <- \"base::factor\"(1L)",
    ] {
        let (_, scope) = check_with_scope(source);
        let out = scope.get("out").unwrap();
        assert!(
            !out.class.contains("widget") && !out.class.contains("factor"),
            "{source}: {out:?}"
        );
    }
}

#[test]
fn constructor_package_stubs_override_specialization() {
    for (pkg, name, call) in [
        ("base", "factor", "base::factor(1L)"),
        ("methods", "new", "methods::new('widget')"),
        ("custom", "new", "custom::new('widget')"),
    ] {
        let json = format!(
            r#"{{"version":"t","functions":{{"{name}":{{"params":["..."],"return":{{"mode":"character","length":"1"}}}}}}}}"#
        );
        let (_, scope) = check_with_stubs(
            &format!("out <- {call}"),
            &[(&format!("{pkg}.json"), &json)],
        );
        assert_eq!(
            scope.get("out").unwrap().mode,
            Mode::Character,
            "{pkg}: {:?}",
            scope.get("out")
        );
    }
}

#[test]
fn default_methods_path_survives_class_declarations_but_not_detach() {
    let (_, scope) =
        check_with_scope("setClass('widget', slots=c(x='integer')); out <- new('widget')");
    assert!(scope.get("out").unwrap().class.contains("widget"));
    let (_, scope) = check_with_scope("detach('package:methods'); out <- methods::new('widget')");
    assert!(scope.get("out").unwrap().class.contains("widget"));
}

#[test]
fn unknown_imports_do_not_fall_back_to_base_constructor_stubs() {
    let mut checker = Checker::new("test.R");
    checker.set_imported_from(HashMap::from([("factor".into(), "unknownpkg".into())]));
    let (_, scope) = checker.check_with_scope(&parse_file("test.R", "out <- factor(1L)"));
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
    let (_, scope) = check_with_scope("out <- methods::factor(1L)");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
}

#[test]
fn uncertain_namespace_constructors_do_not_force_arguments() {
    let diags = check(
        "`::` <- function(pkg,name) function(...) 'custom'; out <- methods::new(missing_class); other <- base::factor(missing_value)",
    );
    assert!(diags.iter().all(|d| d.code != "RY010"), "{diags:?}");
    let (_, scope) = check_with_stubs(
        "`::` <- function(pkg,name) function(...) 1L; out <- custom::new('widget')",
        &[(
            "custom.json",
            r#"{"version":"t","functions":{"new":{"params":["..."],"return":{"mode":"character","length":"1"}}}}"#,
        )],
    );
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
}

#[test]
fn methods_new_binds_and_forces_class_before_dots() {
    for call in [
        "methods::new(value='wrong', Class='widget')",
        "methods::new(value='wrong', Cl='widget')",
        "methods::new(C='attribute', Class='widget')",
        "methods::new(`Class`='widget')",
    ] {
        let (diags, scope) = check_with_scope(&format!("out <- {call}"));
        assert!(scope.get("out").unwrap().class.contains("widget"), "{call}");
        assert!(diags.iter().all(|d| d.code != "RY010"), "{call}: {diags:?}");
    }
    for call in [
        "methods::new(Class='widget', Class='other')",
        "methods::new(C='widget', Cl='other')",
        "methods::new(Class='widget', ...)",
        "methods::new(Class=)",
        "methods::new()",
    ] {
        let (_, scope) = check_with_scope(&format!("out <- {call}"));
        assert!(!scope.get("out").unwrap().class.known, "{call}");
    }
    let (diags, scope) = check_with_scope(
        "marker <- 1L; out <- methods::new(Class={marker <- 'forced'; missing_class})",
    );
    assert_eq!(scope.get("marker").unwrap().mode, Mode::Character);
    assert!(diags.iter().any(|d| d.code == "RY010"), "{diags:?}");
}

#[test]
fn intrinsic_new_values_do_not_acquire_explicit_dispatch_classes() {
    let (diags, scope) = check_with_scope(
        "`+.integer` <- function(e1,e2) 'wrong'; x <- methods::new('integer'); y <- x+1L; z <- y+1L",
    );
    assert!(diags.iter().all(|d| d.code != "RY040"), "{diags:?}");
    assert!(!scope.get("x").unwrap().class.known);
}

#[test]
fn opaque_constructor_effects_do_not_leave_stale_local_or_caller_facts() {
    for source in [
        "f <- function(factor) { x <- 'before'; factor({x <- 1L; 1L}); x + 1L }; out <- f(function(x) x)",
        "f <- function(factor) { x <- 'before'; factor(assign('x', 1L, envir=environment())); x }; out <- f(function(x) x) + 1L",
        "touch <- function() assign('x', 1L, envir=parent.frame()); f <- function(factor) { x <- 'before'; factor(touch()); x }; out <- f(function(x) x) + 1L",
        "f <- function(factor) { x <- 'before'; factor(); x }; out <- f(function() assign('x', 1L, envir=parent.frame())) + 1L",
    ] {
        let (diags, _) = check_with_scope(source);
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "{source}: {diags:?}"
        );
    }
}

#[test]
fn proven_constructor_traversal_retains_argument_effects_and_inert_facts() {
    for source in [
        "x <- 1L; out <- base::factor(1L); after <- x + 1L",
        "x <- 1L; out <- methods::new('widget', value=1L); after <- x + 1L",
        "x <- 'before'; out <- methods::new('widget', value={x <- 1L; 1L}); after <- x + 1L",
    ] {
        let (diags, scope) = check_with_scope(source);
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "{source}: {diags:?}"
        );
        assert_eq!(scope.get("after").unwrap().mode, Mode::Integer, "{source}");
    }
}
