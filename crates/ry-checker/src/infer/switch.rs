use super::*;

impl Checker {
    /// `switch` is a special form: selection precedes evaluation of alternatives.
    /// Return None only when an explicit user callable/stub owns the call.
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
            Expr::String(..)
                | Expr::Integer(..)
                | Expr::Double(..)
                | Expr::Logical(..)
                | Expr::UnaryOp {
                    op: UnaryOpKind::Neg,
                    ..
                }
        );
        // Earlier calls or deferred bodies can replace switch or namespace
        // operators. The current call's own barrier is not a prior effect.
        if literal_selector && !environment_known_before_call {
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
                if self.literal_bindings_may_be_shadowed(["-", "`-`"], &HashSet::new(), scope) {
                    return unknown(scope);
                }
                match expr.as_ref() {
                    Expr::Integer(value, _) => {
                        switch_numeric_index(-(*value as f64), alternatives.len())
                    }
                    Expr::Double(value, _) => switch_numeric_index(-*value, alternatives.len()),
                    _ => return Some(self.infer_switch_call(args, scope)),
                }
            }
            Expr::Missing(_) | Expr::Unknown(_) | Expr::Null(_) | Expr::Na(_, _) => {
                return unknown(scope);
            }
            // Dynamic selectors retain the existing all-alternative inference;
            // this path does not claim general constant propagation.
            _ => return Some(self.infer_switch_call(args, scope)),
        };
        match index {
            None => Some(RType::new(Mode::Null, Length::Zero)),
            Some(index) if matches!(alternatives[index].value, Expr::Missing(_)) => unknown(scope),
            Some(index) => Some(self.infer(&alternatives[index].value, scope)),
        }
    }
}

fn switch_numeric_index(value: f64, count: usize) -> Option<usize> {
    let value = value.trunc();
    (value.is_finite() && value >= 1.0 && value <= i32::MAX as f64 && value <= count as f64)
        .then(|| value as usize - 1)
}
