//! Writes to the current frame that the source shows inside a call rather
//! than as an assignment statement (#867): `eval(quote(x <- v))`,
//! `evalq(x <- v)`, and `rm(x)`. `on.exit(x <- v)` runs after the body, so
//! the function-body walk applies it to the scope-exit snapshot instead.
//!
//! Each write widens the binding to unknown. That is sound whatever the
//! callee resolves to, so only an environment argument that is not provably
//! this frame skips it.

use super::args::match_arguments;
use super::call::callee_name;
use super::*;
use crate::semantic_lists::bare_name;

impl Checker {
    /// Apply the visible frame writes of a call to `scope`, after the call
    /// itself has been inferred.
    pub(crate) fn apply_visible_frame_writes(&self, func: &Expr, args: &[Arg], scope: &mut Scope) {
        enum Verb {
            Eval,
            Evalq,
            Rm,
        }
        let Some(name) = callee_name(func) else {
            return;
        };
        let semantic = scope.function_alias(&name).unwrap_or(&name);
        let verb = match bare_name(semantic_argument_name(semantic)) {
            "eval" => Verb::Eval,
            "evalq" => Verb::Evalq,
            "rm" | "remove" => Verb::Rm,
            _ => return,
        };
        let matched = match_arguments(&["expr", "envir", "enclos"], args);
        let envir = matched.arg_for_param(1).map(|index| &args[index].value);
        let expression = matched.arg_for_param(0).map(|index| &args[index].value);
        match verb {
            Verb::Eval => {
                if let Some(Expr::Call {
                    func: quote,
                    args: quoted,
                    ..
                }) = expression
                    && matches!(quote.as_ref(), Expr::Ident { name, .. }
                        if bare_name(semantic_argument_name(name)) == "quote")
                    && let [quoted] = quoted.as_slice()
                    && self.targets_this_frame(envir, scope)
                {
                    widen_writes(&quoted.value, scope);
                }
            }
            Verb::Evalq => {
                if let Some(expression) = expression
                    && self.targets_this_frame(envir, scope)
                {
                    widen_writes(expression, scope);
                }
            }
            Verb::Rm => self.widen_removed_bindings(args, scope),
        }
    }

    /// Whether an omitted or `environment()` environment argument names the
    /// frame the call runs in.
    pub(super) fn targets_this_frame(&self, envir: Option<&Expr>, scope: &Scope) -> bool {
        envir.is_none_or(|envir| {
            matches!(envir, Expr::Call { func, args, .. }
            if args.is_empty()
                && callee_name(func).is_some_and(|name| {
                    bare_name(&name) == "environment"
                        && self.resolves_to_base_lenient(&name, scope)
                }))
        })
    }

    /// `rm(x, "y")` in this frame. The binding would be gone, but an
    /// installer ry does not model (`makeActiveBinding()`, a helper given
    /// `environment()`) can rebind the name before a later read, which must
    /// not then report RY010. The value widens to unknown instead. A formal
    /// is already exported as unknown, and its parameter-guard RY032 stays.
    fn widen_removed_bindings(&self, args: &[Arg], scope: &mut Scope) {
        let mut names = Vec::new();
        for argument in args {
            match (argument.name.as_deref(), &argument.value) {
                (None, Expr::Ident { name, .. } | Expr::String(name, _)) => names.push(name),
                (Some("envir"), envir) if self.targets_this_frame(Some(envir), scope) => {}
                (Some("inherits"), Expr::Logical(false, _)) => {}
                _ => return,
            }
        }
        for name in names {
            if scope.get(name).is_some() && !scope.is_parameter(name) {
                scope.insert(name.clone(), RType::unknown());
            }
        }
    }
}

/// Widen every name `expression` visibly binds in this frame.
fn widen_writes(expression: &Expr, scope: &mut Scope) {
    for name in crate::collect::expression_frame_writes(expression).names {
        scope.insert(name, RType::unknown());
    }
}
