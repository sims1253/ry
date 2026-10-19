//! Call-head selection and the proof needed to use collected metadata.
//!
//! Select before evaluating arguments. Namespace provenance, lexical snapshots,
//! arity eligibility, declaration identity, and unforced-argument evidence must
//! all describe that same call head, even when an actual rebinds its name.

use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr, walk_stmts};
use std::ops::ControlFlow;

pub(crate) enum ResolvedCallee<'a> {
    User(&'a UserFn),
    Stub(&'a FunctionSig),
    Unknown,
}

enum CallTarget {
    User(UserFn),
    Stub,
    Lexical,
    Unknown,
}

pub(super) struct CallHead {
    target: CallTarget,
    // The selected stub's contract, or evaluation metadata inherited by a
    // project wrapper or lexical callable. Inherited metadata does not select
    // the stub for arity or returns.
    metadata: Option<FunctionSig>,
    pub(super) inherited_s3_metadata: bool,
    pub(super) declaration_function: Option<UserFn>,
    pub(super) unforced_args: Vec<bool>,
    pub(super) lexical_binding: Option<RType>,
    pub(super) function_evidence: CallHeadFunctionEvidence,
    pub(super) generic_receiver: Option<usize>,
    check_project_arguments: bool,
    contract_applies: bool,
}

impl CallHead {
    pub(super) fn user_function(&self) -> Option<&UserFn> {
        match &self.target {
            CallTarget::User(function) => Some(function),
            _ => None,
        }
    }

    /// The selected stub's contract, or inherited evaluation metadata for a
    /// project/lexical callable. Inherited metadata supplies neither arity nor
    /// ordinary return inference; those use `callee` and `user_function`.
    pub(super) fn signature(&self) -> Option<&FunctionSig> {
        self.metadata.as_ref()
    }

    /// The selected definition's call contract, under the proof that lets
    /// RY090/RY091 read its formals plus proof that its body is the one the
    /// call reaches (definitions that agree on formals may differ in bodies).
    pub(super) fn contract(&self) -> Option<(&UserFn, CallContract)> {
        let function = self.user_function().filter(|_| self.contract_applies)?;
        Some((function, function.contract?))
    }

    pub(super) fn callee(&self) -> ResolvedCallee<'_> {
        match &self.target {
            CallTarget::User(function) => ResolvedCallee::User(function),
            CallTarget::Stub => ResolvedCallee::Stub(
                self.metadata
                    .as_ref()
                    .expect("stub call head retains its contract"),
            ),
            CallTarget::Lexical | CallTarget::Unknown => ResolvedCallee::Unknown,
        }
    }
}

impl Checker {
    pub(super) fn resolve_call_head(
        &self,
        name: &str,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &Scope,
        call_span: Span,
    ) -> CallHead {
        // An alias to pkg::f selects that namespace even though its raw head is
        // bare. Lexical binding/evidence below describes the raw call site's
        // environment, and cannot replace a qualified selection.
        let callee = Callee::parse(semantic_name);
        let qualified = callee.is_qualified();
        let lexical_callable = !qualified && scope.is_lexical_function(lookup_name);
        let function = (!lexical_callable)
            .then(|| self.fn_table.functions.select(callee))
            .flatten()
            .cloned();
        let inherited = (!qualified)
            .then(|| self.resolve_user_s3_inherited_sig(lookup_name))
            .flatten();
        let inherited_s3_metadata = inherited.is_some();
        let signature = self.resolve_typeshed_sig(semantic_name).or(inherited);
        let declaration_function =
            if qualified || self.discarding || !self.declarations.has_contracts() {
                None
            } else {
                self.declaration_function(lookup_name, function.as_ref(), scope, call_span)
            };
        // R selects the function before forcing its arguments. A write among
        // the actuals changes later calls, not this call's arity or return.
        let check_project_arguments = !qualified
            && self.fn_table.functions.has_single_contract(lookup_name)
            && (scope.get(lookup_name).is_none()
                || function.as_ref().is_some_and(|function| {
                    function.source_path == self.path
                        && scope.lexical_definition(lookup_name) == Some(function.definition_span)
                }));
        // A contract also needs the body to be the one this call reaches: no
        // other literal binds the name, an eager top-level call follows the
        // definition, an enclosing frame binds the name only to this exact
        // unreplaced literal, no enclosing function's code (body or forced
        // default, before or after this call) may bind it, and no quoted
        // local spelling (`` `f` ``) masks it.
        let semantic = semantic_argument_name(lookup_name);
        let contract_applies = check_project_arguments
            && self.fn_table.functions.has_single_definition(lookup_name)
            && function.as_ref().is_some_and(|function| {
                (!self.enclosing_formals.is_empty()
                    || function.source_path != self.path
                    || function.definition_span.start < call_span.start)
                    && scope
                        .outward_function_definition(lookup_name)
                        .is_none_or(|outward| {
                            outward.is_some_and(|span| {
                                function.source_path == self.path
                                    && span == function.definition_span
                                    && !self.fn_table.was_rebound_after_capture(&self.path, span)
                            })
                        })
            })
            && !self
                .enclosing_formals
                .iter()
                .any(|frame| frame.frame_writes.may_write(semantic))
            && {
                [
                    semantic.to_string(),
                    format!("`{semantic}`"),
                    format!("'{semantic}'"),
                    format!("\"{semantic}\""),
                ]
                .iter()
                .filter(|spelling| spelling.as_str() != lookup_name)
                .all(|spelling| {
                    scope.get(spelling).is_none() && !scope.is_lexical_function(spelling)
                })
            };
        let generic_receiver =
            self.user_generic_receiver(name, lookup_name, function.as_ref(), args, scope);
        let unforced_args = self.unforced_arguments(name, semantic_name, args, scope);
        let lexical_binding = (!qualified)
            .then(|| scope.get(lookup_name).cloned())
            .flatten();
        let function_evidence = self.call_head_function_evidence(name, scope);
        let (target, metadata) = match (function, lexical_callable, signature) {
            (Some(function), _, signature) => (CallTarget::User(function), signature),
            (None, true, signature) => (CallTarget::Lexical, signature),
            (None, false, Some(signature)) => (CallTarget::Stub, Some(signature)),
            (None, false, None) => (CallTarget::Unknown, None),
        };
        CallHead {
            target,
            metadata,
            inherited_s3_metadata,
            declaration_function,
            unforced_args,
            lexical_binding,
            function_evidence,
            generic_receiver,
            check_project_arguments,
            contract_applies,
        }
    }

    /// The literal a call head currently names: the exact lexical
    /// definition, else the single collected global literal that no write
    /// may have rebound.
    pub(super) fn declaration_helper(&self, name: &str, scope: &Scope) -> Option<&UserFn> {
        if let Some(span) = scope.lexical_definition(name) {
            return self
                .fn_table
                .functions
                .get(&FunctionId::new(&self.path, span));
        }
        if scope.get(name).is_some() {
            return None;
        }
        let definitions = self.fn_table.capture_literal_bindings.get(name)?;
        let mut definitions = definitions.iter();
        let key = definitions.next()?;
        if definitions.next().is_some() {
            return None;
        }
        if self.fn_table.rebound_after_capture.contains(key) {
            return None;
        }
        let (path, start, end) = key;
        self.fn_table.functions.get(&FunctionId::new(
            path,
            Span {
                start: *start,
                end: *end,
                ..Span::default()
            },
        ))
    }

    /// Per actual, whether it binds a formal the called definition can never
    /// force. Only a direct, unaliased call to one exact literal qualifies,
    /// and forwarded `...` leaves the positional binding unknown.
    fn unforced_arguments(
        &self,
        name: &str,
        semantic_name: &str,
        args: &[Arg],
        scope: &Scope,
    ) -> Vec<bool> {
        if name != semantic_name
            || name.contains("::")
            || args
                .iter()
                .any(|arg| matches!(&arg.value, Expr::Ident { name, .. } if name == "..."))
        {
            return Vec::new();
        }
        let Some(function) = self.declaration_helper(name, scope).filter(|function| {
            function.params.iter().any(|parameter| parameter.unforced)
                && !self
                    .fn_table
                    .was_rebound_after_capture(&function.source_path, function.definition_span)
                && self.runs_only_stub_code(function)
        }) else {
            return Vec::new();
        };
        let names: Vec<&str> = function
            .params
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        match_arguments(&names, args)
            .param_for_arg
            .iter()
            .map(|formal| formal.is_some_and(|formal| function.params[formal].unforced))
            .collect()
    }

    /// Whether evaluating `function` runs no project code, which could reach
    /// its caller's frame (`get("x", envir = parent.frame())`). Every call
    /// head must name a typeshed function the project does not bind, no
    /// project function may be passed by value or by name, and no call or
    /// operator may dispatch while the project defines methods.
    fn runs_only_stub_code(&self, function: &UserFn) -> bool {
        let project_function = |name: &str| self.fn_table.functions.binds(name);
        let mut dispatches = false;
        let mut visit = |node: AstNode<'_>, _| {
            let closed = match node {
                AstNode::Expr(Expr::Call { func, .. }) => {
                    dispatches = true;
                    match func.as_ref() {
                        Expr::Ident { name, .. } => {
                            let name = semantic_argument_name(name);
                            !project_function(name)
                                && !project_function(&format!("{name}<-"))
                                && !self.fn_table.known_vars.contains(name)
                                && self.resolve_typeshed_sig(name).is_some()
                        }
                        _ => false,
                    }
                }
                AstNode::Expr(Expr::Ident { name, .. }) => {
                    !project_function(semantic_argument_name(name))
                }
                AstNode::Expr(Expr::String(text, _)) => !project_function(text),
                AstNode::Expr(Expr::BinOp { .. } | Expr::UnaryOp { .. } | Expr::Index { .. }) => {
                    dispatches = true;
                    true
                }
                _ => true,
            };
            if closed {
                ControlFlow::Continue(Descend::Into)
            } else {
                ControlFlow::Break(())
            }
        };
        let closed = function
            .source_params
            .iter()
            .filter_map(|parameter| parameter.default.as_ref())
            .all(|default| walk_expr(default, Walk::ALL, &mut visit).is_continue())
            && walk_stmts(&function.body, Walk::ALL, &mut visit).is_continue();
        closed && !(dispatches && self.project_defines_methods())
    }

    /// Whether any project function is named like a method of a typeshed or
    /// group generic (`print.foo`, `Ops.foo`) or registered by `setMethod`.
    fn project_defines_methods(&self) -> bool {
        self.fn_table.functions.names().any(|name| {
            let name = semantic_argument_name(name);
            name.starts_with("__s4__")
                || name.match_indices('.').any(|(end, _)| {
                    end > 0
                        && (crate::semantic_lists::is_group_generic(&name[..end])
                            || self.resolve_typeshed_sig(&name[..end]).is_some())
                })
        })
    }

    /// The definition whose adopted contract a call checks: the exact
    /// current or nearest enclosing literal, else the flat entry when its
    /// source identity agrees with the current binding.
    fn declaration_function(
        &self,
        lookup_name: &str,
        user_function: Option<&UserFn>,
        scope: &Scope,
        call_span: Span,
    ) -> Option<UserFn> {
        // R's function-position lookup skips a proven nonfunction local and
        // searches enclosing environments. A current callable/unknown local
        // instead masks those environments. Keep the nearest exact literal
        // identity when one exists, regardless of which same-spelled entry
        // survived in the flat inference table.
        let local_binding = scope.get(lookup_name);
        let local_is_nonfunction = local_binding
            .is_some_and(|ty| !matches!(ty.mode, Mode::Function | Mode::Opaque | Mode::Union));
        let outward = scope.outward_function_definition(lookup_name);
        let possible_default_shadow = self.enclosing_formals.iter().any(|frame| {
            frame
                .possible_default_writes
                .may_write(semantic_argument_name(lookup_name))
        });
        let local_definition = scope.lexical_definition(lookup_name);
        // Function walks start with a copy of the captured scope. An exact
        // literal that is identical to the nearest enclosing frame is still
        // an outward binding, not a new local one: R's closure keeps the
        // environment live after the literal is created.
        let from_outward = local_is_nonfunction
            || (local_binding.is_none() && local_definition.is_none())
            || (local_definition.is_some() && local_definition == outward.flatten());
        let exact_definition = if from_outward {
            outward.flatten()
        } else {
            local_definition
        };
        let exact = exact_definition
            .filter(|definition| {
                !possible_default_shadow
                    && (!from_outward
                        || !self
                            .fn_table
                            .was_rebound_after_capture(&self.path, *definition))
                    && self
                        .declarations
                        .target(&self.path, *definition)
                        .is_some_and(|decision| decision.signature.is_some())
            })
            .and_then(|definition| {
                self.fn_table
                    .functions
                    .get(&FunctionId::new(&self.path, definition))
            });
        if exact.is_some() {
            return exact.cloned();
        }
        // A local binding may be an earlier literal, a later rebind, or a
        // branch join. The flat table is last-definition-wins, so its
        // declaration is safe only when the current binding is absent (a
        // cross-file/deferred lookup) or has the identical source span.
        user_function
            .filter(|function| {
                if possible_default_shadow || local_is_nonfunction || outward.is_some() {
                    false
                } else if local_binding.is_some() {
                    function.source_path == self.path
                        && exact_definition == Some(function.definition_span)
                } else {
                    // A deferred call inside a function may resolve a later
                    // definition. An eager top-level call cannot use a function
                    // defined later in this same file; the table's future entry
                    // is not evidence about the current binding.
                    !self.enclosing_formals.is_empty()
                        || function.source_path != self.path
                        || function.definition_span.start < call_span.start
                }
            })
            .cloned()
    }

    /// The argument-validation stage: matching is validated only for
    /// signatures whose origin is known. A user definition shadows a
    /// same-named stub. A lexical function binding (a function literal
    /// defined inside an enclosing function body) shadows both the flat
    /// project table and the typeshed/base signature, so neither is
    /// consulted for it — checking `inherits("x")` against
    /// `base::inherits` is a lookup-order bug, not a missing argument.
    pub(super) fn check_call_arguments(
        &mut self,
        original_name: &str,
        lookup_name: &str,
        head: &CallHead,
        arg_types: &[RType],
        args: &[Arg],
        span: Span,
    ) {
        // RY111's identical-name shadow check reads the same resolved
        // formals (user table or stub) the arity checks below consult, and
        // its own gates decide.
        self.check_constant_shadowed_arguments(lookup_name, head.callee(), args);
        match head.callee() {
            ResolvedCallee::User(user_function) if head.check_project_arguments => {
                self.check_user_call_arguments(lookup_name, user_function, args, span);
                self.check_contract_demand(lookup_name, head, args, arg_types);
            }
            ResolvedCallee::Stub(signature) => {
                self.check_typeshed_call_arguments(lookup_name, signature, args, arg_types, span);
            }
            _ => {}
        }
        self.check_declaration_call(
            original_name,
            lookup_name,
            head.declaration_function.as_ref(),
            args,
            arg_types,
        );
    }

    /// A bare call skips non-functions while searching outward. An open
    /// package search path or data mask can still supply the function.
    /// Eager calls use source order. Deferred bodies retain enclosing-frame
    /// evidence and the project inventory, since they may run later.
    fn call_head_function_evidence(&self, name: &str, scope: &Scope) -> CallHeadFunctionEvidence {
        if scope.search_path_unknown
            || scope.data_mask_unknown
            || scope.has_possible_outward_function(name)
        {
            return CallHeadFunctionEvidence::Uncertain;
        }
        // An eager top-level call sees the binding installed so far. The
        // project inventory also contains later or overwritten functions.
        if !self.discarding && self.enclosing_formals.is_empty() {
            return if self.has_external_function(name) {
                CallHeadFunctionEvidence::Suppress
            } else {
                CallHeadFunctionEvidence::Diagnose
            };
        }
        if !self.has_function_anywhere(name) {
            return CallHeadFunctionEvidence::Diagnose;
        }
        if !scope.is_default_parameter(name) && self.fn_table.callable_vars.contains(name) {
            return CallHeadFunctionEvidence::Diagnose;
        }
        CallHeadFunctionEvidence::Suppress
    }

    /// The actual a project `UseMethod` generic dispatches on: the one bound
    /// to its first formal, when the call reaches the table's generic (no
    /// local definition shadows it) and that binding is determinable.
    fn user_generic_receiver(
        &self,
        name: &str,
        lookup_name: &str,
        function: Option<&UserFn>,
        args: &[Arg],
        scope: &Scope,
    ) -> Option<usize> {
        let generic = function?;
        if name.contains("::")
            || scope.is_parameter(lookup_name)
            || !self.is_user_generic(lookup_name)
        {
            return None;
        }
        if scope
            .get(lookup_name)
            .is_some_and(|ty| matches!(ty.mode, Mode::Function | Mode::Opaque | Mode::Union))
            && (generic.source_path != self.path
                || scope.lexical_definition(lookup_name) != Some(generic.definition_span))
        {
            return None;
        }
        // Forwarded dots hide which actual reaches the first formal.
        if args
            .iter()
            .any(|arg| matches!(&arg.value, Expr::Ident { name, .. } if name == "..."))
        {
            return None;
        }
        let formals: Vec<&str> = generic
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect();
        let matched = match_arguments(&formals, args);
        if formals.first() == Some(&"...") {
            matched.param_for_arg.iter().position(Option::is_none)
        } else {
            matched.arg_for_param(0)
        }
    }
}

/// Whether an outward function can satisfy a bare call head at selection time.
pub(super) enum CallHeadFunctionEvidence {
    Suppress,
    Uncertain,
    Diagnose,
}
