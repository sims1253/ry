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
    pub(crate) fn invalidate_all(&mut self) {
        self.bindings.clear();
        self.after_unsafe_read = true;
    }

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
        let mut spellings = HashMap::<String, String>::new();
        let mut writes = HashMap::<String, usize>::new();
        let mut formals = HashSet::new();
        let mut declarations = Vec::new();
        let mut references = Vec::new();
        let mut functions = Vec::new();
        let mut defaults = Vec::new();
        for param in params {
            if let Some(key) = ordinary_spelling(&param.name) {
                if !formals.insert(key.to_string()) {
                    unsafe_scope = true;
                }
                spellings.insert(key.to_string(), param.name.clone());
            } else {
                unsafe_scope = true;
            }
            // Param.span includes its default. Its name is the raw source token.
            let span = Span {
                end: param.span.start + param.name.len(),
                ..param.span
            };
            if source.get(span.start..span.end) == Some(param.name.as_str()) {
                declarations.push((
                    param.name.clone(),
                    span,
                    ReferenceDefinitionKind::Formal,
                    true,
                ));
            } else {
                unsafe_scope = true;
            }
            if let Some(default) = &param.default {
                defaults.push(Stmt::Expr(default.clone()));
            }
        }
        let mut supported_prefix = true;
        for statement in stmts {
            let mut unsupported_statement = false;
            let mut statement_declarations = Vec::new();
            let mut statement_references = Vec::new();
            let mut excluded_targets = HashSet::new();
            let _ = walk_stmts(
                std::slice::from_ref(statement),
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
                                    // Equivalent spellings and writes to formals
                                    // remain outside the ordinary-local contract.
                                    if formals.contains(key)
                                        || spellings.get(key).is_some_and(|raw| raw != name)
                                    {
                                        unsafe_scope = true;
                                    }
                                    spellings.insert(key.to_string(), name.clone());
                                    let count = writes.entry(key.to_string()).or_default();
                                    *count += 1;
                                    if *count > 1 && matches!(value, Expr::Function { .. }) {
                                        unsupported_statement = true;
                                    }
                                } else {
                                    unsafe_scope = true;
                                }
                                statement_declarations.push((
                                    name.clone(),
                                    *span,
                                    ReferenceDefinitionKind::Assignment,
                                ));
                                // Only certify ordinary left assignment from
                                // source tokens; the AST drops some operators.
                                let rhs = span_of(value);
                                let operator = source.get(span.end..rhs.start).map(str::trim);
                                if !matches!(operator, Some("<-" | "=")) || !simple_value(value) {
                                    unsupported_statement = true;
                                }
                            } else {
                                unsupported_statement = true;
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
                        ) => unsupported_statement = true,
                        AstNode::Expr(Expr::Ident { name, span }) => {
                            if ordinary_spelling(name).is_none() {
                                unsupported_statement = true;
                            }
                            if !excluded_targets.contains(span)
                                && Self::source_backed(source, *span)
                            {
                                statement_references.push((name.clone(), *span));
                            }
                        }
                        AstNode::Expr(
                            Expr::Call { .. }
                            | Expr::BinOp { .. }
                            | Expr::UnaryOp { .. }
                            | Expr::Index { .. }
                            | Expr::If { .. }
                            | Expr::Unknown(_)
                            | Expr::Missing(_),
                        ) => unsupported_statement = true,
                        _ => {}
                    }
                    ControlFlow::Continue(Descend::Into)
                },
            );
            // Reject the entire opaque statement, including earlier children.
            // This boundary makes no argument/subexpression ordering claims.
            supported_prefix &= !unsupported_statement;
            declarations.extend(
                statement_declarations
                    .into_iter()
                    .map(|(name, span, kind)| (name, span, kind, supported_prefix)),
            );
            references.extend(
                statement_references
                    .into_iter()
                    .map(|(name, span)| (name, span, supported_prefix)),
            );
        }
        if !unsafe_scope {
            self.eligible_scopes.insert(owner);
            declarations.sort_by_key(|(_, span, _, _)| (span.start, span.end));
            for (name, span, kind, eligible) in declarations {
                if !eligible || !Self::source_backed(source, span) {
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
        for (name, span, eligible) in references {
            let eligible = eligible && !unsafe_scope;
            self.occurrences.entry(span).or_insert(Occurrence {
                record: ReferenceRecord {
                    name,
                    span,
                    resolution: ReferenceResolution::Unsupported,
                    definition: None,
                    type_at_reference: None,
                    reason: Some(if eligible {
                        "not_observed"
                    } else {
                        "unsupported_scope"
                    }),
                },
                owner,
                eligible,
                observed: false,
            });
        }
        // Defaults are lazy expressions, not the function's runtime contract.
        if !defaults.is_empty() {
            self.inventory_scope(source, owner, &[], &defaults, true);
        }
        for (params, body, span) in functions {
            // A body can run after the enclosing prefix has ended. Its textual
            // definition position cannot make later enclosing effects safe.
            self.inventory_scope(
                source,
                span,
                &params,
                &body,
                unsafe_scope || !supported_prefix,
            );
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

// Canonical spelling is used ONLY to reject equivalent mixed spellings and
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
    fn comments_do_not_end_a_supported_reference_prefix() {
        let source = "# header\nx <- 1L\n# before read\ny <- x\n# before function\nf <- function() {\n# local\nz <- 2L\nz\n# tail\n}\n# end\n";
        let captured = facts(source);
        assert_eq!(captured.references.len(), 2);
        for reference in &captured.references {
            assert_eq!(reference.resolution, ReferenceResolution::Resolved);
            assert_eq!(
                reference.type_at_reference.as_ref().unwrap().mode,
                Mode::Integer
            );
            assert_eq!(
                &source[reference.span.start..reference.span.end],
                reference.name
            );
        }
        let opaque = facts("x <- 1L\n# harmless\nmutate()\nx");
        assert_eq!(
            named(&opaque, "x")[0].resolution,
            ReferenceResolution::Unsupported
        );
    }

    #[test]
    fn fixed_reference_panel_preserves_counts_and_inference() {
        let panel: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/corpus/reference-prefix-panel.json"
        ))
        .unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut sources: Vec<_> = panel["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| {
                (
                    case["name"].as_str().unwrap().to_string(),
                    case["source"].as_str().unwrap().to_string(),
                    case["references"].as_u64().unwrap() as usize,
                    case["resolved"].as_u64().unwrap() as usize,
                )
            })
            .collect();
        for case in panel["files"].as_array().unwrap() {
            let path = case["path"].as_str().unwrap();
            sources.push((
                path.to_string(),
                std::fs::read_to_string(root.join(path)).unwrap(),
                case["references"].as_u64().unwrap() as usize,
                case["resolved"].as_u64().unwrap() as usize,
            ));
        }
        for (name, source, references, resolved) in sources {
            let file = file(&source);
            let mut plain = Checker::new("refs.R");
            let (plain_diagnostics, plain_scope) = plain.check_with_scope(&file);
            let mut captured = Checker::new("refs.R");
            captured.enable_reference_capture();
            let (captured_diagnostics, captured_scope) = captured.check_with_scope(&file);
            assert_eq!(plain_scope.bindings, captured_scope.bindings, "{name}");
            assert_eq!(
                format!("{plain_diagnostics:?}"),
                format!("{captured_diagnostics:?}"),
                "{name}"
            );
            let facts = captured.take_reference_facts();
            assert_eq!(facts.references.len(), references, "{name}");
            assert_eq!(
                facts
                    .references
                    .iter()
                    .filter(|read| read.resolution == ReferenceResolution::Resolved)
                    .count(),
                resolved,
                "{name}"
            );
            captured.check(&file);
            assert_eq!(
                format!("{facts:?}"),
                format!("{:?}", captured.take_reference_facts()),
                "{name}"
            );
        }
    }

    #[test]
    fn ordered_reassignments_keep_each_read_at_its_installed_definition() {
        let source = "x <- 1L\nx\ny <- x\nx <- x\nx\nx <- 'later'\nx\ny\n";
        let facts = facts(source);
        let reads = named(&facts, "x");
        assert_eq!(reads.len(), 5);
        assert!(
            reads
                .iter()
                .all(|read| read.resolution == ReferenceResolution::Resolved)
        );
        assert_eq!(reads[0].definition, reads[1].definition);
        assert_eq!(reads[1].definition, reads[2].definition);
        assert_ne!(reads[2].definition, reads[3].definition);
        assert_ne!(reads[3].definition, reads[4].definition);
        for read in &reads[..4] {
            assert_eq!(read.type_at_reference.as_ref().unwrap().mode, Mode::Integer);
        }
        assert_eq!(
            reads[4].type_at_reference.as_ref().unwrap().mode,
            Mode::Character
        );
        let copied = named(&facts, "y")[0];
        assert_eq!(copied.resolution, ReferenceResolution::Resolved);
        assert_ne!(copied.definition, reads[0].definition);
        assert_eq!(
            copied.type_at_reference.as_ref().unwrap().mode,
            Mode::Integer
        );
    }

    #[test]
    fn opaque_statements_preserve_only_the_proven_prefix() {
        for opaque in [
            "mutate()",
            "base::identity(1L)",
            "x + 1L",
            "if (flag) mutate()",
            "z <- { x; mutate() }",
        ] {
            let source = format!("x <- 1L\nx\n{opaque}\nx <- 2L\nx\n");
            let facts = facts(&source);
            let reads = named(&facts, "x");
            assert_eq!(
                reads[0].resolution,
                ReferenceResolution::Resolved,
                "{source}: {facts:?}"
            );
            assert_eq!(
                reads[0].type_at_reference.as_ref().unwrap().mode,
                Mode::Integer
            );
            for read in &reads[1..] {
                assert_eq!(
                    read.resolution,
                    ReferenceResolution::Unsupported,
                    "{source}: {facts:?}"
                );
                assert!(read.definition.is_none() && read.type_at_reference.is_none());
            }
            assert_eq!(facts.definitions.len(), 1, "no later definition: {facts:?}");
        }
        let facts = facts("mutate()\nx <- 1L\nx\n");
        assert_eq!(
            named(&facts, "x")[0].resolution,
            ReferenceResolution::Unsupported
        );
    }

    #[test]
    fn prefix_changes_do_not_rewrite_an_earlier_fact() {
        let original = facts("x <- 1L\ny <- x\n");
        for suffix in [
            "mutate()\ny\n",
            "base::identity(x)\ny\n",
            "if (flag) mutate()\ny\n",
        ] {
            let extended = facts(&format!("x <- 1L\ny <- x\n{suffix}"));
            let before = &original.references[0];
            let after = &extended.references[0];
            assert_eq!(before.span, after.span);
            assert_eq!(before.definition, after.definition);
            assert_eq!(before.type_at_reference, after.type_at_reference);
            assert_eq!(after.resolution, ReferenceResolution::Resolved);
        }
    }

    #[test]
    fn reassignment_does_not_recover_after_formal_or_unknown_reads() {
        for source in [
            "f <- function(p) { x <- 1L; x; p; x <- 2L; x }",
            "x <- 1L; x; unknown_name; x <- 2L; x",
        ] {
            let facts = facts(source);
            let reads = named(&facts, "x");
            assert_eq!(reads[0].resolution, ReferenceResolution::Resolved);
            assert_eq!(reads[1].resolution, ReferenceResolution::Unsupported);
            assert!(reads[1].definition.is_none() && reads[1].type_at_reference.is_none());
        }
        let facts = facts("f <- function(p) { x <- 1L; x; p; x }");
        assert_eq!(
            named(&facts, "p")[0].resolution,
            ReferenceResolution::Resolved
        );
    }

    #[test]
    fn deferred_functions_and_formal_writes_do_not_gain_prefix_evidence() {
        for source in [
            "f <- function() { x <- 1L; x }; mutate()",
            "f <- function() { x <- 1L; x }; f <- function() { 2L }",
            "f <- function(p) { p <- 1L; p }",
            "f <- function(p) { p; p <- 1L; p }",
            "f <- function() { x <- 1L; x }; if (flag) mutate()",
            "f <- function() { x <- 1L; x }; base::identity(1L)",
        ] {
            let facts = facts(source);
            assert!(
                facts
                    .references
                    .iter()
                    .all(|read| read.resolution == ReferenceResolution::Unsupported),
                "{source}: {facts:?}"
            );
        }
    }

    #[test]
    fn ordered_shadowing_keeps_unicode_sites_separate() {
        let source = "`空 白` <- 1L\n`空 白`\nf <- function() { `空 白` <- 'local'; `空 白`; `空 白` <- FALSE; `空 白` }\n`空 白`\n";
        let facts = facts(source);
        let reads = named(&facts, "`空 白`");
        assert_eq!(reads.len(), 4);
        assert_eq!(reads[0].definition, reads[3].definition);
        assert_ne!(reads[0].definition, reads[1].definition);
        assert_ne!(reads[1].definition, reads[2].definition);
        for (read, mode) in
            reads
                .iter()
                .zip([Mode::Integer, Mode::Character, Mode::Logical, Mode::Integer])
        {
            assert_eq!(read.resolution, ReferenceResolution::Resolved);
            assert_eq!(read.type_at_reference.as_ref().unwrap().mode, mode);
            assert_eq!(&source[read.span.start..read.span.end], "`空 白`");
        }
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
    fn reference_capture_rejects_nonordinary_operators() {
        for source in [
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
