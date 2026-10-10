//! Carrier-independent authored constraints.
//!
//! This is deliberately separate from `RType`: an inferred value is evidence
//! about one program point, while a declaration is an author's constraint.

use std::fmt;

use crate::Span;

pub const MAX_DECLARATION_BYTES: usize = 4096;
pub const MAX_DECLARATION_DEPTH: usize = 16;
pub const MAX_DECLARATION_NODES: usize = 64;
pub const MAX_UNION_ALTERNATIVES: usize = 16;
pub const MAX_SIGNATURE_PARAMETERS: usize = 64;
pub const MAX_PARAMETER_NAME_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationError {
    InvalidSyntax(String),
    ResourceLimit(String),
}

impl fmt::Display for DeclarationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSyntax(reason) => write!(f, "invalid declaration: {reason}"),
            Self::ResourceLimit(reason) => write!(f, "declaration resource limit: {reason}"),
        }
    }
}

impl std::error::Error for DeclarationError {}

/// Storage-mode predicates supported by the first declaration language.
/// `numeric` is intentionally absent: providers disagree about its exact
/// predicate, so adapters must translate only after auditing their semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AtomicMode {
    Logical,
    Integer,
    Double,
    Complex,
    Character,
    Raw,
    List,
    Null,
}

impl AtomicMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Logical => "logical",
            Self::Integer => "integer",
            Self::Double => "double",
            Self::Complex => "complex",
            Self::Character => "character",
            Self::Raw => "raw",
            Self::List => "list",
            Self::Null => "null",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredLength {
    Exact(usize),
    Nonempty,
}

/// A supported predicate, not a copy of the inferred `RType` lattice.
/// `Unknown` means no type claim was established; it is not an opt-out from
/// checking and cannot prove compatibility or incompatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    Unknown,
    /// Equality with the single effective value returned by R's `class()`.
    /// This does not imply a storage mode or an explicit class attribute.
    ExactClass(String),
    Atomic {
        mode: AtomicMode,
        length: Option<DeclaredLength>,
    },
    Union(Vec<TypeExpr>),
}

impl TypeExpr {
    pub fn atomic(mode: AtomicMode) -> Self {
        Self::Atomic { mode, length: None }
    }

    /// Stable spelling, with sorted and deduplicated union members. Supplied
    /// expressions obey the declaration budgets, so adapters cannot silently
    /// overflow inference caps.
    pub fn canonical(&self) -> Result<String, DeclarationError> {
        // Check the original tree before cloning or flattening it. Otherwise a
        // large tree of duplicate alternatives could evade the node budget.
        let mut nodes = 0;
        count_type_nodes(self, 0, &mut nodes)?;
        let normalized = self.clone().normalized()?;
        normalized.render()
    }

    fn normalized(self) -> Result<Self, DeclarationError> {
        match self {
            Self::Union(members) => {
                if members.is_empty() {
                    return Err(DeclarationError::InvalidSyntax("empty union".into()));
                }
                if members.len() > MAX_UNION_ALTERNATIVES {
                    return Err(DeclarationError::ResourceLimit(format!(
                        "union has more than {MAX_UNION_ALTERNATIVES} alternatives"
                    )));
                }
                let mut flattened = Vec::new();
                for member in members {
                    match member.normalized()? {
                        Self::Union(inner) => flattened.extend(inner),
                        other => flattened.push(other),
                    }
                }
                if flattened
                    .iter()
                    .any(|member| matches!(member, Self::Unknown))
                {
                    return Err(DeclarationError::InvalidSyntax(
                        "unknown cannot be a union alternative".into(),
                    ));
                }
                let mut keyed = flattened
                    .into_iter()
                    .map(|member| Ok((member.render()?, member)))
                    .collect::<Result<Vec<_>, DeclarationError>>()?;
                keyed.sort_by(|left, right| left.0.cmp(&right.0));
                keyed.dedup_by(|left, right| left.0 == right.0);
                if keyed.len() > MAX_UNION_ALTERNATIVES {
                    return Err(DeclarationError::ResourceLimit(format!(
                        "union has more than {MAX_UNION_ALTERNATIVES} alternatives"
                    )));
                }
                let mut members = keyed
                    .into_iter()
                    .map(|(_, member)| member)
                    .collect::<Vec<_>>();
                Ok(if members.len() == 1 {
                    members.remove(0)
                } else {
                    Self::Union(members)
                })
            }
            other => Ok(other),
        }
    }

    // Only normalized expressions reach this formatter. Input boundaries
    // enforce tree budgets before normalization can remove nodes or nesting.
    fn render(&self) -> Result<String, DeclarationError> {
        let spelling = match self {
            Self::Unknown => "unknown".to_string(),
            Self::ExactClass(name) => {
                if name.is_empty() || name.len() > MAX_PARAMETER_NAME_BYTES {
                    return Err(DeclarationError::InvalidSyntax(
                        "class name must be nonempty and at most 256 bytes".into(),
                    ));
                }
                format!(
                    "class[{}]",
                    serde_json::to_string(name).expect("string serializes")
                )
            }
            Self::Atomic { mode, length } => {
                if *mode == AtomicMode::Null
                    && length.is_some_and(|length| length != DeclaredLength::Exact(0))
                {
                    return Err(DeclarationError::InvalidSyntax(
                        "null has length zero".into(),
                    ));
                }
                let mut spelling = mode.name().to_string();
                if let Some(length) = length {
                    spelling.push_str("<len=");
                    match length {
                        DeclaredLength::Exact(value) => spelling.push_str(&value.to_string()),
                        DeclaredLength::Nonempty => spelling.push_str("1+"),
                    }
                    spelling.push('>');
                }
                spelling
            }
            Self::Union(members) => {
                let rendered = members
                    .iter()
                    .map(Self::render)
                    .collect::<Result<Vec<_>, _>>()?;
                format!("union[{}]", rendered.join(", "))
            }
        };
        if spelling.len() > MAX_DECLARATION_BYTES {
            return Err(DeclarationError::ResourceLimit(format!(
                "canonical form exceeds {MAX_DECLARATION_BYTES} bytes"
            )));
        }
        Ok(spelling)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterForm {
    Ordinary,
    Variadic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplyStatus {
    Required,
    Defaulted,
    /// The R formal has a default, but the authored predicate applies only
    /// when a caller explicitly supplies an actual argument.
    DefaultedSuppliedOnly,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationSemantics {
    Value,
    Promise,
    Quoted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentSemantics {
    EntryOnly,
    PersistentBinding,
    CoercesInput,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredParameter {
    pub name: String,
    pub form: ParameterForm,
    pub supplied: SupplyStatus,
    pub evaluation: EvaluationSemantics,
    /// `None` means no constraint was supplied. `Some(Unknown)` records an
    /// explicit unknown constraint, which cannot prove any compatibility.
    pub constraint: Option<TypeExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredSignature {
    pub parameters: Vec<DeclaredParameter>,
    pub return_constraint: Option<TypeExpr>,
    pub assignment: AssignmentSemantics,
}

impl DeclaredSignature {
    /// Canonical signature spelling includes formals, supplied/defaulted
    /// status, evaluation, assignment effects, and return constraints.
    pub fn canonical(&self) -> Result<String, DeclarationError> {
        self.validate()?;
        let mut text = format!("fn[{}](", self.assignment.name());
        for (index, parameter) in self.parameters.iter().enumerate() {
            if index != 0 {
                text.push_str(", ");
            }
            text.push_str(match parameter.form {
                ParameterForm::Variadic => "variadic",
                ParameterForm::Ordinary => parameter.supplied.name(),
            });
            text.push('/');
            text.push_str(parameter.evaluation.name());
            text.push(' ');
            text.push_str(&serde_json::to_string(&parameter.name).expect("string serializes"));
            text.push_str(": ");
            text.push_str(&match &parameter.constraint {
                Some(constraint) => constraint.canonical()?,
                None => "none".into(),
            });
            if text.len() > MAX_DECLARATION_BYTES {
                return Err(DeclarationError::ResourceLimit(format!(
                    "signature exceeds {MAX_DECLARATION_BYTES} bytes"
                )));
            }
        }
        text.push_str(") -> ");
        text.push_str(&match &self.return_constraint {
            Some(constraint) => constraint.canonical()?,
            None => "none".into(),
        });
        if text.len() > MAX_DECLARATION_BYTES {
            return Err(DeclarationError::ResourceLimit(format!(
                "signature exceeds {MAX_DECLARATION_BYTES} bytes"
            )));
        }
        Ok(text)
    }

    /// The signature `canonical` spells, with each constraint's unions
    /// flattened, sorted, and deduplicated. It obeys the same budgets, which
    /// apply to the original tree before flattening.
    pub fn normalized(&self) -> Result<Self, DeclarationError> {
        self.canonical()?;
        let mut signature = self.clone();
        for parameter in &mut signature.parameters {
            parameter.constraint = parameter
                .constraint
                .take()
                .map(TypeExpr::normalized)
                .transpose()?;
        }
        signature.return_constraint = signature
            .return_constraint
            .take()
            .map(TypeExpr::normalized)
            .transpose()?;
        Ok(signature)
    }

    /// Validate shared formal and type-tree budgets without formatting the
    /// individual constraints. Serializers can then render each one once.
    pub fn validate(&self) -> Result<(), DeclarationError> {
        if self.parameters.len() > MAX_SIGNATURE_PARAMETERS {
            return Err(DeclarationError::ResourceLimit(format!(
                "signature has more than {MAX_SIGNATURE_PARAMETERS} parameters"
            )));
        }
        let mut seen = std::collections::HashSet::new();
        let mut type_nodes = 0;
        for parameter in &self.parameters {
            if parameter.name.len() > MAX_PARAMETER_NAME_BYTES {
                return Err(DeclarationError::ResourceLimit(format!(
                    "parameter name exceeds {MAX_PARAMETER_NAME_BYTES} bytes"
                )));
            }
            if parameter.name.is_empty()
                || (parameter.form == ParameterForm::Variadic) != (parameter.name == "...")
                || !seen.insert(&parameter.name)
            {
                return Err(DeclarationError::InvalidSyntax(
                    "invalid or duplicate formal identity".into(),
                ));
            }
            if parameter.form == ParameterForm::Variadic
                && parameter.supplied != SupplyStatus::Unknown
            {
                return Err(DeclarationError::InvalidSyntax(
                    "variadic formal cannot have required/defaulted status".into(),
                ));
            }
            if let Some(constraint) = &parameter.constraint {
                count_type_nodes(constraint, 0, &mut type_nodes)?;
            }
        }
        if let Some(constraint) = &self.return_constraint {
            count_type_nodes(constraint, 0, &mut type_nodes)?;
        }
        Ok(())
    }
}

fn count_type_nodes(
    ty: &TypeExpr,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), DeclarationError> {
    *nodes += 1;
    if depth >= MAX_DECLARATION_DEPTH || *nodes > MAX_DECLARATION_NODES {
        return Err(DeclarationError::ResourceLimit(
            "type nesting or node count exceeded".into(),
        ));
    }
    if let TypeExpr::Union(members) = ty {
        for member in members {
            count_type_nodes(member, depth + 1, nodes)?;
        }
    }
    Ok(())
}

impl SupplyStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Defaulted => "defaulted",
            Self::DefaultedSuppliedOnly => "defaulted_supplied_only",
            Self::Unknown => "unknown",
        }
    }
}

impl EvaluationSemantics {
    pub fn name(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Promise => "promise",
            Self::Quoted => "quoted",
            Self::Unknown => "unknown",
        }
    }
}

impl AssignmentSemantics {
    pub fn name(self) -> &'static str {
        match self {
            Self::EntryOnly => "entry_only",
            Self::PersistentBinding => "persistent_binding",
            Self::CoercesInput => "coerces_input",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationTarget {
    LocalFunction {
        path: String,
        definition: Span,
        display_name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationSource {
    pub provider: String,
    pub provider_version: Option<String>,
    pub path: String,
    pub span: Span,
    pub raw: String,
    pub target: DeclarationTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidualConstraint {
    pub raw: String,
    pub span: Span,
    pub reason: String,
}

/// Translation status is independent of whether a contract was adopted.
/// A partial conjunction may expose its supported necessary condition while
/// retaining the unsupported residual; an unknown disjunction should use
/// `Unsupported` instead of dropping an alternative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Translation {
    Exact(DeclaredSignature),
    Partial {
        supported: DeclaredSignature,
        residuals: Vec<ResidualConstraint>,
    },
    Unsupported {
        residuals: Vec<ResidualConstraint>,
    },
    InvalidSyntax(String),
    AmbiguousAttachment(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceUse {
    AdoptedContract,
    RuntimeGuard,
    DocumentationCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationRecord {
    pub source: DeclarationSource,
    pub translation: Translation,
    pub evidence: EvidenceUse,
    pub assumptions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atomic(mode: AtomicMode) -> TypeExpr {
        TypeExpr::atomic(mode)
    }

    fn parameter(name: &str, constraint: Option<TypeExpr>) -> DeclaredParameter {
        DeclaredParameter {
            name: name.into(),
            form: ParameterForm::Ordinary,
            supplied: SupplyStatus::Required,
            evaluation: EvaluationSemantics::Value,
            constraint,
        }
    }

    fn entry_only(parameters: Vec<DeclaredParameter>) -> DeclaredSignature {
        DeclaredSignature {
            parameters,
            return_constraint: None,
            assignment: AssignmentSemantics::EntryOnly,
        }
    }

    fn is_resource_limit<T>(result: Result<T, DeclarationError>) -> bool {
        matches!(result, Err(DeclarationError::ResourceLimit(_)))
    }

    #[test]
    fn canonical_spells_lengths_and_normalizes_nested_unions() {
        for (ty, expected) in [
            (atomic(AtomicMode::Integer), "integer"),
            (
                TypeExpr::Atomic {
                    mode: AtomicMode::Integer,
                    length: Some(DeclaredLength::Exact(1)),
                },
                "integer<len=1>",
            ),
            (
                TypeExpr::Atomic {
                    mode: AtomicMode::List,
                    length: Some(DeclaredLength::Nonempty),
                },
                "list<len=1+>",
            ),
            (TypeExpr::Unknown, "unknown"),
        ] {
            assert_eq!(ty.canonical().unwrap(), expected);
        }

        let nested = TypeExpr::Union(vec![
            atomic(AtomicMode::Integer),
            TypeExpr::Union(vec![
                atomic(AtomicMode::Character),
                TypeExpr::Union(vec![
                    atomic(AtomicMode::Double),
                    atomic(AtomicMode::Integer),
                ]),
            ]),
        ]);
        assert_eq!(
            nested.canonical().unwrap(),
            "union[character, double, integer]"
        );
        assert_eq!(
            nested.normalized().unwrap(),
            TypeExpr::Union(vec![
                atomic(AtomicMode::Character),
                atomic(AtomicMode::Double),
                atomic(AtomicMode::Integer),
            ])
        );
        for invalid in [
            TypeExpr::Union(vec![atomic(AtomicMode::Integer), TypeExpr::Unknown]),
            TypeExpr::Atomic {
                mode: AtomicMode::Null,
                length: Some(DeclaredLength::Exact(1)),
            },
        ] {
            assert!(matches!(
                invalid.canonical(),
                Err(DeclarationError::InvalidSyntax(_))
            ));
        }
    }

    #[test]
    fn exact_class_predicate_is_distinct_from_storage_mode_and_bounded() {
        let class = TypeExpr::ExactClass("integer".into());
        assert_eq!(class.canonical().unwrap(), "class[\"integer\"]");
        assert_ne!(class, atomic(AtomicMode::Integer));
        assert_eq!(
            TypeExpr::ExactClass("a \"quoted\" class".into())
                .canonical()
                .unwrap(),
            r#"class["a \"quoted\" class"]"#
        );
        for name in [String::new(), "x".repeat(MAX_PARAMETER_NAME_BYTES + 1)] {
            assert!(TypeExpr::ExactClass(name).canonical().is_err());
        }
    }

    #[test]
    fn flattened_unions_obey_unique_alternative_and_original_node_budgets() {
        let atom = |length| TypeExpr::Atomic {
            mode: AtomicMode::Integer,
            length: Some(DeclaredLength::Exact(length)),
        };
        let nested = |last| {
            TypeExpr::Union(vec![
                TypeExpr::Union((0..8).map(atom).collect()),
                TypeExpr::Union((8..last).map(atom).collect()),
            ])
        };
        let canonical = nested(16).canonical().unwrap();
        let repeated_across_levels = TypeExpr::Union(vec![
            TypeExpr::Union((0..8).map(atom).collect()),
            TypeExpr::Union((7..16).map(atom).collect()),
        ]);
        assert_eq!(repeated_across_levels.canonical().unwrap(), canonical);
        assert_eq!(
            canonical.matches("integer<len=").count(),
            MAX_UNION_ALTERNATIVES
        );
        assert!(is_resource_limit(nested(17).canonical()));
        let direct = TypeExpr::Union((0..=MAX_UNION_ALTERNATIVES).map(atom).collect());
        assert!(is_resource_limit(direct.clone().normalized()));
        assert!(is_resource_limit(direct.canonical()));
        // The flattened result has one alternative, but all 69 input nodes
        // must still count against the original-tree traversal budget.
        let many_duplicates =
            TypeExpr::Union((0..4).map(|_| TypeExpr::Union(vec![atom(1); 16])).collect());
        assert!(is_resource_limit(many_duplicates.canonical()));
        let signature = entry_only(vec![parameter("x", Some(many_duplicates))]);
        assert!(is_resource_limit(signature.normalized()));
        let mut deep = atomic(AtomicMode::Integer);
        for _ in 0..MAX_DECLARATION_DEPTH {
            deep = TypeExpr::Union(vec![deep]);
        }
        assert!(is_resource_limit(deep.canonical()));
    }

    #[test]
    fn nested_empty_union_is_invalid_even_when_flattening_would_hide_it() {
        let invalid = TypeExpr::Union(vec![atomic(AtomicMode::Integer), TypeExpr::Union(vec![])]);
        for expression in [TypeExpr::Union(vec![]), invalid.clone()] {
            assert!(matches!(
                expression.canonical(),
                Err(DeclarationError::InvalidSyntax(reason)) if reason == "empty union"
            ));
        }
        let signature = entry_only(vec![parameter("x", Some(invalid))]);
        assert!(matches!(
            signature.canonical(),
            Err(DeclarationError::InvalidSyntax(reason)) if reason == "empty union"
        ));
        assert_eq!(
            TypeExpr::Union(vec![atomic(AtomicMode::Integer)])
                .canonical()
                .unwrap(),
            "integer"
        );
    }

    #[test]
    fn signature_spelling_preserves_formal_and_effect_semantics() {
        let signature = DeclaredSignature {
            parameters: vec![
                DeclaredParameter {
                    name: "an odd \"name\"".into(),
                    form: ParameterForm::Ordinary,
                    supplied: SupplyStatus::Defaulted,
                    evaluation: EvaluationSemantics::Promise,
                    constraint: Some(TypeExpr::Union(vec![
                        atomic(AtomicMode::Null),
                        atomic(AtomicMode::Integer),
                    ])),
                },
                DeclaredParameter {
                    name: "...".into(),
                    form: ParameterForm::Variadic,
                    supplied: SupplyStatus::Unknown,
                    evaluation: EvaluationSemantics::Unknown,
                    constraint: None,
                },
            ],
            return_constraint: Some(TypeExpr::Union(vec![
                atomic(AtomicMode::Null),
                TypeExpr::Union(vec![atomic(AtomicMode::Integer), atomic(AtomicMode::Null)]),
            ])),
            assignment: AssignmentSemantics::PersistentBinding,
        };
        let canonical = signature.canonical().unwrap();
        assert_eq!(
            canonical,
            r#"fn[persistent_binding](defaulted/promise "an odd \"name\"": union[integer, null], variadic/unknown "...": none) -> union[integer, null]"#
        );
        let normalized = signature.normalized().unwrap();
        let flat = Some(TypeExpr::Union(vec![
            atomic(AtomicMode::Integer),
            atomic(AtomicMode::Null),
        ]));
        assert_eq!(normalized.parameters[0].constraint, flat);
        assert_eq!(normalized.return_constraint, flat);
        assert_eq!(normalized.canonical().unwrap(), canonical);
    }

    #[test]
    fn signature_limits_and_invalid_formals_are_distinct() {
        let too_many = entry_only(
            (0..=MAX_SIGNATURE_PARAMETERS)
                .map(|index| parameter(&format!("p{index}"), None))
                .collect(),
        );
        assert!(is_resource_limit(too_many.validate()));
        let long_name = entry_only(vec![parameter(
            &"x".repeat(MAX_PARAMETER_NAME_BYTES + 1),
            None,
        )]);
        assert!(is_resource_limit(long_name.validate()));
        let integer = || Some(atomic(AtomicMode::Integer));
        assert!(matches!(
            entry_only(vec![parameter("x", integer()), parameter("x", integer())]).validate(),
            Err(DeclarationError::InvalidSyntax(_))
        ));

        // Type nodes are budgeted across the whole signature.
        let mut shared_nodes = entry_only(
            (0..MAX_DECLARATION_NODES)
                .map(|index| parameter(&format!("p{index}"), integer()))
                .collect(),
        );
        shared_nodes.return_constraint = integer();
        assert!(is_resource_limit(shared_nodes.canonical()));

        let long_spelling = entry_only(
            (0..20)
                .map(|index| parameter(&format!("{index}{}", "x".repeat(250)), None))
                .collect(),
        );
        assert!(long_spelling.validate().is_ok());
        assert!(is_resource_limit(long_spelling.canonical()));
    }
}
