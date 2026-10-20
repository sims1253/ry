use super::*;

fn mode_of(source: &str, name: &str) -> (Mode, Length, ClassVector) {
    let (diagnostics, scope) = check_with_scope(source);
    let ty = scope
        .get(name)
        .unwrap_or_else(|| panic!("{source}: {diagnostics:?}"));
    (ty.mode, ty.length, ty.class.clone())
}

#[test]
fn quote_of_symbols_and_calls_is_classless_language() {
    for (source, length) in [
        ("q <- quote(a)", Length::One),
        ("q <- base::quote(a)", Length::One),
        ("q <- quote(expr = a)", Length::Nonempty),
        ("q <- quote((a))", Length::Nonempty),
        ("q <- quote(+a)", Length::Nonempty),
        ("q <- quote(a + b)", Length::Nonempty),
        ("q <- quote(f(x))", Length::Nonempty),
        ("q <- quote(-x)", Length::Nonempty),
        ("q <- quote(x$y)", Length::Nonempty),
        ("q <- quote(function(x) x)", Length::Nonempty),
        ("q <- quote(if (a) b)", Length::Nonempty),
        ("q <- quote({a})", Length::Nonempty),
    ] {
        assert_eq!(
            mode_of(source, "q"),
            (Mode::Language, length, ClassVector::empty()),
            "{source}"
        );
    }
}

#[test]
fn quote_without_a_language_proof_keeps_the_ordinary_result() {
    for source in [
        // `quote(1)` is the number itself.
        "q <- quote(1)",
        "q <- quote('a')",
        "q <- quote(NULL)",
        // Masked, aliased, or formal heads prove nothing.
        "quote <- function(x) 1; q <- quote(a)",
        "f <- function(quote) { q <- quote(a); q }; q <- f(identity)",
        "my_quote <- quote; q <- my_quote(a)",
        "library(notapkg); q <- quote(a)",
    ] {
        let (diagnostics, scope) = check_with_scope(source);
        assert!(
            scope.get("q").is_none_or(|ty| ty.mode != Mode::Language),
            "{source}: {:?} {diagnostics:?}",
            scope.get("q")
        );
    }
}

#[test]
fn formulas_and_environment_constructors_have_unknown_class() {
    assert_eq!(
        mode_of("fm <- y ~ x", "fm"),
        (Mode::Language, Length::Nonempty, ClassVector::unknown())
    );
    for constructor in crate::semantic_lists::ENVIRONMENT_CONSTRUCTORS {
        let source = format!("e <- {constructor}()");
        assert_eq!(
            mode_of(&source, "e"),
            (Mode::Environment, Length::Unknown, ClassVector::unknown()),
            "{source}"
        );
    }
    // `environment()` may return NULL for a primitive.
    assert_ne!(mode_of("e <- environment()", "e").0, Mode::Environment);
}

#[test]
fn language_kinds_change_only_the_known_exceptions() {
    let codes = |source: &str| -> Vec<&'static str> {
        check(source).iter().map(|d| d.code).collect::<Vec<_>>()
    };
    assert_eq!(codes("q <- quote(a + b); q * 2"), ["RY040"]);
    assert_eq!(codes("registry <- new.env(); registry()"), ["RY070"]);
    // `quote(a) == "a"` is TRUE: RY033 needs two atomic operands.
    assert!(codes("quote(a) == 'a'").is_empty());
    // `c` resolves to `base::c` past the environment binding.
    let (_, scope) = check_with_scope("c <- new.env(); v <- c(1, 2)");
    assert_eq!(scope.get("v").map(|ty| ty.mode), Some(Mode::Double));
}
