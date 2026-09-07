use super::*;

impl Checker {
    /// `switch` is a special form: selection precedes evaluation of alternatives.
    /// Return None to leave explicit callables/stubs on the ordinary path.
    pub(crate) fn infer_switch_special(
        &mut self,
        original: &str,
        callee: &Expr,
        semantic: &str,
        args: &[Arg],
        scope: &mut Scope,
        environment_known_before_call: bool,
    ) -> Option<RType> {
        let unknown = |scope: &mut Scope| {
            scope.invalidate_unknown_effects();
            Some(RType::unknown())
        };
        let dynamic = |checker: &mut Self, scope: &mut Scope| {
            if semantic == "switch" {
                Some(checker.infer_switch_call(args, scope))
            } else {
                unknown(scope)
            }
        };
        if original != semantic || (matches!(callee, Expr::String(_, _)) && semantic.contains("::"))
        {
            return unknown(scope);
        }
        if let Some((package, _)) = semantic.rsplit_once("::") {
            if self.literal_bindings_may_be_shadowed(
                ["::", "`::`", ":::", "`:::`"],
                &HashSet::new(),
                scope,
            ) || scope.effects_unknown
            {
                return unknown(scope);
            }
            let package = package.trim_end_matches(':');
            if package != "base" {
                return if self
                    .package_typeshed(package)
                    .is_some_and(|db| db.functions.contains_key("switch"))
                {
                    None
                } else {
                    unknown(scope)
                };
            }
        } else {
            if scope.is_parameter(semantic) {
                return unknown(scope);
            }
            if self.fn_table.fns.contains_key(semantic)
                || scope.get(semantic).is_some_and(|ty| ty.fn_sig.is_some())
            {
                return None;
            }
            if let Some(package) = self.imported_from.get(semantic)
                && package != "base"
            {
                return if self
                    .package_typeshed(package)
                    .is_some_and(|db| db.functions.contains_key("switch"))
                {
                    None
                } else {
                    unknown(scope)
                };
            }
            if scope.effects_unknown
                || scope.data_mask_unknown
                || self.literal_bindings_may_be_shadowed(
                    ["switch", "`switch`"],
                    &HashSet::new(),
                    scope,
                )
                || (!self.imported_from.contains_key(semantic)
                    && (scope.search_path_unknown || !self.bare_loaded.is_empty()))
            {
                return unknown(scope);
            }
        }
        if self.user_stubs.contains_key("base") {
            return None;
        }
        let Some(first) = args.first() else {
            return unknown(scope);
        };
        // Unlike closure matching, the first actual is EXPR even when named.
        // Later EXPR tags remain ordinary alternative names.
        if first.name.as_deref().is_some_and(|name| {
            let name = semantic_argument_name(name);
            name.is_empty() || name.contains('\\') || !"EXPR".starts_with(name)
        }) || args.iter().any(|arg| {
            arg.name.as_deref().is_some_and(|name| name.contains('\\'))
                || matches!(&arg.value, Expr::Ident { name, .. } if name == "...")
        }) {
            return unknown(scope);
        }
        // Parentheses are normalized out of the AST, but R can rebind `(`.
        if self.literal_bindings_may_be_shadowed(["(", "`(`"], &HashSet::new(), scope) {
            return unknown(scope);
        }
        let literal_selector = matches!(
            first.value,
            Expr::String(..) | Expr::Integer(..) | Expr::Double(..) | Expr::Logical(..)
        ) || matches!(&first.value, Expr::UnaryOp { op: UnaryOpKind::Neg, expr, .. }
                if matches!(expr.as_ref(), Expr::Integer(..) | Expr::Double(..)));
        // Earlier calls or deferred bodies can replace switch or namespace
        // operators. The current call's own barrier is not a prior effect.
        if literal_selector
            && (!environment_known_before_call || ops_chooser::syntax_rebound(self, scope))
        {
            return unknown(scope);
        }
        let alternatives = &args[1..];
        let index = match &first.value {
            Expr::String(selector, _) => {
                // Keep this proof within decoded ASCII. Undecoded escapes,
                // NUL recovery and non-ASCII encoding remain opaque.
                if !selector.is_ascii() || selector.contains(['\\', '\0']) {
                    return unknown(scope);
                }
                let defaults: Vec<_> = alternatives
                    .iter()
                    .enumerate()
                    .filter(|(_, arg)| arg.name.is_none())
                    .map(|(index, _)| index)
                    .collect();
                if defaults.len() > 1 {
                    return unknown(scope);
                }
                if let Some(start) = alternatives.iter().position(|arg| {
                    arg.name
                        .as_deref()
                        .is_some_and(|name| semantic_argument_name(name) == selector)
                }) {
                    (start..alternatives.len())
                        .find(|&index| !matches!(alternatives[index].value, Expr::Missing(_)))
                } else {
                    defaults.first().copied()
                }
            }
            Expr::Integer(value, _) => switch_numeric_index(*value as f64, alternatives.len()),
            Expr::Double(value, _) => switch_numeric_index(*value, alternatives.len()),
            Expr::Logical(value, _) => {
                switch_numeric_index(if *value { 1.0 } else { 0.0 }, alternatives.len())
            }
            Expr::UnaryOp {
                op: UnaryOpKind::Neg,
                expr,
                ..
            } => {
                let value = match expr.as_ref() {
                    Expr::Integer(value, _) => *value as f64,
                    Expr::Double(value, _) => *value,
                    _ => return dynamic(self, scope),
                };
                if self.literal_bindings_may_be_shadowed(["-", "`-`"], &HashSet::new(), scope) {
                    return unknown(scope);
                }
                switch_numeric_index(-value, alternatives.len())
            }
            Expr::Missing(_) | Expr::Unknown(_) | Expr::Null(_) | Expr::Na(_, _) => {
                return unknown(scope);
            }
            // Bare dynamic selectors retain the existing all-alternative inference;
            // this path does not claim general constant propagation.
            _ => return dynamic(self, scope),
        };
        match index {
            None => Some(RType::new(Mode::Null, Length::Zero)),
            Some(index) if matches!(alternatives[index].value, Expr::Missing(_)) => unknown(scope),
            Some(index) => {
                // Base switch and its literal selector have not run user code.
                // Remove only our own call barrier so the chosen expression's
                // uncertainty is observable independently of that barrier.
                scope.ops_environment_unknown = false;
                let selected = &alternatives[index].value;
                let pure_constructor = self.pure_switch_constructor(selected, scope);
                let result = self.infer(selected, scope);
                if scope.ops_environment_unknown && !pure_constructor {
                    return unknown(scope);
                }
                Some(result)
            }
        }
    }
    /// Whole-expression constructors with inert actuals can retain their
    /// result despite the ordinary call barrier. This is not a general purity
    /// model for nested calls or user functions.
    fn pure_switch_constructor(&self, expression: &Expr, scope: &Scope) -> bool {
        let Expr::Call { func, args, .. } = expression else {
            return false;
        };
        if ops_chooser::pure_literal_constructor(self, func, args, scope) {
            return true;
        }
        let Some(name) = ident_name(func) else {
            return false;
        };
        if self.user_stubs.contains_key("base")
            || scope.function_alias(name).is_some()
            || !args.iter().all(|arg| {
                matches!(
                    arg.value,
                    Expr::Logical(..)
                        | Expr::Integer(..)
                        | Expr::Double(..)
                        | Expr::String(..)
                        | Expr::Null(..)
                        | Expr::Na(..)
                )
            })
        {
            return false;
        }
        match name {
            "base::list" | "base:::list" => true,
            "list" => {
                !scope.search_path_unknown
                    && self.bare_loaded.is_empty()
                    && self.resolves_to_base(name, scope)
                    && !self.literal_bindings_may_be_shadowed(
                        ["list", "`list`"],
                        &HashSet::new(),
                        scope,
                    )
            }
            _ => false,
        }
    }
}

fn switch_numeric_index(value: f64, count: usize) -> Option<usize> {
    let value = value.trunc();
    (value.is_finite() && value >= 1.0 && value <= i32::MAX as f64 && value <= count as f64)
        .then(|| value as usize - 1)
}
