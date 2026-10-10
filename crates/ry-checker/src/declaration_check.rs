//! Shared static checking of explicitly adopted, source-attached contracts.
//! Provider readers supply records; this module never parses or evaluates R.

use std::collections::{BTreeMap, HashMap};

use ry_core::Span;
use ry_core::declarations::{
    AssignmentSemantics, AtomicMode, DeclarationRecord, DeclarationTarget, DeclaredLength,
    DeclaredSignature, EvaluationSemantics, EvidenceUse, ParameterForm, SupplyStatus, Translation,
    TypeExpr,
};
use ry_core::types::{ClassVector, Length, MAX_UNION_MEMBERS, Mode, RType};

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
            diagnostics.push(crate::Diagnostic::new(
                severity,
                record.span,
                &record.path,
                code,
                record.message.clone(),
            ));
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
}

impl DeclarationSet {
    pub(crate) fn new(records: Vec<DeclarationRecord>) -> Self {
        let mut grouped: BTreeMap<TargetKey, Vec<&DeclarationRecord>> = BTreeMap::new();
        for record in &records {
            let DeclarationTarget::LocalFunction {
                path, definition, ..
            } = &record.source.target;
            grouped
                .entry(TargetKey::new(path, *definition))
                .or_default()
                .push(record);
        }
        let targets = grouped
            .into_iter()
            .map(|(key, records)| {
                let DeclarationTarget::LocalFunction { definition, .. } = records[0].source.target;
                (key, decide_target(definition, &records))
            })
            .collect();
        Self { records, targets }
    }

    pub(crate) fn records(&self) -> &[DeclarationRecord] {
        &self.records
    }

    pub(crate) fn has_contracts(&self) -> bool {
        self.targets
            .values()
            .any(|decision| decision.signature.is_some())
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
    let mut blocked = false;
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
                        blocked = true;
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
                    blocked = true;
                    reports.push((
                        DeclarationFindingKind::Unsupported,
                        format!(
                            "adopted {provider} declaration has assignment effects outside entry-only checking"
                        ),
                    ));
                }
                Err(error) => {
                    blocked = true;
                    reports.push((
                        DeclarationFindingKind::InvalidSyntax,
                        format!("adopted {provider} declaration is invalid: {error}"),
                    ));
                }
            },
            Translation::Partial { residuals, .. } => {
                blocked = true;
                reports.push((
                    DeclarationFindingKind::Partial,
                    format!(
                        "adopted {provider} declaration has {} unsupported residual constraint(s); no complete contract is selected",
                        residuals.len()
                    ),
                ));
            }
            Translation::Unsupported { residuals } => {
                blocked = true;
                reports.push((
                    DeclarationFindingKind::Unsupported,
                    format!(
                        "adopted {provider} declaration is unsupported ({} residual constraint(s))",
                        residuals.len()
                    ),
                ));
            }
            Translation::InvalidSyntax(reason) => {
                blocked = true;
                reports.push((
                    DeclarationFindingKind::InvalidSyntax,
                    format!("adopted {provider} declaration has invalid syntax: {reason}"),
                ));
            }
            Translation::AmbiguousAttachment(reason) => {
                blocked = true;
                reports.push((
                    DeclarationFindingKind::AmbiguousAttachment,
                    format!("adopted {provider} declaration has ambiguous attachment: {reason}"),
                ));
            }
        }
    }
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
/// A signature may constrain an ordered subset of formals; an empty list
/// never means the function has zero arguments.
pub(crate) fn matches_formals<'a>(
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
pub(crate) enum Evidence {
    Compatible,
    Incompatible,
    Insufficient,
}

/// This is a comparison of independently inferred facts with an authored
/// predicate. A mixed union or an unknown fact is never a proven violation.
pub(crate) fn compare(ty: &RType, constraint: &TypeExpr) -> Evidence {
    if ty.mode == Mode::Union {
        let Some(members) = &ty.members else {
            return Evidence::Insufficient;
        };
        return combine_all(members.iter().map(|member| compare(member, constraint)));
    }
    if let TypeExpr::ExactClass(expected) = constraint {
        // `RType::class` describes an explicit class attribute. An empty
        // attribute does not establish effective `class(x)`: dimensions and
        // implicit atomic classes may still determine that result. A class
        // test proves only that the value passed it.
        return if !ty.class.known || ty.class.guarded || ty.class.len == 0 || ty.class.len >= 4 {
            Evidence::Insufficient
        } else if ty.class.len == 1
            && ty.class.names[0]
                .as_deref()
                .is_some_and(|name| name == expected)
        {
            Evidence::Compatible
        } else {
            Evidence::Incompatible
        };
    }
    if ty.mode == Mode::Opaque {
        return Evidence::Insufficient;
    }
    match constraint {
        TypeExpr::Unknown => Evidence::Insufficient,
        TypeExpr::ExactClass(_) => unreachable!("handled before mode comparison"),
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
    match expr {
        Expr::Logical(..) => Some("logical"),
        Expr::Integer(..) => Some("integer"),
        Expr::Double(..) => Some("numeric"),
        Expr::String(..) => Some("character"),
        Expr::Null(..) => Some("NULL"),
        Expr::Na(ty, _) => match ty.mode {
            Mode::Logical => Some("logical"),
            Mode::Integer => Some("integer"),
            Mode::Double => Some("numeric"),
            Mode::Complex => Some("complex"),
            Mode::Character => Some("character"),
            _ => None,
        },
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
            self.declaration_findings.push(DeclarationFinding::new(
                DeclarationFindingKind::AmbiguousAttachment,
                &self.path,
                span,
                reason,
            ));
            return None;
        }
        if signature.parameters.iter().any(|parameter| {
            parameter.form == ParameterForm::Variadic && parameter.constraint.is_some()
        }) {
            self.declaration_findings.push(DeclarationFinding::new(
                DeclarationFindingKind::Partial,
                &self.path,
                span,
                "variadic value constraints are retained but not checked",
            ));
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
            self.declaration_findings.push(DeclarationFinding::new(
                DeclarationFindingKind::Mismatch,
                &self.path,
                parameter.span,
                format!(
                    "default for `{}` conflicts with adopted parameter constraint `{}`",
                    parameter.name,
                    declared
                        .canonical()
                        .expect("selected declaration is canonical")
                ),
            ));
        }
        let entry = if supplied_only {
            None
        } else {
            body_entry_type(declared)
        };
        if entry.is_none() {
            let reason = if supplied_only {
                "only explicitly supplied arguments are constrained; no unconditional body entry type is assumed"
            } else if matches!(declared, TypeExpr::ExactClass(_)) {
                "effective class does not establish a storage mode; no body entry type is assumed"
            } else {
                "constraint cannot be represented by body inference; no entry type is assumed"
            };
            self.declaration_findings.push(DeclarationFinding::new(
                DeclarationFindingKind::Partial,
                &self.path,
                parameter.span,
                format!("constraint for `{}`: {reason}", parameter.name),
            ));
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
        let Some(function) = self.fn_table.definition(&self.path, span).cloned() else {
            self.declaration_findings.push(DeclarationFinding::new(
                DeclarationFindingKind::Partial,
                &self.path,
                span,
                "independent return evidence for this definition is unavailable; return constraint was not checked",
            ));
            return;
        };
        let live_definition = self
            .fn_table
            .fns
            .values()
            .any(|live| live.source_path == self.path && live.definition_span == span);
        let inferred = if live_definition {
            self.return_slots.get(function.return_slot)
        } else {
            // Pass 2 only refines the last function assigned to each name.
            // An adopted earlier literal still has independently checkable
            // body evidence; infer it without seeding its declared entry or
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
            self.declaration_findings.push(DeclarationFinding::new(
                DeclarationFindingKind::Mismatch,
                &self.path,
                span,
                format!(
                    "independently inferred return of `{}` conflicts with adopted return constraint `{}`",
                    function_name.unwrap_or("<function>"),
                    declared.canonical().expect("selected declaration is canonical")
                ),
            ));
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
                // Dots have provider-specific element semantics; preserve
                // them without guessing an entry type for `...`.
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
            self.declaration_findings.push(DeclarationFinding::new(
                DeclarationFindingKind::Mismatch,
                &self.path,
                span,
                format!(
                    "argument for `{lookup_name}` formal `{formal}` conflicts with adopted constraint `{}`",
                    constraint.canonical().expect("selected declaration is canonical")
                ),
            ));
        }
    }
}
