use super::*;

pub(crate) enum Dispatch {
    Value(RType),
    IncompatiblePrimitive {
        left_method: String,
        right_method: String,
    },
}

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
    plain_vectors: bool,
    scope: &Scope,
) -> Option<Dispatch> {
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
        .then(|| Dispatch::Value(RType::unknown()));
    };
    if scope.ops_environment_unknown
        || syntax_rebound(checker, scope)
        || operator_rebound(checker, symbol, scope)
    {
        return Some(Dispatch::Value(RType::unknown()));
    }
    if !left.accepts_two || !right.accepts_two {
        return Some(Dispatch::Value(RType::unknown()));
    }
    if Arc::ptr_eq(left, right) {
        return Some(Dispatch::Value(RType::scalar(left.mode)));
    }
    let choose = |class: &str| {
        scope
            .literal_functions
            .get(&format!("chooseOpsMethod.{class}"))
            .filter(|function| function.accepts_six)
            .and_then(|function| function.choice)
    };
    Some(match (choose(&left_class), choose(&right_class)) {
        (Some(true), _) => Dispatch::Value(RType::scalar(left.mode)),
        (Some(false), Some(true)) => Dispatch::Value(RType::scalar(right.mode)),
        (Some(false), Some(false))
            if left.mode != right.mode
                && ((scalar_primitive(lhs) && scalar_primitive(rhs))
                    || (plain_vectors && known_atomic(lhs) && known_atomic(rhs))) =>
        {
            // Different literal storage modes prove that the method bodies
            // are not identical. Equal modes alone prove nothing about identity.
            Dispatch::IncompatiblePrimitive {
                left_method: left_name,
                right_method: right_name,
            }
        }
        _ => Dispatch::Value(RType::unknown()),
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
pub(crate) fn pure_literal_constructor(
    checker: &Checker,
    func: &Expr,
    args: &[Arg],
    scope: &Scope,
) -> bool {
    if pure_literal_c(checker, func, args, scope) {
        return true;
    }
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
    let literal = |arg: &Arg| {
        matches!(
            arg.value,
            Expr::Logical(..)
                | Expr::Integer(..)
                | Expr::Double(..)
                | Expr::String(..)
                | Expr::Null(..)
        )
    };
    let vector_payload = matches!(args, [payload, class]
        if payload.name.is_none() && class.name.as_deref() == Some("class")
            && matches!(class.value, Expr::String(..))
            && matches!(&payload.value, Expr::Call { func, args, .. } if pure_literal_c(checker, func, args, scope)));
    base && (args.iter().all(literal) || vector_payload) && !syntax_rebound(checker, scope)
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

/// A constant function body ignores its arguments after argument matching.
/// The caller checks that the binding environment still has this identity.
pub(crate) fn literal_operator_return(symbol: &str, scope: &Scope) -> Option<RType> {
    scope
        .literal_functions
        .get(symbol)
        .filter(|function| function.accepts_two)
        .map(|function| RType::scalar(function.mode))
}

fn scalar_primitive(ty: &RType) -> bool {
    ty.length == Length::One
        && matches!(
            ty.mode,
            Mode::Logical | Mode::Integer | Mode::Double | Mode::Character
        )
}

fn known_atomic(ty: &RType) -> bool {
    matches!(ty.length, Length::One | Length::Known(1..))
        && matches!(
            ty.mode,
            Mode::Logical | Mode::Integer | Mode::Double | Mode::Character
        )
}

pub(crate) fn plain_vector(checker: &Checker, value: &Expr, scope: &Scope) -> bool {
    if scope.ops_environment_unknown {
        return false;
    }
    if let Expr::Ident { name, .. } = value {
        return scope
            .plain_ops_vectors
            .contains(semantic_argument_name(name));
    }
    let Expr::Call { func, args, .. } = value else {
        return false;
    };
    if !pure_literal_constructor(checker, func, args, scope) {
        return false;
    }
    let [payload, class] = args.as_slice() else {
        return false;
    };
    payload.name.is_none()
        && class.name.as_deref() == Some("class")
        && matches!(class.value, Expr::String(..))
        && (matches!(
            payload.value,
            Expr::Logical(..) | Expr::Integer(..) | Expr::Double(..) | Expr::String(..)
        ) || matches!(&payload.value, Expr::Call { func, args, .. } if !args.is_empty() && pure_literal_c(checker, func, args, scope)))
}

fn pure_literal_c(checker: &Checker, func: &Expr, args: &[Arg], scope: &Scope) -> bool {
    ident_name(func) == Some("base::c")
        && !scope.ops_environment_unknown
        && !scope.data_mask_unknown
        && !scope.search_path_unknown
        && !checker.user_stubs.contains_key("base")
        && !syntax_rebound(checker, scope)
        && args.iter().all(|arg| {
            arg.name.is_none()
                && matches!(
                    arg.value,
                    Expr::Logical(..) | Expr::Integer(..) | Expr::Double(..) | Expr::String(..)
                )
        })
}
