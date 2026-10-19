//! Declaration checking through real `#|` typehint source where that adapter
//! can express the contract, and through hand-built records for vocabulary
//! no adapter produces (storage modes, unions, variadics, return constraints,
//! checked defaults, provider conflicts, and evidence kinds).

use std::path::Path;
use std::sync::Arc;

use ry_checker::{Checker, DeclarationFinding, DeclarationFindingKind, Project};
use ry_config::config::ScopedPaths;
use ry_core::ast::{Expr, Stmt};
use ry_core::declarations::{
    AssignmentSemantics, AtomicMode, DeclarationRecord, DeclarationSource, DeclarationTarget,
    DeclaredParameter, DeclaredSignature, EvaluationSemantics, EvidenceUse, ParameterForm,
    ResidualConstraint, SupplyStatus, Translation, TypeExpr,
};
use ry_core::types::Mode;
use ry_core::{RParser, SourceFile, Span};

fn parse(path: &str, source: &str) -> SourceFile {
    RParser::new().unwrap().parse(path, source).unwrap()
}

fn named_function_span(file: &SourceFile, name: &str) -> Span {
    file.stmts
        .iter()
        .find_map(|statement| match statement {
            Stmt::Assign {
                target: Expr::Ident { name: bound, .. },
                value: Expr::Function { span, .. },
                ..
            } if bound == name => Some(*span),
            _ => None,
        })
        .expect("fixture has named top-level function")
}

fn record(
    file: &SourceFile,
    function: &str,
    parameter: (&str, AtomicMode, SupplyStatus),
    return_mode: Option<AtomicMode>,
) -> DeclarationRecord {
    let span = named_function_span(file, function);
    DeclarationRecord {
        source: DeclarationSource {
            provider: "structured-test".into(),
            provider_version: Some("1".into()),
            path: "structured-test-input".into(),
            span: Span::default(),
            raw: "structured test record".into(),
            target: DeclarationTarget {
                path: file.path.clone(),
                definition: span,
                display_name: Some(function.into()),
            },
        },
        translation: Translation::Exact(DeclaredSignature {
            parameters: vec![DeclaredParameter {
                name: parameter.0.into(),
                form: ParameterForm::Ordinary,
                supplied: parameter.2,
                evaluation: EvaluationSemantics::Promise,
                constraint: Some(TypeExpr::atomic(parameter.1)),
            }],
            return_constraint: return_mode.map(TypeExpr::atomic),
            assignment: AssignmentSemantics::EntryOnly,
        }),
        evidence: EvidenceUse::AdoptedContract,
        assumptions: vec![],
    }
}

/// Check `source` with its `#|` comments adopted, as `annotations.typehint` does.
fn check_adopted(path: &str, source: &str) -> (SourceFile, Checker) {
    let file = parse(path, source);
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(adopted_records(&file));
    checker.check(&file);
    (file, checker)
}

fn adopted_records(file: &SourceFile) -> Vec<DeclarationRecord> {
    let scope = ScopedPaths::new(Path::new("."), &["**".into()]).unwrap();
    let records = ry_checker::typehint::read_records(file, &scope);
    // A misattached or untranslated annotation would make "no mismatch"
    // expectations pass vacuously.
    assert!(
        !records.is_empty()
            && records
                .iter()
                .all(|record| matches!(record.translation, Translation::Exact(_))),
        "{} must adopt exact typehint contracts: {records:?}",
        file.path
    );
    records
}

fn kinds(checker: &Checker) -> Vec<DeclarationFindingKind> {
    checker
        .declaration_findings()
        .iter()
        .map(|finding| finding.kind)
        .collect()
}

fn mismatch_sources<'a>(checker: &Checker, file: &'a SourceFile) -> Vec<&'a str> {
    checker
        .declaration_findings()
        .iter()
        .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
        .map(|finding| &file.source[finding.span.start..finding.span.end])
        .collect()
}

#[test]
fn captured_helper_effects_do_not_borrow_caller_local_proofs() {
    for (setup, body, local) in [
        (
            "h <- function() get(\"assign\")(\"f\", function(x) x, envir = .GlobalEnv)",
            "h()",
            "h <- function() 1L",
        ),
        (
            "h <- function() get(\"assign\")(\"f\", function(x) x, envir = .GlobalEnv)",
            "h()",
            "pure <- function() 1L; h <- pure",
        ),
        (
            "makeActiveBinding(\"h\", function() { get(\"assign\")(\"f\", function(x) x, envir = .GlobalEnv); 1L }, .GlobalEnv)",
            "h",
            "h <- function() 1L",
        ),
        (
            "makeActiveBinding(\"h\", function() { get(\"assign\")(\"f\", function(x) x, envir = .GlobalEnv); 1L }, .GlobalEnv)",
            "h",
            "pure <- function() 1L; h <- pure",
        ),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\n{setup}\ng <- function() {{ {body} }}\nouter <- function() {{ {local}; g(); f(\"bad\") }}\nouter()\n"
        );
        let (file, checker) = check_adopted("captured-helper-scope.R", &source);
        assert!(
            mismatch_sources(&checker, &file).is_empty(),
            "setup: {setup}, caller local: {local}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn unadopted_records_preserve_ordinary_local_function_diagnostics() {
    let file = parse(
        "unadopted-effects.R",
        "f <- function(x) x\nouter <- function() { g <- function() c(TRUE, FALSE); base::identity(1L); if (g()) 1L }\nouter()\n",
    );
    let baseline = Checker::new(&file.path).check(&file).to_vec();
    assert!(baseline.iter().any(|diagnostic| diagnostic.code == "RY002"));
    for evidence in [
        EvidenceUse::DocumentationCandidate,
        EvidenceUse::RuntimeGuard,
    ] {
        let mut declaration = record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            None,
        );
        declaration.evidence = evidence;
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![declaration.clone()]);
        assert_eq!(checker.check(&file), baseline, "evidence: {evidence:?}");
        assert_eq!(checker.declaration_records(), &[declaration]);
        assert!(checker.declaration_findings().is_empty());
    }
}

#[test]
fn global_helper_project_proofs_respect_unknown_current_bindings() {
    let (file, checker) = check_adopted(
        "unknown-global-helper-binding.R",
        r#"h <- function() 1L
get("assign")("h", function() get("assign")("f", function(x) x, envir = .GlobalEnv), envir = .GlobalEnv)
f <- function(x) {
  #| x integer
  x
}
g <- function() h()
g()
f("bad")
"#,
    );
    assert!(
        mismatch_sources(&checker, &file).is_empty(),
        "findings: {:?}",
        checker.declaration_findings()
    );
}

#[test]
fn captured_helper_project_proofs_respect_unknown_current_bindings() {
    let (file, checker) = check_adopted(
        "unknown-helper-binding.R",
        r#"f <- function(x) x
h <- function() 1L
outer <- function() {
    get("assign")("h", function() get("assign")("f", function(x) x, envir = parent.env(environment())), envir = .GlobalEnv)
    f <- function(x) {
        #| x integer
        x
    }
    g <- function() h()
    g()
    f("bad")
}
outer()
"#,
    );
    assert!(
        mismatch_sources(&checker, &file).is_empty(),
        "findings: {:?}",
        checker.declaration_findings()
    );
}

#[test]
fn adopted_entry_and_return_are_independent_and_rebinding_is_ordinary() {
    let file = parse(
        "entry.R",
        "f <- function(x) { x <- \"rebound\"; x }\ng <- function(x) { x }\nh <- function(x) { \"result\" }\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.enable_scope_capture();
    checker.set_declaration_records(vec![
        record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            None,
        ),
        record(
            &file,
            "g",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            Some(AtomicMode::Integer),
        ),
        record(
            &file,
            "h",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            Some(AtomicMode::Integer),
        ),
    ]);
    checker.check(&file);
    let scopes = checker.take_scope_records();
    let rebound = scopes
        .iter()
        .find(|scope| scope.name.as_deref() == Some("f"))
        .unwrap();
    assert_eq!(rebound.scope.bindings["x"].mode, Mode::Character);
    let adopted = scopes
        .iter()
        .find(|scope| scope.name.as_deref() == Some("g"))
        .unwrap();
    assert_eq!(adopted.scope.bindings["x"].mode, Mode::Integer);
    // `g`'s independent return is unknown; its declaration cannot prove
    // itself correct or incorrect. `h`'s literal return is a proven clash.
    let findings = checker.declaration_findings();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, DeclarationFindingKind::Mismatch);
    assert!(findings[0].message.contains("return of `h`"));
}

#[test]
fn call_matching_respects_names_defaults_missing_and_dots() {
    let file = parse(
        "calls.R",
        "f <- function(longname, ..., y = 2L) { longname }\nf(long = \"bad\")\nf(1L, y = \"bad\")\nf(y = 2L, longname = 1L)\nf(1L, extra = \"anything\")\nf(, y = 2L)\n",
    );
    let mut declaration = record(
        &file,
        "f",
        ("longname", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let Translation::Exact(signature) = &mut declaration.translation else {
        unreachable!();
    };
    signature.parameters.push(DeclaredParameter {
        name: "...".into(),
        form: ParameterForm::Variadic,
        supplied: SupplyStatus::Unknown,
        evaluation: EvaluationSemantics::Promise,
        constraint: Some(TypeExpr::atomic(AtomicMode::Integer)),
    });
    signature.parameters.push(DeclaredParameter {
        name: "y".into(),
        form: ParameterForm::Ordinary,
        supplied: SupplyStatus::Defaulted,
        evaluation: EvaluationSemantics::Promise,
        constraint: Some(TypeExpr::atomic(AtomicMode::Integer)),
    });
    let mut checker = Checker::new(&file.path);
    checker.enable_scope_capture();
    checker.set_declaration_records(vec![declaration]);
    checker.check(&file);
    let function_scope = checker
        .take_scope_records()
        .into_iter()
        .find(|record| record.name.as_deref() == Some("f"))
        .unwrap();
    assert_eq!(function_scope.scope.bindings["y"].mode, Mode::Integer);
    assert!(
        function_scope
            .scope
            .default_parameter_bindings
            .contains("y")
    );
    let findings = checker.declaration_findings();
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .count(),
        2
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.message.contains("longname"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.message.contains("formal `y`"))
    );
    assert!(findings.iter().any(|finding| {
        finding.kind == DeclarationFindingKind::Partial && finding.message.contains("variadic")
    }));
}

#[test]
fn default_mismatch_is_separate_from_omission_and_known_call_mismatch() {
    let file = parse(
        "defaults.R",
        "f <- function(x = \"wrong\") { x }\nf()\nf(1L)\nf(\"wrong\")\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Defaulted),
        None,
    )]);
    checker.check(&file);
    let mismatches: Vec<_> = checker
        .declaration_findings()
        .iter()
        .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
        .collect();
    assert_eq!(mismatches.len(), 2);
    assert!(
        mismatches
            .iter()
            .any(|finding| finding.message.contains("default for `x`"))
    );
    assert!(
        mismatches
            .iter()
            .any(|finding| finding.message.contains("formal `x`"))
    );
}

/// The public view a CLI or editor check shows for a plainly annotated
/// function: one RY114 for the bad actual and no per-formal RY115 notes.
#[test]
fn plain_annotated_function_reports_only_its_bad_actual() {
    let file = parse(
        "plain.R",
        "f <- function(x, y = 1L) {\n  #| x integer\n  #| y integer\n  x\n}\nf(1L)\nf(\"bad\", y = 2L)\n",
    );
    let mut project = new_project(&[&file], &adopted_records(&file));
    let mut diagnostics = project.check();
    ry_checker::append_declaration_diagnostics(&mut diagnostics, project.declaration_findings());
    let shown = diagnostics
        .iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .map(|diagnostic| {
            (
                diagnostic.code,
                &file.source[diagnostic.span.start..diagnostic.span.end],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(shown, vec![("RY114", "\"bad\"")]);
}

#[test]
fn supplied_only_constraint_checks_only_explicit_actuals() {
    let (file, checker) = check_adopted(
        "supplied-only.R",
        "f <- function(x = \"default\") {\n  #| x integer\n  x\n}\nf()\nf(1L)\nf(\"wrong\")\n",
    );
    // The omitted default is not a supplied actual, and a supplied-only
    // constraint establishes no body type, which is not an incomplete check.
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
    assert_eq!(mismatch_sources(&checker, &file), vec!["\"wrong\""]);
}

#[test]
fn authored_default_is_checked_even_when_every_call_supplies_a_valid_actual() {
    let file = parse(
        "default-all-supplied.R",
        "f <- function(x = \"wrong\") { x }\nf(1L)\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Defaulted),
        None,
    )]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
    assert!(
        checker.declaration_findings()[0]
            .message
            .contains("default for `x`")
    );
}

#[test]
fn eager_top_level_call_never_borrows_a_later_same_name_contract() {
    let (file, checker) = check_adopted(
        "top-level-order.R",
        "f <- function(x) { x }\nf(\"text\")\nf <- function(x) {\n  #| x integer\n  x\n}\nf(\"text\")\n",
    );
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
    let after = file.source.rfind("f(\"text\")").unwrap();
    assert!(checker.declaration_findings()[0].span.start >= after);
}

#[test]
fn sequential_top_level_literals_keep_each_adopted_call_contract() {
    for (second, both) in [
        ("f <- function(x) { x }", false),
        ("f <- function(x) {\n  #| x character\n  x\n}", true),
    ] {
        let source =
            format!("f <- function(x) {{\n  #| x integer\n  x\n}}\nf(\"bad\")\n{second}\nf(1L)\n");
        let (file, checker) = check_adopted("sequential-top.R", &source);
        let first_call = file.source.find("f(\"bad\")").unwrap() + "f(".len();
        let second_call = file.source.find("f(1L)").unwrap() + "f(".len();
        let calls = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| finding.span.start)
            .collect::<Vec<_>>();
        let expected = if both {
            vec![first_call, second_call]
        } else {
            vec![first_call]
        };
        assert_eq!(calls, expected, "{source}");
    }
}

#[test]
fn sequential_nested_literals_keep_each_adopted_call_contract() {
    for (second, both) in [
        ("inner <- function(x) { x }", false),
        (
            "inner <- function(x) {\n    #| x character\n    x\n  }",
            true,
        ),
    ] {
        let source = format!(
            "outer <- function() {{\n  inner <- function(x) {{\n    #| x integer\n    x\n  }}\n  inner(\"bad\")\n  {second}\n  inner(1L)\n}}\nouter()\n"
        );
        let (file, checker) = check_adopted("sequential-nested.R", &source);
        let first_call = file.source.find("inner(\"bad\")").unwrap() + "inner(".len();
        let second_call = file.source.find("inner(1L)").unwrap() + "inner(".len();
        let calls = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| finding.span.start)
            .collect::<Vec<_>>();
        let expected = if both {
            vec![first_call, second_call]
        } else {
            vec![first_call]
        };
        assert_eq!(calls, expected, "{source}");
    }
}

#[test]
fn function_position_skips_nested_nonfunction_but_not_same_environment_overwrite() {
    let (_, nested) = check_adopted(
        "nested-nonfunction.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nouter <- function() { f <- 1L; f(\"bad\") }\nouter()\n",
    );
    assert_eq!(kinds(&nested), vec![DeclarationFindingKind::Mismatch]);

    let (_, overwritten) = check_adopted(
        "same-environment.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nf <- 1L\nf(\"bad\")\n",
    );
    assert!(!kinds(&overwritten).contains(&DeclarationFindingKind::Mismatch));

    for shadow in [
        "f <- function(x) { x }",
        "f <- if (flag) function(x) { x } else 1L",
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function(flag) {{ {shadow}; f(\"bad\") }}\nouter(TRUE)\n"
        );
        let (_, checker) = check_adopted("possible-local-shadow.R", &source);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "shadow: {shadow}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn deferred_body_cannot_use_a_captured_global_literal_after_global_rebinding() {
    for replacement in ["f <- function(x) { x }", "f <- 1L"] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function() {{ f(\"bad\") }}\n{replacement}\nouter()\n"
        );
        let (_, checker) = check_adopted("deferred-global-rebind.R", &source);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "replacement: {replacement}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn nested_closure_reads_live_enclosing_environment() {
    for (rebind, expected) in [
        ("f <- function(x) { x }", vec![]),
        ("", vec![DeclarationFindingKind::Mismatch]),
    ] {
        let source = format!(
            "outer <- function() {{\n  f <- function(x) {{\n    #| x integer\n    x\n  }}\n  inner <- function() {{ f(\"bad\") }}\n  {rebind}\n  inner()\n}}\nouter()\n"
        );
        let (_, checker) = check_adopted("nested-environment.R", &source);
        assert_eq!(kinds(&checker), expected, "rebind: {rebind}");
    }
}

#[test]
fn captured_global_contract_is_invalidated_by_executed_write_positions() {
    for write in [
        "invisible(f <- function(x) { x })",
        "if ({ f <- function(x) { x }; TRUE }) 1L",
        "for (f in list(function(x) { x })) 1L",
        "(f <- function(x) { x })",
        "{ f <- function(x) { x } }",
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function() {{ f(\"bad\") }}\n{write}\nouter()\n"
        );
        let (_, checker) = check_adopted("capture-write-positions.R", &source);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "write: {write}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn later_expression_write_does_not_erase_an_earlier_direct_known_call() {
    let (file, checker) = check_adopted(
        "ordered-expression-write.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nf(\"bad\")\ninvisible(f <- function(x) { x })\n",
    );
    let mismatches = checker
        .declaration_findings()
        .iter()
        .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
        .collect::<Vec<_>>();
    assert_eq!(mismatches.len(), 1);
    assert_eq!(
        &file.source[mismatches[0].span.start..mismatches[0].span.end],
        "\"bad\""
    );
}

#[test]
fn nested_local_write_does_not_invalidate_outer_but_superassignment_can() {
    for (write, mismatch) in [
        ("invisible(function() { f <- function(x) { x } })", true),
        (
            "mutate <- function() { f <- function(x) { x } }; mutate()",
            true,
        ),
        (
            "mutate <- function() { f <<- function(x) { x } }; mutate()",
            false,
        ),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function() {{ f(\"bad\") }}\n{write}\nouter()\n"
        );
        let (_, checker) = check_adopted("nested-write-scope.R", &source);
        assert_eq!(
            kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            mismatch,
            "write: {write}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn nested_capture_tracks_expression_writes_in_its_enclosing_frame() {
    let (_, checker) = check_adopted(
        "nested-expression-write.R",
        "outer <- function() {\n  f <- function(x) {\n    #| x integer\n    x\n  }\n  inner <- function() { f(\"bad\") }\n  if ({ f <- function(x) { x }; TRUE }) 1L\n  inner()\n}\nouter()\n",
    );
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
}

#[test]
fn forced_default_superassignment_can_replace_an_outward_contract() {
    let (_, checker) = check_adopted(
        "default-superassign.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nouter <- function(z = (f <<- function(x) { x })) { z; f(\"bad\") }\nouter()\n",
    );
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
}

#[test]
fn forced_default_local_assignment_can_shadow_an_outward_contract() {
    let (_, checker) = check_adopted(
        "default-local-assign.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nouter <- function(z = (f <- function(x) { x })) { z; f(\"bad\") }\nouter()\n",
    );
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
}

#[test]
fn default_outward_write_is_distinct_from_local_write() {
    for (default, body, mismatch) in [
        (
            "(function() { f <<- function(x) { x } })()",
            "z; f(\"bad\")",
            false,
        ),
        (
            "(function(y = (f <<- function(x) { x })) { y })()",
            "z; f(\"bad\")",
            false,
        ),
        (
            "(function() { (function() { f <<- function(x) { x } })() })()",
            "z; f(\"bad\")",
            false,
        ),
        (
            // An outward write anywhere in a default counts, as it does in a body.
            "function() { f <<- function(x) { x } }",
            "z; f(\"bad\")",
            false,
        ),
        (
            "(function() { f <- function(x) { x } })()",
            "z; f(\"bad\")",
            true,
        ),
        (
            "(function() { f <<- function(x) { x } })()",
            "f(\"bad\")",
            false, // the default may remain unforced; its effect is uncertain
        ),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function(z = {default}) {{ {body} }}\nouter()\n"
        );
        let (_, checker) = check_adopted("default-iife.R", &source);
        assert_eq!(
            kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            mismatch,
            "default: {default}; body: {body}; findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn forced_default_invalidates_only_when_it_contains_an_outward_write() {
    for (default, expected_mismatch) in [
        ("{ g <- function() { f <<- function(x) x }; g() }", false),
        (
            "{ invisible(g <- function() { f <<- function(x) x }); g() }",
            false,
        ),
        ("{ g <- function() { f <<- function(x) x }; g }", false),
        ("{ g <- function() { f <- function(x) x }; g() }", true),
        (
            "{ g <- function() { h <- function() { f <<- function(x) x }; h() }; g() }",
            false,
        ),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function(z = {default}) {{ z; f(\"bad\") }}\nouter()\n"
        );
        let (_, checker) = check_adopted("default-helper.R", &source);
        assert_eq!(
            kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            expected_mismatch,
            "default: {default}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn explicit_global_environment_writes_invalidate_captured_contracts() {
    for (body, expected_mismatch) in [
        ("assign(\"f\", function(x) x, envir = .GlobalEnv)", false),
        ("assign(\"f\", function(x) x, .GlobalEnv)", false),
        ("assign(\"f\", function(x) x, envir = globalenv())", false),
        (
            "env <- .GlobalEnv; assign(\"f\", function(x) x, envir = env)",
            false,
        ),
        (
            "assign(value = function(x) x, x = \"f\", pos = .GlobalEnv)",
            false,
        ),
        ("eval(quote(f <- function(x) x), envir = .GlobalEnv)", false),
        ("eval(quote(f <- function(x) x), .GlobalEnv)", false),
        ("evalq(f <- function(x) x, envir = .GlobalEnv)", false),
        ("assign(\"f\", function(x) x, envir = new.env())", false),
        ("eval(base::quote(1L))", false),
        ("eval(quote(1L), envir = .GlobalEnv)", false),
        ("helper()", true),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nhelper <- function() 1L\nouter <- function() {{ {body}; f(\"bad\") }}\nouter()\n"
        );
        let (_, checker) = check_adopted("global-environment.R", &source);
        assert_eq!(
            kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            expected_mismatch,
            "body: {body}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn eager_calls_use_the_binding_before_but_not_after_a_runtime_installation() {
    for write in [
        "base::assign(\"f\", function(x) x, envir = .GlobalEnv)",
        "assign(\"f\", function(x) x, envir = .GlobalEnv)",
        "base::delayedAssign(\"f\", function(x) x, assign.env = .GlobalEnv)",
        "methods::setGeneric(\"f\", function(x) standardGeneric(\"f\"))",
        "methods::setMethod(\"f\", \"ANY\", function(x) x)",
        "base::rm(f, envir = .GlobalEnv)",
        "base::assign(target, function(x) x, envir = .GlobalEnv)",
        "(function() { f <<- function(x) x })()",
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nf(\"before\")\n{write}\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("eager-installation.R", &source);
        let mismatches = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .collect::<Vec<_>>();
        assert_eq!(
            mismatches.len(),
            1,
            "write: {write}, findings: {:?}",
            checker.declaration_findings()
        );
        assert_eq!(
            &file.source[mismatches[0].span.start..mismatches[0].span.end],
            "\"before\"",
            "write: {write}"
        );
    }
}

#[test]
fn a_known_invoked_helper_invalidates_only_its_outward_writes() {
    for (body, expected_after_mismatch) in [
        ("f <<- function(x) x", false),
        ("`\\x66` <<- function(x) x", false),
        (
            "base::assign(\"f\", function(x) x, envir = .GlobalEnv)",
            false,
        ),
        // Known gap (S5): an ordinary `<<-` can run an active binding's
        // setter (testdata/oracle/typehint_active_setter_replacement.R).
        ("g <<- function(x) x", true),
        ("local <- function(x) x", true),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nmutate <- function() {{ {body} }}\nf(\"before\")\nmutate()\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("invoked-helper-write.R", &source);
        let mismatch_spans = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &file.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>();
        let expected = if expected_after_mismatch {
            vec!["\"before\"", "\"after\""]
        } else {
            vec!["\"before\""]
        };
        assert_eq!(mismatch_spans, expected, "helper body: {body}");
    }
}

#[test]
fn wrapper_alias_and_forced_default_keep_before_call_but_drop_stale_after_call() {
    for (setup, invoke, after_mismatch) in [
        (
            "mutate <- function() f <<- function(x) x\nsaved <- mutate",
            "saved()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function() mutate()",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function() { saved <- mutate; saved() }",
            "bridge()",
            false,
        ),
        (
            "bridge <- function() { inner <- function() f <<- function(x) x; inner() }",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function() (function() mutate())()",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function(z = mutate()) z",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function() do.call(mutate, list())",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function() do.call(\"mutate\", list())",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function() base::do.call(\"mutate\", list())",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function() { helper <- get(\"mutate\"); helper() }",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x",
            "get(\"mutate\")()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x",
            "(function() mutate())()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x",
            "(function() 1L)()",
            true,
        ),
        (
            "mutate <- function() f <<- function(x) x\nsaved <- function() 1L\nbridge <- function(saved) saved()",
            "bridge(mutate)",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nsaved <- function() 1L\nbridge <- function() { saved <- get(\"mutate\"); saved() }",
            "bridge()",
            false,
        ),
        (
            "mutate <- function() f <<- function(x) x\nbridge <- function() 1L",
            "bridge()",
            true,
        ),
        (
            "mutate <- function() f <<- function(x) x\npure <- function() 1L\nbridge <- function() pure()",
            "bridge()",
            false,
        ),
        ("mutate <- function() f <<- function(x) x", "1L", true),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\n{setup}\nf(\"before\")\n{invoke}\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("known-wrapper.R", &source);
        let spans = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &file.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>();
        let expected = if after_mismatch {
            vec!["\"before\"", "\"after\""]
        } else {
            vec!["\"before\""]
        };
        assert_eq!(spans, expected, "setup: {setup}, invoke: {invoke}");
    }
}

#[test]
fn a_checked_call_and_known_pure_helper_do_not_erase_its_own_contract() {
    let (file, checker) = check_adopted(
        "pure-helper.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\npure <- function() 1L\nf(\"before\")\npure()\nf(\"after\")\n",
    );
    let spans = checker
        .declaration_findings()
        .iter()
        .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
        .map(|finding| &file.source[finding.span.start..finding.span.end])
        .collect::<Vec<_>>();
    assert_eq!(spans, vec!["\"before\"", "\"after\""]);
}

#[test]
fn forcing_a_possible_active_or_delayed_binding_drops_stale_contract_identity() {
    for setup in [
        "e <- new.env(); makeActiveBinding(\"trigger\", function() { assign(\"f\", function(x) x, envir = .GlobalEnv); 1L }, e)",
        "e <- new.env(); delayedAssign(\"trigger\", { assign(\"f\", function(x) x, envir = .GlobalEnv); 1L }, assign.env = e)",
        // The retrieval is actually inert, but no source-level proof that a
        // binding in an arbitrary environment is ordinary is available.
        "e <- new.env(); e$trigger <- 1L",
    ] {
        let source = format!(
            "{setup}\nf <- function(x) {{\n  #| x integer\n  x\n}}\nf(\"before\")\nget(\"trigger\", envir = e)\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("forced-binding.R", &source);
        let spans = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &file.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>();
        assert_eq!(spans, vec!["\"before\""], "setup: {setup}");
    }
}

#[test]
fn standalone_and_helper_value_reads_can_force_a_replacement() {
    for (setup, action, expected) in [
        (
            "makeActiveBinding(\"trigger\", function() { assign(\"f\", function(x) x, envir = .GlobalEnv); 1L }, .GlobalEnv)",
            "trigger",
            vec!["\"before\""],
        ),
        (
            "makeActiveBinding(\"trigger\", function() { assign(\"f\", function(x) x, envir = .GlobalEnv); 1L }, .GlobalEnv)",
            "touch <- function() trigger; touch()",
            vec!["\"before\""],
        ),
        (
            "e <- new.env(); makeActiveBinding(\"trigger\", function() { assign(\"f\", function(x) x, envir = .GlobalEnv); .GlobalEnv }, e)",
            "e$trigger",
            vec!["\"before\""],
        ),
        (
            "e <- new.env(); makeActiveBinding(\"trigger\", function() { assign(\"f\", function(x) x, envir = .GlobalEnv); .GlobalEnv }, e)",
            "e[[\"trigger\"]]",
            vec!["\"before\""],
        ),
        (
            "e <- new.env(); makeActiveBinding(\"trigger\", function() { assign(\"f\", function(x) x, envir = .GlobalEnv); .GlobalEnv }, e)",
            "touch <- function() e$trigger; touch()",
            vec!["\"before\""],
        ),
        (
            "e <- new.env(); makeActiveBinding(\"trigger\", function() { assign(\"f\", function(x) x, envir = .GlobalEnv); .GlobalEnv }, e)",
            "base::eval(base::quote(1L), envir = e$trigger)",
            vec!["\"before\""],
        ),
        (
            "e <- new.env(); makeActiveBinding(\"trigger\", function() { assign(\"f\", function(x) x, envir = .GlobalEnv); .GlobalEnv }, e)",
            "base::evalq(1L, envir = e$trigger)",
            vec!["\"before\""],
        ),
        (
            "e <- new.env(); e$trigger <- .GlobalEnv",
            "base::eval(base::quote(1L), envir = e$trigger)",
            vec!["\"before\""],
        ),
        (
            "e <- new.env(); e$trigger <- .GlobalEnv",
            "base::eval(base::quote(1L))",
            vec!["\"before\""],
        ),
    ] {
        let source = format!(
            "{setup}\nf <- function(x) {{\n  #| x integer\n  x\n}}\nf(\"before\")\n{action}\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("forced-value-read.R", &source);
        let spans = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &file.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>();
        assert_eq!(spans, expected, "action: {action}");
    }
}

#[test]
fn active_call_heads_cannot_reuse_removed_function_inventory() {
    for (setup, action, expected) in [
        (
            "trigger <- function() NULL\nrm(trigger)\nmakeActiveBinding(\"trigger\", function() { f <<- function(x) x; function() 1L }, .GlobalEnv)",
            "trigger()",
            vec!["\"before\""],
        ),
        (
            "trigger <- function() NULL\nrm(trigger)\nmakeActiveBinding(\"trigger\", function() { f <<- function(x) x; function() 1L }, .GlobalEnv)",
            "touch <- function() trigger()\ntouch()",
            vec!["\"before\""],
        ),
        (
            "trigger <- function() 1L",
            "touch <- function() trigger()\ntouch()",
            vec!["\"before\""],
        ),
        (
            "trigger <- function() NULL\nrm(trigger)\nmakeActiveBinding(\"trigger\", function() { f <<- function(x) x; function() 1L }, .GlobalEnv)",
            "touch <- function() { trigger(); trigger <- function() 1L }\ntouch()",
            vec!["\"before\""],
        ),
        (
            "trigger <- function() 1L",
            "touch <- function() { trigger <- function() 1L; trigger() }\ntouch()",
            vec!["\"before\""],
        ),
    ] {
        let source = format!(
            "{setup}\nf <- function(x) {{\n  #| x integer\n  x\n}}\nf(\"before\")\n{action}\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("call-head-identity.R", &source);
        assert_eq!(
            mismatch_sources(&checker, &file),
            expected,
            "action: {action}"
        );
    }
}

#[test]
fn evaluated_assignment_targets_can_replace_a_declared_callable() {
    for (action, expected) in [
        (
            "touch <- function() e$trigger[1] <- 1L\ntouch()",
            vec!["\"before\""],
        ),
        (
            "touch <- function() e[[swap()]] <- 1L\ntouch()",
            vec!["\"before\""],
        ),
        ("e$trigger[1] <- 1L", vec!["\"before\""]),
        ("result <- (e$trigger[1] <- 1L)", vec!["\"before\""]),
        ("e[[swap()]] <- 1L", vec!["\"before\""]),
        (
            "touch <- function() { local <- 1L; 1L }\ntouch()",
            vec!["\"before\"", "\"after\""],
        ),
    ] {
        let source = format!(
            "e <- new.env()\nmakeActiveBinding(\"trigger\", function(value) {{ f <<- function(x) x; if (missing(value)) c(0L) else NULL }}, e)\nswap <- function() {{ f <<- function(x) x; \"other\" }}\nf <- function(x) {{\n  #| x integer\n  x\n}}\nf(\"before\")\n{action}\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("assignment-target-effects.R", &source);
        assert_eq!(
            mismatch_sources(&checker, &file),
            expected,
            "action: {action}"
        );
    }
}

#[test]
fn direct_masked_operators_drop_only_later_declaration_identity() {
    for (setup, action) in [
        (
            "`+` <- function(e1, e2) { f <<- function(x) x; 1L }",
            "1L + 1L",
        ),
        ("`-` <- function(e1) { f <<- function(x) x; 1L }", "-1L"),
    ] {
        let source = format!(
            "{setup}\nf <- function(x) {{\n  #| x integer\n  x\n}}\nf(\"before\")\n{action}\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("direct-operator-effect.R", &source);
        assert_eq!(
            mismatch_sources(&checker, &file),
            vec!["\"before\""],
            "action: {action}"
        );
    }
}

#[test]
fn warm_helper_value_read_edits_match_cold_declaration_findings() {
    let source = |body: &str| {
        format!(
            "e <- new.env()\nmakeActiveBinding(\"trigger\", function() {{ assign(\"f\", function(x) x, envir = .GlobalEnv); 1L }}, e)\nf <- function(x) {{\n  #| x integer\n  x\n}}\ntouch <- function() {body}\ntouch()\nf(\"after\")\n"
        )
    };
    let first = parse("reader.R", &source("1L"));
    let mut project = new_project(&[&first], &adopted_records(&first));
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &first.path), 1);

    for (body, expected) in [("e$trigger", 0), ("1L", 1)] {
        let edited = parse("reader.R", &source(body));
        edit_warm(&mut project, &edited, &[&edited], &adopted_records(&edited));
        assert_eq!(project_mismatch_count(&project, &first.path), expected);
    }
}

#[test]
fn forcing_a_nonliteral_actual_promise_can_replace_the_declared_callee() {
    for (definition, supply, call, expected) in [
        (
            "f <- function(x) x",
            SupplyStatus::Required,
            "f(e$trigger)",
            vec!["\"before\""],
        ),
        (
            "f <- function(x = e$trigger) x",
            SupplyStatus::Defaulted,
            "f()",
            vec!["\"before\""],
        ),
        (
            "f <- function(x) { is.integer(e$trigger); x }",
            SupplyStatus::Required,
            "f(1L)",
            vec!["\"before\""],
        ),
        (
            "f <- function(x) x",
            SupplyStatus::Required,
            "f(1L)",
            vec!["\"before\"", "\"after\""],
        ),
    ] {
        let source = format!(
            "e <- new.env()\nmakeActiveBinding(\"trigger\", function() {{ assign(\"f\", function(x) x, envir = .GlobalEnv); 1L }}, e)\n{definition}\nf(\"before\")\n{call}\nf(\"after\")\n"
        );
        let file = parse("active-actual.R", &source);
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![record(
            &file,
            "f",
            ("x", AtomicMode::Integer, supply),
            None,
        )]);
        checker.check(&file);
        let spans = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &file.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>();
        assert_eq!(spans, expected, "definition: {definition}; call: {call}");
    }
}

#[test]
fn loop_binder_callable_shadows_a_same_named_pure_outer_helper() {
    for (body, expected) in [
        (
            "for (h in list(function() f <<- function(x) x)) h()",
            vec!["\"before\""],
        ),
        ("for (h in 1L) 1L", vec!["\"before\"", "\"after\""]),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nh <- function() 1L\nbridge <- function() {{ {body} }}\nf(\"before\")\nbridge()\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("loop-helper-shadow.R", &source);
        let spans = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &file.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>();
        assert_eq!(spans, expected, "body: {body}");
    }
}

#[test]
fn saved_alias_keeps_the_aliased_function_identity_after_name_rebinding() {
    for (first, second, after_mismatch) in [
        ("1L", "f <<- function(x) x", true),
        ("f <<- function(x) x", "1L", false),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nmutate <- function() {first}\nsaved <- mutate\nmutate <- function() {second}\nf(\"before\")\nsaved()\nf(\"after\")\n"
        );
        let (file, checker) = check_adopted("saved-identity.R", &source);
        let spans = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &file.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>();
        let expected = if after_mismatch {
            vec!["\"before\"", "\"after\""]
        } else {
            vec!["\"before\""]
        };
        assert_eq!(
            spans, expected,
            "saved body: {first}, rebound body: {second}"
        );
    }
}

#[test]
fn mutating_argument_does_not_change_the_selected_call_head() {
    let file = parse(
        "argument-mutates-callee.R",
        "f <- function(x) x\nmutate <- function() f <<- function(x) x\nf(\"before\")\nf({ mutate(); \"during\" })\nf(\"after\")\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&file);
    let spans = checker
        .declaration_findings()
        .iter()
        .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
        .map(|finding| &file.source[finding.span.start..finding.span.end])
        .collect::<Vec<_>>();
    assert_eq!(spans, vec!["\"before\"", "{ mutate(); \"during\" }"]);
}

#[test]
fn cross_file_helper_installation_changes_only_later_eager_calls_and_warm_edits() {
    let first = parse(
        "first.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nf(\"before\")\nmutate()\nf(\"after\")\n",
    );
    let mutator = parse("mutator.R", "mutate <- function() f <<- function(x) x");
    let records = adopted_records(&first);
    let mut project = new_project(&[&first, &mutator], &records);
    project.check_incremental();
    let kinds_in_first = |project: &Project| {
        project
            .declaration_findings()
            .iter()
            .filter(|(path, _)| *path == first.path)
            .flat_map(|(_, findings)| findings)
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &first.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>()
    };
    assert_eq!(kinds_in_first(&project), vec!["\"before\""]);

    for (body, expected) in [
        ("mutate <- function() g <<- function(x) x", 2),
        ("mutate <- function() f <<- function(x) x", 1),
    ] {
        let edited = parse("mutator.R", body);
        edit_warm(&mut project, &edited, &[&first, &edited], &records);
        assert_eq!(kinds_in_first(&project).len(), expected, "mutator: {body}");
    }
}

#[test]
fn cross_file_wrapper_effect_retracts_after_warm_helper_edit() {
    let first = parse(
        "first.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nbridge <- function() mutate()\nf(\"before\")\nbridge()\nf(\"after\")\n",
    );
    let mutator = parse("mutator.R", "mutate <- function() f <<- function(x) x");
    let records = adopted_records(&first);
    let mut project = new_project(&[&first, &mutator], &records);
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &first.path), 1);

    for (body, expected) in [
        ("mutate <- function() 1L", 1),
        ("mutate <- function() f <<- function(x) x", 1),
    ] {
        let edited = parse("mutator.R", body);
        edit_warm(&mut project, &edited, &[&first, &edited], &records);
        assert_eq!(project_mismatch_count(&project, &first.path), expected);
    }
}

#[test]
fn eager_forward_call_has_no_future_declaration_but_deferred_call_can_use_it() {
    let (_, eager) = check_adopted(
        "forward.R",
        "f(1L)\nf <- function(x) {\n  #| x character\n  x\n}\n",
    );
    assert!(!kinds(&eager).contains(&DeclarationFindingKind::Mismatch));

    let (_, deferred) = check_adopted(
        "deferred.R",
        "g <- function() { f(1L) }\nf <- function(x) {\n  #| x character\n  x\n}\ng()\n",
    );
    assert!(kinds(&deferred).contains(&DeclarationFindingKind::Mismatch));
}

#[test]
fn same_offset_in_another_file_cannot_identify_a_local_eager_binding() {
    let literal = "f <- function(x) {\n  #| x character\n  x\n}\n";
    let first = parse("a.R", &format!("{literal}f(1L)\n"));
    let second = parse("b.R", literal);
    assert_eq!(
        named_function_span(&first, "f"),
        named_function_span(&second, "f")
    );
    // Only b.R is adopted.
    let mut project = new_project(&[&first, &second], &adopted_records(&second));
    project.check();
    assert!(project.declaration_findings()[0].1.is_empty());
}

#[test]
fn project_keeps_same_named_declarations_in_distinct_files() {
    let first = parse(
        "first.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nf(\"bad\")\n",
    );
    let second = parse(
        "second.R",
        "f <- function(x) {\n  #| x character\n  x\n}\nf(1L)\n",
    );
    let records = [adopted_records(&first), adopted_records(&second)].concat();
    let mut project = new_project(&[&first, &second], &records);
    project.check();
    let findings = project.declaration_findings();
    assert_eq!(findings.len(), 2);
    for (_, file_findings) in findings {
        assert_eq!(
            file_findings
                .iter()
                .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
                .count(),
            1
        );
    }
}

#[test]
fn project_deferred_capture_stays_uncertain_across_files() {
    let first = parse(
        "first.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nouter <- function() { f(\"bad\") }\n",
    );
    let second = parse("second.R", "f <- function(x) { x }\n");
    let mut project = new_project(&[&first, &second], &adopted_records(&first));
    project.check();
    assert!(project.declaration_findings().iter().all(|(_, findings)| {
        !findings
            .iter()
            .any(|finding| finding.kind == DeclarationFindingKind::Mismatch)
    }));
}

fn new_project(files: &[&SourceFile], records: &[DeclarationRecord]) -> Project {
    let mut project = Project::new();
    for file in files {
        project.add_file(file.path.clone(), (*file).clone());
    }
    project.set_declaration_records(records.to_vec());
    project
}

fn cold_findings(
    files: &[&SourceFile],
    records: &[DeclarationRecord],
) -> Vec<(String, Vec<DeclarationFinding>)> {
    let mut cold = new_project(files, records);
    cold.check();
    cold.declaration_findings().to_vec()
}

/// Apply `edited` to the warm project; its findings must equal a cold check
/// of `files` (which include the edit) under the same records.
fn edit_warm(
    warm: &mut Project,
    edited: &SourceFile,
    files: &[&SourceFile],
    records: &[DeclarationRecord],
) {
    warm.update_file(edited.path.clone(), Arc::new(edited.clone()));
    warm.set_declaration_records(records.to_vec());
    warm.check_incremental();
    assert_eq!(
        warm.declaration_findings(),
        cold_findings(files, records),
        "warm edit of {}:\n{}",
        edited.path,
        edited.source
    );
}

fn project_mismatch_count(project: &Project, path: &str) -> usize {
    project
        .declaration_findings()
        .iter()
        .find(|(file, _)| file == path)
        .expect("project contains requested file")
        .1
        .iter()
        .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
        .count()
}

#[test]
fn project_capture_inventory_includes_global_expression_control_binder_and_outward_writes() {
    let first = parse(
        "first.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nf(\"bad\")\nouter <- function() { f(\"bad\") }\n",
    );
    let records = adopted_records(&first);
    for (second_source, expected) in [
        ("invisible(f <- function(x) { x })", 1),
        ("if ({ f <- function(x) { x }; TRUE }) 1L", 1),
        ("for (f in list(function(x) { x })) 1L", 1),
        ("for (`f` in list(function(x) { x })) 1L", 1),
        (
            "mutate <- function() { f <<- function(x) { x } }; mutate()",
            1,
        ),
        (
            "mutate <- function(z = (f <<- function(x) { x })) { z }; mutate()",
            1,
        ),
        (
            "mutate <- function(z = (function() { f <<- function(x) { x } })()) { z }; mutate()",
            1,
        ),
        (
            "mutate <- function() { env <- .GlobalEnv; assign(\"f\", function(x) x, envir = env) }; mutate()",
            1,
        ),
        (
            "mutate <- function() { eval(quote(f <- function(x) x), envir = .GlobalEnv) }; mutate()",
            1,
        ),
        ("base::assign(\"f\", function(x) x, envir = .GlobalEnv)", 1),
        (
            "base::eval(quote(f <- function(x) x), envir = .GlobalEnv)",
            1,
        ),
        ("base::evalq(f <- function(x) x, envir = .GlobalEnv)", 1),
        (
            "setGeneric(\"f\", function(x) standardGeneric(\"f\")); setMethod(\"f\", \"ANY\", function(x) x)",
            1,
        ),
        (
            "methods::setGeneric(\"f\", function(x) standardGeneric(\"f\")); methods::setMethod(\"f\", \"ANY\", function(x) x)",
            1,
        ),
        (
            "delayedAssign(\"f\", function(x) x, assign.env = .GlobalEnv)",
            1,
        ),
        (
            "base::delayedAssign(value = function(x) x, x = \"f\", assign.env = .GlobalEnv)",
            1,
        ),
        (
            "rm(f, envir = .GlobalEnv); makeActiveBinding(\"f\", function() function(x) x, .GlobalEnv)",
            1,
        ),
        (
            "base::rm(f, envir = .GlobalEnv); base::makeActiveBinding(\"f\", function() function(x) x, .GlobalEnv)",
            1,
        ),
        ("base::remove(list = base::c(\"f\"), envir = .GlobalEnv)", 1),
        ("rm(list = target_name, envir = .GlobalEnv)", 1),
        (
            "delayedAssign(target_name, function(x) x, assign.env = .GlobalEnv)",
            1,
        ),
        ("setGeneric(\"g\", function(x) standardGeneric(\"g\"))", 2),
        ("g <- 1L; rm(g, envir = .GlobalEnv)", 2),
        (
            "g <- 1L; remove(list = base::c(\"g\"), envir = .GlobalEnv)",
            2,
        ),
        (
            "c <- function(...) \"f\"; rm(list = c(\"g\"), envir = .GlobalEnv)",
            1,
        ),
        (
            "quote <- function(...) base::quote(f <- function(x) x); eval(quote(1L), envir = .GlobalEnv)",
            1,
        ),
        (
            "mutate <- function() { local <- 1L; rm(local) }; mutate()",
            2,
        ),
        ("globalVariables(\"f\")", 2),
        (
            "mutate <- function(z = (function() { f <- function(x) { x } })()) { z }; mutate()",
            2,
        ),
        (
            "mutate <- function() { f <- function(x) { x } }; mutate()",
            2,
        ),
        ("g <- function(x) { x }", 2),
    ] {
        let mut project = new_project(&[&first, &parse("second.R", second_source)], &records);
        project.check();
        assert_eq!(
            project_mismatch_count(&project, &first.path),
            expected,
            "second file: {second_source}, findings: {:?}",
            project.declaration_findings()
        );
    }
}

#[test]
fn project_capture_write_edits_and_removal_match_cold_analysis() {
    let first = parse(
        "first.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nouter <- function() { f(\"bad\") }\n",
    );
    let records = adopted_records(&first);
    let mut project = new_project(&[&first, &parse("second.R", "g <- 1L\n")], &records);
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &first.path), 1);

    for (source, expected) in [
        ("invisible(f <- function(x) { x })\n", 0),
        ("g <- 1L\n", 1),
        ("mutate <- function() { f <<- function(x) { x } }\n", 0),
        ("g <- 2L\n", 1),
    ] {
        let second = parse("second.R", source);
        edit_warm(&mut project, &second, &[&first, &second], &records);
        assert_eq!(project_mismatch_count(&project, &first.path), expected);
    }
    project.remove_file("second.R");
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &first.path), 1);
}

#[test]
fn write_only_warm_edits_reemit_a_deferred_capture() {
    let first = parse(
        "first.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nouter <- function() f(\"bad\")\n",
    );
    let records = adopted_records(&first);
    let mut project = new_project(
        &[&first, &parse("writer.R", "invisible(f <- 1L)")],
        &records,
    );
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &first.path), 0);

    for (source, expected) in [
        ("invisible(g <- 1L)", 1),
        ("invisible(f <- 1L)", 0),
        ("invisible(g <- 1L)", 1),
    ] {
        let writer = parse("writer.R", source);
        edit_warm(&mut project, &writer, &[&first, &writer], &records);
        assert_eq!(project.emit_count, 2, "write-only edit: {source}");
        assert_eq!(project_mismatch_count(&project, &first.path), expected);
    }
}

#[test]
fn generic_installation_warm_edits_match_cold_capture_checking() {
    let first = parse(
        "first.R",
        "f <- function(x) {\n  #| x integer\n  x\n}\nouter <- function() f(\"bad\")\n",
    );
    let records = adopted_records(&first);
    let generic = "methods::setGeneric(\"f\", function(x) standardGeneric(\"f\")); methods::setMethod(\"f\", \"ANY\", function(x) x)";
    let mut project = new_project(&[&first, &parse("writer.R", generic)], &records);
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &first.path), 0);

    for (source, expected) in [
        ("globalVariables(\"f\")", 1),
        (generic, 0),
        ("g <- function(x) x", 1),
    ] {
        let writer = parse("writer.R", source);
        edit_warm(&mut project, &writer, &[&first, &writer], &records);
        assert_eq!(project_mismatch_count(&project, &first.path), expected);
    }
}

#[test]
fn quoted_and_plain_bindings_share_capture_identity_cold_and_warm() {
    for (first_name, second_write, expected) in [
        ("f", "`f` <- function(x) x", 0),
        ("`f`", "f <- function(x) x", 0),
        ("f", "invisible(`f` <- function(x) x)", 0),
        (
            "`f`",
            "mutate <- function() { `f` <<- function(x) x }; mutate()",
            0,
        ),
        ("f", "g <- function(x) x", 1),
    ] {
        let first = parse(
            "first.R",
            &format!(
                "{first_name} <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function() f(\"bad\")\n"
            ),
        );
        let records = adopted_records(&first);
        let mut project = new_project(&[&first, &parse("second.R", "g <- 1L\n")], &records);
        project.check_incremental();
        assert_eq!(
            project_mismatch_count(&project, &first.path),
            1,
            "initial {first_name}"
        );
        let second = parse("second.R", second_write);
        edit_warm(&mut project, &second, &[&first, &second], &records);
        assert_eq!(
            project_mismatch_count(&project, &first.path),
            expected,
            "{first_name}: {second_write}"
        );
    }
}

#[test]
fn quoted_binding_replacement_invalidates_a_same_file_deferred_call() {
    for (definition, replacement, expected) in [
        ("f", "`f` <- function(x) x", 0),
        ("`f`", "f <- function(x) x", 0),
        ("`f`", "g <- function(x) x", 1),
    ] {
        let (_, checker) = check_adopted(
            "quoted-local.R",
            &format!(
                "{definition} <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function() f(\"bad\")\n{replacement}\nouter()\n"
            ),
        );
        assert_eq!(
            kinds(&checker)
                .iter()
                .filter(|kind| **kind == DeclarationFindingKind::Mismatch)
                .count(),
            expected,
            "{definition}: {replacement}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn quoted_formal_reads_keep_only_live_parameter_identity() {
    for (formal, read, default, call, expected) in [
        ("q", "q", "= 1L", "outer()", 1),
        ("`q`", "`q`", "= 1L", "outer()", 1),
        ("q", "`q`", "= 1L", "outer()", 1),
        ("`q`", "q", "= 1L", "outer()", 1),
        ("`a b`", "`a b`", "= 1L", "outer()", 1),
        ("`q`", "q", "", "outer(1L)", 1),
        ("q", "`q`", "", "outer(1L)", 1),
        // A same-R-name assignment under another raw spelling no longer
        // certifies that the read still refers to the original formal.
        ("`q`", "q <- 1L; q", "= 1L", "outer()", 0),
        ("q", "`q` <- 1L; `q`", "= 1L", "outer()", 0),
        ("q", "other", "= 1L", "outer()", 0),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function({formal}{default}) {{ {read}; f(\"bad\") }}\n{call}\n"
        );
        let (_, checker) = check_adopted("formal-spelling.R", &source);
        let actual = kinds(&checker)
            .iter()
            .filter(|kind| **kind == DeclarationFindingKind::Mismatch)
            .count();
        assert_eq!(
            actual,
            expected,
            "{source}: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn quoted_formal_edits_retract_and_restore_warm_declaration_findings() {
    let source = |read: &str| {
        format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nouter <- function(`q` = 1L) {{ {read}; f(\"bad\") }}\nouter()\n"
        )
    };
    let original = parse("formal-warm.R", &source("`q`"));
    let records = adopted_records(&original);
    let mut warm = new_project(&[&original], &records);
    warm.check_incremental();
    assert_eq!(project_mismatch_count(&warm, &original.path), 1);

    for (read, expected) in [("other", 0), ("q", 1), ("`q`", 1)] {
        let edited = parse("formal-warm.R", &source(read));
        edit_warm(&mut warm, &edited, &[&edited], &records);
        assert_eq!(project_mismatch_count(&warm, &edited.path), expected);
    }
}

#[test]
fn declaration_attachment_and_calls_match_semantic_formal_names() {
    for (source, declared, expected) in [
        (
            "f <- function(x) x\nf(\"bad\")\n",
            "x",
            Some(DeclarationFindingKind::Mismatch),
        ),
        (
            "f <- function(`x`) `x`\nf(\"bad\")\n",
            "x",
            Some(DeclarationFindingKind::Mismatch),
        ),
        (
            "f <- function(`x`) `x`\nf(x = \"bad\")\n",
            "x",
            Some(DeclarationFindingKind::Mismatch),
        ),
        (
            "f <- function(x) x\nf(`x` = \"bad\")\n",
            "x",
            Some(DeclarationFindingKind::Mismatch),
        ),
        (
            "f <- function(`longname`) `longname`\nf(long = \"bad\")\n",
            "longname",
            Some(DeclarationFindingKind::Mismatch),
        ),
        (
            "f <- function(`a b`) `a b`\nf(`a b` = \"bad\")\n",
            "a b",
            Some(DeclarationFindingKind::Mismatch),
        ),
        (
            "f <- function(x, `x y`) x\nf(`x y` = \"bad\", x = 1L)\n",
            "x",
            None,
        ),
        (
            "f <- function(x, `x y`) x\nf(`x y` = \"bad\", x = 1L)\n",
            "x y",
            Some(DeclarationFindingKind::Mismatch),
        ),
        (
            "f <- function(`x`) `x`\nf(\"bad\")\n",
            "y",
            Some(DeclarationFindingKind::AmbiguousAttachment),
        ),
        (
            "f <- function(`x`) `x`\nf(\"bad\")\n",
            "`x`",
            Some(DeclarationFindingKind::AmbiguousAttachment),
        ),
    ] {
        let file = parse("formal-attachment.R", source);
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![record(
            &file,
            "f",
            (declared, AtomicMode::Integer, SupplyStatus::Required),
            None,
        )]);
        checker.check(&file);
        let actual = kinds(&checker);
        assert_eq!(
            actual.as_slice(),
            expected.as_slice(),
            "{source}: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn quoted_formal_entry_and_attachment_edits_match_cold_analysis() {
    let source = |formal: &str, actual: &str| {
        format!("f <- function({formal}) {{ {formal} }}\nf({actual} = \"bad\")\n")
    };
    let original = parse("formal-attachment-warm.R", &source("x", "x"));
    let declaration = record(
        &original,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let mut warm = new_project(&[&original], &[declaration]);
    warm.check_incremental();
    assert_eq!(project_mismatch_count(&warm, &original.path), 1);

    for (formal, actual, declared, expected) in [
        ("`x`", "x", "x", 1),
        ("x", "`x`", "x", 1),
        ("`a b`", "`a b`", "a b", 1),
        ("`x`", "x", "y", 0),
        ("x", "x", "x", 1),
    ] {
        let edited = parse("formal-attachment-warm.R", &source(formal, actual));
        let declaration = record(
            &edited,
            "f",
            (declared, AtomicMode::Integer, SupplyStatus::Required),
            None,
        );
        edit_warm(&mut warm, &edited, &[&edited], &[declaration]);
        assert_eq!(project_mismatch_count(&warm, &edited.path), expected);
    }

    let file = parse("formal-entry.R", "f <- function(`x`) { `x` }\n");
    let mut checker = Checker::new(&file.path);
    checker.enable_scope_capture();
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&file);
    let scope = checker
        .take_scope_records()
        .into_iter()
        .find(|record| record.name.as_deref() == Some("f"))
        .expect("attached function scope");
    assert_eq!(scope.scope.bindings["`x`"].mode, Mode::Integer);
}

#[test]
fn quoted_default_is_checked_against_its_semantic_declared_formal() {
    for formal in ["x", "`x`"] {
        let source = format!("f <- function({formal} = \"wrong\") {{ {formal} }}\nf(x = 1L)\n");
        let file = parse("quoted-default.R", &source);
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Defaulted),
            None,
        )]);
        checker.check(&file);
        assert_eq!(
            kinds(&checker),
            vec![DeclarationFindingKind::Mismatch],
            "{source}: {:?}",
            checker.declaration_findings()
        );
        assert!(
            checker.declaration_findings()[0]
                .message
                .contains("default for")
        );
    }
}

#[test]
fn escaped_backtick_replacement_cannot_keep_a_decoded_contract() {
    for source in [
        "f <- function(x) {\n  #| x integer\n  x\n}\nf(\"before\")\n`\\x66` <- function(x) x\nf(\"after\")\n",
        "f <- function(x) {\n  #| x integer\n  x\n}\nouter <- function() f(\"bad\")\n`\\x66` <- function(x) x\nouter()\n",
    ] {
        let (file, checker) = check_adopted("escaped-binding.R", source);
        let spans = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| &file.source[finding.span.start..finding.span.end])
            .collect::<Vec<_>>();
        let expected = if source.contains("outer <-") {
            vec![]
        } else {
            vec!["\"before\""]
        };
        assert_eq!(spans, expected, "source: {source}");
    }
}

#[test]
fn other_file_global_write_does_not_replace_a_nested_local_literal() {
    let first = parse(
        "first.R",
        "outer <- function() {\n  f <- function(x) {\n    #| x integer\n    x\n  }\n  inner <- function() { f(\"bad\") }\n  inner()\n}\n",
    );
    let records = adopted_records(&first);
    for second_source in [
        "invisible(f <- function(x) { x })",
        "mutate <- function() { f <<- function(x) { x } }; mutate()",
    ] {
        let mut project = new_project(&[&first, &parse("second.R", second_source)], &records);
        project.check();
        assert_eq!(project_mismatch_count(&project, &first.path), 1);
    }
}

#[test]
fn mixed_or_unknown_evidence_does_not_prove_a_call_mismatch() {
    let file = parse(
        "uncertain.R",
        "f <- function(x) { x }\nf(TRUE)\nf(if (flag) 1L else \"text\")\nf(missing_value)\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&file);
    let mismatches: Vec<_> = checker
        .declaration_findings()
        .iter()
        .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
        .collect();
    assert_eq!(mismatches.len(), 1);
    assert_eq!(
        &file.source[mismatches[0].span.start..mismatches[0].span.end],
        "TRUE"
    );
}

#[test]
fn equivalent_provenance_conflicts_and_unadopted_records_stay_distinct() {
    let file = parse("status.R", "f <- function(x) { x }\nf(\"bad\")\n");
    let one = record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let mut equivalent = one.clone();
    equivalent.source.provider = "second-provider".into();
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![one.clone(), equivalent]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
    assert_eq!(checker.declaration_records().len(), 2);

    let mut different = one.clone();
    different.source.provider = "third-provider".into();
    if let Translation::Exact(signature) = &mut different.translation {
        signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Character));
    }
    checker.set_declaration_records(vec![one.clone(), different]);
    assert!(checker.declaration_findings().is_empty());
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Conflict]);

    let mut partial = one.clone();
    partial.translation = Translation::Partial {
        supported: match &one.translation {
            Translation::Exact(signature) => signature.clone(),
            _ => unreachable!(),
        },
        residuals: vec![ResidualConstraint {
            raw: "not(NA)".into(),
            span: Span::default(),
            reason: "value exclusion unsupported".into(),
        }],
    };
    checker.set_declaration_records(vec![one.clone(), partial]);
    checker.check(&file);
    assert!(kinds(&checker).contains(&DeclarationFindingKind::Partial));
    assert!(kinds(&checker).contains(&DeclarationFindingKind::Conflict));
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));

    let mut candidate = one;
    candidate.evidence = EvidenceUse::DocumentationCandidate;
    checker.set_declaration_records(vec![candidate]);
    let candidate_diagnostics = checker.check(&file).to_vec();
    assert!(checker.declaration_findings().is_empty());
    let mut disabled = Checker::new(&file.path);
    assert_eq!(candidate_diagnostics, disabled.check(&file));
}

#[test]
fn normalized_union_equivalence_and_uncheckable_statuses_keep_provenance() {
    let file = parse("union-status.R", "f <- function(x) { x }\nf(TRUE)\n");
    let mut nested = record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    if let Translation::Exact(signature) = &mut nested.translation {
        signature.parameters[0].constraint = Some(TypeExpr::Union(vec![
            TypeExpr::atomic(AtomicMode::Integer),
            TypeExpr::Union(vec![
                TypeExpr::atomic(AtomicMode::Character),
                TypeExpr::atomic(AtomicMode::Double),
            ]),
        ]));
    }
    let mut flat = nested.clone();
    flat.source.provider = "flat-provider".into();
    if let Translation::Exact(signature) = &mut flat.translation {
        signature.parameters[0].constraint = Some(TypeExpr::Union(vec![
            TypeExpr::atomic(AtomicMode::Character),
            TypeExpr::atomic(AtomicMode::Double),
            TypeExpr::atomic(AtomicMode::Integer),
        ]));
    }
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![nested.clone(), flat]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
    assert_eq!(checker.declaration_records().len(), 2);

    let mut unsupported = nested.clone();
    unsupported.translation = Translation::Unsupported {
        residuals: vec![ResidualConstraint {
            raw: "custom(x)".into(),
            span: Span::default(),
            reason: "custom predicate is not modeled".into(),
        }],
    };
    checker.set_declaration_records(vec![unsupported]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Unsupported]);

    let mut quoted = nested.clone();
    if let Translation::Exact(signature) = &mut quoted.translation {
        signature.parameters[0].evaluation = EvaluationSemantics::Quoted;
    }
    checker.set_declaration_records(vec![quoted]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Unsupported]);

    let mut unknown = nested.clone();
    if let Translation::Exact(signature) = &mut unknown.translation {
        signature.parameters[0].constraint = Some(TypeExpr::Unknown);
    }
    checker.set_declaration_records(vec![unknown]);
    checker.check(&file);
    assert!(checker.declaration_findings().is_empty());

    let mut invalid = nested;
    if let Translation::Exact(signature) = &mut invalid.translation {
        signature.parameters[0].constraint = Some(TypeExpr::Union(vec![]));
    }
    checker.set_declaration_records(vec![invalid]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::InvalidSyntax]);
}

#[test]
fn attachment_formals_and_parse_errors_block_adoption() {
    let file = parse("formals.R", "f <- function(y) { y }\nf(\"bad\")\n");
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&file);
    assert_eq!(
        kinds(&checker),
        vec![DeclarationFindingKind::AmbiguousAttachment]
    );

    let ordered = parse("order.R", "f <- function(x, y) { x }\nf(\"bad\", 1L)\n");
    let mut reversed = record(
        &ordered,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    if let Translation::Exact(signature) = &mut reversed.translation {
        signature.parameters.push(DeclaredParameter {
            name: "y".into(),
            form: ParameterForm::Ordinary,
            supplied: SupplyStatus::Required,
            evaluation: EvaluationSemantics::Promise,
            constraint: Some(TypeExpr::atomic(AtomicMode::Integer)),
        });
        signature.parameters.reverse();
    }
    checker.set_declaration_records(vec![reversed]);
    checker.check(&ordered);
    assert_eq!(
        kinds(&checker),
        vec![DeclarationFindingKind::AmbiguousAttachment]
    );

    let broken = parse("broken.R", "f <- function(x) { x }\nf(\"bad\")\n(\n");
    assert!(!broken.parse_errors.is_empty());
    checker.set_declaration_records(vec![record(
        &broken,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&broken);
    assert!(checker.declaration_findings().is_empty());
}

#[test]
fn declaration_union_above_inference_cap_is_visible_without_inventing_body_type() {
    let file = parse("wide.R", "f <- function(x) { x }\nf(TRUE)\n");
    let mut declaration = record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    if let Translation::Exact(signature) = &mut declaration.translation {
        signature.parameters[0].constraint = Some(TypeExpr::Union(
            [
                AtomicMode::Integer,
                AtomicMode::Character,
                AtomicMode::Double,
                AtomicMode::Complex,
                AtomicMode::Raw,
            ]
            .into_iter()
            .map(TypeExpr::atomic)
            .collect(),
        ));
    }
    let mut checker = Checker::new(&file.path);
    checker.enable_scope_capture();
    checker.set_declaration_records(vec![declaration]);
    checker.check(&file);
    let scope = checker
        .take_scope_records()
        .into_iter()
        .find(|scope| scope.name.as_deref() == Some("f"))
        .unwrap();
    assert_eq!(scope.scope.bindings["x"].mode, Mode::Opaque);
    assert_eq!(
        kinds(&checker),
        vec![
            DeclarationFindingKind::Partial,
            DeclarationFindingKind::Mismatch
        ]
    );
}

#[test]
fn canonical_equivalents_have_order_independent_body_entry() {
    let file = parse("canonical-order.R", "f <- function(x) { x }\n");
    let plain = record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let mut repeated = plain.clone();
    repeated.source.provider = "repeated".into();
    if let Translation::Exact(signature) = &mut repeated.translation {
        signature.parameters[0].constraint =
            Some(TypeExpr::Union(vec![
                TypeExpr::atomic(AtomicMode::Integer);
                5
            ]));
    }
    let mut results = Vec::new();
    for declarations in [
        vec![plain.clone(), repeated.clone()],
        vec![repeated.clone(), plain.clone()],
    ] {
        let mut checker = Checker::new(&file.path);
        checker.enable_scope_capture();
        checker.set_declaration_records(declarations.clone());
        checker.check(&file);
        let scope = checker
            .take_scope_records()
            .into_iter()
            .find(|scope| scope.name.as_deref() == Some("f"))
            .unwrap();
        assert_eq!(checker.declaration_records(), declarations);
        results.push((scope.scope.bindings["x"].mode, kinds(&checker)));
    }
    assert_eq!(results, vec![(Mode::Integer, vec![]); 2]);
}

#[test]
fn flattened_wide_union_stays_out_of_body_in_any_form_or_order() {
    let file = parse("flat-cap.R", "f <- function(x) { x }\n");
    let mut flat = record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let five = TypeExpr::Union(
        [
            AtomicMode::Character,
            AtomicMode::Complex,
            AtomicMode::Double,
            AtomicMode::Integer,
            AtomicMode::Raw,
        ]
        .map(TypeExpr::atomic)
        .into(),
    );
    if let Translation::Exact(signature) = &mut flat.translation {
        signature.parameters[0].constraint = Some(five);
    }
    let mut nested = flat.clone();
    nested.source.provider = "nested".into();
    if let Translation::Exact(signature) = &mut nested.translation {
        signature.parameters[0].constraint = Some(TypeExpr::Union(vec![
            TypeExpr::atomic(AtomicMode::Integer),
            TypeExpr::Union(vec![
                TypeExpr::atomic(AtomicMode::Character),
                TypeExpr::atomic(AtomicMode::Double),
                TypeExpr::atomic(AtomicMode::Complex),
                TypeExpr::atomic(AtomicMode::Raw),
            ]),
        ]));
    }
    for declarations in [
        vec![flat.clone()],
        vec![nested.clone()],
        vec![flat.clone(), nested.clone()],
        vec![nested.clone(), flat.clone()],
    ] {
        let mut checker = Checker::new(&file.path);
        checker.enable_scope_capture();
        checker.set_declaration_records(declarations);
        checker.check(&file);
        let scope = checker
            .take_scope_records()
            .into_iter()
            .find(|scope| scope.name.as_deref() == Some("f"))
            .unwrap();
        assert_eq!(scope.scope.bindings["x"].mode, Mode::Opaque);
        assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Partial]);
    }
}

#[test]
fn direct_nested_calls_use_exact_lexical_definition_after_shadowing_and_rebinding() {
    let (file, checker) = check_adopted(
        "nested-calls.R",
        "first <- function() {\n  inner <- function(x) {\n    #| x integer\n    x\n  }\n  inner(\"bad\")\n}\nsecond <- function() {\n  inner <- function(x) {\n    #| x character\n    x\n  }\n  inner(\"good\")\n}\nfirst()\nsecond()\n",
    );
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
    let mismatch = &checker.declaration_findings()[0];
    assert_eq!(
        &file.source[mismatch.span.start..mismatch.span.end],
        "\"bad\""
    );

    for (path, source) in [
        (
            "rebound-nested.R",
            "outer <- function() {\n  inner <- function(x) {\n    #| x integer\n    x\n  }\n  inner <- function(x) { x }\n  inner(\"bad\")\n}\nouter()\n",
        ),
        (
            "conditional-nested.R",
            "outer <- function(flag) {\n  inner <- function(x) {\n    #| x integer\n    x\n  }\n  if (flag) inner <- function(x) { x }\n  inner(\"bad\")\n}\nouter(TRUE)\n",
        ),
    ] {
        let (_, checker) = check_adopted(path, source);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "{source}"
        );
    }
}

#[test]
fn loop_exits_keep_agreed_nested_identity_and_never_borrow_flat_name() {
    for loop_body in [
        "for (i in 1L) { 1L }",
        "while (FALSE) { 1L }",
        "repeat { 1L; break }",
        "for (i in 1L) { next }",
        "for (i in 1L) { break }",
    ] {
        for (actual, expected) in [
            ("\"bad\"", vec![DeclarationFindingKind::Mismatch]),
            ("1L", Vec::new()),
        ] {
            let source = format!(
                "inner <- function(x) {{\n  #| x character\n  x\n}}\nouter <- function() {{\n  inner <- function(x) {{\n    #| x integer\n    x\n  }}\n  {loop_body}\n  inner({actual})\n}}\nouter()\n"
            );
            let (_, checker) = check_adopted("loop-identity.R", &source);
            assert_eq!(
                kinds(&checker),
                expected,
                "loop: {loop_body}, actual: {actual}"
            );
        }
    }
}

#[test]
fn loop_rebinding_drops_exact_identity_but_keeps_lexical_shadow() {
    for loop_body in [
        "for (i in 1L) { inner <- function(x) { x } }",
        "while (flag) { inner <- function(x) { x }; break }",
        "repeat { inner <- function(x) { x }; break }",
    ] {
        let source = format!(
            "inner <- function(x) {{\n  #| x character\n  x\n}}\nouter <- function(flag) {{\n  inner <- function(x) {{\n    #| x integer\n    x\n  }}\n  {loop_body}\n  inner(1L)\n}}\nouter(TRUE)\n"
        );
        let (_, checker) = check_adopted("loop-rebind.R", &source);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "loop: {loop_body}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn zero_or_one_iteration_does_not_select_either_declared_literal() {
    let character = "inner <- function(x) {\n      #| x character\n      x\n    }";
    for (exit, expected_integer_mismatch) in [
        ("while (flag) {", "break", false),
        ("while (flag) {", "flag <- FALSE", false),
        ("for (i in 1L) {", "", true),
    ]
    .map(|(head, tail, expected)| {
        (
            format!("{head}\n    {character}\n    {tail}\n  }}"),
            expected,
        )
    }) {
        for actual in ["1L", "\"text\""] {
            let source = format!(
                "outer <- function(flag) {{\n  inner <- function(x) {{\n    #| x integer\n    x\n  }}\n  {exit}\n  inner({actual})\n}}\nouter(TRUE)\n"
            );
            let (_, checker) = check_adopted("two-loop-definitions.R", &source);
            let mismatch = kinds(&checker).contains(&DeclarationFindingKind::Mismatch);
            assert_eq!(
                mismatch,
                expected_integer_mismatch && actual == "1L",
                "{source}: {:?}",
                checker.declaration_findings()
            );
        }
    }
}

#[test]
fn branch_only_function_introduction_cannot_borrow_flat_identity() {
    for actual in ["1L", "\"text\""] {
        let source = format!(
            "inner <- function(x) {{\n  #| x character\n  x\n}}\nouter <- function(flag) {{\n  if (flag) inner <- function(x) {{\n    #| x integer\n    x\n  }}\n  inner({actual})\n}}\nouter(TRUE)\n"
        );
        let (_, checker) = check_adopted("branch-introduction.R", &source);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "actual: {actual}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn loop_nonfunction_rebind_falls_through_to_enclosing_definition() {
    let (_, checker) = check_adopted(
        "loop-nonfunction.R",
        "inner <- function(x) {\n  #| x character\n  x\n}\nouter <- function() {\n  inner <- function(x) {\n    #| x integer\n    x\n  }\n  for (i in 1L) { inner <- 1L }\n  inner(1L)\n}\nouter()\n",
    );
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
}

#[test]
fn loop_carried_widening_never_borrows_flat_contract_before_rebinding() {
    for loop_body in [
        "for (i in 1L) { inner(1L); inner <- function(x) { x } }",
        "while (flag) { inner(1L); inner <- function(x) { x }; break }",
        "repeat { inner(1L); inner <- function(x) { x }; break }",
    ] {
        let source = format!(
            "inner <- function(x) {{\n  #| x character\n  x\n}}\nouter <- function(flag) {{\n  inner <- function(x) {{\n    #| x integer\n    x\n  }}\n  {loop_body}\n}}\nouter(TRUE)\n"
        );
        let (_, checker) = check_adopted("loop-entry.R", &source);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "loop: {loop_body}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn iterator_function_lookup_skips_proven_nonfunctions() {
    for (iter, expected) in [
        ("list(function(x) { x })", Vec::new()),
        // R skips an integer binding in function position and resolves the
        // enclosing top-level `inner`, whose contract applies.
        ("1L", vec![DeclarationFindingKind::Mismatch]),
    ] {
        let source = format!(
            "inner <- function(x) {{\n  #| x character\n  x\n}}\nouter <- function() {{ for (inner in {iter}) {{ inner(1L) }} }}\nouter()\n"
        );
        let (_, checker) = check_adopted("loop-iterator.R", &source);
        assert_eq!(kinds(&checker), expected, "iterator: {iter}");
    }
}

#[test]
fn earlier_same_named_definition_keeps_its_own_independent_return_evidence() {
    for (first_return, second_return, expected) in [
        ("\"old\"", "1L", vec![DeclarationFindingKind::Mismatch]),
        ("1L", "\"new\"", vec![]),
    ] {
        let file = parse(
            "shadowed.R",
            &format!(
                "f <- function(x) {{ {first_return} }}\nf <- function(x) {{ {second_return} }}\nf(1L)\n"
            ),
        );
        let declaration = record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            Some(AtomicMode::Integer),
        );
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![declaration.clone()]);
        checker.check(&file);
        assert_eq!(kinds(&checker), expected, "first: {first_return}");

        let mut project = Project::new();
        project.add_file(file.path.clone(), file.clone());
        project.set_declaration_records(vec![declaration]);
        project.check_incremental();
        assert_eq!(project_mismatch_count(&project, &file.path), expected.len());
    }
}

#[test]
fn earlier_definition_return_evidence_survives_warm_annotation_edits() {
    let file = parse(
        "shadowed-warm.R",
        "f <- function(x) { \"old\" }\nf <- function(x) { 1L }\n",
    );
    let mut declaration = record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        Some(AtomicMode::Integer),
    );
    let mut project = new_project(&[&file], std::slice::from_ref(&declaration));
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &file.path), 1);

    if let Translation::Exact(signature) = &mut declaration.translation {
        signature.return_constraint = Some(TypeExpr::atomic(AtomicMode::Character));
    }
    project.set_declaration_records(vec![declaration.clone()]);
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &file.path), 0);
    assert_eq!(
        project.declaration_findings(),
        cold_findings(&[&file], &[declaration])
    );
}

#[test]
fn project_annotation_only_record_changes_converge_with_cold_analysis() {
    let defs = parse("defs.R", "f <- function(x) { x }\n");
    let calls = parse("calls.R", "f(\"text\")\n");
    let integer = record(
        &defs,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let mut character = integer.clone();
    if let Translation::Exact(signature) = &mut character.translation {
        signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Character));
    }
    let mut project = Project::new();
    project.add_file("defs.R".into(), defs.clone());
    project.add_file("calls.R".into(), calls.clone());
    project.set_declaration_records(vec![integer.clone()]);
    project.check();
    assert_eq!(project.declaration_records(), &[integer]);
    assert_eq!(project.declaration_findings()[1].1.len(), 1);
    assert_eq!(
        project.declaration_findings()[1].1[0].kind,
        DeclarationFindingKind::Mismatch
    );
    project.check_incremental();
    assert_eq!(project.emit_count, 0);
    assert_eq!(project.declaration_findings()[1].1.len(), 1);

    project.set_declaration_records(vec![character.clone()]);
    assert!(project.declaration_findings().is_empty());
    project.check_incremental();
    assert_eq!(project.emit_count, 2);
    assert!(project.declaration_findings()[1].1.is_empty());

    assert_eq!(
        project.declaration_findings(),
        cold_findings(&[&defs, &calls], &[character])
    );

    // Editing only the source comment changes the target's byte identity.
    // The caller supplies records from the edited buffer; warm and cold
    // analysis must agree on the newly attached constraint.
    let edited_defs = parse("defs.R", "# annotation changed\nf <- function(x) { x }\n");
    let edited_integer = record(
        &edited_defs,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    edit_warm(
        &mut project,
        &edited_defs,
        &[&edited_defs, &calls],
        &[edited_integer],
    );
    assert_eq!(project.emit_count, 2);
    assert_eq!(project.declaration_findings()[1].1.len(), 1);

    project.set_declaration_records(Vec::new());
    project.check_incremental();
    assert!(
        project
            .declaration_findings()
            .iter()
            .all(|(_, findings)| findings.is_empty())
    );
    assert_eq!(project.emit_count, 2);
    project.set_declaration_records(Vec::new());
    project.check_incremental();
    assert_eq!(project.emit_count, 0);
}

/// The frame-effect classifier (infer/call_effect.rs) is shared with scalar
/// facts. An audited base call keeps scalar facts but not identities: even
/// a literal call can force a caller's promise or signal a condition to a
/// caller's calling handler, which can rebind an outward name
/// (testdata/oracle/typehint_outer_handler_replacement.R). An inert user
/// helper keeps both, as before.
#[test]
fn base_calls_drop_contract_identity_and_inert_helpers_keep_it() {
    for (between, mismatch) in [
        ("message(\"hi\")", false),
        ("is.null(NULL)", false),
        ("length(\"a\")", false),
        ("stopifnot(TRUE)", false),
        ("sum(1:3)", false),
        ("noop()", true),
        ("touch()", false),
        ("1L", true),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nnoop <- function() NULL\ntouch <- function() get(\"x\")\n{between}\nf(\"after\")\n"
        );
        let (_, checker) = check_adopted("base-calls.R", &source);
        assert_eq!(
            kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            mismatch,
            "between: {between}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

/// Sol review witnesses: a literal `message()` reaching an outer calling
/// handler, and `base::length()` under a project `::` mask, can both
/// replace the annotated local. RY114 stays silent, as on main.
#[test]
fn outer_handler_and_masked_namespace_operator_drop_identity() {
    for (setup, between) in [
        ("", "message(\"hi\")"),
        (
            "`::` <- function(pkg, name) function(...) assign(\"cls\", function(x) x, envir = parent.frame())\n",
            "base::length(\"a\")",
        ),
    ] {
        let source = format!(
            "{setup}run <- function() {{\n  cls <- function(x) {{\n    #| x integer\n    x\n  }}\n  {between}\n  cls(\"after\")\n}}\nrun()\n"
        );
        let (_, checker) = check_adopted("sol-witness.R", &source);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "between: {between}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

/// R evaluates the RHS of `&&` in this frame (testdata/oracle/
/// typehint_short_circuit_replacement.R), so its effects reach the
/// continuation even though inference ran it on a cloned scope.
#[test]
fn short_circuit_replacement_drops_contract_identity() {
    for (condition, mismatch) in [
        ("TRUE && swap()", false),
        ("FALSE || swap()", false),
        // Base calls drop identities (see above); a literal RHS keeps them.
        ("TRUE && is.null(NULL)", false),
        ("TRUE && TRUE", true),
    ] {
        let source = format!(
            "f <- function(x) {{\n  #| x integer\n  x\n}}\nswap <- function() {{ f <<- function(x) x; TRUE }}\nif ({condition}) NULL\nf(\"after\")\n"
        );
        let (_, checker) = check_adopted("short-circuit.R", &source);
        assert_eq!(
            kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            mismatch,
            "condition: {condition}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

/// Known gap (S13, pre-existing): a helper body without calls still runs
/// syntax functions that a project may mask
/// (testdata/oracle/typehint_masked_if_helper.R), but the outward-write
/// summary trusts it, so a stale identity can report RY114 where R reaches
/// the replacement. Pinned until helper summaries prove syntax identity.
#[test]
fn known_gap_masked_syntax_helper_keeps_identity() {
    for mask in [
        "`if` <- function(cond, yes, no) do.call(\"assign\", list(\"f\", function(x) x, envir = globalenv()))",
        "",
    ] {
        let source = format!(
            "{mask}\nf <- function(x) {{\n  #| x integer\n  x\n}}\npure <- function() if (TRUE) NULL\npure()\nf(\"after\")\n"
        );
        let (_, checker) = check_adopted("masked-syntax.R", &source);
        assert!(
            kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "mask: {mask}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

/// S4: an audited call whose actual reads a formal may force the caller's
/// promise, which can replace an outward name
/// (testdata/oracle/typehint_parameter_force_replacement.R).
#[test]
fn promise_actual_drops_identity() {
    let source = |between: &str| {
        format!(
            "cls <- function(x) {{\n  #| x integer\n  x\n}}\nrun <- function(y) {{\n  {between}\n  cls(\"after\")\n}}\nrun(1L)\n"
        )
    };
    for between in ["length(y)", "is.null(y)", "length(\"a\")"] {
        let (_, checker) = check_adopted("promise-actual.R", &source(between));
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "between: {between}, findings: {:?}",
            checker.declaration_findings()
        );
    }
    let (_, checker) = check_adopted("promise-none.R", &source("1L"));
    assert!(kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
    // Known gap: a direct statement-level read of the formal forces the
    // same promise but keeps the identity (`proven_parameter_read`); R
    // returns "after". Fixing it needs forced-parameter tracking.
    let (_, checker) = check_adopted("promise-direct-read.R", &source("y"));
    assert!(kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
}
