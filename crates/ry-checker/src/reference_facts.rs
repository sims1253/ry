//! Optional, deliberately narrow reference facts from the diagnostic walk.
//!
//! The inventory rejects syntax; it does not resolve names. Only a binding
//! installed by the semantic walk can supply a definition to a reference.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

use ry_core::ast::{Expr, Param, SourceFile, Stmt};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use ry_core::{RType, Span};

use crate::infer::span_of;
use crate::{Checker, Scope, whole_file_span};

/// File-local identity, valid only within one returned facts snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefinitionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceDefinitionKind {
    Assignment,
    Formal,
}

/// A declaration installed by the diagnostic walk, with original-source spans.
#[derive(Debug, Clone)]
pub struct ReferenceDefinition {
    pub id: DefinitionId,
    pub name: String,
    pub scope_span: Span,
    pub span: Span,
    pub kind: ReferenceDefinitionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceResolution {
    Resolved,
    Ambiguous,
    Unresolved,
    Unsupported,
}

/// An original-source value occurrence, never a scope-exit snapshot.
#[derive(Debug, Clone)]
pub struct ReferenceRecord {
    pub name: String,
    pub span: Span,
    pub resolution: ReferenceResolution,
    pub definition: Option<DefinitionId>,
    pub type_at_reference: Option<RType>,
    pub reason: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
pub struct ReferenceFacts {
    pub definitions: Vec<ReferenceDefinition>,
    pub references: Vec<ReferenceRecord>,
}

#[derive(Debug, Clone)]
pub(crate) struct BindingProvenance {
    definition: DefinitionId,
    owner: Span,
    type_known: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ScopeProvenance {
    owner: Span,
    after_unsafe_read: bool,
    bindings: HashMap<String, BindingProvenance>,
}

impl ScopeProvenance {
    pub(crate) fn invalidate(&mut self, name: &str) {
        self.bindings.remove(name);
    }
}

#[derive(Debug)]
struct Occurrence {
    record: ReferenceRecord,
    owner: Span,
    eligible: bool,
    observed: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ReferenceCapture {
    definitions: Vec<ReferenceDefinition>,
    declarations: HashMap<Span, DefinitionId>,
    installed: HashSet<DefinitionId>,
    occurrences: HashMap<Span, Occurrence>,
    eligible_scopes: HashSet<Span>,
}

impl ReferenceCapture {
    pub(crate) fn new(file: &SourceFile) -> Self {
        let mut capture = Self::default();
        capture.inventory_scope(
            &file.source,
            whole_file_span(&file.source),
            &[],
            &file.stmts,
            !file.parse_errors.is_empty(),
        );
        capture
    }

    fn source_backed(source: &str, span: Span) -> bool {
        span.start < span.end && source.get(span.start..span.end).is_some()
    }

    // This pass only inventories declarations/occurrences and rejects unsafe
    // scopes. It never matches a reference spelling to a declaration.
    fn inventory_scope(
        &mut self,
        source: &str,
        owner: Span,
        params: &[Param],
        stmts: &[Stmt],
        inherited_unsupported: bool,
    ) {
        let mut unsafe_scope = inherited_unsupported;
        let mut writes = HashMap::<String, usize>::new();
        let mut declarations = Vec::new();
        let mut excluded_targets = HashSet::new();
        let mut references = Vec::new();
        let mut functions = Vec::new();
        let mut defaults = Vec::new();
        for param in params {
            if let Some(key) = ordinary_spelling(&param.name) {
                *writes.entry(key.to_string()).or_default() += 1;
            } else {
                unsafe_scope = true;
            }
            // Param.span includes its default. Its name is the raw source token.
            let span = Span {
                end: param.span.start + param.name.len(),
                ..param.span
            };
            if source.get(span.start..span.end) == Some(param.name.as_str()) {
                declarations.push((param.name.clone(), span, ReferenceDefinitionKind::Formal));
            } else {
                unsafe_scope = true;
            }
            if let Some(default) = &param.default {
                defaults.push(Stmt::Expr(default.clone()));
            }
        }
        let _ = walk_stmts(
            stmts,
            Walk {
                dollar_args: false,
                assign_operands: false,
                ..Walk::ALL
            },
            |node, _| {
                match node {
                    AstNode::Stmt(Stmt::Assign { target, value, .. }) => {
                        if let Expr::Ident { name, span } = target {
                            excluded_targets.insert(*span);
                            if let Some(key) = ordinary_spelling(name) {
                                *writes.entry(key.to_string()).or_default() += 1;
                            } else {
                                unsafe_scope = true;
                            }
                            declarations.push((
                                name.clone(),
                                *span,
                                ReferenceDefinitionKind::Assignment,
                            ));
                            // The small AST drops some assignment operators. Only
                            // certify ordinary left assignment from source tokens.
                            let rhs = span_of(value);
                            let operator = source.get(span.end..rhs.start).map(str::trim);
                            if !matches!(operator, Some("<-" | "=")) || !simple_value(value) {
                                unsafe_scope = true;
                            }
                        } else {
                            unsafe_scope = true;
                        }
                    }
                    AstNode::Stmt(Stmt::FunctionDef { params, body, span })
                    | AstNode::Expr(Expr::Function { params, body, span }) => {
                        functions.push((params.clone(), body.clone(), *span));
                        return ControlFlow::<(), Descend>::Continue(Descend::Skip);
                    }
                    AstNode::Stmt(
                        Stmt::If { .. }
                        | Stmt::For { .. }
                        | Stmt::While { .. }
                        | Stmt::Return { .. },
                    ) => unsafe_scope = true,
                    AstNode::Expr(Expr::Ident { name, span }) => {
                        if ordinary_spelling(name).is_none() {
                            unsafe_scope = true;
                        }
                        if !excluded_targets.contains(span) && Self::source_backed(source, *span) {
                            references.push((name.clone(), *span));
                        }
                    }
                    AstNode::Expr(
                        Expr::Call { .. }
                        | Expr::BinOp { .. }
                        | Expr::UnaryOp { .. }
                        | Expr::Index { .. }
                        | Expr::If { .. }
                        | Expr::Unknown(_),
                    ) => unsafe_scope = true,
                    _ => {}
                }
                ControlFlow::Continue(Descend::Into)
            },
        );
        unsafe_scope |= writes.values().any(|count| *count != 1);
        if !unsafe_scope {
            self.eligible_scopes.insert(owner);
            declarations.sort_by_key(|(_, span, _)| (span.start, span.end));
            for (name, span, kind) in declarations {
                if !Self::source_backed(source, span) {
                    continue;
                }
                let id = DefinitionId(self.definitions.len() as u32);
                self.definitions.push(ReferenceDefinition {
                    id,
                    name,
                    scope_span: owner,
                    span,
                    kind,
                });
                self.declarations.insert(span, id);
            }
        }
        for (name, span) in references {
            self.occurrences.entry(span).or_insert(Occurrence {
                record: ReferenceRecord {
                    name,
                    span,
                    resolution: ReferenceResolution::Unsupported,
                    definition: None,
                    type_at_reference: None,
                    reason: Some(if unsafe_scope {
                        "unsupported_scope"
                    } else {
                        "not_observed"
                    }),
                },
                owner,
                eligible: !unsafe_scope,
                observed: false,
            });
        }
        // Defaults are lazy expressions, not the function's runtime contract.
        // Their references and any nested functions remain unsupported.
        if !defaults.is_empty() {
            self.inventory_scope(source, owner, &[], &defaults, true);
        }
        for (params, body, span) in functions {
            self.inventory_scope(source, span, &params, &body, unsafe_scope);
        }
    }

    fn finish(self) -> ReferenceFacts {
        let mut references: Vec<_> = self.occurrences.into_values().map(|o| o.record).collect();
        references.sort_by_key(|r| (r.span.start, r.span.end));
        let mut definitions: Vec<_> = self
            .definitions
            .into_iter()
            .filter(|d| self.installed.contains(&d.id))
            .collect();
        definitions.sort_by_key(|d| (d.span.start, d.span.end));
        ReferenceFacts {
            definitions,
            references,
        }
    }
}

// Canonical spelling is used ONLY to reject multiple syntactic writes and
// unsupported names. The semantic lookup and provenance retain the checker's
// raw names. Escaped names need a full R decoder and are outside this subset.
fn ordinary_spelling(name: &str) -> Option<&str> {
    let quoted = name
        .strip_prefix('`')
        .and_then(|name| name.strip_suffix('`'));
    let key = quoted.unwrap_or(name);
    if key.contains('\\')
        || matches!(key, "<-" | "=" | "->" | "{" | "(" | "function")
        || (quoted.is_none() && key.contains("::"))
        || key == "..."
        || key
            .strip_prefix("..")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
    {
        None
    } else {
        Some(key)
    }
}

fn simple_value(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Logical(..)
            | Expr::Integer(..)
            | Expr::Double(..)
            | Expr::String(..)
            | Expr::Null(..)
            | Expr::Na(..)
            | Expr::Ident { .. }
            | Expr::Function { .. }
    )
}

impl Checker {
    /// Opt into conservative reference facts from the final diagnostic walk.
    pub fn enable_reference_capture(&mut self) {
        self.capture_references = true;
    }

    /// Take this file's most recent reference snapshot. Disabled by default.
    pub fn take_reference_facts(&mut self) -> ReferenceFacts {
        self.reference_capture
            .take()
            .map(|capture| capture.finish())
            .unwrap_or_default()
    }

    pub(crate) fn start_reference_scope(&self, scope: &mut Scope, owner: Span) {
        if self.reference_capture.is_none() || self.discarding {
            return;
        }
        if let Some(provenance) = scope.reference_provenance.as_mut() {
            provenance.owner = owner;
        } else {
            scope.reference_provenance = Some(Box::new(ScopeProvenance {
                owner,
                after_unsafe_read: false,
                bindings: HashMap::new(),
            }));
        }
    }

    pub(crate) fn reference_value_known(&self, value: &Expr, scope: &Scope) -> bool {
        match value {
            Expr::Logical(..)
            | Expr::Integer(..)
            | Expr::Double(..)
            | Expr::String(..)
            | Expr::Null(..)
            | Expr::Na(..) => true,
            Expr::Ident { name, .. } => scope.reference_provenance.as_ref().is_some_and(|p| {
                p.bindings
                    .get(name)
                    .is_some_and(|b| b.owner == p.owner && b.type_known)
            }),
            _ => false,
        }
    }

    pub(crate) fn install_reference_definition(
        &mut self,
        scope: &mut Scope,
        name: &str,
        span: Span,
        type_known: bool,
    ) {
        if self.discarding {
            return;
        }
        let (Some(capture), Some(provenance)) = (
            self.reference_capture.as_mut(),
            scope.reference_provenance.as_mut(),
        ) else {
            return;
        };
        if provenance.after_unsafe_read || !capture.eligible_scopes.contains(&provenance.owner) {
            return;
        }
        let Some(&definition) = capture.declarations.get(&span) else {
            return;
        };
        let declaration = &capture.definitions[definition.0 as usize];
        if declaration.scope_span != provenance.owner || declaration.name != name {
            return;
        }
        capture.installed.insert(definition);
        provenance.bindings.insert(
            name.to_string(),
            BindingProvenance {
                definition,
                owner: provenance.owner,
                type_known,
            },
        );
    }

    pub(crate) fn finish_reference_read(&self, name: &str, scope: &mut Scope) {
        // A formal, inherited, or untracked read may force arbitrary code.
        // It can mutate this frame or install active bindings for future writes.
        // Only an ordinary value already installed in this scope is safe.
        if self.discarding {
            return;
        }
        let formal = scope.is_parameter(name);
        if let Some(provenance) = scope.reference_provenance.as_mut() {
            let ordinary_local = provenance
                .bindings
                .get(name)
                .is_some_and(|binding| binding.owner == provenance.owner);
            if formal || !ordinary_local {
                provenance.bindings.clear();
                provenance.after_unsafe_read = true;
            }
        }
    }

    pub(crate) fn observe_reference(
        &mut self,
        name: &str,
        span: Span,
        scope: &Scope,
        ty: Option<&RType>,
        unresolved: bool,
    ) {
        if self.discarding {
            return;
        }
        let (Some(capture), Some(provenance)) = (
            self.reference_capture.as_mut(),
            scope.reference_provenance.as_ref(),
        ) else {
            return;
        };
        let Some(occurrence) = capture.occurrences.get_mut(&span) else {
            return;
        };
        if !occurrence.eligible
            || occurrence.owner != provenance.owner
            || occurrence.record.name != name
        {
            return;
        }
        let (resolution, definition, type_at_reference, reason) = if provenance.after_unsafe_read {
            (
                ReferenceResolution::Unsupported,
                None,
                None,
                Some("after_unsafe_read"),
            )
        } else if scope.data_mask_unknown || scope.search_path_unknown {
            (
                ReferenceResolution::Unsupported,
                None,
                None,
                Some("unknown_environment"),
            )
        } else if let Some(ty) = ty {
            match provenance.bindings.get(name) {
                Some(binding) if binding.owner == provenance.owner => (
                    ReferenceResolution::Resolved,
                    Some(binding.definition),
                    Some(if binding.type_known {
                        ty.clone()
                    } else {
                        RType::unknown()
                    }),
                    None,
                ),
                Some(_) => (
                    ReferenceResolution::Unsupported,
                    None,
                    None,
                    Some("deferred_capture"),
                ),
                None => (
                    ReferenceResolution::Unsupported,
                    None,
                    None,
                    Some("untracked_binding"),
                ),
            }
        } else if unresolved {
            (
                ReferenceResolution::Unresolved,
                None,
                None,
                Some("unbound_name"),
            )
        } else {
            (
                ReferenceResolution::Unsupported,
                None,
                None,
                Some("untracked_lookup"),
            )
        };
        if occurrence.observed
            && (occurrence.record.resolution != resolution
                || occurrence.record.definition != definition
                || occurrence.record.type_at_reference != type_at_reference)
        {
            occurrence.record.resolution = ReferenceResolution::Ambiguous;
            occurrence.record.definition = None;
            occurrence.record.type_at_reference = None;
            occurrence.record.reason = Some("conflicting_observations");
        } else if !occurrence.observed {
            occurrence.record.resolution = resolution;
            occurrence.record.definition = definition;
            occurrence.record.type_at_reference = type_at_reference;
            occurrence.record.reason = reason;
        }
        occurrence.observed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Project;
    use ry_core::{Mode, RParser};

    fn file(source: &str) -> SourceFile {
        RParser::new().unwrap().parse("refs.R", source).unwrap()
    }

    fn facts(source: &str) -> ReferenceFacts {
        let mut checker = Checker::new("refs.R");
        checker.enable_reference_capture();
        checker.check(&file(source));
        checker.take_reference_facts()
    }

    fn named<'a>(facts: &'a ReferenceFacts, name: &str) -> Vec<&'a ReferenceRecord> {
        facts.references.iter().filter(|r| r.name == name).collect()
    }

    #[test]
    fn reference_capture_uses_the_semantic_write_and_lookup() {
        let source = "x <- 1L\ny <- x\nz <- y\n";
        let facts = facts(source);
        assert_eq!(facts.references.len(), 2);
        for reference in &facts.references {
            assert_eq!(reference.resolution, ReferenceResolution::Resolved);
            assert_eq!(
                reference.type_at_reference.as_ref().unwrap().mode,
                Mode::Integer
            );
            let definition = facts
                .definitions
                .iter()
                .find(|d| Some(d.id) == reference.definition)
                .unwrap();
            assert_eq!(definition.name, reference.name);
            assert!(definition.span.end < reference.span.start);
            assert_eq!(
                &source[definition.span.start..definition.span.end],
                definition.name
            );
        }
    }

    #[test]
    fn reference_capture_rejects_reassignment_and_nonordinary_operators() {
        for source in [
            "x <- 1L\ny <- x\nx <- 'later'",
            "x := 1L\ny <- x",
            "1L ->> x\ny <- x",
            "x <<- 1L\ny <- x",
            "x <- 1L\ny <- x + 1L",
        ] {
            let facts = facts(source);
            assert!(!facts.references.is_empty(), "{source}");
            assert!(
                facts
                    .references
                    .iter()
                    .all(|r| r.resolution == ReferenceResolution::Unsupported
                        && r.definition.is_none()
                        && r.type_at_reference.is_none()),
                "{source}: {facts:?}"
            );
        }
    }

    #[test]
    fn reference_capture_rejects_aliased_and_special_spellings() {
        for source in [
            "x <- 1L\n`x` <- 'later'\ny <- x",
            "x <- 1L\n`\\x78` <- 'later'\ny <- x",
            "base::x <- 1L\ny <- base::x",
            "f <- function(...) { x <- 1L; x }",
            "`<-` <- function(a, b) NULL\nx <- 1L\ny <- x",
        ] {
            let facts = facts(source);
            assert!(!facts.references.is_empty(), "{source}");
            assert!(
                facts
                    .references
                    .iter()
                    .all(|r| r.resolution == ReferenceResolution::Unsupported
                        && r.definition.is_none()
                        && r.type_at_reference.is_none()),
                "{source}: {facts:?}"
            );
        }
    }

    #[test]
    fn reference_capture_untracked_reads_are_permanent_effect_barriers() {
        for source in [
            "x <- 1L\ny <- missing_unlikely\nz <- x",
            "x <- 1L\ny <- sqrt\nz <- x",
            "outer <- 1L\nf <- function() { x <- 1L; y <- outer; z <- x }",
            "f <- function(p) { y <- p; x <- 1L; x }",
        ] {
            let facts = facts(source);
            let later = named(&facts, "x").last().copied().unwrap();
            assert_eq!(
                later.resolution,
                ReferenceResolution::Unsupported,
                "{source}: {facts:?}"
            );
            assert!(later.definition.is_none() && later.type_at_reference.is_none());
        }
    }

    #[test]
    fn reference_capture_formal_identity_does_not_assert_default_type() {
        let source = "x <- 'outer'\nf <- function(x = 1L) { copied <- x; copied }";
        let facts = facts(source);
        let formal_read = named(&facts, "x")[0];
        assert_eq!(formal_read.resolution, ReferenceResolution::Resolved);
        assert_eq!(formal_read.type_at_reference, Some(RType::unknown()));
        let definition = facts
            .definitions
            .iter()
            .find(|d| Some(d.id) == formal_read.definition)
            .unwrap();
        assert_eq!(definition.kind, ReferenceDefinitionKind::Formal);
        assert_eq!(&source[definition.span.start..definition.span.end], "x");
        let copy = named(&facts, "copied")[0];
        assert_eq!(copy.resolution, ReferenceResolution::Unsupported);
        assert_eq!(copy.type_at_reference, None);
    }

    #[test]
    fn reference_capture_formal_force_invalidates_earlier_bindings() {
        for source in [
            "f <- function(p) { x <- 1L; y <- p; z <- x }",
            "f <- function(p = { x <- 'changed'; 1L }) { x <- 1L; y <- p; z <- x }",
        ] {
            let facts = facts(source);
            let formal = named(&facts, "p")[0];
            assert_eq!(formal.resolution, ReferenceResolution::Resolved);
            assert_eq!(formal.type_at_reference, Some(RType::unknown()));
            let after = named(&facts, "x").last().copied().unwrap();
            assert_eq!(after.resolution, ReferenceResolution::Unsupported);
            assert!(after.definition.is_none() && after.type_at_reference.is_none());
        }
    }

    #[test]
    fn reference_capture_outer_and_forward_bindings_are_not_local_definitions() {
        let facts = facts("x <- 'outer'\nf <- function() { before <- x; x <- 1L; after <- x }");
        let reads = named(&facts, "x");
        assert_eq!(reads.len(), 2);
        assert_eq!(reads[0].resolution, ReferenceResolution::Unsupported);
        assert_eq!(reads[0].reason, Some("deferred_capture"));
        assert_eq!(reads[1].resolution, ReferenceResolution::Unsupported);
        assert!(reads[1].type_at_reference.is_none());
        let facts = self::facts("before <- later\nlater <- 1L\nmissing_name_unlikely");
        assert_eq!(
            named(&facts, "later")[0].resolution,
            ReferenceResolution::Unsupported
        );
        assert_eq!(
            named(&facts, "missing_name_unlikely")[0].resolution,
            ReferenceResolution::Unsupported
        );
        assert_eq!(
            self::facts("missing_name_unlikely").references[0].resolution,
            ReferenceResolution::Unresolved
        );
    }

    #[test]
    fn reference_capture_unsafe_wrappers_reach_nested_occurrences() {
        for source in [
            "quote(function(p) { x <- 1L; x })",
            "with(data, function(p) p)",
            "data |> f(function(p) p)",
            "if (condition) { f <- function(p) p }",
            "f <- function(p = function(q) q) p",
        ] {
            let facts = facts(source);
            assert!(!facts.references.is_empty(), "{source}");
            for reference in &facts.references {
                // The last p in the final fixture is a supported own formal.
                if source.starts_with("f <-") && reference.name == "p" {
                    continue;
                }
                assert_eq!(
                    reference.resolution,
                    ReferenceResolution::Unsupported,
                    "{source}: {facts:?}"
                );
                assert!(reference.definition.is_none() && reference.type_at_reference.is_none());
            }
        }
    }

    #[test]
    fn reference_capture_project_refreshes_even_when_diagnostics_are_cached() {
        let mut project = Project::new();
        project.add_file("refs.R".into(), file("x <- 1L\ny <- x"));
        project.check();
        project.enable_reference_capture();
        project.check();
        let first = project.take_reference_facts();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1.references.len(), 1);
        project.check();
        let second = project.take_reference_facts();
        assert_eq!(format!("{first:?}"), format!("{second:?}"));
    }

    #[test]
    fn reference_capture_does_not_change_types_or_diagnostics() {
        let file = file("f <- function(x = 1L) { y <- x; y }\nf()\ny <- unknown_name");
        let mut plain = Checker::new("refs.R");
        let (plain_diagnostics, plain_scope) = plain.check_with_scope(&file);
        assert!(plain_scope.reference_provenance.is_none());
        assert!(plain.take_reference_facts().references.is_empty());
        let mut captured = Checker::new("refs.R");
        captured.enable_reference_capture();
        let (captured_diagnostics, captured_scope) = captured.check_with_scope(&file);
        assert_eq!(plain_scope.bindings, captured_scope.bindings);
        assert_eq!(
            format!("{plain_diagnostics:?}"),
            format!("{captured_diagnostics:?}")
        );
        assert_eq!(
            std::mem::size_of::<Option<Box<ScopeProvenance>>>(),
            std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn reference_capture_conflicting_observations_fail_closed() {
        let source = file("x <- 1L\nx");
        let mut checker = Checker::new("refs.R");
        checker.enable_reference_capture();
        let (_, scope) = checker.check_with_scope(&source);
        let Stmt::Expr(Expr::Ident { span, .. }) = &source.stmts[1] else {
            unreachable!()
        };
        checker.observe_reference(
            "x",
            *span,
            &scope,
            Some(&RType::scalar(Mode::Character)),
            false,
        );
        let facts = checker.take_reference_facts();
        assert_eq!(
            facts.references[0].resolution,
            ReferenceResolution::Ambiguous
        );
        assert!(
            facts.references[0].definition.is_none()
                && facts.references[0].type_at_reference.is_none()
        );
    }
}
