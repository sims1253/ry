use super::*;

#[test]
fn structure_matches_data_and_preserves_list_columns() {
    for call in [
        "structure(list(a = 1L), class = 'widget')",
        "base::structure(.Data = list(a = 1L), class = 'widget')",
        "base::structure(`.Data` = list(a = 1L), class = 'widget')",
        "base:::structure(.D = list(a = 1L), class = 'widget')",
        "base::structure(x = 'attribute', .Data = list(a = 1L), class = 'widget')",
        "base::structure(.D = 'attribute', .Data = list(a = 1L), class = 'widget')",
    ] {
        let (diags, scope) = check_with_scope(&format!("out <- {call}\nfield <- out$a"));
        assert!(diags.is_empty(), "{call}: {diags:?}");
        let out = scope.get("out").unwrap();
        assert_eq!(out.mode, Mode::List, "{call}: {out:?}");
        assert!(out.class.contains("widget"), "{call}: {out:?}");
        assert_eq!(scope.get("field").unwrap().mode, Mode::Integer);
    }
}

#[test]
fn structure_uncertain_binding_does_not_pick_a_later_payload() {
    for call in [
        "base::structure(x = 1L, class = 'widget')",
        "base::structure(.Data = , class = 'widget')",
        "base::structure(1L, other = , class = 'widget')",
        "base::structure(NULL, class = 'widget')",
        "base::structure(unknown(), 1L, class = 'widget')",
        "base::structure(.Data = 1L, .Data = 'x', class = 'widget')",
        "base::structure(.D = 1L, .Da = 'x', class = 'widget')",
        "base::structure(1L, class = 'first', class = 'last')",
    ] {
        let (_, scope) = check_with_scope(&format!("out <- {call}"));
        let out = scope.get("out").unwrap();
        assert_eq!(out.mode, Mode::Opaque, "{call}: {out:?}");
        assert!(!out.class.known, "{call}: {out:?}");
    }
    for control in ["recursive = 'TRUE'", "use.names = 'TRUE'"] {
        let (_, scope) = check_with_scope(&format!(
            "out <- base::structure(1L, class = base::c('widget', {control}))"
        ));
        assert!(!scope.get("out").unwrap().class.known, "{control}");
    }
    let (_, scope) = check_with_scope("out <- base::structure(unknown(), class = 'widget')");
    assert_eq!(scope.get("out").unwrap().mode, Mode::Opaque);
    assert!(scope.get("out").unwrap().class.contains("widget"));
    let (_, scope) = check_with_scope(
        "f <- function(...) base::structure(..., .Data = list(a = 1L), class = 'widget')\nout <- f()",
    );
    assert!(!scope.get("out").unwrap().class.known);
}

#[test]
fn structure_evaluates_payload_before_attributes_and_class() {
    let (diags, scope) = check_with_scope(
        "out <- base::structure(class = { marker <- 'class'; 'widget' }, .Data = { marker <- 1L; list(a = 1L) })\nafter <- marker\nother <- base::structure(1L, class = missing_class)",
    );
    assert_eq!(scope.get("after").unwrap().mode, Mode::Character);
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains("missing_class")),
        "{diags:?}"
    );
}

#[test]
fn structure_name_attributes_drop_stale_schema_but_class_keeps_it() {
    for attr in ["names = 'b'", ".Names = 'b'", "names = NULL"] {
        let (_, scope) = check_with_scope(&format!(
            "out <- base::structure(list(a = 1L), {attr}, class = 'widget')"
        ));
        let out = scope.get("out").unwrap();
        assert!(out.columns.is_none(), "{attr}: {out:?}");
        assert!(out.class.contains("widget"));
    }
    let (_, scope) =
        check_with_scope("out <- base::structure(list(a = 1L), dim = c(1L, 1L), class = 'widget')");
    assert!(scope.get("out").unwrap().columns.is_some());
}

#[test]
fn structure_class_vectors_require_base_c_and_handle_removal() {
    let (_, scope) = check_with_scope(
        "out <- base::structure(1L, class = base::c('a', 'b'))\nplain <- base::structure(out, class = NULL)\nfactor_value <- base::structure(1, class = 'factor')",
    );
    assert!(scope.get("out").unwrap().class.contains("b"));
    assert_eq!(scope.get("plain").unwrap().class, ClassVector::empty());
    assert_eq!(scope.get("factor_value").unwrap().mode, Mode::Integer);
    let (_, scope) = check_with_scope(
        "out <- base::structure(1L, class = c('widget'), later = { c <- function(...) 'other'; 1L })",
    );
    assert!(scope.get("out").unwrap().class.contains("widget"));
    for source in [
        "c <- function(...) 'other'; out <- base::structure(1L, class = c('widget'))",
        "out <- base::structure(1L, earlier = { c <- function(...) 'other'; 1L }, class = c('widget'))",
        "f <- function(c) base::structure(1L, class = c('widget')); out <- f()",
        "f <- function(c) base::structure(1L, class = c('widget')); out <- f(function(...) 'actual')",
        "f <- function(structure) structure(1L, class = 'widget'); out <- f(function(...) 'custom')",
    ] {
        let (_, scope) = check_with_scope(source);
        assert!(
            !scope.get("out").unwrap().class.contains("widget"),
            "{source}: {:?}",
            scope.get("out")
        );
    }
}

#[test]
fn structure_constructor_respects_callee_provenance_and_base_stubs() {
    let (_, scope) = check_with_scope(
        "structure <- function(...) 'custom'\nout <- structure(1L, class = 'widget')\nbase_out <- base::structure(1L, class = 'widget')",
    );
    assert_eq!(scope.get("out").unwrap().mode, Mode::Character);
    assert!(!scope.get("out").unwrap().class.contains("widget"));
    assert!(scope.get("base_out").unwrap().class.contains("widget"));
    let stub = r#"{"version":"t","functions":{"structure":{"params":["..."],"return":{"mode":"character","length":"1"}}}}"#;
    for (file, call) in [
        ("base.json", "base::structure"),
        ("base.json", "structure"),
        ("otherpkg.json", "otherpkg::structure"),
    ] {
        let (_, scope) = check_with_stubs(
            &format!("out <- {call}(1L, class = 'widget')"),
            &[(file, stub)],
        );
        let out = scope.get("out").unwrap();
        assert_eq!(out.mode, Mode::Character, "{file}: {out:?}");
        assert!(!out.class.contains("widget"));
    }
}

#[test]
fn structure_ambiguous_search_path_does_not_borrow_base_payload_facts() {
    for setup in ["library(unmodeledpkg)", "load('unknown.RData')"] {
        let (diags, scope) = check_with_scope(&format!(
            "{setup}\nout <- structure(missing_payload, class = missing_class)"
        ));
        assert!(diags.is_empty(), "{setup}: {diags:?}");
        let out = scope.get("out").unwrap();
        assert_eq!(out.mode, Mode::Opaque);
        assert!(!out.class.known);
    }
}
