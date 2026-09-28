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
