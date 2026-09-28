//! Structured records exercise the shared checker before a source adapter exists.

use std::sync::Arc;

use ry_checker::{Checker, DeclarationFindingKind, Project};
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

fn nested_function_span(file: &SourceFile, outer: &str, inner: &str) -> Span {
    let body = file
        .stmts
        .iter()
        .find_map(|statement| match statement {
            Stmt::Assign {
                target: Expr::Ident { name, .. },
                value: Expr::Function { body, .. },
                ..
            } if name == outer => Some(body),
            _ => None,
        })
        .expect("fixture has outer function");
    body.iter()
        .find_map(|statement| match statement {
            Stmt::Assign {
                target: Expr::Ident { name, .. },
                value: Expr::Function { span, .. },
                ..
            } if name == inner => Some(*span),
            _ => None,
        })
        .expect("fixture has named nested function")
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
            target: DeclarationTarget::LocalFunction {
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

fn kinds(checker: &Checker) -> Vec<DeclarationFindingKind> {
    checker
        .declaration_findings()
        .iter()
        .map(|finding| finding.kind)
        .collect()
}

fn all_named_function_spans(file: &SourceFile, name: &str) -> Vec<Span> {
    use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
    use std::ops::ControlFlow;

    let mut spans = Vec::new();
    let _ = walk_stmts(&file.stmts, Walk::ALL, |node, _| {
        if let AstNode::Stmt(Stmt::Assign {
            target: Expr::Ident { name: bound, .. },
            value: Expr::Function { span, .. },
            ..
        }) = node
            && bound == name
        {
            spans.push(*span);
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    });
    spans
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
    let file = parse(
        "top-level-order.R",
        "f <- function(x) { x }\nf(\"text\")\nf <- function(x) { x }\nf(\"text\")\n",
    );
    let spans = all_named_function_spans(&file, "f");
    assert_eq!(spans.len(), 2);
    let mut later = record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    later.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: spans[1],
        display_name: Some("f".into()),
    };
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![later]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
    let after = file.source.rfind("f(\"text\")").unwrap();
    assert!(checker.declaration_findings()[0].span.start >= after);
}

#[test]
fn sequential_top_level_literals_keep_each_adopted_call_contract() {
    let file = parse(
        "sequential-top.R",
        "f <- function(x) { x }\nf(\"bad\")\nf <- function(x) { x }\nf(1L)\n",
    );
    let spans = all_named_function_spans(&file, "f");
    assert_eq!(spans.len(), 2);
    let mut first = record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    first.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: spans[0],
        display_name: Some("f".into()),
    };
    let mut second = first.clone();
    second.source.provider = "second".into();
    second.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: spans[1],
        display_name: Some("f".into()),
    };
    if let Translation::Exact(signature) = &mut second.translation {
        signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Character));
    }
    let first_call = file.source.find("f(\"bad\")").unwrap() + "f(".len();
    let second_call = file.source.find("f(1L)").unwrap() + "f(".len();
    for (records, expected) in [
        (vec![first.clone()], vec![first_call]),
        (vec![first, second], vec![first_call, second_call]),
    ] {
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(records);
        checker.check(&file);
        let calls = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| finding.span.start)
            .collect::<Vec<_>>();
        assert_eq!(calls, expected);
    }
}

#[test]
fn sequential_nested_literals_keep_each_adopted_call_contract() {
    let file = parse(
        "sequential-nested.R",
        "outer <- function() { inner <- function(x) { x }; inner(\"bad\"); inner <- function(x) { x }; inner(1L) }\nouter()\n",
    );
    let spans = all_named_function_spans(&file, "inner");
    assert_eq!(spans.len(), 2);
    let mut first = record(
        &file,
        "outer",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    first.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: spans[0],
        display_name: Some("inner".into()),
    };
    let mut second = first.clone();
    second.source.provider = "second".into();
    second.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: spans[1],
        display_name: Some("inner".into()),
    };
    if let Translation::Exact(signature) = &mut second.translation {
        signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Character));
    }
    let first_call = file.source.find("inner(\"bad\")").unwrap() + "inner(".len();
    let second_call = file.source.find("inner(1L)").unwrap() + "inner(".len();
    for (records, expected) in [
        (vec![first.clone()], vec![first_call]),
        (vec![first, second], vec![first_call, second_call]),
    ] {
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(records);
        checker.check(&file);
        let calls = checker
            .declaration_findings()
            .iter()
            .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
            .map(|finding| finding.span.start)
            .collect::<Vec<_>>();
        assert_eq!(calls, expected);
    }
}

#[test]
fn function_position_skips_nested_nonfunction_but_not_same_environment_overwrite() {
    let nested = parse(
        "nested-nonfunction.R",
        "f <- function(x) { x }\nouter <- function() { f <- 1L; f(\"bad\") }\nouter()\n",
    );
    let mut checker = Checker::new(&nested.path);
    checker.set_declaration_records(vec![record(
        &nested,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&nested);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);

    let overwritten = parse(
        "same-environment.R",
        "f <- function(x) { x }\nf <- 1L\nf(\"bad\")\n",
    );
    checker.set_declaration_records(vec![record(
        &overwritten,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&overwritten);
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));

    for shadow in [
        "f <- function(x) { x }",
        "f <- if (flag) function(x) { x } else 1L",
    ] {
        let source = format!(
            "f <- function(x) {{ x }}\nouter <- function(flag) {{ {shadow}; f(\"bad\") }}\nouter(TRUE)\n"
        );
        let file = parse("possible-local-shadow.R", &source);
        checker.set_declaration_records(vec![record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            None,
        )]);
        checker.check(&file);
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
            "f <- function(x) {{ x }}\nouter <- function() {{ f(\"bad\") }}\n{replacement}\nouter()\n"
        );
        let file = parse("deferred-global-rebind.R", &source);
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            None,
        )]);
        checker.check(&file);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "replacement: {replacement}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn nested_closure_reads_live_enclosing_environment() {
    let file = parse(
        "nested-environment.R",
        "outer <- function() { f <- function(x) { x }; inner <- function() { f(\"bad\") }; f <- function(x) { x }; inner() }\nouter()\n",
    );
    let first = all_named_function_spans(&file, "f")[0];
    let mut declaration = record(
        &file,
        "outer",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    declaration.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: first,
        display_name: Some("f".into()),
    };
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![declaration]);
    checker.check(&file);
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));

    let stable = parse(
        "nested-stable.R",
        "outer <- function() { f <- function(x) { x }; inner <- function() { f(\"bad\") }; inner() }\nouter()\n",
    );
    declaration = record(
        &stable,
        "outer",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    declaration.source.target = DeclarationTarget::LocalFunction {
        path: stable.path.clone(),
        definition: all_named_function_spans(&stable, "f")[0],
        display_name: Some("f".into()),
    };
    checker.set_declaration_records(vec![declaration]);
    checker.check(&stable);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
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
            "f <- function(x) {{ x }}\nouter <- function() {{ f(\"bad\") }}\n{write}\nouter()\n"
        );
        let file = parse("capture-write-positions.R", &source);
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            None,
        )]);
        checker.check(&file);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "write: {write}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn later_expression_write_does_not_erase_an_earlier_direct_known_call() {
    let file = parse(
        "ordered-expression-write.R",
        "f <- function(x) { x }\nf(\"bad\")\ninvisible(f <- function(x) { x })\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&file);
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
            "f <- function(x) {{ x }}\nouter <- function() {{ f(\"bad\") }}\n{write}\nouter()\n"
        );
        let file = parse("nested-write-scope.R", &source);
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            None,
        )]);
        checker.check(&file);
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
    let file = parse(
        "nested-expression-write.R",
        "outer <- function() { f <- function(x) { x }; inner <- function() { f(\"bad\") }; if ({ f <- function(x) { x }; TRUE }) 1L; inner() }\nouter()\n",
    );
    let mut declaration = record(
        &file,
        "outer",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    declaration.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: all_named_function_spans(&file, "f")[0],
        display_name: Some("f".into()),
    };
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![declaration]);
    checker.check(&file);
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
}

#[test]
fn forced_default_superassignment_can_replace_an_outward_contract() {
    let file = parse(
        "default-superassign.R",
        "f <- function(x) { x }\nouter <- function(z = (f <<- function(x) { x })) { z; f(\"bad\") }\nouter()\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&file);
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
}

#[test]
fn forced_default_local_assignment_can_shadow_an_outward_contract() {
    let file = parse(
        "default-local-assign.R",
        "f <- function(x) { x }\nouter <- function(z = (f <- function(x) { x })) { z; f(\"bad\") }\nouter()\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    )]);
    checker.check(&file);
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
}

#[test]
fn default_iife_outward_write_is_distinct_from_literal_and_local_write() {
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
            "function() { f <<- function(x) { x } }",
            "z; f(\"bad\")",
            true,
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
            "f <- function(x) {{ x }}\nouter <- function(z = {default}) {{ {body} }}\nouter()\n"
        );
        let file = parse("default-iife.R", &source);
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![record(
            &file,
            "f",
            ("x", AtomicMode::Integer, SupplyStatus::Required),
            None,
        )]);
        checker.check(&file);
        assert_eq!(
            kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            mismatch,
            "default: {default}; body: {body}; findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn eager_forward_call_has_no_future_declaration_but_deferred_call_can_use_it() {
    let eager = parse("forward.R", "f(1L)\nf <- function(x) { x }\n");
    let mut checker = Checker::new(&eager.path);
    checker.set_declaration_records(vec![record(
        &eager,
        "f",
        ("x", AtomicMode::Character, SupplyStatus::Required),
        None,
    )]);
    checker.check(&eager);
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));

    let deferred = parse(
        "deferred.R",
        "g <- function() { f(1L) }\nf <- function(x) { x }\ng()\n",
    );
    checker.set_declaration_records(vec![record(
        &deferred,
        "f",
        ("x", AtomicMode::Character, SupplyStatus::Required),
        None,
    )]);
    checker.check(&deferred);
    assert!(kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
}

#[test]
fn same_offset_in_another_file_cannot_identify_a_local_eager_binding() {
    let first = parse("a.R", "f <- function(x) { x }\nf(1L)\n");
    let second = parse("b.R", "f <- function(x) { x }\n");
    assert_eq!(
        named_function_span(&first, "f"),
        named_function_span(&second, "f")
    );
    let mut project = Project::new();
    project.add_file(first.path.clone(), first);
    project.add_file(second.path.clone(), second.clone());
    project.set_declaration_records(vec![record(
        &second,
        "f",
        ("x", AtomicMode::Character, SupplyStatus::Required),
        None,
    )]);
    project.check();
    assert!(project.declaration_findings()[0].1.is_empty());
}

#[test]
fn project_keeps_same_named_declarations_in_distinct_files() {
    let first = parse("first.R", "f <- function(x) { x }\nf(\"bad\")\n");
    let second = parse("second.R", "f <- function(x) { x }\nf(1L)\n");
    let integer = record(
        &first,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let mut character = record(
        &second,
        "f",
        ("x", AtomicMode::Character, SupplyStatus::Required),
        None,
    );
    character.source.provider = "second".into();
    let mut project = Project::new();
    project.add_file(first.path.clone(), first);
    project.add_file(second.path.clone(), second);
    project.set_declaration_records(vec![integer, character]);
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
        "f <- function(x) { x }\nouter <- function() { f(\"bad\") }\n",
    );
    let second = parse("second.R", "f <- function(x) { x }\n");
    let declaration = record(
        &first,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let mut project = Project::new();
    project.add_file(first.path.clone(), first);
    project.add_file(second.path.clone(), second);
    project.set_declaration_records(vec![declaration]);
    project.check();
    assert!(project.declaration_findings().iter().all(|(_, findings)| {
        !findings
            .iter()
            .any(|finding| finding.kind == DeclarationFindingKind::Mismatch)
    }));
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
        "f <- function(x) { x }\nf(\"bad\")\nouter <- function() { f(\"bad\") }\n",
    );
    let declaration = record(
        &first,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    for (second_source, expected) in [
        ("invisible(f <- function(x) { x })", 1),
        ("if ({ f <- function(x) { x }; TRUE }) 1L", 1),
        ("for (f in list(function(x) { x })) 1L", 1),
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
            "mutate <- function(z = (function() { f <- function(x) { x } })()) { z }; mutate()",
            2,
        ),
        (
            "mutate <- function() { f <- function(x) { x } }; mutate()",
            2,
        ),
        ("g <- function(x) { x }", 2),
    ] {
        let second = parse("second.R", second_source);
        let mut project = Project::new();
        project.add_file(first.path.clone(), first.clone());
        project.add_file(second.path.clone(), second);
        project.set_declaration_records(vec![declaration.clone()]);
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
        "f <- function(x) { x }\nouter <- function() { f(\"bad\") }\n",
    );
    let declaration = record(
        &first,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let mut project = Project::new();
    project.add_file(first.path.clone(), first.clone());
    project.add_file("second.R".into(), parse("second.R", "g <- 1L\n"));
    project.set_declaration_records(vec![declaration.clone()]);
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &first.path), 1);

    for (source, expected) in [
        ("invisible(f <- function(x) { x })\n", 0),
        ("g <- 1L\n", 1),
        ("mutate <- function() { f <<- function(x) { x } }\n", 0),
        ("g <- 2L\n", 1),
    ] {
        let second = parse("second.R", source);
        project.update_file("second.R".into(), Arc::new(second.clone()));
        project.check_incremental();
        assert_eq!(project_mismatch_count(&project, &first.path), expected);

        let mut cold = Project::new();
        cold.add_file(first.path.clone(), first.clone());
        cold.add_file(second.path.clone(), second);
        cold.set_declaration_records(vec![declaration.clone()]);
        cold.check();
        assert_eq!(project.declaration_findings(), cold.declaration_findings());
    }
    project.remove_file("second.R");
    project.check_incremental();
    assert_eq!(project_mismatch_count(&project, &first.path), 1);
}

#[test]
fn other_file_global_write_does_not_replace_a_nested_local_literal() {
    let first = parse(
        "first.R",
        "outer <- function() { f <- function(x) { x }; inner <- function() { f(\"bad\") }; inner() }\n",
    );
    let mut declaration = record(
        &first,
        "outer",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    declaration.source.target = DeclarationTarget::LocalFunction {
        path: first.path.clone(),
        definition: all_named_function_spans(&first, "f")[0],
        display_name: Some("f".into()),
    };
    for second_source in [
        "invisible(f <- function(x) { x })",
        "mutate <- function() { f <<- function(x) { x } }; mutate()",
    ] {
        let second = parse("second.R", second_source);
        let mut project = Project::new();
        project.add_file(first.path.clone(), first.clone());
        project.add_file(second.path.clone(), second);
        project.set_declaration_records(vec![declaration.clone()]);
        project.check();
        assert_eq!(project_mismatch_count(&project, &first.path), 1);
    }
}

#[test]
fn mixed_or_unknown_evidence_does_not_prove_a_call_mismatch() {
    let file = parse(
        "uncertain.R",
        "f <- function(x) { x }\nf(if (flag) 1L else \"text\")\nf(missing_value)\nf(TRUE)\n",
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
        signature.parameters[0].constraint =
            Some(TypeExpr::parse("union[character, double, integer]").unwrap());
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
    let five = TypeExpr::parse("union[character, complex, double, integer, raw]").unwrap();
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
    let file = parse(
        "nested-calls.R",
        "first <- function() { inner <- function(x) { x }; inner(\"bad\") }\nsecond <- function() { inner <- function(x) { x }; inner(\"good\") }\nfirst()\nsecond()\n",
    );
    let mut integer = record(
        &file,
        "first",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    let mut character = integer.clone();
    for (declaration, outer, provider) in [
        (&mut integer, "first", "integer-nested"),
        (&mut character, "second", "character-nested"),
    ] {
        declaration.source.provider = provider.into();
        declaration.source.target = DeclarationTarget::LocalFunction {
            path: file.path.clone(),
            definition: nested_function_span(&file, outer, "inner"),
            display_name: Some("inner".into()),
        };
    }
    if let Translation::Exact(signature) = &mut character.translation {
        signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Character));
    }
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![integer, character]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Mismatch]);
    let mismatch = &checker.declaration_findings()[0];
    assert_eq!(
        &file.source[mismatch.span.start..mismatch.span.end],
        "\"bad\""
    );

    let rebound = parse(
        "rebound-nested.R",
        "outer <- function() { inner <- function(x) { x }; inner <- function(x) { x }; inner(\"bad\") }\nouter()\n",
    );
    let mut stale = record(
        &rebound,
        "outer",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    stale.source.target = DeclarationTarget::LocalFunction {
        path: rebound.path.clone(),
        definition: nested_function_span(&rebound, "outer", "inner"),
        display_name: Some("inner".into()),
    };
    checker.set_declaration_records(vec![stale]);
    checker.check(&rebound);
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));

    let conditional = parse(
        "conditional-nested.R",
        "outer <- function(flag) { inner <- function(x) { x }; if (flag) inner <- function(x) { x }; inner(\"bad\") }\nouter(TRUE)\n",
    );
    let mut conditional_record = record(
        &conditional,
        "outer",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        None,
    );
    conditional_record.source.target = DeclarationTarget::LocalFunction {
        path: conditional.path.clone(),
        definition: nested_function_span(&conditional, "outer", "inner"),
        display_name: Some("inner".into()),
    };
    checker.set_declaration_records(vec![conditional_record]);
    checker.check(&conditional);
    assert!(!kinds(&checker).contains(&DeclarationFindingKind::Mismatch));
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
                "inner <- function(x) {{ x }}\nouter <- function() {{ inner <- function(x) {{ x }}; {loop_body}; inner({actual}) }}\nouter()\n"
            );
            let file = parse("loop-identity.R", &source);
            let mut top = record(
                &file,
                "inner",
                ("x", AtomicMode::Character, SupplyStatus::Required),
                None,
            );
            let mut nested = top.clone();
            nested.source.provider = "nested-provider".into();
            nested.source.target = DeclarationTarget::LocalFunction {
                path: file.path.clone(),
                definition: nested_function_span(&file, "outer", "inner"),
                display_name: Some("inner".into()),
            };
            if let Translation::Exact(signature) = &mut nested.translation {
                signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Integer));
            }
            top.source.provider = "top-provider".into();
            let mut checker = Checker::new(&file.path);
            checker.set_declaration_records(vec![top, nested]);
            checker.check(&file);
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
            "inner <- function(x) {{ x }}\nouter <- function(flag) {{ inner <- function(x) {{ x }}; {loop_body}; inner(1L) }}\nouter(TRUE)\n"
        );
        let file = parse("loop-rebind.R", &source);
        let top = record(
            &file,
            "inner",
            ("x", AtomicMode::Character, SupplyStatus::Required),
            None,
        );
        let mut nested = top.clone();
        nested.source.target = DeclarationTarget::LocalFunction {
            path: file.path.clone(),
            definition: nested_function_span(&file, "outer", "inner"),
            display_name: Some("inner".into()),
        };
        if let Translation::Exact(signature) = &mut nested.translation {
            signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Integer));
        }
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![top, nested]);
        checker.check(&file);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "loop: {loop_body}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn zero_or_one_iteration_does_not_select_either_declared_literal() {
    for (loop_body, expected_integer_mismatch) in [
        ("while (flag) { inner <- function(x) { x }; break }", false),
        (
            "while (flag) { inner <- function(x) { x }; flag <- FALSE }",
            false,
        ),
        ("for (i in 1L) { inner <- function(x) { x } }", true),
    ] {
        for actual in ["1L", "\"text\""] {
            let source = format!(
                "outer <- function(flag) {{ inner <- function(x) {{ x }}; {loop_body}; inner({actual}) }}\nouter(TRUE)\n"
            );
            let file = parse("two-loop-definitions.R", &source);
            let spans = all_named_function_spans(&file, "inner");
            assert_eq!(spans.len(), 2);
            let mut first = record(
                &file,
                "outer",
                ("x", AtomicMode::Integer, SupplyStatus::Required),
                None,
            );
            first.source.target = DeclarationTarget::LocalFunction {
                path: file.path.clone(),
                definition: spans[0],
                display_name: Some("inner".into()),
            };
            let mut second = first.clone();
            second.source.provider = "second".into();
            second.source.target = DeclarationTarget::LocalFunction {
                path: file.path.clone(),
                definition: spans[1],
                display_name: Some("inner".into()),
            };
            if let Translation::Exact(signature) = &mut second.translation {
                signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Character));
            }
            let mut checker = Checker::new(&file.path);
            checker.set_declaration_records(vec![first, second]);
            checker.check(&file);
            let mismatch = kinds(&checker).contains(&DeclarationFindingKind::Mismatch);
            assert_eq!(
                mismatch,
                expected_integer_mismatch && actual == "1L",
                "loop: {loop_body}, actual: {actual}, findings: {:?}",
                checker.declaration_findings()
            );
        }
    }
}

#[test]
fn branch_only_function_introduction_cannot_borrow_flat_identity() {
    for actual in ["1L", "\"text\""] {
        let source = format!(
            "inner <- function(x) {{ x }}\nouter <- function(flag) {{ if (flag) inner <- function(x) {{ x }}; inner({actual}) }}\nouter(TRUE)\n"
        );
        let file = parse("branch-introduction.R", &source);
        let spans = all_named_function_spans(&file, "inner");
        assert_eq!(spans.len(), 2);
        let mut top = record(
            &file,
            "inner",
            ("x", AtomicMode::Character, SupplyStatus::Required),
            None,
        );
        let mut nested = top.clone();
        nested.source.provider = "nested".into();
        nested.source.target = DeclarationTarget::LocalFunction {
            path: file.path.clone(),
            definition: spans[1],
            display_name: Some("inner".into()),
        };
        if let Translation::Exact(signature) = &mut nested.translation {
            signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Integer));
        }
        top.source.target = DeclarationTarget::LocalFunction {
            path: file.path.clone(),
            definition: spans[0],
            display_name: Some("inner".into()),
        };
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![top, nested]);
        checker.check(&file);
        assert!(
            !kinds(&checker).contains(&DeclarationFindingKind::Mismatch),
            "actual: {actual}, findings: {:?}",
            checker.declaration_findings()
        );
    }
}

#[test]
fn loop_nonfunction_rebind_falls_through_to_enclosing_definition() {
    let file = parse(
        "loop-nonfunction.R",
        "inner <- function(x) { x }\nouter <- function() { inner <- function(x) { x }; for (i in 1L) { inner <- 1L }; inner(1L) }\nouter()\n",
    );
    let spans = all_named_function_spans(&file, "inner");
    let mut top = record(
        &file,
        "inner",
        ("x", AtomicMode::Character, SupplyStatus::Required),
        None,
    );
    let mut nested = top.clone();
    nested.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: spans[1],
        display_name: Some("inner".into()),
    };
    if let Translation::Exact(signature) = &mut nested.translation {
        signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Integer));
    }
    top.source.target = DeclarationTarget::LocalFunction {
        path: file.path.clone(),
        definition: spans[0],
        display_name: Some("inner".into()),
    };
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![top, nested]);
    checker.check(&file);
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
            "inner <- function(x) {{ x }}\nouter <- function(flag) {{ inner <- function(x) {{ x }}; {loop_body} }}\nouter(TRUE)\n"
        );
        let file = parse("loop-entry.R", &source);
        let top = record(
            &file,
            "inner",
            ("x", AtomicMode::Character, SupplyStatus::Required),
            None,
        );
        let mut nested = top.clone();
        nested.source.target = DeclarationTarget::LocalFunction {
            path: file.path.clone(),
            definition: nested_function_span(&file, "outer", "inner"),
            display_name: Some("inner".into()),
        };
        if let Translation::Exact(signature) = &mut nested.translation {
            signature.parameters[0].constraint = Some(TypeExpr::atomic(AtomicMode::Integer));
        }
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![top, nested]);
        checker.check(&file);
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
            "inner <- function(x) {{ x }}\nouter <- function() {{ for (inner in {iter}) {{ inner(1L) }} }}\nouter()\n"
        );
        let file = parse("loop-iterator.R", &source);
        let mut checker = Checker::new(&file.path);
        checker.set_declaration_records(vec![record(
            &file,
            "inner",
            ("x", AtomicMode::Character, SupplyStatus::Required),
            None,
        )]);
        checker.check(&file);
        assert_eq!(kinds(&checker), expected, "iterator: {iter}");
    }
}

#[test]
fn shadowed_definition_does_not_borrow_another_return_slot() {
    let file = parse(
        "shadowed.R",
        "f <- function(x) { \"old\" }\nf <- function(x) { 1L }\nf(1L)\n",
    );
    let mut checker = Checker::new(&file.path);
    checker.set_declaration_records(vec![record(
        &file,
        "f",
        ("x", AtomicMode::Integer, SupplyStatus::Required),
        Some(AtomicMode::Integer),
    )]);
    checker.check(&file);
    assert_eq!(kinds(&checker), vec![DeclarationFindingKind::Partial]);
    assert!(
        checker.declaration_findings()[0]
            .message
            .contains("return evidence")
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

    let mut cold = Project::new();
    cold.add_file("defs.R".into(), defs);
    cold.add_file("calls.R".into(), calls.clone());
    cold.set_declaration_records(vec![character]);
    cold.check();
    assert_eq!(project.declaration_findings(), cold.declaration_findings());

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
    project.update_file("defs.R".into(), Arc::new(edited_defs.clone()));
    project.set_declaration_records(vec![edited_integer.clone()]);
    project.check_incremental();
    assert_eq!(project.emit_count, 2);
    let mut edited_cold = Project::new();
    edited_cold.add_file("defs.R".into(), edited_defs);
    edited_cold.add_file("calls.R".into(), calls);
    edited_cold.set_declaration_records(vec![edited_integer]);
    edited_cold.check();
    assert_eq!(
        project.declaration_findings(),
        edited_cold.declaration_findings()
    );
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
