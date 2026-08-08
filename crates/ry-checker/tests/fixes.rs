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
