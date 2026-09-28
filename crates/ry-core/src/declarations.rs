//! Carrier-independent authored constraints and safe inferred-fact export.
//!
//! This is deliberately separate from `RType`: an inferred value is evidence
//! about one program point, while a declaration is an author's constraint.

use std::fmt;

use crate::Span;
use crate::types::{Length, Mode, RType};

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

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "logical" => Self::Logical,
            "integer" => Self::Integer,
            "double" => Self::Double,
            "complex" => Self::Complex,
            "character" => Self::Character,
            "raw" => Self::Raw,
            "list" => Self::List,
            "null" => Self::Null,
            _ => return None,
        })
    }

    fn from_inferred(mode: Mode) -> Option<Self> {
        Some(match mode {
            Mode::Logical => Self::Logical,
            Mode::Integer => Self::Integer,
            Mode::Double => Self::Double,
            Mode::Complex => Self::Complex,
            Mode::Character => Self::Character,
            Mode::Raw => Self::Raw,
            Mode::List => Self::List,
            Mode::Null => Self::Null,
            Mode::Function | Mode::Opaque | Mode::Union => return None,
        })
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

    pub fn parse(source: &str) -> Result<Self, DeclarationError> {
        if source.len() > MAX_DECLARATION_BYTES {
            return Err(DeclarationError::ResourceLimit(format!(
                "source exceeds {MAX_DECLARATION_BYTES} bytes"
            )));
        }
        let mut parser = Parser {
            source,
            cursor: 0,
            nodes: 0,
        };
        let ty = parser.ty(0)?;
        parser.whitespace();
        if parser.cursor != source.len() {
            return Err(DeclarationError::InvalidSyntax(format!(
                "unexpected text at byte {}",
                parser.cursor
            )));
        }
        ty.normalized()
    }

    /// Stable parseable spelling, with sorted and deduplicated union members.
    /// Programmatically supplied expressions obey the same budgets as parsed
    /// expressions, so adapters cannot silently overflow inference caps.
    pub fn canonical(&self) -> Result<String, DeclarationError> {
        let mut nodes = 0;
        self.render(0, &mut nodes)
    }

    fn normalized(self) -> Result<Self, DeclarationError> {
        match self {
            Self::Union(members) => {
                if members.len() > MAX_UNION_ALTERNATIVES {
                    return Err(DeclarationError::ResourceLimit(format!(
                        "union has more than {MAX_UNION_ALTERNATIVES} alternatives"
                    )));
                }
                let members = members
                    .into_iter()
                    .map(Self::normalized)
                    .collect::<Result<Vec<_>, _>>()?;
                if members.iter().any(|member| matches!(member, Self::Unknown)) {
                    return Err(DeclarationError::InvalidSyntax(
                        "unknown cannot be a union alternative".into(),
                    ));
                }
                let mut keyed = members
                    .into_iter()
                    .map(|member| Ok((member.canonical()?, member)))
                    .collect::<Result<Vec<_>, DeclarationError>>()?;
                keyed.sort_by(|left, right| left.0.cmp(&right.0));
                keyed.dedup_by(|left, right| left.0 == right.0);
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

    fn render(&self, depth: usize, nodes: &mut usize) -> Result<String, DeclarationError> {
        *nodes += 1;
        if depth >= MAX_DECLARATION_DEPTH || *nodes > MAX_DECLARATION_NODES {
            return Err(DeclarationError::ResourceLimit(
                "type nesting or node count exceeded".into(),
            ));
        }
        let spelling = match self {
            Self::Unknown => "unknown".to_string(),
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
                if members.is_empty() {
                    return Err(DeclarationError::InvalidSyntax("empty union".into()));
                }
                if members.len() > MAX_UNION_ALTERNATIVES {
                    return Err(DeclarationError::ResourceLimit(format!(
                        "union has more than {MAX_UNION_ALTERNATIVES} alternatives"
                    )));
                }
                let mut rendered = members
                    .iter()
                    .map(|member| member.render(depth + 1, nodes))
                    .collect::<Result<Vec<_>, _>>()?;
                if rendered.iter().any(|member| member == "unknown") {
                    return Err(DeclarationError::InvalidSyntax(
                        "unknown cannot be a union alternative".into(),
                    ));
                }
                rendered.sort();
                rendered.dedup();
                if rendered.len() == 1 {
                    rendered.remove(0)
                } else {
                    format!("union[{}]", rendered.join(", "))
                }
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

struct Parser<'a> {
    source: &'a str,
    cursor: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .source
            .as_bytes()
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn consume(&mut self, token: &str) -> bool {
        self.whitespace();
        if self.source[self.cursor..].starts_with(token) {
            self.cursor += token.len();
            true
        } else {
            false
        }
    }

    fn require(&mut self, token: &str) -> Result<(), DeclarationError> {
        if self.consume(token) {
            Ok(())
        } else {
            Err(DeclarationError::InvalidSyntax(format!(
                "expected `{token}` at byte {}",
                self.cursor
            )))
        }
    }

    fn identifier(&mut self) -> &'_ str {
        self.whitespace();
        let start = self.cursor;
        while self
            .source
            .as_bytes()
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            self.cursor += 1;
        }
        &self.source[start..self.cursor]
    }

    fn json_string(&mut self) -> Result<String, DeclarationError> {
        self.whitespace();
        let start = self.cursor;
        if !self.consume("\"") {
            return Err(DeclarationError::InvalidSyntax(format!(
                "expected quoted parameter name at byte {start}"
            )));
        }
        let mut escaped = false;
        let mut closed = false;
        for (offset, byte) in self.source[self.cursor..].bytes().enumerate() {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                self.cursor += offset + 1;
                closed = true;
                break;
            }
        }
        if !closed {
            return Err(DeclarationError::InvalidSyntax(
                "unterminated parameter name".into(),
            ));
        }
        let name: String = serde_json::from_str(&self.source[start..self.cursor])
            .map_err(|_| DeclarationError::InvalidSyntax("invalid quoted parameter name".into()))?;
        if name.len() > MAX_PARAMETER_NAME_BYTES {
            return Err(DeclarationError::ResourceLimit(format!(
                "parameter name exceeds {MAX_PARAMETER_NAME_BYTES} bytes"
            )));
        }
        Ok(name)
    }

    fn ty(&mut self, depth: usize) -> Result<TypeExpr, DeclarationError> {
        self.nodes += 1;
        if depth >= MAX_DECLARATION_DEPTH || self.nodes > MAX_DECLARATION_NODES {
            return Err(DeclarationError::ResourceLimit(
                "type nesting or node count exceeded".into(),
            ));
        }
        let identifier = self.identifier().to_string();
        if identifier == "union" {
            self.require("[")?;
            let mut members = Vec::new();
            loop {
                if members.len() >= MAX_UNION_ALTERNATIVES {
                    return Err(DeclarationError::ResourceLimit(format!(
                        "union has more than {MAX_UNION_ALTERNATIVES} alternatives"
                    )));
                }
                members.push(self.ty(depth + 1)?);
                if self.consume("]") {
                    break;
                }
                self.require(",")?;
            }
            if members.len() < 2 {
                return Err(DeclarationError::InvalidSyntax(
                    "union needs at least two alternatives".into(),
                ));
            }
            return Ok(TypeExpr::Union(members));
        }
        if identifier == "unknown" {
            return Ok(TypeExpr::Unknown);
        }
        let mode = AtomicMode::parse(&identifier).ok_or_else(|| {
            DeclarationError::InvalidSyntax(format!(
                "unsupported type `{identifier}` at byte {}",
                self.cursor
            ))
        })?;
        let length = if self.consume("<") {
            self.require("len")?;
            self.require("=")?;
            self.whitespace();
            let start = self.cursor;
            while self
                .source
                .as_bytes()
                .get(self.cursor)
                .is_some_and(u8::is_ascii_digit)
            {
                self.cursor += 1;
            }
            if start == self.cursor {
                return Err(DeclarationError::InvalidSyntax(
                    "length requires a nonnegative integer".into(),
                ));
            }
            let value = self.source[start..self.cursor]
                .parse::<usize>()
                .map_err(|_| DeclarationError::ResourceLimit("length does not fit usize".into()))?;
            let nonempty = self.consume("+");
            self.require(">")?;
            Some(if nonempty {
                if value != 1 {
                    return Err(DeclarationError::InvalidSyntax(
                        "only `1+` is supported as a lower bound".into(),
                    ));
                }
                DeclaredLength::Nonempty
            } else {
                DeclaredLength::Exact(value)
            })
        } else {
            None
        };
        if mode == AtomicMode::Null
            && length.is_some_and(|length| length != DeclaredLength::Exact(0))
        {
            return Err(DeclarationError::InvalidSyntax(
                "null has length zero".into(),
            ));
        }
        Ok(TypeExpr::Atomic { mode, length })
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

    pub fn parse(source: &str) -> Result<Self, DeclarationError> {
        if source.len() > MAX_DECLARATION_BYTES {
            return Err(DeclarationError::ResourceLimit(format!(
                "source exceeds {MAX_DECLARATION_BYTES} bytes"
            )));
        }
        let mut parser = Parser {
            source,
            cursor: 0,
            nodes: 0,
        };
        parser.require("fn")?;
        parser.require("[")?;
        let assignment_name = parser.identifier().to_string();
        let assignment = AssignmentSemantics::parse(&assignment_name)?;
        parser.require("]")?;
        parser.require("(")?;
        let mut parameters = Vec::new();
        if !parser.consume(")") {
            loop {
                if parameters.len() >= MAX_SIGNATURE_PARAMETERS {
                    return Err(DeclarationError::ResourceLimit(format!(
                        "signature has more than {MAX_SIGNATURE_PARAMETERS} parameters"
                    )));
                }
                let supplied_name = parser.identifier().to_string();
                let (form, supplied) = if supplied_name == "variadic" {
                    (ParameterForm::Variadic, SupplyStatus::Unknown)
                } else {
                    (
                        ParameterForm::Ordinary,
                        SupplyStatus::parse(&supplied_name)?,
                    )
                };
                parser.require("/")?;
                let evaluation_name = parser.identifier().to_string();
                let evaluation = EvaluationSemantics::parse(&evaluation_name)?;
                let name = parser.json_string()?;
                parser.require(":")?;
                let constraint = if parser.consume("none") {
                    None
                } else {
                    Some(parser.ty(0)?.normalized()?)
                };
                parameters.push(DeclaredParameter {
                    name,
                    form,
                    supplied,
                    evaluation,
                    constraint,
                });
                if parser.consume(")") {
                    break;
                }
                parser.require(",")?;
            }
        }
        parser.require("->")?;
        let return_constraint = if parser.consume("none") {
            None
        } else {
            Some(parser.ty(0)?.normalized()?)
        };
        parser.whitespace();
        if parser.cursor != source.len() {
            return Err(DeclarationError::InvalidSyntax(format!(
                "unexpected text at byte {}",
                parser.cursor
            )));
        }
        let signature = Self {
            parameters,
            return_constraint,
            assignment,
        };
        signature.validate()?;
        Ok(signature)
    }

    fn validate(&self) -> Result<(), DeclarationError> {
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
    fn name(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Defaulted => "defaulted",
            Self::Unknown => "unknown",
        }
    }

    fn parse(name: &str) -> Result<Self, DeclarationError> {
        match name {
            "required" => Ok(Self::Required),
            "defaulted" => Ok(Self::Defaulted),
            "unknown" => Ok(Self::Unknown),
            _ => Err(DeclarationError::InvalidSyntax(format!(
                "unsupported supplied status `{name}`"
            ))),
        }
    }
}

impl EvaluationSemantics {
    fn name(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Promise => "promise",
            Self::Quoted => "quoted",
            Self::Unknown => "unknown",
        }
    }

    fn parse(name: &str) -> Result<Self, DeclarationError> {
        match name {
            "value" => Ok(Self::Value),
            "promise" => Ok(Self::Promise),
            "quoted" => Ok(Self::Quoted),
            "unknown" => Ok(Self::Unknown),
            _ => Err(DeclarationError::InvalidSyntax(format!(
                "unsupported evaluation semantics `{name}`"
            ))),
        }
    }
}

impl AssignmentSemantics {
    fn name(self) -> &'static str {
        match self {
            Self::EntryOnly => "entry_only",
            Self::PersistentBinding => "persistent_binding",
            Self::CoercesInput => "coerces_input",
            Self::Unknown => "unknown",
        }
    }

    fn parse(name: &str) -> Result<Self, DeclarationError> {
        match name {
            "entry_only" => Ok(Self::EntryOnly),
            "persistent_binding" => Ok(Self::PersistentBinding),
            "coerces_input" => Ok(Self::CoercesInput),
            "unknown" => Ok(Self::Unknown),
            _ => Err(DeclarationError::InvalidSyntax(format!(
                "unsupported assignment semantics `{name}`"
            ))),
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
    PackageFunction {
        package: String,
        name: String,
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

/// The source of the inferred fact determines whether turning it into an
/// authored contract would assert more than the evidence established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportSite {
    ValueAtAssignment,
    ScopeExitValue,
    FunctionParameter {
        default_derived: bool,
        scope_exit: bool,
    },
    FunctionReturn {
        scope_exit: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversion {
    Exact(TypeExpr),
    Proposed {
        constraint: TypeExpr,
        reasons: Vec<String>,
    },
    Refused {
        reasons: Vec<String>,
    },
}

/// Convert only the initial storage-mode/length vocabulary. A proposal
/// requires an explicit user acknowledgement before it can become a source
/// edit; this function never inserts text or treats inference as proof of
/// the author's intended domain.
pub fn convert_inferred(ty: &RType, site: ExportSite) -> Conversion {
    let mut reasons = Vec::new();
    match site {
        ExportSite::ScopeExitValue => reasons
            .push("scope-exit type is a snapshot, not a type at every assignment or use".into()),
        ExportSite::FunctionParameter {
            default_derived: true,
            ..
        } => {
            return Conversion::Refused {
                reasons: vec![
                    "a default-derived type does not establish the accepted input domain".into(),
                ],
            };
        }
        ExportSite::FunctionParameter {
            scope_exit: true, ..
        }
        | ExportSite::FunctionReturn { scope_exit: true } => {
            return Conversion::Refused {
                reasons: vec![
                    "scope-exit evidence cannot establish a function entry or return contract"
                        .into(),
                ],
            };
        }
        _ => {}
    }
    let mut inspected_nodes = 0;
    match convert_type(ty, &mut reasons, 0, &mut inspected_nodes) {
        Ok(constraint) => match constraint.canonical() {
            Ok(_) if reasons.is_empty() => Conversion::Exact(constraint),
            Ok(_) => Conversion::Proposed {
                constraint,
                reasons,
            },
            Err(error) => {
                reasons.push(format!("converted constraint cannot be exported: {error}"));
                Conversion::Refused { reasons }
            }
        },
        Err(reason) => {
            reasons.push(reason);
            Conversion::Refused { reasons }
        }
    }
}

fn convert_type(
    ty: &RType,
    reasons: &mut Vec<String>,
    depth: usize,
    inspected_nodes: &mut usize,
) -> Result<TypeExpr, String> {
    *inspected_nodes += 1;
    if *inspected_nodes > MAX_DECLARATION_NODES {
        return Err("inferred type traversal exceeds declaration node budget".into());
    }
    if depth >= MAX_DECLARATION_DEPTH {
        return Err("inferred type nesting exceeds declaration budget".into());
    }
    if ty.class.len >= 4 {
        return Err("class vector may have been truncated by inference".into());
    }
    if !ty.class.known {
        reasons.push("class information is unknown and is omitted".into());
    } else if ty.class.len > 0 {
        reasons.push("known class constraint is outside the initial declaration vocabulary".into());
    }
    if let Some(schema) = &ty.columns {
        reasons.push(if schema.columns.len() > MAX_DECLARATION_NODES {
            "schema exceeds conversion inspection budget; field identity cannot be exported".into()
        } else if schema
            .columns
            .iter()
            .any(|(name, _)| name.starts_with("[["))
        {
            "schema keys may be synthetic; field identity cannot be exported".into()
        } else {
            "known schema fields are outside the initial declaration vocabulary".into()
        });
    }
    if ty.fn_sig.is_some() || ty.mode == Mode::Function {
        return Err("callable parameter information is incomplete".into());
    }
    if ty.mode == Mode::Opaque {
        return Err("unknown storage mode cannot establish a contract".into());
    }
    if ty.mode == Mode::Union {
        let members = ty
            .members
            .as_ref()
            .ok_or("union alternatives are unavailable")?;
        if members.len() > MAX_UNION_ALTERNATIVES {
            return Err("union exceeds declaration budget".into());
        }
        let converted = members
            .iter()
            .map(|member| convert_type(member, reasons, depth + 1, inspected_nodes))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(outer_length) = convert_length(ty.length)
            && members
                .iter()
                .any(|member| convert_length(member.length) != Some(outer_length))
        {
            reasons.push(
                "union-level length is not represented by its member constraints; that length evidence would be omitted"
                    .into(),
            );
        }
        return TypeExpr::Union(converted)
            .normalized()
            .map_err(|error| error.to_string());
    }
    let mode = AtomicMode::from_inferred(ty.mode).ok_or("unsupported storage mode")?;
    let length = convert_length(ty.length);
    Ok(TypeExpr::Atomic { mode, length })
}

fn convert_length(length: Length) -> Option<DeclaredLength> {
    match length {
        Length::Zero => Some(DeclaredLength::Exact(0)),
        Length::One => Some(DeclaredLength::Exact(1)),
        Length::Known(value) => Some(DeclaredLength::Exact(value)),
        Length::Nonempty => Some(DeclaredLength::Nonempty),
        Length::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::types::{ClassVector, ColumnSchema, FunctionSignature};

    #[test]
    fn canonical_type_round_trip_normalizes_unions_and_length() {
        for input in [
            "integer",
            "integer<len=1>",
            "list<len=1+>",
            "unknown",
            "union[ character, integer<len=01>, character ]",
        ] {
            let canonical = TypeExpr::parse(input).unwrap().canonical().unwrap();
            assert_eq!(
                TypeExpr::parse(&canonical).unwrap().canonical().unwrap(),
                canonical
            );
        }
        assert_eq!(
            TypeExpr::parse("union[character, integer, character]")
                .unwrap()
                .canonical()
                .unwrap(),
            "union[character, integer]"
        );
    }

    #[test]
    fn signature_round_trip_preserves_formal_and_effect_semantics() {
        let signature = DeclaredSignature {
            parameters: vec![
                DeclaredParameter {
                    name: "an odd \"name\"".into(),
                    form: ParameterForm::Ordinary,
                    supplied: SupplyStatus::Defaulted,
                    evaluation: EvaluationSemantics::Promise,
                    constraint: Some(TypeExpr::Union(vec![
                        TypeExpr::atomic(AtomicMode::Integer),
                        TypeExpr::atomic(AtomicMode::Null),
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
            return_constraint: Some(TypeExpr::Unknown),
            assignment: AssignmentSemantics::PersistentBinding,
        };
        let canonical = signature.canonical().unwrap();
        assert_eq!(DeclaredSignature::parse(&canonical).unwrap(), signature);
        assert_eq!(
            DeclaredSignature::parse(&canonical)
                .unwrap()
                .canonical()
                .unwrap(),
            canonical
        );
        assert!(canonical.contains("fn[persistent_binding]"));
        assert!(canonical.contains("variadic/unknown \"...\": none"));
        assert!(canonical.ends_with("-> unknown"));
    }

    #[test]
    fn resource_limits_and_unsupported_syntax_are_distinct() {
        assert!(matches!(
            TypeExpr::parse(&"integer ".repeat(600)),
            Err(DeclarationError::ResourceLimit(_))
        ));
        assert!(matches!(
            TypeExpr::parse(&"union[".repeat(20)),
            Err(DeclarationError::ResourceLimit(_))
        ));
        for input in [
            "numeric",
            "integer<len=0+>",
            "null<len=1>",
            "union[integer, unknown]",
        ] {
            assert!(matches!(
                TypeExpr::parse(input),
                Err(DeclarationError::InvalidSyntax(_))
            ));
        }
        let too_many = format!(
            "fn[entry_only]({}) -> none",
            (0..=MAX_SIGNATURE_PARAMETERS)
                .map(|index| format!("required/value \"p{index}\": integer"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        assert!(matches!(
            DeclaredSignature::parse(&too_many),
            Err(DeclarationError::ResourceLimit(_))
        ));
        assert!(matches!(
            DeclaredSignature::parse(
                "fn[entry_only](required/value \"x\": integer, required/value \"x\": integer) -> none"
            ),
            Err(DeclarationError::InvalidSyntax(_))
        ));
        assert!(matches!(
            DeclaredSignature::parse(
                "fn[entry_only](required/value \"x\": union[integer, unknown]) -> none"
            ),
            Err(DeclarationError::InvalidSyntax(_))
        ));

        let signature = DeclaredSignature {
            parameters: (0..MAX_DECLARATION_NODES)
                .map(|index| DeclaredParameter {
                    name: format!("p{index}"),
                    form: ParameterForm::Ordinary,
                    supplied: SupplyStatus::Required,
                    evaluation: EvaluationSemantics::Value,
                    constraint: Some(TypeExpr::atomic(AtomicMode::Integer)),
                })
                .collect(),
            return_constraint: Some(TypeExpr::atomic(AtomicMode::Integer)),
            assignment: AssignmentSemantics::EntryOnly,
        };
        assert!(matches!(
            signature.canonical(),
            Err(DeclarationError::ResourceLimit(_))
        ));
    }

    #[test]
    fn malformed_declarations_never_panic_or_escape_budgets() {
        // Deterministic byte soup covers delimiters, quotes, escapes, and
        // multibyte input without making fuzz tooling a build requirement.
        let alphabet = [
            "[", "]", "<", ">", "=", ",", "\\", "\"", "é", "😀", "\0", "x",
        ];
        let mut state = 0x8de4_3a1f_u64;
        for _ in 0..1_000 {
            let mut source = String::new();
            for _ in 0..80 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                source.push_str(alphabet[(state as usize) % alphabet.len()]);
            }
            for result in [
                TypeExpr::parse(&source).map(|ty| ty.canonical()),
                DeclaredSignature::parse(&source).map(|signature| signature.canonical()),
            ] {
                if let Ok(Ok(canonical)) = result {
                    assert!(canonical.len() <= MAX_DECLARATION_BYTES);
                }
            }
        }

        // Mutating valid prefixes also exercises states that random strings
        // rarely reach, including nested unions and quoted formal names.
        for seed in [
            "union[integer<len=1>, character, list<len=1+>]",
            "fn[entry_only](required/promise \"x\\\"y\": union[integer, null]) -> character",
        ] {
            for offset in 0..seed.len() {
                for replacement in [b'[', b'\"', b'\\', b',', b'?', b'0'] {
                    let mut mutated = seed.as_bytes().to_vec();
                    mutated[offset] = replacement;
                    let Ok(source) = std::str::from_utf8(&mutated) else {
                        continue;
                    };
                    if let Ok(ty) = TypeExpr::parse(source) {
                        assert!(ty.canonical().unwrap().len() <= MAX_DECLARATION_BYTES);
                    }
                    if let Ok(signature) = DeclaredSignature::parse(source) {
                        assert!(signature.canonical().unwrap().len() <= MAX_DECLARATION_BYTES);
                    }
                }
            }
        }
    }

    #[test]
    fn inference_export_never_turns_snapshot_or_partial_callable_into_proof() {
        let plain = RType::scalar(Mode::Integer);
        assert!(matches!(
            convert_inferred(&plain, ExportSite::ValueAtAssignment),
            Conversion::Exact(_)
        ));
        assert!(matches!(
            convert_inferred(&plain, ExportSite::ScopeExitValue),
            Conversion::Proposed { .. }
        ));
        assert!(matches!(
            convert_inferred(
                &plain,
                ExportSite::FunctionParameter {
                    default_derived: true,
                    scope_exit: false,
                }
            ),
            Conversion::Refused { .. }
        ));
        let callable = RType::scalar(Mode::Function).with_fn_sig(Arc::new(FunctionSignature {
            params: vec![],
            return_type: Box::new(plain),
        }));
        assert!(matches!(
            convert_inferred(&callable, ExportSite::ValueAtAssignment),
            Conversion::Refused { .. }
        ));
    }

    #[test]
    fn class_capacity_and_uncertain_field_names_cannot_be_exact() {
        let classed = RType::scalar(Mode::Integer)
            .with_class(ClassVector::from_slice(&["a", "b", "c", "d", "e"]));
        assert!(matches!(
            convert_inferred(&classed, ExportSite::ValueAtAssignment),
            Conversion::Refused { .. }
        ));
        let schema = RType::new(Mode::List, Length::One).with_columns(Arc::new(ColumnSchema {
            columns: vec![("[[1]]".into(), RType::scalar(Mode::Integer))],
            complete: true,
            locally_constructed: true,
        }));
        let Conversion::Proposed { reasons, .. } =
            convert_inferred(&schema, ExportSite::ValueAtAssignment)
        else {
            panic!("field-name ambiguity must be visible");
        };
        assert!(reasons.iter().any(|reason| reason.contains("synthetic")));
    }

    #[test]
    fn conversion_bounds_total_nodes_even_when_unions_share_subgraphs() {
        fn distinct_tree(depth: usize, next: &mut usize) -> RType {
            if depth == 0 {
                *next += 1;
                return RType::new(Mode::Integer, Length::Known(*next));
            }
            RType::union(
                (0..4)
                    .map(|_| distinct_tree(depth - 1, next))
                    .collect::<Vec<_>>()
                    .into(),
            )
        }
        let mut next = 0;
        let eighty_five_nodes = distinct_tree(3, &mut next);
        let Conversion::Refused { reasons } =
            convert_inferred(&eighty_five_nodes, ExportSite::ValueAtAssignment)
        else {
            panic!("85-node tree must exceed the global declaration budget");
        };
        assert!(reasons.iter().any(|reason| reason.contains("node budget")));

        let mut shared = RType::scalar(Mode::Integer);
        for _ in 0..10 {
            shared = RType::union(vec![shared.clone(); 4].into());
        }
        let Conversion::Refused { reasons } =
            convert_inferred(&shared, ExportSite::ValueAtAssignment)
        else {
            panic!("shared DAG must be refused after bounded input work");
        };
        assert!(reasons.iter().any(|reason| reason.contains("node budget")));
    }

    #[test]
    fn successful_conversion_is_always_canonical_and_schema_scan_is_bounded() {
        let compatible =
            RType::union(vec![RType::scalar(Mode::Integer), RType::scalar(Mode::Character)].into());
        let Conversion::Exact(constraint) =
            convert_inferred(&compatible, ExportSite::ValueAtAssignment)
        else {
            panic!("small union should convert exactly");
        };
        assert!(constraint.canonical().is_ok());

        let mut narrowed = RType::union(
            vec![
                RType::new(Mode::Integer, Length::Unknown),
                RType::new(Mode::Character, Length::Unknown),
            ]
            .into(),
        );
        narrowed.length = Length::One;
        let Conversion::Proposed {
            constraint,
            reasons,
        } = convert_inferred(&narrowed, ExportSite::ValueAtAssignment)
        else {
            panic!("narrowed union must not silently discard its outer length");
        };
        assert!(constraint.canonical().is_ok());
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("union-level length"))
        );

        let impossible_null = RType::new(Mode::Null, Length::One);
        let Conversion::Refused { reasons } =
            convert_inferred(&impossible_null, ExportSite::ValueAtAssignment)
        else {
            panic!("an invalid final constraint must not be exported");
        };
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("cannot be exported"))
        );

        let schema = RType::new(Mode::List, Length::Unknown).with_columns(Arc::new(ColumnSchema {
            columns: (0..100_000)
                .map(|index| (format!("field{index}"), RType::scalar(Mode::Integer)))
                .collect(),
            complete: true,
            locally_constructed: true,
        }));
        let Conversion::Proposed {
            constraint,
            reasons,
        } = convert_inferred(&schema, ExportSite::ValueAtAssignment)
        else {
            panic!("unrepresented schema must need an explicit proposal");
        };
        assert!(constraint.canonical().is_ok());
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("inspection budget"))
        );
    }
}
