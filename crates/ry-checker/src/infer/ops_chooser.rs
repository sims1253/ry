use super::*;

/// Evidence about a function value installed by an ordinary assignment.
/// Aliases share this allocation; equal return types alone are not identity.
#[derive(Debug)]
pub(crate) struct LiteralFunction {
    mode: Mode,
    choice: Option<bool>,
    accepts_two: bool,
    accepts_six: bool,
}

pub(crate) fn literal_function(
    checker: &Checker,
    value: &Expr,
    scope: &Scope,
) -> Option<Arc<LiteralFunction>> {
    if scope.ops_environment_unknown || !checker.bare_loaded.is_empty() {
        return None;
    }
    if let Expr::Ident { name, .. } = value {
        let function = scope.literal_functions.get(semantic_argument_name(name))?;
        return (!syntax_rebound(checker, scope)).then(|| Arc::clone(function));
    }
    let Expr::Function { params, body, .. } = value else {
        return None;
    };
    let [Stmt::Expr(value)] = body.as_slice() else {
        return None;
    };
    let (mode, choice) = match value {
        Expr::Logical(value, _) => (Mode::Logical, Some(*value)),
        Expr::Integer(..) => (Mode::Integer, None),
        Expr::Double(..) => (Mode::Double, None),
        Expr::String(..) => (Mode::Character, None),
        _ => return None,
    };
    if syntax_rebound(checker, scope) {
        return None;
    }
    let accepts = |names: &[&str]| {
        params
            .iter()
            .map(|p| p.name.as_str())
            .eq(names.iter().copied())
            || (params.last().is_some_and(|p| p.name == "...")
                && params.len() <= names.len() + 1
                && params[..params.len() - 1]
                    .iter()
                    .map(|p| p.name.as_str())
                    .eq(names[..params.len() - 1].iter().copied()))
    };
    Some(Arc::new(LiteralFunction {
        mode,
        choice,
        accepts_two: accepts(&["e1", "e2"]),
        accepts_six: accepts(&["x", "y", "mx", "my", "cl", "reverse"]),
    }))
}

pub(crate) fn dispatch(
    checker: &Checker,
    symbol: &str,
    lhs: &RType,
    rhs: &RType,
    scope: &Scope,
) -> Option<RType> {
    if scope.data_mask_unknown || scope.search_path_unknown {
        return None;
    }
    let first_class = |ty: &RType| {
        if ty.class.is_unknown()
            || ty.class.len == 0
            || ty.class.names[0].as_deref() == Some("default")
        {
            None
        } else {
            ty.class.names[0].clone()
        }
    };
    let left_class = first_class(lhs)?;
    let right_class = first_class(rhs)?;
    // A known first-class specific method wins before any group method or
    // later class. Never infer a miss from the incomplete project registry.
    let left_name = format!("{symbol}.{left_class}");
    let right_name = format!("{symbol}.{right_class}");
    let lexical_method = |name: &str| {
        scope
            .get(name)
            .or_else(|| scope.get(&format!("`{name}`")))
            .is_some_and(|ty| matches!(ty.mode, Mode::Function | Mode::Opaque))
    };
    let (Some(left), Some(right)) = (
        scope.literal_functions.get(&left_name),
        scope.literal_functions.get(&right_name),
    ) else {
        return (left_class != right_class
            && lexical_method(&left_name)
            && lexical_method(&right_name))
        .then(RType::unknown);
    };
    if scope.ops_environment_unknown
        || syntax_rebound(checker, scope)
        || operator_rebound(checker, symbol, scope)
    {
        return Some(RType::unknown());
    }
    if !left.accepts_two || !right.accepts_two {
        return Some(RType::unknown());
    }
    if Arc::ptr_eq(left, right) {
        return Some(RType::scalar(left.mode));
    }
    let choose = |class: &str| {
        scope
            .literal_functions
            .get(&format!("chooseOpsMethod.{class}"))
            .filter(|function| function.accepts_six)
            .and_then(|function| function.choice)
    };
    Some(match choose(&left_class) {
        Some(true) => RType::scalar(left.mode),
        Some(false) if choose(&right_class) == Some(true) => RType::scalar(right.mode),
        // Missing, dynamic, and two-FALSE selection stay unknown. Primitive
        // fallback has its own attribute rules and is not a method lookup miss.
        _ => RType::unknown(),
    })
}

/// These spellings can change execution even when parsing retained only the
/// ordinary syntax shape. Include both parser spellings without allocating.
pub(crate) fn syntax_rebound(checker: &Checker, scope: &Scope) -> bool {
    checker.literal_bindings_may_be_shadowed(
        crate::semantic_lists::OPS_PROOF_SYNTAX.iter().copied(),
        &HashSet::new(),
        scope,
    )
}

/// A closed, literal-only constructor cannot run user code or force a promise.
/// Bare lookup is admitted only before any uncertain effect, with no visible
/// masking or package search path. Qualified lookup still checks syntax.
pub(crate) fn pure_structure_call(
    checker: &Checker,
    func: &Expr,
    args: &[Arg],
    scope: &Scope,
) -> bool {
    if scope.ops_environment_unknown
        || scope.data_mask_unknown
        || scope.search_path_unknown
        || checker.user_stubs.contains_key("base")
    {
        return false;
    }
    let Some(name) = ident_name(func) else {
        return false;
    };
    let base = name == "base::structure"
        || (name == "structure"
            && scope.get("structure").is_none()
            && scope.get("`structure`").is_none()
            && !checker.fn_table.known_vars.contains("structure")
            && !checker.fn_table.known_vars.contains("`structure`")
            && !checker.fn_table.fns.contains_key("structure")
            && checker.bare_loaded.is_empty()
            && !checker.external_bindings.contains("structure")
            && checker
                .imported_from
                .get("structure")
                .is_none_or(|package| package == "base"));
    base && args.iter().all(|arg| {
        matches!(
            arg.value,
            Expr::Logical(..)
                | Expr::Integer(..)
                | Expr::Double(..)
                | Expr::String(..)
                | Expr::Null(..)
        )
    }) && !syntax_rebound(checker, scope)
}

/// Some statement operators lose their identity during lowering. Admit only
/// the ordinary left-assignment tokens, as reference provenance does.
pub(crate) fn ordinary_assignment(checker: &Checker, target: &Expr, value: &Expr) -> bool {
    matches!(
        checker
            .source
            .get(span_of(target).end..span_of(value).start)
            .map(str::trim),
        Some("<-" | "=")
    )
}

pub(crate) fn operator_rebound(checker: &Checker, symbol: &str, scope: &Scope) -> bool {
    checker.literal_bindings_may_be_shadowed(
        [symbol, &format!("`{symbol}`")],
        &HashSet::new(),
        scope,
    )
}
