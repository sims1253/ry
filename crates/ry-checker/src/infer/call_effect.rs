//! What one call may do to the bindings its caller observes. This is the only
//! place scalar facts (docs/scalar-guards.md) and adopted declaration
//! identities (docs/declarations.md) learn about a call's frame effect.

use super::call::callee_name;
use super::*;
use crate::semantic_lists::{
    LITERAL_MESSAGE_CALLS, SCALAR_FACT_SAFE_CALLS, bare_name, frame_effect_dispatch_closure,
};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use std::ops::ControlFlow;

/// What one call may do to bindings its caller observes, from weakest to
/// strongest: `None < RunsCallerCode < Writes < RebindsAny < Unknown`.
///
/// Polarity: a kept scalar fact only suppresses RY032, so trusting a call
/// too much there costs a false negative. A kept declaration identity
/// enables RY114, so declarations never keep an identity across a call
/// that the declaration checks dropped it for before this classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallEffect {
    /// An exactly identified user function with literal actuals and no
    /// outward writes.
    None,
    /// An audited base call: it runs no project code of its own, but may
    /// force a caller's promise or signal a condition to a caller's calling
    /// handler. That code reaches this frame's own bindings only through
    /// frame reflection (not modelled, so scalar facts survive), but can
    /// rebind any name this frame resolves outward (identities do not).
    RunsCallerCode,
    /// An exactly identified user function with literal actuals whose body
    /// replaces these names through plain `<<-`. When `opaque`, the body may
    /// also do anything else, as `Unknown`; the named writes still apply.
    Writes(crate::collect::WriteSet),
    /// An exported external `pkg::fn` or base `I()`: it may assign ordinary
    /// values, but is trusted not to install active or delayed bindings.
    RebindsAny,
    /// Everything else.
    Unknown,
}

impl CallEffect {
    /// Whether current scalar facts survive the call.
    pub(crate) fn keeps_scalar_facts(&self) -> bool {
        matches!(self, CallEffect::None | CallEffect::RunsCallerCode)
    }
}

impl Checker {
    /// Classify a call before its arguments run (R fixes the head first).
    /// Pure in the AST, the scope's bindings, and the project tables: it
    /// reads neither `discarding` nor declaration state, so every pass and
    /// configuration gets the same answer.
    pub(crate) fn call_effect(&self, func: &Expr, args: &[Arg], scope: &Scope) -> CallEffect {
        let Some(name) = callee_name(func) else {
            return match func {
                Expr::Function { params, body, .. } => user_function_effect(args, || {
                    crate::collect::function_outward_writes(params, body)
                }),
                _ => CallEffect::Unknown,
            };
        };
        // An exactly identified project definition is what runs, whatever
        // its name spells (`"base::length"(...)`, `"pkg::f"(...)`).
        if let Some(function) = self.declaration_helper(semantic_argument_name(&name), scope) {
            return user_function_effect(args, || function.outward_writes().clone());
        }
        if self.audited_base_call(&name, args, scope) {
            CallEffect::RunsCallerCode
        } else if self.trusted_external_call(&name, args, scope) {
            CallEffect::RebindsAny
        } else {
            CallEffect::Unknown
        }
    }

    /// The only writer of call-driven invalidation for both fact families.
    pub(crate) fn apply_call_effect(&self, effect: &CallEffect, scope: &mut Scope) {
        // A frame that already refuses facts has none left to drop.
        if !scope.uncertainty.dynamic_bindings {
            match effect {
                CallEffect::Unknown => scope.invalidate_scalar_assertions(),
                CallEffect::Writes(writes) if writes.opaque => {
                    scope.invalidate_scalar_assertions();
                }
                CallEffect::RebindsAny => scope.clear_scalar_assertions(),
                CallEffect::Writes(writes) => scope.retain_scalar_assertions(|name| {
                    !writes.names.contains(semantic_argument_name(name))
                }),
                CallEffect::None | CallEffect::RunsCallerCode => {}
            }
        }
        // A condition alias dies with any binding the call may rebind.
        match effect {
            CallEffect::None | CallEffect::RunsCallerCode => {}
            CallEffect::Writes(writes) if !writes.opaque => {
                scope.retain_condition_aliases(|name| !writes.names.contains(name));
            }
            _ => scope.clear_condition_aliases(),
        }
        match effect {
            CallEffect::None => {}
            CallEffect::Writes(writes) => {
                if writes.opaque {
                    self.invalidate_declaration_identities(scope);
                }
                // Even beside unknown effects, the named targets lose their
                // values; the declaration checks always replaced them.
                if !self.discarding && self.declarations.has_contracts() {
                    for name in &writes.names {
                        scope.insert(name.clone(), RType::unknown());
                    }
                }
            }
            _ => self.invalidate_declaration_identities(scope),
        }
    }

    /// A short-circuit RHS runs in this frame on a cloned scope. Every
    /// declaration identity its effects dropped is gone here too.
    pub(crate) fn merge_condition_identities(&self, scope: &mut Scope, evaluated: &Scope) {
        if self.discarding || !self.declarations.has_contracts() {
            return;
        }
        // A wipe replaces every local definition (first half) and binds every
        // adopted name to unknown; the second half catches adopted functions
        // with no binding in this scope, such as one from another file.
        let lost: Vec<String> = scope
            .lexical_definitions
            .iter()
            .filter(|(name, span)| evaluated.lexical_definitions.get(*name) != Some(*span))
            .map(|(name, _)| name.clone())
            .chain(
                self.adopted_function_names()
                    .filter(|name| evaluated.get(name).is_some() && scope.get(name).is_none()),
            )
            .collect();
        for name in lost {
            scope.insert(name, RType::unknown());
        }
    }

    /// Whether `call` is an audited base call (G1, not `message`) with no
    /// project definition behind its head: the narrower rule that keeps
    /// related-location evidence (#672), which inert user helpers do not.
    pub(crate) fn call_proves_no_rebind(&self, func: &Expr, args: &[Arg], scope: &Scope) -> bool {
        callee_name(func).is_some_and(|name| {
            !LITERAL_MESSAGE_CALLS.contains(&bare_name(&name))
                && self.call_effect(func, args, scope) == CallEffect::RunsCallerCode
        })
    }

    /// Whether any call `inert` rejects, or a replacement assignment, in
    /// `body` (outside nested function bodies) may replace a binding.
    pub(crate) fn loop_body_may_replace_bindings(
        &self,
        body: &[Stmt],
        inert: impl Fn(&Expr, &[Arg]) -> bool,
    ) -> bool {
        let policy = Walk {
            fn_bodies: false,
            ..Walk::ALL
        };
        walk_stmts(body, policy, |node, _| match node {
            AstNode::Expr(Expr::Call { func, args, .. }) if !inert(func, args) => {
                ControlFlow::Break(())
            }
            AstNode::Stmt(Stmt::Assign { target, .. }) if binding_name(target).is_none() => {
                ControlFlow::Break(())
            }
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::Assign | BinOpKind::SuperAssign,
                lhs,
                ..
            }) if binding_name(lhs).is_none() => ControlFlow::Break(()),
            _ => ControlFlow::Continue(Descend::Into),
        })
        .is_break()
    }

    /// An R-audited base call that runs no project code. Each entry names
    /// the generics it may dispatch internally; a project or imported
    /// method for any of them could run arbitrary code.
    fn audited_base_call(&self, name: &str, args: &[Arg], scope: &Scope) -> bool {
        let bare = bare_name(name);
        let Some(closure) = frame_effect_dispatch_closure(bare) else {
            return false;
        };
        if !self.base_call_head(name, scope) {
            return false;
        }
        let dispatch_free = !closure
            .iter()
            .any(|generic| self.project_defines_frame_effect_method(generic));
        if LITERAL_MESSAGE_CALLS.contains(&bare) {
            return dispatch_free && literal_message_arguments(args);
        }
        if bare == "stopifnot" {
            // `exprs`/`exprObject` evaluate quoted code the walk never sees.
            // A predicate value without a class attribute reaches none of
            // its dispatch closure.
            return !stopifnot_evaluates_quoted(args)
                && (dispatch_free
                    || args
                        .iter()
                        .filter(|arg| stopifnot_predicate_arg(arg))
                        .all(|arg| {
                            self.plain_logical(&arg.value, scope)
                                || proven_unclassed_atomic(&arg.value, scope)
                        }));
        }
        dispatch_free
    }

    /// A head that resolves to base, including the `::`/`:::` operator of a
    /// qualified head: a project mask of the operator runs instead.
    fn base_call_head(&self, name: &str, scope: &Scope) -> bool {
        let operator = if name.contains(":::") { ":::" } else { "::" };
        self.resolves_to_base_lenient(name, scope)
            && (!name.contains("::") || !ops_chooser::operator_rebound(self, operator, scope))
    }

    /// A predicate whose value is an unclassed logical, so `stopifnot`'s
    /// own `anyNA`/`all` calls cannot dispatch on it.
    fn plain_logical(&self, expr: &Expr, scope: &Scope) -> bool {
        match expr {
            Expr::Logical(..) => true,
            Expr::BinOp { op, .. } if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) => {
                !ops_chooser::operator_rebound(self, op_symbol(*op), scope)
            }
            Expr::BinOp { op, .. } if is_comparison(*op) => {
                !ops_chooser::operator_rebound(self, op_symbol(*op), scope)
                    && !self.project_defines_comparison_method()
            }
            Expr::UnaryOp {
                op: UnaryOpKind::Not,
                expr,
                ..
            } => {
                !ops_chooser::operator_rebound(self, "!", scope) && self.plain_logical(expr, scope)
            }
            Expr::Call { func, .. } => callee_name(func).is_some_and(|name| {
                let bare = bare_name(&name);
                bare.starts_with("is.")
                    && SCALAR_FACT_SAFE_CALLS.contains(&bare)
                    && self.base_call_head(&name, scope)
            }),
            _ => false,
        }
    }

    /// Calls that end current scalar facts but still let a later assertion
    /// prove one: base `I()`, and a call into another package's namespace.
    /// This trades soundness for the common deprecation or validation call
    /// at the top of a function: an external package that deliberately
    /// installs an active binding for its caller's variable is not modelled.
    fn trusted_external_call(&self, name: &str, args: &[Arg], scope: &Scope) -> bool {
        let callee = if bare_name(name) == "I" {
            self.resolves_to_base_lenient(name, scope)
        } else if let Some((package, member)) = crate::resolve::split_qualified(name)
            && !name.contains(":::")
        {
            // Only an exported `pkg::fn`; `pkg:::fn` reaches internals.
            !crate::resolve::BASE_DATABASE_PACKAGES.contains(&package)
                && !self.fn_table.functions.binds(member)
                && !self.known_vars.contains(member)
                && !ops_chooser::operator_rebound(self, "::", scope)
        } else {
            false
        };
        // Nested calls classify themselves. A closure, or a name that may
        // hold one, could run later; only a value of known non-function
        // mode is safe to pass. An environment may hold closures and a
        // language object may be evaluated, so neither is safe.
        callee
            && args.iter().all(|arg| match &arg.value {
                Expr::Function { .. } => false,
                Expr::Ident { name, .. } => {
                    scope.get(name).is_some_and(|ty| {
                        !ty.mode.may_be_function() && !ty.mode.is_known_non_vector()
                    }) && !self.fn_table.functions.binds(name)
                }
                _ => true,
            })
    }
}

/// A literal, or a local whose inferred value is an atomic vector known to
/// carry no class attribute: internal dispatch cannot select a method for
/// it. A formal's default says nothing about a supplied actual.
fn proven_unclassed_atomic(expr: &Expr, scope: &Scope) -> bool {
    match expr {
        Expr::Ident { name, .. } => {
            !scope.is_default_parameter(name)
                && scope.get(name).is_some_and(|ty| {
                    ty.class.known
                        && ty.class.len == 0
                        && (ty.mode.is_atomic() || ty.mode == Mode::Null)
                })
        }
        expr => is_scalar_literal(expr),
    }
}

/// A user function called with literal actuals (forcing any other promise
/// may read an active binding): its syntactic outward-write summary.
fn user_function_effect(
    args: &[Arg],
    writes: impl FnOnce() -> crate::collect::WriteSet,
) -> CallEffect {
    if !args.iter().all(|arg| is_scalar_literal(&arg.value)) {
        return CallEffect::Unknown;
    }
    let writes = writes();
    if writes.opaque || !writes.names.is_empty() {
        CallEffect::Writes(writes)
    } else {
        CallEffect::None
    }
}

/// `message(..., domain = NULL, appendLF = TRUE)` matches formals after
/// `...` exactly, so only those exact names are controls; every other
/// actual is payload. A character literal payload needs no `as.character`
/// dispatch, and a forwarded `...` may carry anything.
fn literal_message_arguments(args: &[Arg]) -> bool {
    args.iter()
        .all(|arg| match (arg.name.as_deref(), &arg.value) {
            (Some("domain"), value) => {
                matches!(value, Expr::Null(_) | Expr::Na(..) | Expr::String(..))
            }
            (Some("appendLF"), value) => matches!(value, Expr::Logical(..)),
            (_, value) => matches!(value, Expr::String(..)),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_config::config::ScopedPaths;

    /// Classify every top-level call statement of `source`, with `x` a
    /// formal of the classifying frame.
    fn classify(checker: &Checker, source: &str) -> Vec<CallEffect> {
        let file = crate::tests::parse_file("call-effect.R", source);
        let mut scope = Scope::default();
        scope.insert_parameter("x", RType::unknown());
        file.stmts
            .iter()
            .filter_map(|statement| match statement {
                Stmt::Expr(Expr::Call { func, args, .. }) => {
                    Some(checker.call_effect(func, args, &scope))
                }
                _ => None,
            })
            .collect()
    }

    fn checker_for(source: &str) -> Checker {
        let file = crate::tests::parse_file("call-effect.R", source);
        let mut checker = Checker::new("call-effect.R");
        checker.collect_fns(&file.stmts);
        checker
    }

    fn writes(names: &[&str], opaque: bool) -> CallEffect {
        CallEffect::Writes(crate::collect::WriteSet {
            names: names.iter().map(|name| name.to_string()).collect(),
            opaque,
        })
    }

    #[test]
    fn call_effect_grades_each_call_shape() {
        use CallEffect::*;
        let checker = checker_for(
            "noop <- function() NULL\nbump <- function() g <<- 1L\ntouch <- function() get('x')\n",
        );
        let calls = "length('a')\nlength(x)\nstopifnot(TRUE)\nstopifnot(exprs = TRUE)\nnoop()\nnoop(x)\nbump()\ntouch()\n(function() 1L)()\nlifecycle::deprecate_warn('1.0.0', 'f()')\nI('a')\nI(function() 1L)\nsum(1:3)\nmatch.arg(x)\nbase::message('hi')\n";
        assert_eq!(
            classify(&checker, calls),
            vec![
                RunsCallerCode,
                RunsCallerCode,
                RunsCallerCode,
                Unknown,
                None,
                Unknown,
                writes(&["g"], false),
                writes(&[], true),
                None,
                RebindsAny,
                RebindsAny,
                Unknown,
                Unknown,
                Unknown,
                RunsCallerCode,
            ]
        );
        assert!(RunsCallerCode.keeps_scalar_facts() && !writes(&["g"], false).keeps_scalar_facts());
        // A project definition named like a base or external head runs
        // instead, whatever the head spells.
        let shadowing = checker_for(
            "`pkg::poke` <- function(...) { x <<- 1L; invisible(NULL) }\n`base::length` <- function(...) NULL\n",
        );
        assert_eq!(
            classify(&shadowing, "\"pkg::poke\"('a')\n\"base::length\"('a')\n"),
            vec![writes(&["x"], true), None]
        );
        // A masked qualification operator runs instead of base `length`.
        let masked = checker_for("`::` <- function(pkg, name) function(...) NULL\n");
        assert_eq!(
            classify(&masked, "base::length('a')\nlength('a')\n"),
            vec![Unknown, RunsCallerCode]
        );
    }

    #[test]
    fn message_matches_controls_exactly_and_needs_literal_payload() {
        let checker = checker_for("");
        for (call, inert) in [
            ("message('hi')", true),
            ("message('a', 'b', appendLF = TRUE)", true),
            ("message('hi', domain = NA)", true),
            ("message('hi', domain = NULL, appendLF = FALSE)", true),
            ("message(payload = 'hi')", true),
            ("message(obj)", false),
            ("message(payload = obj)", false),
            ("message('hi', appendL = FALSE)", false),
            ("message('hi', appendLF = flag)", false),
            ("message('hi', domain = d)", false),
            ("message(...)", false),
            ("message(1L)", false),
        ] {
            assert_eq!(
                classify(&checker, call),
                vec![if inert {
                    CallEffect::RunsCallerCode
                } else {
                    CallEffect::Unknown
                }],
                "{call}"
            );
        }
        let screened = checker_for("conditionMessage.simpleMessage <- function(c) 'm'\n");
        assert_eq!(
            classify(&screened, "message('hi')"),
            vec![CallEffect::Unknown]
        );
    }

    #[test]
    fn dispatch_closures_screen_only_their_own_generics() {
        let checker = checker_for("anyNA.probe <- function(x, recursive = FALSE) FALSE\n");
        assert_eq!(
            classify(
                &checker,
                "stopifnot(y)\nstopifnot(is.null(y) || y > 0L)\nlength(y)\nnchar('a')\n"
            ),
            vec![
                CallEffect::Unknown,
                CallEffect::RunsCallerCode,
                CallEffect::RunsCallerCode,
                CallEffect::RunsCallerCode,
            ]
        );
        let checker = checker_for("length.probe <- function(x) 1L\n");
        assert_eq!(
            classify(&checker, "length(y)\nnchar('a')\n"),
            vec![CallEffect::Unknown, CallEffect::RunsCallerCode]
        );
    }

    #[test]
    fn classification_ignores_pass_mode_and_adopted_records() {
        let source = "f <- function(x) {\n  #| x integer\n  x\n}\nnoop <- function() NULL\n";
        let calls = "f('a')\nnoop()\nlength(x)\nmessage('hi')\nis.null(NULL)\npkg::run()\n";
        let file = crate::tests::parse_file("call-effect.R", source);
        let mut plain = checker_for(source);
        let mut adopted = checker_for(source);
        let scope = ScopedPaths::new(std::path::Path::new("."), &["**".into()]).unwrap();
        adopted.set_declaration_records(crate::typehint::read_records(&file, &scope));
        assert!(adopted.declarations.has_contracts());
        let expected = classify(&plain, calls);
        for checker in [&mut plain, &mut adopted] {
            for discarding in [false, true] {
                checker.discarding = discarding;
                assert_eq!(classify(checker, calls), expected);
            }
        }
    }
}
