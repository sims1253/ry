use ry_checker::{Checker, Diagnostic, Fix};
use ry_core::{RParser, Span};
use serde_json::{Value, json};

struct Case {
    code: &'static str,
    source: &'static str,
}

const CASES: &[Case] = &[
    Case {
        code: "RY032",
        source: "f <- function(x) x && c(TRUE, FALSE)\n",
    },
    Case {
        code: "RY034",
        source: "f <- function(x) x == NA\n",
    },
    Case {
        code: "RY090",
        source: "length(xx = 1L)\n",
    },
    Case {
        code: "RY093",
        source: "length(c(1L, 2L) > 0L)\n",
    },
    Case {
        code: "RY100",
        source: "f <- function(x) abs(x > 0L)\n",
    },
    Case {
        code: "RY101",
        source: "args <- list(font = \"mono\")\nidentical(args[\"font\"], \"mono\")\n",
    },
    Case {
        code: "RY102",
        source: "list(ok = 1L, \"wi\\\"dget\" <- 2L)\n",
    },
    Case {
        code: "RY103",
        source: "f <- function(x) if (class(x) == \"wi\\\"dget\") 1L else 2L\n",
    },
];

fn check(source: &str) -> Vec<Diagnostic> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse("fix.R", source).expect("parse source");
    let mut checker = Checker::new("fix.R");
    checker.check(&file);
    checker.take_diagnostics()
}

fn apply(source: &str, fix: &Fix) -> String {
    assert!(fix.span.start <= fix.span.end);
    assert!(source.is_char_boundary(fix.span.start));
    assert!(source.is_char_boundary(fix.span.end));
    format!(
        "{}{}{}",
        &source[..fix.span.start],
        fix.replacement,
        &source[fix.span.end..]
    )
}

fn normalized(diagnostics: &[Diagnostic]) -> Vec<Value> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            json!({
                "code": diagnostic.code,
                "severity": diagnostic.severity.as_str(),
                "span": [diagnostic.span.start, diagnostic.span.end],
                "message": diagnostic.message,
                "fix": diagnostic.fix.as_ref().map(|fix| json!({
                    "span": [fix.span.start, fix.span.end],
                    "replacement": fix.replacement,
                })),
            })
        })
        .collect()
}

#[test]
fn every_concrete_suggestion_is_a_structured_parse_clean_fix() {
    let mut snapshots = Vec::new();
    for case in CASES {
        let before = check(case.source);
        let diagnostic = before
            .iter()
            .find(|diagnostic| diagnostic.code == case.code)
            .unwrap_or_else(|| panic!("{} did not fire: {before:?}", case.code));
        let fix = diagnostic
            .fix
            .as_ref()
            .unwrap_or_else(|| panic!("{} did not offer a structured fix", case.code));
        let edited = apply(case.source, fix);

        let mut parser = RParser::new().expect("parser init");
        let parsed = parser
            .parse("fixed.R", &edited)
            .expect("parse edited source");
        assert!(
            parsed.parse_errors.is_empty(),
            "{} fix produced RY000 parse regions: {:?}\nsource: {edited}",
            case.code,
            parsed.parse_errors,
        );

        let after = check(&edited);
        assert!(
            !after.iter().any(|diagnostic| diagnostic.code == case.code
                && diagnostic.span.start <= fix.span.start
                && diagnostic.span.end >= fix.span.start),
            "{} still fires at the replaced location after applying {fix:?}: {after:?}\nsource: {edited}",
            case.code,
        );
        snapshots.push(json!({
            "code": case.code,
            "source": case.source,
            "before": normalized(&before),
            "edited": edited,
            "after": normalized(&after),
        }));
    }
    insta::assert_yaml_snapshot!("structured_fix_oracle", snapshots);
}

#[test]
fn ry032_does_not_offer_vectorizing_fix_in_scalar_conditions() {
    for source in [
        "f <- function(x) if (x && c(TRUE, FALSE)) 1L else 2L\n",
        "f <- function(x) while (x || c(TRUE, FALSE)) break\n",
        "f <- function(x) if (!(x && c(TRUE, FALSE))) 1L else 2L\n",
    ] {
        let diagnostics = check(source);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "RY032")
            .unwrap_or_else(|| panic!("RY032 did not fire: {diagnostics:?}"));
        assert!(
            diagnostic.fix.is_none(),
            "scalar condition must keep the warning but not offer a vectorizing fix: {diagnostic:?}"
        );
    }
}

#[test]
fn ry032_guard_warning_does_not_offer_an_eager_evaluation_fix() {
    let source = "f <- function(x) is.null(x) || x == \"a\"\n";
    let diagnostics = check(source);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY032")
        .unwrap_or_else(|| panic!("guarded parameter did not emit RY032: {diagnostics:?}"));
    assert!(
        diagnostic.fix.is_none(),
        "a guard-based short circuit must not be rewritten to eager evaluation: {diagnostic:?}"
    );
}

#[test]
fn ry103_rhs_fix_uses_scope_after_short_circuit_lhs() {
    let source = "custom <- function(x) \"widget\"\nf <- function(x) (class <- custom) && class(x) == \"widget\"\n";
    let diagnostics = check(source);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY103")
        .unwrap_or_else(|| panic!("RHS class comparison did not emit RY103: {diagnostics:?}"));
    assert!(
        diagnostic.fix.is_none(),
        "the LHS rebinds class before the RHS executes: {diagnostic:?}"
    );
}

#[test]
fn ry032_fix_targets_the_syntax_operator_not_comment_text() {
    for (source, expected) in [
        (
            "f <- function(x) (x # misleading &&\n  && c(TRUE, FALSE))\n",
            "f <- function(x) (x # misleading &&\n  & c(TRUE, FALSE))\n",
        ),
        (
            "f <- function(x) (x # misleading ||\n  || c(TRUE, FALSE))\n",
            "f <- function(x) (x # misleading ||\n  | c(TRUE, FALSE))\n",
        ),
    ] {
        let diagnostics = check(source);
        let fix = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "RY032")
            .and_then(|diagnostic| diagnostic.fix.as_ref())
            .unwrap_or_else(|| panic!("RY032 did not offer a fix: {diagnostics:?}"));
        assert_eq!(apply(source, fix), expected);
    }
}

#[test]
fn comparison_fixes_preserve_inter_operand_comments() {
    for (code, source, expected) in [
        (
            "RY093",
            "length(c(1L, 2L) # why compare here\n > 0L)\n",
            "length(c(1L, 2L) # why compare here\n ) > 0L\n",
        ),
        (
            "RY100",
            "f <- function(x) abs(x > # threshold\n 0L)\n",
            "f <- function(x) abs(x) > # threshold\n 0L\n",
        ),
    ] {
        let diagnostics = check(source);
        let fix = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .and_then(|diagnostic| diagnostic.fix.as_ref())
            .unwrap_or_else(|| panic!("{code} did not offer a fix: {diagnostics:?}"));
        assert_eq!(apply(source, fix), expected);
    }
}

#[test]
fn parenthesized_comparison_fixes_are_parse_clean() {
    for (code, source) in [
        ("RY093", "length((c(1L, 2L) > 0L))\n"),
        ("RY100", "f <- function(x) abs(((x) > (0L)))\n"),
    ] {
        let diagnostics = check(source);
        let fix = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .and_then(|diagnostic| diagnostic.fix.as_ref())
            .unwrap_or_else(|| panic!("{code} did not offer a fix: {diagnostics:?}"));
        let edited = apply(source, fix);
        let mut parser = RParser::new().expect("parser init");
        let parsed = parser
            .parse("fixed.R", &edited)
            .expect("parse fixed source");
        assert!(
            parsed.parse_errors.is_empty(),
            "{code} produced invalid parenthesized output: {edited}"
        );
        assert!(
            !check(&edited)
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "{code} still fires after applying fix: {edited}"
        );
    }
}

#[test]
fn ry102_fix_only_replaces_the_assignment_operator() {
    let source = "list(\"a\" <- # keep this explanation\n  1L)\n";
    let diagnostics = check(source);
    let fix = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY102")
        .and_then(|diagnostic| diagnostic.fix.as_ref())
        .unwrap_or_else(|| panic!("RY102 did not offer a fix: {diagnostics:?}"));
    assert_eq!(
        apply(source, fix),
        "list(\"a\" = # keep this explanation\n  1L)\n"
    );
    assert_eq!(&source[fix.span.start..fix.span.end], "<-");
}

#[test]
fn ry103_only_fixes_calls_known_to_be_base_class() {
    for source in [
        "f <- function(x) if (otherpkg::class(x) == \"widget\") 1L else 2L\n",
        "class <- function(x) \"widget\"\nf <- function(x) if (class(x) == \"widget\") 1L else 2L\n",
        "f <- function(x, class) if (class(x) == \"widget\") 1L else 2L\n",
        "library(otherpkg)\nf <- function(x) if (class(x) == \"widget\") 1L else 2L\n",
    ] {
        let diagnostics = check(source);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "RY103")
            .unwrap_or_else(|| panic!("RY103 did not fire: {diagnostics:?}"));
        assert!(
            diagnostic.fix.is_none(),
            "unsafe class callee must retain the warning without a fix: {diagnostic:?}"
        );
    }

    let source = "f <- function(x) if (base::class(x) == \"widget\") 1L else 2L\n";
    let diagnostics = check(source);
    let fix = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY103")
        .and_then(|diagnostic| diagnostic.fix.as_ref())
        .unwrap_or_else(|| panic!("base::class should have a safe RY103 fix: {diagnostics:?}"));
    assert_eq!(
        apply(source, fix),
        "f <- function(x) if (inherits(x, \"widget\")) 1L else 2L\n"
    );
}

#[test]
fn ry103_does_not_fix_a_comparison_between_two_class_calls() {
    let source = "f <- function(x, y) if (class(x) == class(y)) 1L else 2L\n";
    let diagnostics = check(source);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY103")
        .unwrap_or_else(|| panic!("RY103 did not fire: {diagnostics:?}"));
    assert!(
        diagnostic.fix.is_none(),
        "there is no single class name operand for inherits(): {diagnostic:?}"
    );
}

#[test]
fn ry103_escaped_string_fix_is_not_reconstructed_from_unescaped_ast_value() {
    let source = r#"f <- function(x) if (class(x) == "quote: \" slash: \\") 1L else 2L
"#;
    let diagnostics = check(source);
    let fix = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY103")
        .and_then(|diagnostic| diagnostic.fix.as_ref())
        .expect("RY103 structured fix");
    let edited = apply(source, fix);
    let mut parser = RParser::new().unwrap();
    let parsed = parser.parse("fixed.R", &edited).unwrap();
    assert!(parsed.parse_errors.is_empty(), "unparseable fix: {edited}");
    assert!(edited.contains(r#"inherits(x, "quote: \" slash: \\")"#));
}

#[test]
fn fix_is_minimal_public_data() {
    let fix = Fix {
        span: Span::new(1, 2, 0, 1),
        replacement: "x".to_string(),
    };
    assert_eq!(fix.span.start, 1);
    assert_eq!(fix.replacement, "x");
}

#[test]
fn ry090_tied_nearest_parameters_warn_without_a_structured_fix() {
    let diagnostics = check("f <- function(cat, car) 1L\nf(cap = 1L)\n");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY090")
        .expect("RY090 warning for unmatched named argument");

    assert!(
        diagnostic.fix.is_none(),
        "tied suggestion must not be fixed: {diagnostic:?}"
    );
}

#[test]
fn ry090_unique_parameter_fix_replaces_only_the_argument_name() {
    let source = "f <- function(length) 1L\nf(lenght # keep\n = 1L)\n";
    let diagnostics = check(source);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "RY090")
        .expect("RY090 warning for unmatched named argument");
    let fix = diagnostic
        .fix
        .as_ref()
        .expect("unique suggestion must be fixed");

    assert_eq!(&source[fix.span.start..fix.span.end], "lenght");
    assert_eq!(
        apply(source, fix),
        "f <- function(length) 1L\nf(length # keep\n = 1L)\n"
    );
}
