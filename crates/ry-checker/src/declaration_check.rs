//! Shared static checking of explicitly adopted, source-attached contracts.
//! Provider readers supply records; this module never parses or evaluates R.

use std::collections::{BTreeMap, HashMap};

use ry_core::Span;
use ry_core::declarations::{
    AssignmentSemantics, AtomicMode, DeclarationRecord, DeclaredLength, DeclaredSignature,
    EvaluationSemantics, EvidenceUse, ParameterForm, SupplyStatus, Translation, TypeExpr,
};
use ry_core::types::{ClassVector, Length, MAX_UNION_MEMBERS, Mode, RType};

use crate::definitions::FunctionId;
use crate::infer::semantic_argument_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationFindingKind {
    /// A known value contradicts an adopted constraint.
    Mismatch,
    /// A translated necessary part was retained, but full checking is unsafe.
    Partial,
    /// The provider expression or effect cannot be checked by this vocabulary.
    Unsupported,
    /// Adopted sources for one lexical target cannot be selected as one contract.
    Conflict,
    InvalidSyntax,
    AmbiguousAttachment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationFinding {
    pub kind: DeclarationFindingKind,
    pub path: String,
    pub span: Span,
    pub message: String,
    /// The assertion probe (#344) the finding was produced inside,
    /// if any, so its diagnostic view is demoted one confidence tier
    /// like an ordinary emit inside the probe. Captured at creation:
    /// conversion runs after the probe stack is gone. The demotion
    /// decides from the kind and the converted severity, mirroring
    /// `Checker::probe_demotes`.
    pub(crate) probe: Option<crate::ProbeKind>,
}

/// Append the public diagnostic view of structured declaration findings.
/// CLI and LSP call this after either a fresh or warm project check, so the
/// editor's byte spans and suppression pipeline use the same records.
pub fn append_diagnostics(
    output: &mut [(String, Vec<crate::Diagnostic>)],
    findings: &[(String, Vec<DeclarationFinding>)],
) {
    for (path, records) in findings {
        let Some((_, diagnostics)) = output.iter_mut().find(|(file, _)| file == path) else {
            continue;
        };
        for record in records {
            let (code, severity) = match record.kind {
                DeclarationFindingKind::Mismatch => ("RY114", crate::Severity::Warning),
                DeclarationFindingKind::Partial | DeclarationFindingKind::Unsupported => {
                    ("RY115", crate::Severity::Info)
                }
                DeclarationFindingKind::Conflict => ("RY116", crate::Severity::Warning),
                DeclarationFindingKind::InvalidSyntax
                | DeclarationFindingKind::AmbiguousAttachment => {
                    ("RY117", crate::Severity::Warning)
                }
            };
            let mut diagnostic = crate::Diagnostic::new(
                severity,
                record.span,
                &record.path,
                code,
                record.message.clone(),
            );
            if record
                .probe
                .is_some_and(|kind| kind.catches_errors() || severity != crate::Severity::Error)
            {
                diagnostic.confidence = diagnostic.confidence.demote();
            }
            diagnostics.push(diagnostic);
        }
    }
}

impl DeclarationFinding {
    pub(crate) fn new(
        kind: DeclarationFindingKind,
        path: &str,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: path.into(),
            span,
            message: message.into(),
            probe: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct TargetKey {
    path: String,
    start: usize,
    end: usize,
}

impl TargetKey {
    fn new(path: &str, span: Span) -> Self {
        Self {
            path: path.into(),
            start: span.start,
            end: span.end,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TargetDecision {
    pub(crate) span: Span,
    pub(crate) signature: Option<DeclaredSignature>,
    pub(crate) reports: Vec<(DeclarationFindingKind, String)>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DeclarationSet {
    records: Vec<DeclarationRecord>,
    targets: BTreeMap<TargetKey, TargetDecision>,
    /// Whether any target selected a contract. Hot paths check it.
    has_contracts: bool,
}

impl DeclarationSet {
    pub(crate) fn new(records: Vec<DeclarationRecord>) -> Self {
        let mut grouped: BTreeMap<TargetKey, Vec<&DeclarationRecord>> = BTreeMap::new();
        for record in &records {
            let target = &record.source.target;
            grouped
                .entry(TargetKey::new(&target.path, target.definition))
                .or_default()
                .push(record);
        }
        let targets = grouped
            .into_iter()
            .map(|(key, records)| {
                let definition = records[0].source.target.definition;
                (key, decide_target(definition, &records))
            })
            .collect::<BTreeMap<_, _>>();
        let has_contracts = targets
            .values()
            .any(|decision| decision.signature.is_some());
        Self {
            records,
            targets,
            has_contracts,
        }
    }

    pub(crate) fn records(&self) -> &[DeclarationRecord] {
        &self.records
    }

    pub(crate) fn has_contracts(&self) -> bool {
        self.has_contracts
    }

    pub(crate) fn target(&self, path: &str, span: Span) -> Option<&TargetDecision> {
        self.targets.get(&TargetKey::new(path, span))
    }

    pub(crate) fn reports_for(&self, path: &str) -> Vec<DeclarationFinding> {
        self.targets
            .iter()
            .filter(|(key, _)| key.path == path)
            .flat_map(|(_, decision)| {
                decision.reports.iter().map(|(kind, message)| {
                    DeclarationFinding::new(*kind, path, decision.span, message)
                })
            })
            .collect()
    }
}

fn decide_target(span: Span, records: &[&DeclarationRecord]) -> TargetDecision {
    let mut exact = HashMap::<String, DeclaredSignature>::new();
    let mut reports = Vec::new();
    for record in records {
        if record.evidence != EvidenceUse::AdoptedContract {
            continue;
        }
        let provider = record.source.provider.as_str();
        match &record.translation {
            Translation::Exact(signature) => match signature.canonical() {
                Ok(canonical) if signature.assignment == AssignmentSemantics::EntryOnly => {
                    if signature.parameters.iter().any(|parameter| {
                        parameter.constraint.is_some()
                            && !matches!(
                                parameter.evaluation,
                                EvaluationSemantics::Promise | EvaluationSemantics::Value
                            )
                    }) {
                        reports.push((
                            DeclarationFindingKind::Unsupported,
                            format!(
                                "adopted {provider} declaration has quoted or unknown parameter evaluation; no entry assumption is selected"
                            ),
                        ));
                    } else {
                        // Canonical spelling is also the checking identity. Keep the
                        // authored record untouched for provenance, but never let
                        // record order choose a differently shaped equivalent tree.
                        exact.entry(canonical).or_insert_with(|| {
                            signature
                                .normalized()
                                .expect("canonical signature normalizes")
                        });
                    }
                }
                Ok(_) => {
                    reports.push((
                        DeclarationFindingKind::Unsupported,
                        format!(
                            "adopted {provider} declaration has assignment effects outside entry-only checking"
                        ),
                    ));
                }
                Err(error) => {
                    reports.push((
                        DeclarationFindingKind::InvalidSyntax,
                        format!("adopted {provider} declaration is invalid: {error}"),
                    ));
                }
            },
            Translation::Partial { residuals, .. } => {
                reports.push((
                    DeclarationFindingKind::Partial,
                    format!(
                        "adopted {provider} declaration has {} unsupported residual constraint(s); no complete contract is selected",
                        residuals.len()
                    ),
                ));
            }
            Translation::Unsupported { residuals } => {
                reports.push((
                    DeclarationFindingKind::Unsupported,
                    format!(
                        "adopted {provider} declaration is unsupported ({} residual constraint(s))",
                        residuals.len()
                    ),
                ));
            }
            Translation::InvalidSyntax(reason) => {
                reports.push((
                    DeclarationFindingKind::InvalidSyntax,
                    format!("adopted {provider} declaration has invalid syntax: {reason}"),
                ));
            }
            Translation::AmbiguousAttachment(reason) => {
                reports.push((
                    DeclarationFindingKind::AmbiguousAttachment,
                    format!("adopted {provider} declaration has ambiguous attachment: {reason}"),
                ));
            }
        }
    }
    let blocked = !reports.is_empty();
    if exact.len() > 1 || (blocked && !exact.is_empty()) {
        reports.push((
            DeclarationFindingKind::Conflict,
            "adopted declarations for this lexical function cannot be reconciled; none is used as a contract"
                .into(),
        ));
    }
    let signature = if blocked || exact.len() != 1 {
        None
    } else {
        exact.into_values().next()
    };
    TargetDecision {
        span,
        signature,
        reports,
    }
}

/// Validates only metadata that is independently known from the R formals.
/// A signature may constrain an ordered subset of formals. An empty list
/// never means the function has zero arguments.
fn matches_formals<'a>(
    signature: &DeclaredSignature,
    formals: impl Iterator<Item = (&'a str, bool)>,
) -> Result<(), String> {
    let formals: Vec<_> = formals.collect();
    let mut next = 0;
    for parameter in &signature.parameters {
        let Some(offset) = formals[next..]
            .iter()
            .position(|(name, _)| semantic_argument_name(name) == parameter.name)
        else {
            return Err(
                if formals
                    .iter()
                    .any(|(name, _)| semantic_argument_name(name) == parameter.name)
                {
                    format!(
                        "declared parameter `{}` has a different order from the attached function",
                        parameter.name
                    )
                } else {
                    format!(
                        "declared parameter `{}` is absent from the attached function",
                        parameter.name
                    )
                },
            );
        };
        let (_, defaulted) = formals[next + offset];
        next += offset + 1;
        if (parameter.form == ParameterForm::Variadic) != (parameter.name == "...") {
            return Err(format!(
                "declared parameter `{}` has the wrong formal form",
                parameter.name
            ));
        }
        if (matches!(
            parameter.supplied,
            SupplyStatus::Defaulted | SupplyStatus::DefaultedSuppliedOnly
        ) && !defaulted)
            || (parameter.supplied == SupplyStatus::Required && defaulted)
        {
            return Err(format!(
                "declared supplied/defaulted status of `{}` disagrees with its R formal",
                parameter.name
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Evidence {
    Compatible,
    Incompatible,
    Insufficient,
}

/// Compare an independently inferred type with an authored predicate. A
/// mixed union or an unknown fact is never a proven violation.
fn compare(ty: &RType, constraint: &TypeExpr) -> Evidence {
    if ty.mode == Mode::Union {
        let Some(members) = &ty.members else {
            return Evidence::Insufficient;
        };
        return combine_all(members.iter().map(|member| compare(member, constraint)));
    }
    match constraint {
        // `RType::class` describes an explicit class attribute. An empty
        // attribute does not establish effective `class(x)`: dimensions and
        // implicit atomic classes may still determine that result. A class
        // test proves only that the value passed it.
        TypeExpr::ExactClass(expected) => {
            if !ty.class.known || ty.class.guarded || ty.class.len == 0 || ty.class.len >= 4 {
                Evidence::Insufficient
            } else if ty.class.len == 1
                && ty.class.names[0]
                    .as_deref()
                    .is_some_and(|name| name == expected)
            {
                Evidence::Compatible
            } else {
                Evidence::Incompatible
            }
        }
        _ if ty.mode.is_unknown_or_non_vector() => Evidence::Insufficient,
        TypeExpr::Unknown => Evidence::Insufficient,
        TypeExpr::Union(alternatives) => {
            combine_any(alternatives.iter().map(|part| compare(ty, part)))
        }
        TypeExpr::Atomic { mode, length } => {
            if ty.mode != atomic_mode(*mode) {
                return Evidence::Incompatible;
            }
            match length {
                None => Evidence::Compatible,
                Some(DeclaredLength::Exact(expected)) => match ty.length {
                    Length::Zero if *expected == 0 => Evidence::Compatible,
                    Length::One if *expected == 1 => Evidence::Compatible,
                    Length::Known(actual) if actual == *expected => Evidence::Compatible,
                    Length::Zero | Length::One | Length::Known(_) => Evidence::Incompatible,
                    Length::Nonempty if *expected == 0 => Evidence::Incompatible,
                    Length::Nonempty | Length::Unknown => Evidence::Insufficient,
                },
                Some(DeclaredLength::Nonempty) => match ty.length {
                    Length::Zero | Length::Known(0) => Evidence::Incompatible,
                    Length::One | Length::Known(_) | Length::Nonempty => Evidence::Compatible,
                    Length::Unknown => Evidence::Insufficient,
                },
            }
        }
    }
}

/// Direct literal syntax proves the default implicit class without relying
/// on RType's empty explicit-class attribute (which may hide dimensions).
fn direct_literal_class(expr: &ry_core::ast::Expr) -> Option<&'static str> {
    use ry_core::ast::Expr;
    let mode = match expr {
        Expr::Logical(..) => Mode::Logical,
        Expr::Integer(..) => Mode::Integer,
        Expr::Double(..) => Mode::Double,
        Expr::String(..) => Mode::Character,
        Expr::Null(..) => return Some("NULL"),
        Expr::Na(ty, _) => ty.mode,
        _ => return None,
    };
    match mode {
        Mode::Logical => Some("logical"),
        Mode::Integer => Some("integer"),
        Mode::Double => Some("numeric"),
        Mode::Complex => Some("complex"),
        Mode::Character => Some("character"),
        _ => None,
    }
}

fn compare_actual(ty: &RType, expression: &ry_core::ast::Expr, constraint: &TypeExpr) -> Evidence {
    match constraint {
        TypeExpr::ExactClass(expected) => match compare(ty, constraint) {
            Evidence::Insufficient => {
                direct_literal_class(expression).map_or(Evidence::Insufficient, |actual| {
                    if actual == expected {
                        Evidence::Compatible
                    } else {
                        Evidence::Incompatible
                    }
                })
            }
            known => known,
        },
        TypeExpr::Union(alternatives) => combine_any(
            alternatives
                .iter()
                .map(|part| compare_actual(ty, expression, part)),
        ),
        _ => compare(ty, constraint),
    }
}

/// A union constraint holds when any alternative holds, and is violated
/// only when every alternative is (an empty union proves nothing).
fn combine_any(results: impl Iterator<Item = Evidence>) -> Evidence {
    let mut any = false;
    let mut all_incompatible = true;
    for result in results {
        if result == Evidence::Compatible {
            return Evidence::Compatible;
        }
        any = true;
        all_incompatible &= result == Evidence::Incompatible;
    }
    if any && all_incompatible {
        Evidence::Incompatible
    } else {
        Evidence::Insufficient
    }
}

fn combine_all(results: impl Iterator<Item = Evidence>) -> Evidence {
    let mut any = false;
    let mut all_compatible = true;
    let mut all_incompatible = true;
    for result in results {
        any = true;
        all_compatible &= result == Evidence::Compatible;
        all_incompatible &= result == Evidence::Incompatible;
    }
    if !any {
        Evidence::Insufficient
    } else if all_compatible {
        Evidence::Compatible
    } else if all_incompatible {
        Evidence::Incompatible
    } else {
        Evidence::Insufficient
    }
}

fn atomic_mode(mode: AtomicMode) -> Mode {
    match mode {
        AtomicMode::Logical => Mode::Logical,
        AtomicMode::Integer => Mode::Integer,
        AtomicMode::Double => Mode::Double,
        AtomicMode::Complex => Mode::Complex,
        AtomicMode::Character => Mode::Character,
        AtomicMode::Raw => Mode::Raw,
        AtomicMode::List => Mode::List,
        AtomicMode::Null => Mode::Null,
    }
}

pub(crate) fn body_entry_type(constraint: &TypeExpr) -> Option<RType> {
    match constraint {
        TypeExpr::Unknown => Some(RType::unknown()),
        TypeExpr::ExactClass(_) => None,
        TypeExpr::Atomic { mode, length } => {
            let length = match length {
                Some(DeclaredLength::Exact(0)) => Length::Zero,
                Some(DeclaredLength::Exact(1)) => Length::One,
                Some(DeclaredLength::Exact(value)) => Length::Known(*value),
                Some(DeclaredLength::Nonempty) => Length::Nonempty,
                None if *mode == AtomicMode::Null => Length::Zero,
                None => Length::Unknown,
            };
            // A storage-mode declaration does not prove a known-empty class.
            Some(RType::new(atomic_mode(*mode), length).with_class(ClassVector::unknown()))
        }
        TypeExpr::Union(members) if !members.is_empty() && members.len() <= MAX_UNION_MEMBERS => {
            let members = members
                .iter()
                .map(body_entry_type)
                .collect::<Option<Vec<_>>>()?;
            Some(RType::union(members.into()).with_class(ClassVector::unknown()))
        }
        TypeExpr::Union(_) => None,
    }
}

impl crate::Checker {
    fn push_declaration_finding(
        &mut self,
        kind: DeclarationFindingKind,
        span: Span,
        message: impl Into<String>,
    ) {
        let mut finding = DeclarationFinding::new(kind, &self.path, span, message);
        finding.probe = self
            .assertion_probe_spans
            .iter()
            .filter(|(probe, _)| probe.start <= span.start && span.end <= probe.end)
            .map(|(_, kind)| *kind)
            .max_by_key(|kind| kind.catches_errors());
        self.declaration_findings.push(finding);
    }

    pub(crate) fn declaration_for_body(
        &mut self,
        span: Span,
        params: &[ry_core::ast::Param],
    ) -> Option<DeclaredSignature> {
        if self.discarding {
            return None;
        }
        let signature = self
            .declarations
            .target(&self.path, span)?
            .signature
            .clone()?;
        if let Err(reason) = matches_formals(
            &signature,
            params
                .iter()
                .map(|parameter| (parameter.name.as_str(), parameter.default.is_some())),
        ) {
            self.push_declaration_finding(
                DeclarationFindingKind::AmbiguousAttachment,
                span,
                reason,
            );
            return None;
        }
        if signature.parameters.iter().any(|parameter| {
            parameter.form == ParameterForm::Variadic && parameter.constraint.is_some()
        }) {
            self.push_declaration_finding(
                DeclarationFindingKind::Partial,
                span,
                "variadic value constraints are retained but not checked",
            );
        }
        Some(signature)
    }

    pub(crate) fn declared_body_parameter(
        &mut self,
        signature: Option<&DeclaredSignature>,
        parameter: &ry_core::ast::Param,
        independent_default: &RType,
    ) -> Option<RType> {
        let selected = signature?.parameters.iter().find(|declared| {
            declared.name == semantic_argument_name(&parameter.name)
                && declared.form == ParameterForm::Ordinary
        })?;
        let declared = selected.constraint.as_ref()?;
        let supplied_only = selected.supplied == SupplyStatus::DefaultedSuppliedOnly;
        if !supplied_only
            && parameter.default.as_ref().is_some_and(|default| {
                compare_actual(independent_default, default, declared) == Evidence::Incompatible
            })
        {
            self.push_declaration_finding(
                DeclarationFindingKind::Mismatch,
                parameter.span,
                format!(
                    "default for `{}` conflicts with adopted parameter constraint `{}`",
                    parameter.name,
                    declared
                        .canonical()
                        .expect("selected declaration is canonical")
                ),
            );
        }
        // A supplied-only constraint and an effective-class predicate never
        // establish a body type. That is their meaning, not a checking gap.
        if supplied_only || matches!(declared, TypeExpr::ExactClass(_)) {
            return None;
        }
        let entry = body_entry_type(declared);
        if entry.is_none() {
            self.push_declaration_finding(
                DeclarationFindingKind::Partial,
                parameter.span,
                format!(
                    "constraint for `{}` cannot be represented by body inference; no entry type is assumed",
                    parameter.name
                ),
            );
        }
        entry
    }

    pub(crate) fn check_declaration_return(
        &mut self,
        span: Span,
        function_name: Option<&str>,
        signature: Option<&DeclaredSignature>,
    ) {
        let Some(declared) = signature.and_then(|signature| signature.return_constraint.as_ref())
        else {
            return;
        };
        let id = FunctionId::new(&self.path, span);
        let Some(function) = self.fn_table.functions.get(&id).cloned() else {
            self.push_declaration_finding(
                DeclarationFindingKind::Partial,
                span,
                "independent return evidence for this definition is unavailable; return constraint was not checked",
            );
            return;
        };
        let inferred = if self.fn_table.functions.is_live(&id) {
            self.return_slots.get(function.return_slot)
        } else {
            // Pass 2 only refines the last function assigned to each name.
            // An adopted earlier literal still has independently checkable
            // body evidence. Infer it without seeding its declared entry or
            // altering the caller-visible last-definition table.
            let was_discarding = self.discarding;
            self.discarding = true;
            let inferred = self
                .infer_definition_return(function_name.unwrap_or("<function>"), &function)
                .unwrap_or_else(RType::unknown);
            self.discarding = was_discarding;
            inferred
        };
        if compare(&inferred, declared) == Evidence::Incompatible {
            self.push_declaration_finding(
                DeclarationFindingKind::Mismatch,
                span,
                format!(
                    "independently inferred return of `{}` conflicts with adopted return constraint `{}`",
                    function_name.unwrap_or("<function>"),
                    declared.canonical().expect("selected declaration is canonical")
                ),
            );
        }
    }

    pub(crate) fn check_declaration_call(
        &mut self,
        original_name: &str,
        lookup_name: &str,
        function: Option<&crate::UserFn>,
        args: &[ry_core::ast::Arg],
        arg_types: &[RType],
    ) {
        if self.discarding || original_name.contains("::") {
            return;
        }
        let Some(function) = function else {
            return;
        };
        let Some(signature) = self
            .declarations
            .target(&function.source_path, function.definition_span)
            .and_then(|decision| decision.signature.as_ref())
        else {
            return;
        };
        if matches_formals(
            signature,
            function
                .params
                .iter()
                .map(|parameter| (parameter.name.as_str(), parameter.defaulted)),
        )
        .is_err()
        {
            return;
        }
        let names: Vec<_> = function
            .params
            .iter()
            .map(|parameter| semantic_argument_name(&parameter.name))
            .collect();
        let bindings = crate::infer::match_argument_names(
            &names,
            args.iter()
                .map(|argument| argument.name.as_deref().map(semantic_argument_name)),
        );
        let mut mismatches = Vec::new();
        for (index, (argument, actual)) in args.iter().zip(arg_types).enumerate() {
            if matches!(argument.value, ry_core::ast::Expr::Missing(_)) {
                continue;
            }
            let Some(formal) =
                bindings.param_for_arg[index].and_then(|formal| function.params.get(formal))
            else {
                // Dots have provider-specific element semantics. Do not guess
                // an entry type for `...`.
                continue;
            };
            let Some(constraint) = signature
                .parameters
                .iter()
                .find(|declared| declared.name == semantic_argument_name(&formal.name))
                .and_then(|declared| declared.constraint.as_ref())
            else {
                continue;
            };
            if compare_actual(actual, &argument.value, constraint) == Evidence::Incompatible {
                mismatches.push((argument.span, formal.name.clone(), constraint.clone()));
            }
        }
        for (span, formal, constraint) in mismatches {
            self.push_declaration_finding(
                DeclarationFindingKind::Mismatch,
                span,
                format!(
                    "argument for `{lookup_name}` formal `{formal}` conflicts with adopted constraint `{}`",
                    constraint.canonical().expect("selected declaration is canonical")
                ),
            );
        }
    }
}
