use super::*;
use crate::infer::*;
use crate::semantic_lists::bare_name;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr, walk_stmt, walk_stmts};
use std::ops::ControlFlow;

/// Only these expressions certify that an installer receives a frame made by
/// the helper itself. Any other value may be its caller's frame. In
/// particular, `get("env")` and an immediately invoked closure can return a
/// formal without exposing that formal to the direct identifier walk.
fn definitely_local_installer_env(expression: &Expr, local_names: &HashSet<String>) -> bool {
    match expression {
        Expr::Ident { name, .. } => local_names.contains(name),
        Expr::Call { func, args, .. } => {
            let Some(name) = ident_name(func) else {
                return false;
            };
            match name {
                "base::environment" => args.is_empty(),
                "base::new.env" => true,
                _ => false,
            }
        }
        _ => false,
    }
}

/// A supplied callback is safe for this binding-effect check only when its
/// body cannot read a promise, call another function, or mutate an environment.
/// This intentionally recognizes a small, closed class of literal no-ops.
pub(crate) fn inert_caller_binding_actual(expression: &Expr) -> bool {
    let Expr::Function { body, .. } = expression else {
        return false;
    };
    inert_caller_binding_body(body)
}

pub(crate) fn inert_caller_binding_body(body: &[Stmt]) -> bool {
    let inert_value = |value: &Expr| {
        matches!(
            value,
            Expr::Null(_)
                | Expr::Logical(_, _)
                | Expr::Integer(_, _)
                | Expr::Double(_, _)
                | Expr::String(_, _)
                | Expr::Na(_, _)
        )
    };
    body.iter().all(|statement| match statement {
        Stmt::Expr(value) if inert_value(value) => true,
        // A nested function literal is a value. Its body and defaults are
        // deferred until that returned function is invoked.
        Stmt::FunctionDef { .. } => true,
        Stmt::Return { value, .. } => value.as_ref().is_none_or(&inert_value),
        Stmt::Expr(Expr::Call { func, args, .. })
            if ident_name(func)
                .is_some_and(|name| matches!(name, "base::invisible" | "base::return")) =>
        {
            args.len() <= 1 && args.iter().all(|arg| inert_value(&arg.value))
        }
        _ => false,
    })
}

/// Keep callable provenance when a value is wrapped before it is assigned or
/// passed to another call. The enclosing call decides whether the value can
/// be invoked; a pure `base::identity` or `base::list` alone does not.
/// Exhaustion or an escaped name withdraws the negative effect proof.
fn caller_binding_value_sources(expression: &Expr) -> Option<HashSet<String>> {
    let mut sources = HashSet::new();
    let mut uncertain = false;
    let walk = Walk {
        assign_targets: false,
        assign_operands: true,
        dollar_args: false,
        fn_bodies: true,
        control_tests: true,
    };
    let _ = walk_expr(expression, walk, |node, _| {
        if let AstNode::Expr(Expr::Ident { name, .. }) = node {
            match caller_binding_identity(name) {
                Some(name) if sources.len() < 128 => {
                    sources.insert(name);
                }
                _ => uncertain = true,
            }
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    });
    (!uncertain).then_some(sources)
}

fn expand_block_value_sources(
    sources: HashSet<String>,
    aliases: &HashMap<String, HashSet<String>>,
) -> HashSet<String> {
    fn unknown() -> HashSet<String> {
        HashSet::from([UNKNOWN_CALLER_BINDING_IDENTITY.to_string()])
    }
    fn expand_one(
        source: &str,
        aliases: &HashMap<String, HashSet<String>>,
        completed: &mut HashMap<String, HashSet<String>>,
        visiting: &mut HashSet<String>,
        remaining: &mut usize,
    ) -> HashSet<String> {
        if let Some(result) = completed.get(source) {
            return result.clone();
        }
        if *remaining == 0 || !visiting.insert(source.to_string()) {
            return unknown();
        }
        *remaining -= 1;
        let result = if let Some(values) = aliases.get(source) {
            let mut result = HashSet::new();
            for value in values {
                result.extend(expand_one(value, aliases, completed, visiting, remaining));
                if result.len() > 128 {
                    result = unknown();
                    break;
                }
            }
            result
        } else {
            HashSet::from([source.to_string()])
        };
        visiting.remove(source);
        completed.insert(source.to_string(), result.clone());
        result
    }

    let mut completed = HashMap::new();
    let mut visiting = HashSet::new();
    let mut remaining = 128;
    let mut resolved = HashSet::new();
    for source in sources {
        resolved.extend(expand_one(
            &source,
            aliases,
            &mut completed,
            &mut visiting,
            &mut remaining,
        ));
        if resolved.len() > 128 {
            return unknown();
        }
    }
    resolved
}

/// Alias entries produced by this scanner contain values captured at the
/// assignment, rather than names to be resolved again after a later write.
/// A bare name read before any known local assignment may come from an outer
/// binding; copying it cannot certify a harmless callable. Formal values are
/// retained so their supplied callback is checked at the call site.
fn local_value_sources_at_point(
    sources: HashSet<String>,
    aliases: &HashMap<String, HashSet<String>>,
    formal_names: &HashSet<String>,
    capture: bool,
) -> HashSet<String> {
    let mut resolved = HashSet::new();
    for source in sources {
        if source.contains("::") && !source.starts_with(LITERAL_QUALIFIED_CALLER_BINDING_PREFIX) {
            // A namespace reference cannot be replaced by a same-spelled
            // backticked local binding.
            resolved.insert(source);
        } else if let Some(values) = aliases.get(&source) {
            resolved.extend(values.iter().cloned());
        } else if capture
            && source != UNKNOWN_CALLER_BINDING_IDENTITY
            && !formal_names.contains(&source)
            && !(formal_names.contains("...") && variadic_callable_source(&source))
        {
            resolved.insert(UNKNOWN_CALLER_BINDING_IDENTITY.to_string());
        } else {
            resolved.insert(source);
        }
        if resolved.len() > 128 {
            return HashSet::from([UNKNOWN_CALLER_BINDING_IDENTITY.to_string()]);
        }
    }
    resolved
}

/// Follow a top-level value only through base operations whose result is the
/// selected argument. A stored list is not a callable alias; extracting one
/// of its elements is. An unmodeled value can still be a callable installer,
/// so it cannot certify that a later invocation leaves the caller untouched.
pub(crate) fn global_caller_binding_value_sources(
    expression: &Expr,
    remaining: usize,
) -> HashSet<String> {
    let unknown = || HashSet::from([UNKNOWN_CALLER_BINDING_IDENTITY.to_string()]);
    if remaining == 0 {
        return unknown();
    }
    match expression {
        Expr::Ident { name, .. } => HashSet::from([caller_binding_identity(name)
            .unwrap_or_else(|| UNKNOWN_CALLER_BINDING_IDENTITY.to_string())]),
        Expr::Call { func, args, .. }
            if args.is_empty()
                && matches!(func.as_ref(), Expr::Function { params, body, .. }
                    if params.is_empty() && body.len() == 1) =>
        {
            let Expr::Function { body, .. } = func.as_ref() else {
                unreachable!();
            };
            match &body[0] {
                Stmt::Expr(value)
                | Stmt::Return {
                    value: Some(value), ..
                } => global_caller_binding_value_sources(value, remaining - 1),
                Stmt::FunctionDef { body, .. } if inert_caller_binding_body(body) => HashSet::new(),
                _ => unknown(),
            }
        }
        Expr::Call { func, args, .. }
            if ident_name(func).is_some_and(|name| {
                matches!(name, "base::identity" | "base::force" | "base::invisible")
            }) && args.len() == 1
                && args[0].name.as_deref().is_none_or(|name| name == "x") =>
        {
            global_caller_binding_value_sources(&args[0].value, remaining - 1)
        }
        Expr::Call { func, .. } if ident_name(func) == Some("base::list") => HashSet::new(),
        Expr::Index {
            base,
            kind: IndexKind::Double,
            args,
            ..
        } => {
            if let Expr::Call {
                func, args: values, ..
            } = base.as_ref()
                && ident_name(func) == Some("base::list")
                && let [
                    Arg {
                        name: None,
                        value: Expr::Integer(index, _),
                        ..
                    },
                ] = args.as_slice()
                && *index > 0
                && let Some(value) = values.get((*index - 1) as usize)
            {
                let mut sources = global_caller_binding_value_sources(&value.value, remaining - 1);
                // `[[` uses an ordinary R operator binding even when the
                // list constructor is qualified. A masked extractor may
                // return a different callable than the selected element.
                sources.insert(UNKNOWN_CALLER_BINDING_IDENTITY.to_string());
                sources
            } else {
                unknown()
            }
        }
        Expr::BinOp {
            op: BinOpKind::Assign,
            rhs,
            ..
        } => global_caller_binding_value_sources(rhs, remaining - 1),
        Expr::Block { body, .. } if body.len() < remaining => {
            let mut aliases = HashMap::new();
            let mut uncertain = false;
            for statement in body.iter().take(body.len().saturating_sub(1)) {
                match statement {
                    Stmt::Assign {
                        target: Expr::Ident { name, .. },
                        value,
                        ..
                    } => {
                        if let Some(name) = caller_binding_identity(name) {
                            let sources = global_caller_binding_value_sources(value, remaining - 1);
                            aliases.insert(
                                name,
                                local_value_sources_at_point(
                                    sources,
                                    &aliases,
                                    &HashSet::new(),
                                    true,
                                ),
                            );
                        } else {
                            uncertain = true;
                        }
                    }
                    Stmt::Expr(
                        Expr::Null(_)
                        | Expr::Logical(_, _)
                        | Expr::Integer(_, _)
                        | Expr::Double(_, _)
                        | Expr::String(_, _),
                    ) => {}
                    _ => uncertain = true,
                }
            }
            let mut sources = match body.last() {
                Some(Stmt::Expr(value) | Stmt::Assign { value, .. }) => {
                    global_caller_binding_value_sources(value, remaining - body.len())
                }
                Some(Stmt::FunctionDef { body, .. }) if inert_caller_binding_body(body) => {
                    HashSet::new()
                }
                _ => unknown(),
            };
            sources = local_value_sources_at_point(sources, &aliases, &HashSet::new(), false);
            if uncertain {
                sources.insert(UNKNOWN_CALLER_BINDING_IDENTITY.to_string());
            }
            sources
        }
        Expr::If { then, else_, .. } => {
            let mut sources = global_caller_binding_value_sources(then, remaining - 1);
            if let Some(other) = else_ {
                sources.extend(global_caller_binding_value_sources(other, remaining - 1));
            }
            if sources.len() > 128 {
                unknown()
            } else {
                sources
            }
        }
        Expr::Function { .. } if inert_caller_binding_actual(expression) => HashSet::new(),
        Expr::Null(_)
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::Na(_, _) => HashSet::new(),
        Expr::String(_, _) => unknown(),
        _ => unknown(),
    }
}

fn variadic_callable_source(name: &str) -> bool {
    name == "..."
        || name
            .strip_prefix("..")
            .and_then(|index| index.parse::<usize>().ok())
            .is_some_and(|index| index > 0)
}

/// `Err` means a tagged actual's semantic name is unknown, so callers must
/// keep the installer effect uncertain rather than treating it as omitted.
fn installer_environment_arg<'a>(name: &str, args: &'a [Arg]) -> Result<Option<&'a Expr>, ()> {
    if name.starts_with(LITERAL_QUALIFIED_CALLER_BINDING_PREFIX) {
        return Ok(None);
    }
    let (formals, environment) = match bare_name(name) {
        "makeActiveBinding" => (["sym", "fun", "env"].as_slice(), 2),
        "delayedAssign" => (["x", "value", "eval.env", "assign.env"].as_slice(), 3),
        "assign" => (
            ["x", "value", "pos", "envir", "inherits", "immediate"].as_slice(),
            3,
        ),
        _ => return Ok(None),
    };
    // Exact names bind first, then unique partial names, then the remaining
    // unnamed actuals. A raw index is wrong after a later named actual fills
    // an earlier formal (notably `eval.env =` before `assign.env`).
    let matched = crate::match_caller_binding_argument_names(formals, args).ok_or(())?;
    if bare_name(name) == "assign"
        && matched
            .arg_for_param(4)
            .and_then(|index| args.get(index))
            .is_some_and(|arg| !matches!(arg.value, Expr::Logical(false, _)))
    {
        // `inherits = TRUE` may write through an otherwise fresh local
        // environment to one of its parents. An unknown value is no proof
        // that the write stays local either.
        return Err(());
    }
    Ok(matched
        .arg_for_param(environment)
        .and_then(|index| args.get(index))
        .map(|arg| &arg.value))
}

/// An installer can replace a binding in this frame even after an assertion
/// has read its old value. Only an explicit qualified fresh constructor
/// certifies a distinct target frame; `base::environment()` is this frame
/// here, although it is local to a separately called helper.
pub(crate) fn is_caller_binding_installer_source(name: &str) -> bool {
    if name.starts_with(LITERAL_QUALIFIED_CALLER_BINDING_PREFIX) {
        return false;
    }
    let primitive = name
        .strip_prefix("base:::")
        .unwrap_or_else(|| bare_name(name));
    matches!(primitive, "assign" | "delayedAssign" | "makeActiveBinding")
}

pub(crate) fn installer_may_replace_current_binding(name: &str, args: &[Arg]) -> bool {
    if !is_caller_binding_installer_source(name) {
        return false;
    }
    let primitive = name
        .strip_prefix("base:::")
        .unwrap_or_else(|| bare_name(name));
    if name.contains("::") && !crate::semantic_lists::is_base_qualified(name) {
        // Another namespace's function has no certified base argument
        // contract, even if one actual looks like a fresh environment.
        return true;
    }
    // Base installers have no `...`. An unambiguously unmatched or duplicate
    // actual fails argument matching before its body can replace a binding.
    let formals: &[&str] = match primitive {
        "assign" => &["x", "value", "pos", "envir", "inherits", "immediate"],
        "delayedAssign" => &["x", "value", "eval.env", "assign.env"],
        "makeActiveBinding" => &["sym", "fun", "env"],
        _ => &[],
    };
    if let Some(matched) = crate::match_caller_binding_argument_names(formals, args)
        && (!matched.unmatched_named.is_empty()
            || matched.param_for_arg.iter().any(Option::is_none)
            || (0..formals.len()).any(|index| {
                matched
                    .param_for_arg
                    .iter()
                    .filter(|formal| **formal == Some(index))
                    .count()
                    > 1
            }))
    {
        return false;
    }
    match installer_environment_arg(primitive, args) {
        Ok(Some(environment)) => !definitely_fresh_installer_env(environment),
        Ok(None) | Err(()) => true,
    }
}

/// The installer target is distinct from the caller's frame when an
/// explicitly qualified constructor is the returned environment value.
/// A preceding literal function definition only constructs a value; its body
/// is deferred. Other block statements may affect the caller and cannot
/// certify the negative path here.
pub(crate) fn definitely_fresh_installer_env(expression: &Expr) -> bool {
    match expression {
        Expr::Call { func, .. } if ident_name(func) == Some("base::new.env") => true,
        Expr::Call { func, args, .. }
            if ident_name(func).is_some_and(|name| {
                matches!(name, "base::identity" | "base::invisible" | "base::force")
            }) && args.len() == 1
                && args[0].name.as_deref().is_none_or(|name| name == "x") =>
        {
            definitely_fresh_installer_env(&args[0].value)
        }
        Expr::Block { body, .. } => {
            let Some((last, prefix)) = body.split_last() else {
                return false;
            };
            prefix.iter().all(|statement| {
                matches!(
                    statement,
                    Stmt::Assign {
                        target: Expr::Ident { .. },
                        value: Expr::Function { .. },
                        ..
                    }
                )
            }) && matches!(last, Stmt::Expr(value) if definitely_fresh_installer_env(value))
        }
        _ => false,
    }
}

fn note_do_call_sources(
    sources: impl IntoIterator<Item = String>,
    callees: &mut HashSet<String>,
    indirect_call: &mut bool,
) {
    for source in sources {
        if matches!(
            source.as_str(),
            "delayedAssign"
                | "makeActiveBinding"
                | "base::delayedAssign"
                | "base::makeActiveBinding"
                | UNKNOWN_CALLER_BINDING_IDENTITY
        ) {
            *indirect_call = true;
        } else {
            callees.insert(source);
        }
    }
}

/// A repeated body can call today's value before a later assignment becomes
/// tomorrow's target. The source-order pass below sees only one iteration;
/// withdraw its negative proof when the body can write a callable target.
/// A literal atomic for-sequence has at most one element. A spelled `1:1`
/// cannot certify that fact because R permits a masked `:` operator.
fn loop_can_carry_do_call_target(
    body: &[Stmt],
    repeated_condition: Option<&Expr>,
    remaining: &mut usize,
) -> bool {
    let policy = Walk {
        assign_targets: false,
        assign_operands: true,
        dollar_args: false,
        fn_bodies: false,
        control_tests: true,
    };
    let mut targets = HashSet::new();
    let mut possible_writes = HashSet::new();
    let mut exhausted = false;
    let mut visit = |node: AstNode<'_>, _| {
        if *remaining == 0 {
            exhausted = true;
            return ControlFlow::<(), Descend>::Break(());
        }
        *remaining -= 1;
        if let AstNode::Expr(Expr::Call { func, args, .. }) = node
            && ident_name(func).is_some_and(|name| bare_name(name) == "do.call")
        {
            match crate::match_caller_binding_argument_names(
                &["what", "args", "quote", "envir"],
                args,
            ) {
                Some(matched) => {
                    if let Some(target) = matched.arg_for_param(0).and_then(|index| args.get(index))
                    {
                        targets.extend(global_caller_binding_value_sources(&target.value, 64));
                    }
                }
                None => exhausted = true,
            }
        }
        let assignment = match node {
            AstNode::Stmt(Stmt::Assign { target, value, .. }) => Some((target, value)),
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::Assign,
                lhs,
                rhs,
                ..
            }) => Some((lhs.as_ref(), rhs.as_ref())),
            _ => None,
        };
        if let Some((Expr::Ident { name, .. }, value)) = assignment
            && !global_caller_binding_value_sources(value, 64).is_empty()
        {
            if let Some(name) = caller_binding_identity(name) {
                possible_writes.insert(name);
            } else {
                exhausted = true;
            }
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    };
    if let Some(condition) = repeated_condition {
        let _ = walk_expr(condition, policy, &mut visit);
    }
    let _ = walk_stmts(body, policy, &mut visit);
    exhausted || !targets.is_disjoint(&possible_writes)
}

/// Follow local `do.call` target assignments in source order, in this
/// helper's lexical body. A later overwrite cannot affect an earlier call,
/// and a same-named assignment in an uncalled nested function is not local
/// provenance for that call. Control-flow assignments merge conservatively.
fn local_do_call_effects(
    body: &[Stmt],
    aliases: &mut HashMap<String, HashSet<String>>,
    formal_names: &HashSet<String>,
    callees: &mut HashSet<String>,
    indirect_call: &mut bool,
    remaining: &mut usize,
) {
    let policy = Walk {
        assign_targets: false,
        assign_operands: true,
        dollar_args: false,
        fn_bodies: false,
        control_tests: true,
    };
    for statement in body {
        if *remaining == 0 {
            *indirect_call = true;
            return;
        }
        // Count block nesting as well as visited AST nodes so a chain of
        // single-statement blocks cannot bypass the total-work limit.
        *remaining -= 1;
        match statement {
            Stmt::For { iter, body, .. }
                if !matches!(
                    iter,
                    Expr::Integer(..)
                        | Expr::Double(..)
                        | Expr::Logical(..)
                        | Expr::String(..)
                        | Expr::Na(..)
                        | Expr::Null(..)
                ) =>
            {
                *indirect_call |= loop_can_carry_do_call_target(body, None, remaining);
            }
            Stmt::While { cond, body, .. } => {
                *indirect_call |= loop_can_carry_do_call_target(body, Some(cond), remaining);
            }
            _ => {}
        }
        if let Stmt::Expr(Expr::Block { body, .. }) = statement {
            local_do_call_effects(
                body,
                aliases,
                formal_names,
                callees,
                indirect_call,
                remaining,
            );
            continue;
        }
        let direct_assignment = match statement {
            Stmt::Assign { target, value, .. } => Some((target, value)),
            Stmt::Expr(Expr::BinOp {
                op: BinOpKind::Assign,
                lhs,
                rhs,
                ..
            }) => Some((lhs.as_ref(), rhs.as_ref())),
            _ => None,
        };
        {
            let mut visit = |node: AstNode<'_>, _| {
                if *remaining == 0 {
                    *indirect_call = true;
                    return ControlFlow::<(), Descend>::Break(());
                }
                *remaining -= 1;
                if let AstNode::Expr(Expr::Call { func, args, .. }) = node {
                    if ident_name(func).is_some_and(|name| bare_name(name) == "do.call") {
                        match crate::match_caller_binding_argument_names(
                            &["what", "args", "quote", "envir"],
                            args,
                        ) {
                            Some(matched) => {
                                if let Some(target) =
                                    matched.arg_for_param(0).and_then(|index| args.get(index))
                                {
                                    let sources =
                                        global_caller_binding_value_sources(&target.value, 64);
                                    note_do_call_sources(
                                        local_value_sources_at_point(
                                            sources,
                                            aliases,
                                            formal_names,
                                            false,
                                        ),
                                        callees,
                                        indirect_call,
                                    );
                                }
                            }
                            None => *indirect_call = true,
                        }
                    }
                }
                let assignment = match node {
                    AstNode::Stmt(Stmt::Assign { target, value, .. }) => Some((target, value)),
                    AstNode::Expr(Expr::BinOp {
                        op: BinOpKind::Assign,
                        lhs,
                        rhs,
                        ..
                    }) => Some((lhs.as_ref(), rhs.as_ref())),
                    _ => None,
                };
                if let Some((Expr::Ident { name, .. }, value)) = assignment {
                    if let Some(name) = caller_binding_identity(name) {
                        let sources = local_value_sources_at_point(
                            global_caller_binding_value_sources(value, 64),
                            aliases,
                            formal_names,
                            true,
                        );
                        aliases.entry(name).or_default().extend(sources);
                    } else {
                        *indirect_call = true;
                    }
                }
                ControlFlow::<(), Descend>::Continue(Descend::Into)
            };
            if let Some((_, value)) = direct_assignment {
                let _ = walk_expr(value, policy, &mut visit);
            } else {
                let _ = walk_stmt(statement, policy, &mut visit);
            }
        }
        if let Some((Expr::Ident { name, .. }, value)) = direct_assignment {
            if let Some(name) = caller_binding_identity(name) {
                let sources = local_value_sources_at_point(
                    global_caller_binding_value_sources(value, 64),
                    aliases,
                    formal_names,
                    true,
                );
                aliases.insert(name, sources);
            } else {
                *indirect_call = true;
            }
        }
    }
}

/// A small caller-effect summary, collected once with the function body.
/// `parent.frame()` or an environment supplied through a formal can pass the
/// caller's frame to a binding installer, including through local value
/// flows. Defaults that may be forced are executable code; unused defaults
/// are not.
/// Direct calls let wrappers inherit this conservative summary. A local
/// installer without a caller-frame route does not taint callers.
pub(crate) fn helper_caller_binding_summary(
    params: &[Param],
    body: &[Stmt],
) -> (
    bool,
    Vec<String>,
    Vec<String>,
    Vec<CallerBindingCallbackCall>,
) {
    // Share one budget through recursively reached function-valued defaults.
    // Exhaustion must withdraw the negative binding-effect proof.
    let mut remaining_default_bodies = 64;
    helper_caller_binding_summary_bounded(params, body, &mut remaining_default_bodies)
}

/// A one-call helper can be certified harmless for a fresh actual only when
/// its sole installer target is a formal and all other actual expressions
/// are closed values. Bare `c()` additionally needs its base identity at the
/// caller; a lexical mask can execute arbitrary code while forming `value`.
pub(crate) fn helper_fresh_target_formal(
    params: &[String],
    body: &[Stmt],
) -> Option<(String, bool)> {
    fn pure_value(value: &Expr) -> Option<bool> {
        match value {
            Expr::Null(_)
            | Expr::Na(_, _)
            | Expr::Logical(_, _)
            | Expr::Integer(_, _)
            | Expr::Double(_, _)
            | Expr::String(_, _) => Some(false),
            Expr::Call { func, args, .. } if matches!(ident_name(func), Some("c" | "base::c")) => {
                let mut needs_base_c = ident_name(func) == Some("c");
                for arg in args {
                    needs_base_c |= pure_value(&arg.value)?;
                }
                Some(needs_base_c)
            }
            _ => None,
        }
    }
    let [Stmt::Expr(Expr::Call { func, args, .. })] = body else {
        return None;
    };
    let name = ident_name(func)?;
    if !crate::semantic_lists::is_base_qualified(name) || !is_caller_binding_installer_source(name)
    {
        return None;
    }
    let environment = installer_environment_arg(name, args).ok().flatten()?;
    let Expr::Ident { name: formal, .. } = environment else {
        return None;
    };
    let formal = caller_binding_identity(formal)?;
    if !params.contains(&formal) || formal == "..." || params.iter().any(|name| name == "c") {
        return None;
    }
    let mut needs_base_c = false;
    for arg in args {
        if std::ptr::eq(&arg.value, environment) {
            continue;
        }
        needs_base_c |= pure_value(&arg.value)?;
    }
    Some((formal, needs_base_c))
}

pub(crate) fn helper_forwarded_installer(params: &[String], body: &[Stmt]) -> Option<String> {
    if params != ["..."] {
        return None;
    }
    let [Stmt::Expr(Expr::Call { func, args, .. })] = body else {
        return None;
    };
    let name = ident_name(func)?;
    (crate::semantic_lists::is_base_qualified(name)
        && is_caller_binding_installer_source(name)
        && matches!(args.as_slice(), [Arg { name: None, value: Expr::Ident { name, .. }, .. }] if name == "..."))
    .then(|| name.to_string())
}

pub(crate) fn local_caller_binding_function(
    params: &[Param],
    body: &[Stmt],
    definition: Span,
) -> LocalCallerBindingFunction {
    let (may_install, _, called_formals, _) = helper_caller_binding_summary(params, body);
    let names: Vec<_> = params
        .iter()
        .map(|param| caller_binding_identity(&param.name).unwrap_or_default())
        .collect();
    let fresh_target = helper_fresh_target_formal(&names, body);
    LocalCallerBindingFunction {
        params: names.clone(),
        may_install,
        called_formals,
        fresh_target_formal: fresh_target.as_ref().map(|(formal, _)| formal.clone()),
        fresh_target_needs_base_c: fresh_target.is_some_and(|(_, needs)| needs),
        forwarded_installer: helper_forwarded_installer(&names, body),
        definition,
    }
}

fn helper_caller_binding_summary_bounded(
    params: &[Param],
    body: &[Stmt],
    remaining_default_bodies: &mut usize,
) -> (
    bool,
    Vec<String>,
    Vec<String>,
    Vec<CallerBindingCallbackCall>,
) {
    let default_names: HashSet<String> = params
        .iter()
        .filter(|param| param.default.is_some())
        .filter_map(|param| caller_binding_identity(&param.name))
        .collect();
    let direct_walk = Walk {
        assign_targets: false,
        assign_operands: false,
        dollar_args: false,
        fn_bodies: false,
        control_tests: true,
    };
    // A default can itself force another default. Direct reads are a useful
    // positive signal, but their absence is not proof of non-evaluation:
    // `get("done")` and a called local closure can force `done` without a
    // direct identifier read in this function's body. Unknown calls leave
    // every default potentially forced. A body made only of known ordinary
    // value forms (such as `invisible(NULL)`) keeps unused defaults lazy.
    let mut pending = Vec::new();
    let note_default_force = |node: AstNode<'_>, pending: &mut Vec<String>| match node {
        AstNode::Expr(Expr::Ident { name, .. })
            if caller_binding_identity(name).is_some_and(|name| default_names.contains(&name)) =>
        {
            pending.push(caller_binding_identity(name).unwrap());
        }
        AstNode::Expr(Expr::Call { func, .. })
            if !ident_name(func).is_some_and(|name| {
                matches!(
                    name,
                    "base::invisible"
                        | "base::return"
                        | "base::force"
                        | "base::parent.frame"
                        | "base::environment"
                        | "base::new.env"
                )
            }) =>
        {
            pending.extend(default_names.iter().cloned());
        }
        _ => {}
    };
    let _ = walk_stmts(body, direct_walk, |node, _| {
        note_default_force(node, &mut pending);
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    });
    let mut forced_defaults = HashSet::new();
    while let Some(name) = pending.pop() {
        if !forced_defaults.insert(name.clone()) {
            continue;
        }
        if let Some(default) = params
            .iter()
            .find(|param| caller_binding_identity(&param.name).as_deref() == Some(name.as_str()))
            .and_then(|param| param.default.as_ref())
        {
            let _ = walk_expr(default, direct_walk, |node, _| {
                note_default_force(node, &mut pending);
                ControlFlow::<(), Descend>::Continue(Descend::Into)
            });
        }
    }

    let mut call_sites = Vec::new();
    let mut callees = HashSet::new();
    let mut potential_callback_arguments = Vec::new();
    let mut local_aliases: HashMap<String, HashSet<String>> = HashMap::new();
    let mut do_call_targets = Vec::new();
    let mut indirect_call = false;
    let mut collect_effect = |node: AstNode<'_>, _| {
        let assignment = match node {
            AstNode::Stmt(Stmt::Assign { target, value, .. }) => Some((target, value)),
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::Assign,
                lhs,
                rhs,
                ..
            }) => Some((lhs.as_ref(), rhs.as_ref())),
            _ => None,
        };
        if let Some((Expr::Ident { name: target, .. }, value)) = assignment {
            match (
                caller_binding_identity(target),
                caller_binding_value_sources(value),
            ) {
                (Some(target), Some(sources)) => {
                    local_aliases.entry(target).or_default().extend(sources);
                }
                _ => indirect_call = true,
            }
        }
        if let AstNode::Expr(Expr::Call { func, args, span }) = node {
            if ident_name(func).is_some_and(|name| bare_name(name) == "do.call") {
                match crate::match_caller_binding_argument_names(
                    &["what", "args", "quote", "envir"],
                    args,
                ) {
                    Some(matched) => {
                        if let Some(target) =
                            matched.arg_for_param(0).and_then(|index| args.get(index))
                        {
                            do_call_targets
                                .push(global_caller_binding_value_sources(&target.value, 64));
                        }
                    }
                    None => indirect_call = true,
                }
            }
            if let Some(name) = ident_name(func).and_then(caller_binding_identity) {
                // An unmodeled callee can invoke a function-valued argument
                // (`do.call(act, list())`, callbacks, dispatch). Passing a
                // formal to a known value-only base wrapper does not invoke
                // it, so that route stays available as a quiet control.
                if !matches!(
                    name.as_str(),
                    "base::invisible"
                        | "base::return"
                        | "base::identity"
                        | "base::force"
                        | "base::list"
                        | "base::is.function"
                ) && !(crate::semantic_lists::is_base_qualified(&name)
                    && is_caller_binding_installer_source(&name))
                {
                    for arg in args {
                        match caller_binding_value_sources(&arg.value) {
                            Some(sources) => {
                                potential_callback_arguments.extend(
                                    sources.into_iter().map(|passed| (name.clone(), passed)),
                                );
                            }
                            None => indirect_call = true,
                        }
                    }
                }
                callees.insert(name.clone());
                call_sites.push((span.start, name, args.clone()));
            } else if !matches!(func.as_ref(), Expr::Function { .. }) {
                // A computed function value can be a known installer reached
                // through `get("install")`, indexing, or another expression.
                indirect_call = true;
            }
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    };
    let _ = walk_stmts(body, Walk::ALL, &mut collect_effect);
    for param in params {
        if caller_binding_identity(&param.name).is_some_and(|name| forced_defaults.contains(&name))
            && let Some(default) = &param.default
        {
            let _ = walk_expr(default, direct_walk, &mut collect_effect);
        }
    }
    if params
        .iter()
        .any(|param| caller_binding_identity(&param.name).is_none())
        && !call_sites.is_empty()
    {
        indirect_call = true;
    }
    for param in params {
        if let Some(Expr::Ident { name, .. }) = &param.default {
            match (
                caller_binding_identity(&param.name),
                caller_binding_identity(name),
            ) {
                (Some(target), Some(source)) => {
                    local_aliases.entry(target).or_default().insert(source);
                }
                _ => indirect_call = true,
            }
        }
    }
    // A target found in a nested/default body still keeps its prior global
    // conservative classification. The lexical pass adds local assignment
    // provenance for directly executed calls, including string values that
    // become callable only when passed as `what`.
    for sources in &do_call_targets {
        note_do_call_sources(sources.iter().cloned(), &mut callees, &mut indirect_call);
    }
    if !do_call_targets.is_empty() {
        let mut local_aliases_at_call = HashMap::new();
        let mut remaining = 4096;
        let formal_names: HashSet<String> = params
            .iter()
            .filter_map(|param| caller_binding_identity(&param.name))
            .collect();
        local_do_call_effects(
            body,
            &mut local_aliases_at_call,
            &formal_names,
            &mut callees,
            &mut indirect_call,
            &mut remaining,
        );
    }
    // A local function-valued alias may be called long after the assignment,
    // including through another alias. Retain every syntactically possible
    // source: a conditional assignment cannot justify discarding a route.
    let expand_aliases = |callees: &mut HashSet<String>| {
        let mut pending_aliases: Vec<_> = callees.iter().cloned().collect();
        while let Some(callee) = pending_aliases.pop() {
            if let Some(sources) = local_aliases.get(&callee) {
                for source in sources {
                    if callees.insert(source.clone()) {
                        pending_aliases.push(source.clone());
                    }
                }
            }
        }
    };
    let formal_names: HashSet<String> = params
        .iter()
        .filter_map(|param| caller_binding_identity(&param.name))
        .collect();
    let has_dots = formal_names.contains("...");
    for (receiver, passed) in potential_callback_arguments {
        let mut receiver_sources = HashSet::from([receiver]);
        expand_aliases(&mut receiver_sources);
        // Calling a formal already makes its supplied value an effect route.
        // Its other arguments are not themselves callbacks merely because
        // that formal might call them; a pure supplied callable cannot do so.
        if receiver_sources.is_disjoint(&formal_names) {
            let mut passed_sources = HashSet::from([passed]);
            expand_aliases(&mut passed_sources);
            callees.extend(passed_sources.into_iter().filter(|source| {
                formal_names.contains(source) || (has_dots && variadic_callable_source(source))
            }));
        }
    }
    // A function literal in a default is just a value until the helper calls
    // that formal. At that point its body executes with access to the outer
    // environment. A later default can invoke one declared earlier, so keep
    // discovering callees until no unvisited function default is reachable.
    let mut visited_defaults = HashSet::new();
    let mut nested_callback_calls = Vec::new();
    loop {
        expand_aliases(&mut callees);
        let next = params.iter().enumerate().find(|(index, param)| {
            !visited_defaults.contains(index)
                && caller_binding_identity(&param.name).is_some_and(|name| callees.contains(&name))
                && matches!(&param.default, Some(Expr::Function { .. }))
        });
        let Some((index, param)) = next else {
            break;
        };
        if *remaining_default_bodies == 0 {
            indirect_call = true;
            break;
        }
        *remaining_default_bodies -= 1;
        visited_defaults.insert(index);
        if let Some(Expr::Function {
            params: inner_params,
            body: inner_body,
            ..
        }) = &param.default
        {
            let (effect, nested_callees, nested_called_formals, callback_calls) =
                helper_caller_binding_summary_bounded(
                    inner_params,
                    inner_body,
                    remaining_default_bodies,
                );
            indirect_call |= effect || !nested_called_formals.is_empty();
            callees.extend(nested_callees);
            nested_callback_calls.extend(callback_calls);
        }
    }
    let mut installer_environments = Vec::new();
    for (start, name, args) in &call_sites {
        if name.contains("::")
            && !name.starts_with("base::")
            && args.iter().any(|arg| match &arg.value {
                Expr::Ident { name, .. } => {
                    let Some(name) = caller_binding_identity(name) else {
                        return true;
                    };
                    let mut sources = HashSet::from([name]);
                    expand_aliases(&mut sources);
                    params.iter().any(|param| {
                        caller_binding_identity(&param.name)
                            .is_some_and(|name| sources.contains(&name))
                    })
                }
                Expr::Call { func, .. } => ident_name(func)
                    .is_some_and(|name| matches!(bare_name(name), "environment" | "parent.frame")),
                _ => false,
            })
        {
            // Without a package identity, `other::install(env)` cannot be
            // equated to this project's `install`; but a qualified call
            // receiving a possibly caller-owned frame can still replace
            // the caller's binding. Keep the effect uncertain instead of
            // discarding the route from the local wrapper summary.
            indirect_call = true;
        }
        let mut possible_names = vec![name.as_str()];
        let mut seen = HashSet::new();
        while let Some(candidate) = possible_names.pop() {
            if !seen.insert(candidate) {
                continue;
            }
            match installer_environment_arg(candidate, args) {
                Ok(Some(environment)) => {
                    installer_environments.push((*start, environment.clone()));
                }
                Err(()) => indirect_call = true,
                Ok(None) => {}
            }
            if let Some(sources) = local_aliases.get(candidate) {
                possible_names.extend(sources.iter().map(String::as_str));
            }
        }
    }
    // Certify an identifier only when a straight-line statement has made a
    // fresh local frame before the installer call. A conditional assignment,
    // unknown call, or later write leaves the value uncertain.
    let mut straight_local = HashSet::new();
    let mut certified_calls = HashSet::new();
    for statement in body {
        match statement {
            Stmt::Assign {
                target: Expr::Ident { name, .. },
                value,
                ..
            } => {
                if definitely_local_installer_env(value, &straight_local) {
                    straight_local.insert(name.clone());
                } else {
                    if !matches!(
                        value,
                        Expr::Integer(_, _)
                            | Expr::Double(_, _)
                            | Expr::Logical(_, _)
                            | Expr::String(_, _)
                            | Expr::Null(_)
                            | Expr::Na(_, _)
                    ) {
                        straight_local.clear();
                    }
                    straight_local.remove(name);
                }
            }
            Stmt::Expr(Expr::Call { func, args, span }) => {
                if let Some(name) = ident_name(func)
                    && let Ok(Some(environment)) = installer_environment_arg(bare_name(name), args)
                    && definitely_local_installer_env(environment, &straight_local)
                {
                    certified_calls.insert(span.start);
                }
                straight_local.clear();
            }
            _ => straight_local.clear(),
        }
    }
    let uncertain_installer_environment =
        installer_environments.iter().any(|(start, environment)| {
            !certified_calls.contains(start)
                && !definitely_local_installer_env(environment, &HashSet::new())
        });
    let mut called_formals: Vec<String> = params
        .iter()
        .filter_map(|param| {
            let name = caller_binding_identity(&param.name)?;
            callees.contains(&name).then_some(name)
        })
        .collect();
    if params.iter().any(|param| param.name == "...") {
        called_formals.extend(
            callees
                .iter()
                .filter(|name| variadic_callable_source(name))
                .cloned(),
        );
        called_formals.sort_unstable();
        called_formals.dedup();
    }
    let mut callback_calls = nested_callback_calls;
    for (_, callee, args) in call_sites {
        let mut possible = HashSet::from([callee]);
        expand_aliases(&mut possible);
        if possible.len() > 128 || callback_calls.len().saturating_add(possible.len()) > 256 {
            indirect_call = true;
            break;
        }
        let forwarded_only: Vec<bool> = args
            .iter()
            .map(|arg| {
                let sources = global_caller_binding_value_sources(&arg.value, 64);
                let sources = expand_block_value_sources(sources, &local_aliases);
                !sources.is_empty() && sources.iter().all(|source| formal_names.contains(source))
            })
            .collect();
        callback_calls.extend(
            possible
                .into_iter()
                .map(|callee| CallerBindingCallbackCall {
                    callee,
                    args: args.clone(),
                    forwarded_only: forwarded_only.clone(),
                }),
        );
    }
    (
        uncertain_installer_environment || indirect_call,
        callees.into_iter().collect(),
        called_formals,
        callback_calls,
    )
}

impl Checker {
    pub(crate) fn collect_fns(&mut self, stmts: &[Stmt]) {
        // Project refinement has no single source file; carry escaped local
        // names and formals through the collected table as well as emission.
        if custom_operator::has_escaped_names(stmts) {
            Arc::make_mut(&mut self.fn_table).has_escaped_operator_names = true;
        }
        if custom_operator::has_escaped_slot_names(stmts) {
            Arc::make_mut(&mut self.fn_table).has_escaped_slot_names = true;
        }
        // Statement-level walk on the shared core: a binding statement can
        // appear at the top level, in `if` branches, and in `for`/`while`
        // bodies, so those are the only statements whose children this
        // traversal enters — never the tests (`control_tests` off), never
        // assignment values or expression interiors (a literal without a
        // binding name has nothing to record; literals inside a bound body
        // go through `collect_nested_fns_in_body`). The declared-globals
        // scan below still covers tests and expression interiors because
        // it walks each reached statement's whole subtree.
        let _ = walk_stmts(
            stmts,
            Walk {
                control_tests: false,
                ..Walk::ALL
            },
            |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                let AstNode::Stmt(statement) = node else {
                    // Expression interiors are not definition sites.
                    return ControlFlow::Continue(Descend::Skip);
                };
                self.collect_declared_globals_stmt(statement);
                if let Stmt::Assign { target, value, .. } = statement {
                    self.collect_fns_assign(target, value);
                }
                ControlFlow::Continue(match statement {
                    Stmt::If { .. } | Stmt::For { .. } | Stmt::While { .. } => Descend::Into,
                    _ => Descend::Skip,
                })
            },
        );
    }

    // Records one identifier-bound assignment from the collection
    // traversal: the bound name in `known_vars` (and `callable_vars` for
    // constructor calls), and, when the value is a function literal, the
    // function itself.
    fn collect_fns_assign(&mut self, target: &Expr, value: &Expr) {
        if !matches!(value, Expr::Function { .. })
            && let Expr::Ident { name: alias, .. } = target
        {
            let sources = global_caller_binding_value_sources(value, 64);
            if !sources.is_empty() {
                let table = Arc::make_mut(&mut self.fn_table);
                match caller_binding_identity(alias) {
                    Some(alias) => {
                        table
                            .caller_binding_aliases
                            .entry(alias)
                            .or_default()
                            .extend(sources);
                    }
                    None => table.caller_binding_unresolved_alias_target = true,
                }
            }
        }
        // Record every identifier-bound top-level assignment in
        // `known_vars`. This is independent of whether the RHS
        // is a function literal: regular variable assignments
        // (`my_const <- 42`, `GeomRect <- ggproto(...)`) need
        // to be resolvable from other files (and from later in
        // this same file) without triggering RY010.
        if let Some(name) = binding_name(target) {
            let table = Arc::make_mut(&mut self.fn_table);
            table.has_escaped_binding_names |= name.contains('\\');
            table.has_escaped_operator_names |=
                custom_operator::escaped_name_may_mask_operator(name);
            table.has_escaped_slot_names |= custom_operator::escaped_name_may_mask_slot(name);
            table.known_vars.insert(name.to_string());
            // Keep raw names for type/provenance lookup, but recognize an
            // ordinary read of a backtick-bound symbol as an existing value.
            // String assignment targets name their literal contents instead;
            // escaped identifiers require an R decoder before aliasing.
            if matches!(target, Expr::Ident { .. })
                && !name.contains('\\')
                && let Some(unquoted) = name.strip_prefix('`').and_then(|s| s.strip_suffix('`'))
            {
                table.known_vars.insert(unquoted.to_string());
            }
            if is_callable_object_constructor(value) {
                table.callable_vars.insert(name.to_string());
            } else {
                table.callable_vars.remove(name);
            }
        }
        if let (Some(name), Expr::Function { params, body, .. }) = (binding_name(target), value) {
            // An S3 method named like `print.foo` is recorded both
            // as a regular function (so the name resolves to its
            // return type if called directly) and as an S3 method
            // (so dispatch from `print(x)` on a classed value
            // finds it). We record the body once and share the
            // return slot between both entries.
            //
            // Group generics are unambiguous and may dispatch through
            // `...` alone (notably `Summary.foo <- function(...)`).
            // Other dotted names retain the first-parameter heuristic
            // so ordinary helpers are not misregistered as methods.
            let semantic_name = semantic_argument_name(name);
            let looks_like_s3 = split_s3_method_name(semantic_name, &self.typeshed.globals)
                .or_else(|| {
                    split_s3_operator_method_name(semantic_name)
                        .map(|(generic, class)| (generic.to_string(), class))
                })
                .filter(|(generic, _)| {
                    crate::semantic_lists::is_group_generic(generic)
                        || params.first().is_some_and(|p| {
                            p.name == "x"
                                || (is_operator_generic(generic.as_str())
                                    && matches!(p.name.as_str(), "e1" | "e2"))
                        })
                });
            if let Some((generic, class)) = looks_like_s3 {
                let slot = self.record_fn(name.to_string(), params, body.clone());
                Arc::make_mut(&mut self.fn_table)
                    .s3_methods
                    .insert((generic.to_string(), class), slot);
            } else {
                let _ = self.record_fn(name.to_string(), params, body.clone());
            }
            self.collect_forwarded_calls(name, params, body);
            self.collect_nested_fns_in_body(name, body);
        }
    }

    // Full-subtree scan, run once per statement the collection traversal
    // reaches, for table entries that live outside lexical scope:
    // syntactic call sites, `globalVariables()` declarations,
    // namespace-targeted `assign()`, and S4 registration calls. R
    // evaluates every position this walk enters — control tests, `$`
    // subscript arguments, nested function bodies — so the policy is
    // `Walk::ALL` with no skip rules at all.
    fn collect_declared_globals_stmt(&mut self, s: &Stmt) {
        let _ = walk_stmt(
            s,
            Walk::ALL,
            |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                if let AstNode::Expr(Expr::Call { func, args, .. }) = node
                    && let Expr::Ident { name, .. } = func.as_ref()
                {
                    let bare = bare_name(name);
                    Arc::make_mut(&mut self.fn_table)
                        .call_sites
                        .entry(bare.to_string())
                        .or_default()
                        .push(args.iter().map(|argument| argument.name.clone()).collect());
                    if bare == "globalVariables" {
                        if let Some(first) = args.first() {
                            for declared in string_literals(&first.value) {
                                let table = Arc::make_mut(&mut self.fn_table);
                                table.has_escaped_binding_names |= declared.contains('\\');
                                table.has_escaped_operator_names |=
                                    custom_operator::escaped_name_may_mask_operator(&declared);
                                table.has_escaped_slot_names |=
                                    custom_operator::escaped_name_may_mask_slot(&declared);
                                table.known_vars.insert(declared);
                            }
                        }
                    }
                    if bare == "assign"
                        && args.iter().any(|arg| {
                            arg.name.as_deref() == Some("envir")
                                && matches!(
                                    &arg.value,
                                    Expr::Call { func, .. }
                                        if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "asNamespace")
                                )
                        })
                        && let Some(first) = args.first()
                        && let Some(binding) = string_literal(&first.value)
                    {
                        let table = Arc::make_mut(&mut self.fn_table);
                        table.has_escaped_binding_names |= binding.contains('\\');
                        table.has_escaped_operator_names |= custom_operator::escaped_name_may_mask_operator(binding);
                        table.has_escaped_slot_names |= custom_operator::escaped_name_may_mask_slot(binding);
                        table.known_vars.insert(binding.to_string());
                    }
                    self.collect_s4_call(bare, args);
                }
                ControlFlow::Continue(Descend::Into)
            },
        );
    }

    fn collect_s4_call(&mut self, name: &str, args: &[Arg]) {
        match name {
            "setGeneric" => {
                if let Some(generic) = args.first().and_then(|arg| string_literal(&arg.value)) {
                    let table = Arc::make_mut(&mut self.fn_table);
                    table.has_escaped_binding_names |= generic.contains('\\');
                    table.has_escaped_operator_names |=
                        custom_operator::escaped_name_may_mask_operator(generic);
                    table.has_escaped_slot_names |=
                        custom_operator::escaped_name_may_mask_slot(generic);
                    table.known_vars.insert(generic.to_string());
                }
            }
            "setMethod" => {
                let Some(generic) = args.first().and_then(|arg| string_literal(&arg.value)) else {
                    return;
                };
                let Some(class) = args.get(1).and_then(|arg| s4_signature_class(&arg.value)) else {
                    return;
                };
                let Some(Expr::Function { params, body, .. }) = args
                    .iter()
                    .skip(2)
                    .find(|arg| matches!(arg.value, Expr::Function { .. }))
                    .map(|arg| &arg.value)
                else {
                    return;
                };
                let method_name = format!("__s4__{generic}__{class}");
                let slot = self.record_fn(method_name.clone(), params, body.clone());
                if let Some(first) = Arc::make_mut(&mut self.fn_table)
                    .fns
                    .get_mut(&method_name)
                    .and_then(|function| function.params.first_mut())
                {
                    first.type_ = RType::unknown().with_class(ClassVector::single(&class));
                }
                Arc::make_mut(&mut self.fn_table)
                    .s4_methods
                    .insert((generic.to_string(), class), slot);
            }
            _ => {}
        }
    }

    fn collect_forwarded_calls(&mut self, caller: &str, params: &[Param], body: &[Stmt]) {
        let mut calls = Vec::new();
        collect_forwarded_calls_in_stmts(caller, params, body, &mut calls);
        Arc::make_mut(&mut self.fn_table)
            .forwarded_calls
            .extend(calls);
    }

    // Walk a function body looking for `inner <- function(...) ...`
    // definitions and record them with the mangled name
    // `<outer>$<inner>`. The mangled name is internal: it exists so
    // the fixpoint can refine the inner function's return type, which
    // `refine_fn_return` reads back when building the outer function's
    // `fn_sig`. Callers that close over the inner function via a
    // captured `Function`-typed value go through that `fn_sig`, not
    // through the table entry. Users never see the mangled name.
    //
    // Recursion is bounded by the AST's literal nesting (small in
    // practice). The inference depth is separately bounded by
    // `MAX_CLOSURE_DEPTH` in `build_function_signature`.
    //
    // Like `collect_fns`, this is a statement-level walk on the shared
    // core: nested definitions are recorded wherever a binding statement
    // can appear — `if` branches and `for`/`while` bodies — never in
    // control tests or expression interiors.
    fn collect_nested_fns_in_body(&mut self, outer: &str, body: &[Stmt]) {
        let _ = walk_stmts(
            body,
            Walk {
                control_tests: false,
                ..Walk::ALL
            },
            |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                let AstNode::Stmt(statement) = node else {
                    return ControlFlow::Continue(Descend::Skip);
                };
                if let Stmt::Assign { target, value, .. } = statement
                    && let (
                        Expr::Ident { name: inner, .. },
                        Expr::Function {
                            params,
                            body: inner_body,
                            ..
                        },
                    ) = (target, value)
                {
                    let mangled = format!("{}${}", outer, inner);
                    let next_outer = mangled.clone();
                    let _ = self.record_fn(mangled, params, inner_body.clone());
                    // Recurse one more level so doubly-nested factories
                    // are also collected.
                    self.collect_nested_fns_in_body(&next_outer, inner_body);
                }
                ControlFlow::Continue(match statement {
                    Stmt::If { .. } | Stmt::For { .. } | Stmt::While { .. } => Descend::Into,
                    _ => Descend::Skip,
                })
            },
        );
    }

    // Record a user-defined function. Returns the index of the
    // allocated return slot so callers can wire up S3 dispatch entries
    // that share the same slot.
    pub(crate) fn record_fn(&mut self, name: String, params: &[Param], body: Vec<Stmt>) -> usize {
        let (
            may_install_caller_binding,
            caller_binding_callees,
            caller_binding_called_formals,
            caller_binding_callback_calls,
        ) = helper_caller_binding_summary(params, &body);
        // We infer param types from defaults alone; params without a
        // default start as UNKNOWN (callers can refine them later).
        let params: Vec<UserParam> = params
            .iter()
            .map(|p| {
                let t = match &p.default {
                    // Defer inference to first fixpoint iteration by
                    // starting as UNKNOWN; if a literal default is present
                    // we can compute it now without a scope.
                    Some(e) => infer_literal_default(e),
                    None => RType::unknown(),
                };
                let required =
                    p.name != "..." && p.default.is_none() && block_force_flow(&body, &p.name).0;
                UserParam {
                    name: p.name.clone(),
                    type_: t,
                    required,
                    defused: parameter_is_defused(&body, &p.name),
                    quoting: parameter_is_quoted(&body, params, &p.name),
                    injection: None,
                }
            })
            .collect();
        let slot = self.return_slots.0.len();
        Arc::make_mut(&mut self.return_slots).set(slot, RType::unknown());
        // Wrap the body in an Arc so the per-fixpoint clone in
        // refine_fn_return is a refcount bump, not a deep copy.
        let body: Arc<[Stmt]> = Arc::from(body);
        let fn_table = Arc::make_mut(&mut self.fn_table);
        fn_table.has_escaped_binding_names |= name.contains('\\');
        fn_table.has_escaped_operator_names |=
            custom_operator::escaped_name_may_mask_operator(&name);
        fn_table.has_escaped_slot_names |= custom_operator::escaped_name_may_mask_slot(&name);
        let prev = fn_table.fns.insert(
            name.clone(),
            UserFn {
                params,
                body,
                may_install_caller_binding,
                caller_binding_callees,
                caller_binding_called_formals,
                caller_binding_callback_calls: Arc::from(caller_binding_callback_calls),
                return_slot: slot,
            },
        );
        if let Some(prev) = prev {
            Arc::make_mut(&mut self.fn_table)
                .forwarded_calls
                .retain(|call| call.caller != name);
            tracing::debug!(fn_name = %name, prev_slot = prev.return_slot, "shadowed earlier def");
        }
        slot
    }

    // Pass 2: refine one function's inferred return type by walking its
    // body once. Returns are collected from `return(...)` calls and from
    // the trailing expression of the body, then joined.
    pub(crate) fn refine_fn_return(&mut self, name: &str) -> bool {
        // Pull the body out by reference so we can re-borrow self during
        // the walk. We can't simply clone the body since that's expensive
        // for large functions; instead we snapshot the slot index.
        let (body_clone, params, slot) = match self.fn_table.fns.get(name) {
            Some(f) => (f.body.clone(), f.params.clone(), f.return_slot),
            None => return false,
        };
        // Cycle detection: if this function is already on the inference
        // stack, leave its return as UNKNOWN and bail out. The fixpoint
        // will converge on subsequent iterations.
        if self.inferring.iter().any(|n| n == name) {
            return false;
        }
        #[cfg(test)]
        {
            *self.refinement_counts.entry(name.to_string()).or_default() += 1;
        }
        self.inferring.push(name.to_string());

        let mut scope = Scope::default();
        // Deferred execution can observe later syntax and constructor changes.
        scope.invalidate_ops_environment();
        for parameter in &params {
            scope.insert_parameter(parameter.name.clone(), parameter.type_.clone());
        }
        // The function's own name is in scope as a function value, so
        // recursive calls resolve to a user-fn lookup.
        scope.insert(name.to_string(), RType::scalar(Mode::Function));
        insert_s3_dispatch_context(name, &mut scope, &self.typeshed.globals);

        // Keep deferred expressions (notably `on.exit(expr)`) in the same
        // exit-time lexical context during fixpoint inference as during the
        // final diagnostic walk.
        self.deferred_captures
            .push(assigned_names_in_body(&body_clone));

        let mut returns: Vec<RType> = Vec::new();
        // Walk the body via the unified walker in discarding mode, with
        // return collection enabled. The discarding flag is set by the
        // caller (refine_fn_return runs inside the fixpoint which sets
        // discarding=true at the run_fixpoint entry).
        for s in body_clone.iter() {
            self.walk_stmt(s, &mut scope, Some(&mut returns));
        }
        // Trailing expression of a braced body is the implicit return.
        // A trailing `Stmt::FunctionDef` is the implicit return value
        // for the `function() { function() { 1L } }` shape;
        // `trailing_return_type` handles both forms and attaches an
        // inferred `fn_sig` when the trailing expression is itself a
        // function literal (the closure-factory pattern).
        if let Some(t) = self.trailing_return_type(&body_clone[..], &mut scope, 0) {
            returns.push(t);
        }

        // Fold the collected return types. We start from the first
        // element rather than UNKNOWN because join() treats Opaque as
        // absorbing (correct for control-flow merge but wrong for an
        // empty-fold identity).
        let joined = if returns.is_empty() {
            RType::unknown()
        } else {
            let mut iter = returns.into_iter();
            let first = iter.next().unwrap_or(RType::unknown());
            iter.fold(first, |acc, t| acc.join(t))
        };
        let changed = self.return_slots.0.get(slot) != Some(&joined);
        if changed {
            Arc::make_mut(&mut self.return_slots).set(slot, joined);
        }
        self.deferred_captures.pop();
        self.inferring.pop();
        changed
    }
}

/// Whether `expression` is a call to an S7 object constructor
/// (`S7::new_class()` and friends), whose result is a callable S7 class
/// or generic object rather than a plain value.
fn is_callable_object_constructor(expression: &Expr) -> bool {
    let Expr::Call { func, .. } = expression else {
        return false;
    };
    matches!(
        func.as_ref(),
        Expr::Ident { name, .. }
            if crate::semantic_lists::S7_OBJECT_CONSTRUCTORS.contains(&name.as_str())
    )
}

fn parameter_is_quoted(body: &[Stmt], params: &[Param], parameter: &str) -> bool {
    let quotes = |statement: &Stmt| stmt_any(statement, &|e| quotes_parameter(e, parameter));
    let captures_promise =
        |statement: &Stmt| stmt_any(statement, &|e| captures_promise_parameter(e, parameter));
    body.iter()
        .any(|statement| stmt_any(statement, &captures_all_arguments))
        || (params.iter().any(|formal| formal.name == parameter)
            && (body.iter().any(quotes)
                || (parameter == "..." && body.iter().any(captures_promise)))
            // Promise-capture helpers only make a promise safe to pass
            // unevaluated when that promise is not also used normally in
            // this function.  This preserves eager diagnostics for mixed
            // bodies such as a capture followed by `print(x)`.
            && !(body.iter().any(captures_promise)
                && parameter_has_normal_use(body, parameter)))
}

/// Generic statement walker shared by the quoting-capture predicates:
/// the shared ry-core walker in "does any expression satisfy `pred`"
/// mode. Tests `pred` at every expression node and short-circuits on
/// the first hit. Function bodies are opaque (skips `Expr::Function`
/// and `Stmt::FunctionDef` bodies): quoting inside a nested function
/// belongs to that function's own formals.
fn stmt_any(statement: &Stmt, pred: &impl Fn(&Expr) -> bool) -> bool {
    walk_stmt(
        statement,
        Walk {
            fn_bodies: false,
            ..Walk::ALL
        },
        |node, _| match node {
            AstNode::Expr(e) if pred(e) => ControlFlow::Break(()),
            _ => ControlFlow::Continue(Descend::Into),
        },
    )
    .is_break()
}

/// Expression half of `stmt_any`. The quoting-capture predicates all
/// early-return on non-call nodes, so testing at every node is
/// equivalent to testing at call nodes alone.
fn expr_any(expression: &Expr, pred: &impl Fn(&Expr) -> bool) -> bool {
    walk_expr(
        expression,
        Walk {
            fn_bodies: false,
            ..Walk::ALL
        },
        |node, _| match node {
            AstNode::Expr(e) if pred(e) => ControlFlow::Break(()),
            _ => ControlFlow::Continue(Descend::Into),
        },
    )
    .is_break()
}

/// Leaf predicate for `stmt_any`: the call reflects its complete call
/// site, so every formal of the enclosing function is captured.
fn captures_all_arguments(expression: &Expr) -> bool {
    matches!(
        expression,
        Expr::Call { func, .. }
            if matches!(ident_name(func).map(bare_name), Some("match.call" | "sys.call"))
    )
}

/// Leaf predicate for `stmt_any`: the call quotes `parameter` directly
/// (`substitute(x)`, a promise-capture helper) or unquotes it inside a
/// `bquote(...)` template.
fn quotes_parameter(expression: &Expr, parameter: &str) -> bool {
    let Expr::Call { func, args, .. } = expression else {
        return false;
    };
    captured_arguments(func, args)
        .iter()
        .zip(args)
        .any(|(capture, arg)| *capture && is_parameter(&arg.value, parameter))
        || (matches!(ident_name(func).map(bare_name), Some("bquote"))
            && args
                .iter()
                .any(|argument| bquote_references_parameter(&argument.value, parameter)))
}

/// Leaf predicate for `stmt_any`: the call captures `parameter`'s promise
/// without evaluating it.
fn captures_promise_parameter(expression: &Expr, parameter: &str) -> bool {
    let Expr::Call { func, args, .. } = expression else {
        return false;
    };
    captured_arguments(func, args)
        .iter()
        .zip(args)
        .any(|(capture, arg)| *capture && is_parameter(&arg.value, parameter))
}

/// Whether any use of `parameter` in `body` reads it as an ordinary value,
/// as opposed to a quoted, defused, or promise-captured use.
fn parameter_has_normal_use(body: &[Stmt], parameter: &str) -> bool {
    let mut uses = ParameterUses::default();
    for statement in body {
        collect_parameter_uses_in_stmt(statement, parameter, &mut uses);
    }
    uses.normal
}

/// Classify actuals using only the capturing formals of embedded signatures.
/// Bare names retain the existing inventory union, without lexical resolution.
/// Neither this index nor qualified lookup reads per-checker user stubs.
pub(crate) fn captured_arguments(function: &Expr, args: &[Arg]) -> Vec<bool> {
    let Some(name) = ident_name(function) else {
        return Vec::new();
    };
    if let Some((package, name)) = name.rsplit_once("::") {
        let package = package.trim_end_matches(':');
        let typeshed = if package == "base" {
            ry_typeshed::load_base_cached().ok()
        } else {
            ry_typeshed::load_package(package)
        };
        if let Some(signature) = typeshed.and_then(|db| db.functions.get(name))
            && signature
                .eval
                .values()
                .any(|mode| *mode == EvalMode::CapturesPromise)
        {
            let mut captured = vec![false; args.len()];
            capture_signature_arguments(signature, args, &mut captured);
            return captured;
        }
    } else if let Some(signatures) = promise_capture_index().get(name) {
        let mut captured = vec![false; args.len()];
        for signature in signatures {
            capture_signature_arguments(signature, args, &mut captured);
        }
        return captured;
    }
    Vec::new()
}

fn capture_signature_arguments(signature: &FunctionSig, args: &[Arg], captured: &mut [bool]) {
    if !signature
        .eval
        .values()
        .any(|mode| *mode == EvalMode::CapturesPromise)
    {
        return;
    }
    let all_capture = signature
        .params
        .iter()
        .all(|param| signature.eval.get(&param.name) == Some(&EvalMode::CapturesPromise));
    // Recovery spellings do not prove R argument names. Exact and partial
    // matching otherwise share the ordinary call matcher's occupancy map.
    if args
        .iter()
        .any(|arg| arg.name.as_deref().is_some_and(|name| name.contains('\\')))
    {
        // This does not validate escaped names. In any successful call to an
        // all-capture signature, no explicit actual can move to normal
        // evaluation. Preserve that fact without changing call diagnostics.
        if all_capture {
            for (index, arg) in args.iter().enumerate() {
                if !matches!(arg.value, Expr::Missing(_))
                    && !matches!(&arg.value, Expr::Ident { name, .. } if name == "...")
                {
                    captured[index] = true;
                }
            }
        }
        return;
    }
    let params: Vec<_> = signature.params.iter().map(|p| p.name.as_str()).collect();
    let names: Vec<_> = args
        .iter()
        .map(|a| a.name.as_deref().map(semantic_argument_name))
        .collect();
    // A forwarded dots expression expands to zero or more actuals, not one
    // concrete positional slot. Match only explicit actuals here; the proof
    // below separately limits which bindings survive the unknown expansion.
    let explicit: Vec<_> = args
        .iter()
        .enumerate()
        .filter(|(_, arg)| !matches!(&arg.value, Expr::Ident { name, .. } if name == "..."))
        .map(|(index, _)| index)
        .collect();
    let matched = match_argument_names(&params, explicit.iter().map(|&index| names[index]));
    let mut param_for_arg = vec![None; args.len()];
    for (&index, formal) in explicit.iter().zip(&matched.param_for_arg) {
        param_for_arg[index] = *formal;
    }
    let end = matched.dots.unwrap_or(params.len());
    let exact: Vec<_> = params.iter().map(|p| names.contains(&Some(*p))).collect();
    // The general matcher tolerates malformed calls. A capture proof cannot
    // borrow its fallback for duplicate bindings or ambiguous partial names.
    let mut occupied = vec![false; params.len()];
    for &index in &explicit {
        let name = &names[index];
        if let Some(formal) = param_for_arg[index] {
            if occupied[formal] {
                return;
            }
            occupied[formal] = true;
        } else if matched.dots.is_none() {
            return;
        }
        if let Some(name) = name {
            if name.is_empty() {
                return;
            }
            if !params.contains(name) {
                let candidates: Vec<_> = params[..end]
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| !exact[*i] && p.starts_with(name))
                    .map(|(i, _)| i)
                    .collect();
                if candidates.len() > 1
                    || candidates
                        .first()
                        .is_some_and(|formal| param_for_arg[index] != Some(*formal))
                {
                    return;
                }
            }
        }
    }
    let forwarded = args
        .iter()
        .any(|arg| matches!(&arg.value, Expr::Ident { name, .. } if name == "..."));
    for (index, arg) in args.iter().enumerate() {
        // Preserve the existing blanket forwarding approximation: a wrapper's
        // `...` remains quoting when passed to a capturing dots formal. Runtime
        // names can still bind normal controls (e.g. enquos(.named=...)); the
        // boolean collected mode cannot represent that per-actual distinction.
        // Explicit control actuals are matched normally below.
        if matches!(&arg.value, Expr::Ident { name, .. } if name == "...") {
            captured[index] |= matched.dots.is_some_and(|formal| {
                signature.eval.get(params[formal]) == Some(&EvalMode::CapturesPromise)
            });
            continue;
        }
        let exact_capture = names[index].is_some_and(|name| params.contains(&name));
        // Exact tags cannot change their formal in a successful call. An
        // all-capture signature cannot move a value to an ordinary formal.
        if matches!(arg.value, Expr::Missing(_))
            || (forwarded && end != 0 && !exact_capture && !all_capture)
        {
            continue;
        }
        if let Some(formal) = param_for_arg[index].or(matched.dots) {
            captured[index] |=
                signature.eval.get(params[formal]) == Some(&EvalMode::CapturesPromise);
        }
    }
}

/// Sparse one-time inventory: only signatures with capture metadata allocate
/// an entry. Multiple packages with the same bare name retain their union.
fn promise_capture_index() -> &'static std::collections::HashMap<String, Vec<&'static FunctionSig>>
{
    static INDEX: std::sync::OnceLock<
        std::collections::HashMap<String, Vec<&'static FunctionSig>>,
    > = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index: std::collections::HashMap<String, Vec<&'static FunctionSig>> =
            std::collections::HashMap::new();
        let mut add = |typeshed: &'static ry_typeshed::Typeshed| {
            for (name, signature) in &typeshed.functions {
                if signature
                    .eval
                    .values()
                    .any(|mode| *mode == EvalMode::CapturesPromise)
                {
                    index.entry(name.clone()).or_default().push(signature);
                }
            }
        };
        if let Ok(base) = ry_typeshed::load_base_cached() {
            add(base);
        }
        for package in ry_typeshed::known_packages() {
            // Skip stubs that cannot contribute: the prefilter keeps
            // embedded packages that declare no promise capture from
            // being parsed at all (they load lazily per package).
            if !ry_typeshed::package_has_captures_promise(package) {
                continue;
            }
            if let Some(typeshed) = ry_typeshed::load_package(package) {
                add(typeshed);
            }
        }
        index
    })
}

/// Whether a `.(parameter)` unquote anywhere inside the expression —
/// including inside braced statement blocks — references `parameter`.
fn bquote_references_parameter(expression: &Expr, parameter: &str) -> bool {
    expr_any(expression, &|e| unquotes_parameter(e, parameter))
}

/// Leaf predicate for `stmt_any`: the call is a bquote unquote `.(...)`
/// applied to `parameter` directly.
fn unquotes_parameter(expression: &Expr, parameter: &str) -> bool {
    matches!(
        expression,
        Expr::Call { func, args, .. }
            if matches!(ident_name(func).map(bare_name), Some("."))
                && args
                    .iter()
                    .any(|argument| is_parameter(&argument.value, parameter))
    )
}

fn is_parameter(expression: &Expr, parameter: &str) -> bool {
    matches!(expression, Expr::Ident { name, .. } if name == parameter)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FirstParameterUse {
    Defused,
    Normal,
}

fn parameter_is_defused(body: &[Stmt], parameter: &str) -> bool {
    if parameter == "..." {
        let mut uses = ParameterUses::default();
        for statement in body {
            collect_parameter_uses_in_stmt(statement, parameter, &mut uses);
        }
        return uses.defused && !uses.normal;
    }
    body.iter()
        .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
        == Some(FirstParameterUse::Defused)
}

#[derive(Default, Debug, PartialEq, Eq)]
struct ParameterUses {
    defused: bool,
    normal: bool,
}

/// Collect whether `parameter` is used defused or normally across one
/// statement subtree, on the shared walker (`Walk::ALL`): every position
/// is visited, including nested function bodies that do not shadow the
/// parameter. Three rules depend on the ENCLOSING construct, so they are
/// decided when the walker hands over the parent and the classified leaf
/// visits that follow are suppressed:
///
/// - a simple `p <- value` binding: R never evaluates the left-hand side
///   of a plain binding, so re-binding the parameter is not a use (a
///   complex target such as `arr[p] <- v` does evaluate its substructure
///   and stays visited);
/// - a defusing call's direct symbol argument (`substitute(p)`,
///   promise-capture helpers, `match.call`) records a defused use;
/// - `missing(p)` inspects whether a promise was supplied without
///   forcing it, so it records neither kind.
fn collect_parameter_uses_in_stmt(statement: &Stmt, parameter: &str, uses: &mut ParameterUses) {
    // Leaf identifier nodes their enclosing construct already classified.
    // Each AST node is visited at most once per walk and the entries are
    // node identities, so an entry can never suppress a different node.
    // Raw pointers dodge the higher-ranked lifetime of the callback's
    // `AstNode<'_>`; they are only compared, never dereferenced.
    let mut classified: Vec<*const Expr> = Vec::new();
    let _ = walk_stmt(
        statement,
        Walk::ALL,
        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Stmt(Stmt::Assign { target, .. }) => {
                    if matches!(target, Expr::Ident { name, .. } if name == parameter) {
                        classified.push(target);
                    }
                }
                AstNode::Stmt(Stmt::For { name, .. }) => {
                    // The loop variable re-binds the name on every
                    // iteration; that re-binding counts as an ordinary
                    // use so downstream analysis stays conservative.
                    if name == parameter {
                        uses.normal = true;
                    }
                }
                // A closure has its own formals: a same-named formal
                // shadows the parameter, and uses inside its body belong
                // to the inner scope.
                AstNode::Stmt(Stmt::FunctionDef { params, .. })
                | AstNode::Expr(Expr::Function { params, .. }) => {
                    if params.iter().any(|formal| formal.name == parameter) {
                        return ControlFlow::Continue(Descend::Skip);
                    }
                }
                AstNode::Expr(Expr::Call { func, args, .. }) => {
                    let bare = ident_name(func).map(bare_name);
                    let probes_promise = matches!(bare, Some("missing"));
                    let captured = captured_arguments(func, args);
                    for (index, argument) in args.iter().enumerate() {
                        let defused = captured.get(index).copied().unwrap_or(false)
                            || matches!(bare, Some("match.call"));
                        if (probes_promise || defused) && is_parameter(&argument.value, parameter) {
                            uses.defused |= defused;
                            classified.push(&argument.value);
                        }
                    }
                }
                AstNode::Expr(ident @ Expr::Ident { name, .. })
                    if name == parameter
                        && !classified.iter().any(|seen| std::ptr::eq(*seen, ident)) =>
                {
                    uses.normal = true;
                }
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}

// The `first_parameter_use` family below is deliberately NOT expressed
// through the shared walker in `ry_core::walk`: it is a first-use query
// in evaluation order whose rules select individual children of a node
// rather than whole subtrees. `Stmt::Assign` answers from the value side
// before the target substructure because R evaluates the right-hand side
// of a complex assignment first (R-lang, "Complex assignments"), while
// the walker descends target-first; `if` arms are combined by
// `conservative_branch_use`, which must walk BOTH branches past their
// first hits (a Normal use in either branch dominates a Defused use in
// the other), while the walker's `Break` halts the entire walk at the
// first payload; and a `for` loop's variable re-binding is checked
// strictly between the iterator walk and the body walk, where the walker
// provides no hook. Same judgment as the force family in `infer/quoting.rs`.
fn first_parameter_use_in_stmt(statement: &Stmt, parameter: &str) -> Option<FirstParameterUse> {
    match statement {
        Stmt::Assign { target, value, .. } => first_parameter_use_in_expr(value, parameter)
            .or_else(|| match target {
                Expr::Ident { .. } => None,
                target => first_parameter_use_in_expr(target, parameter),
            }),
        Stmt::Expr(expression) => first_parameter_use_in_expr(expression, parameter),
        Stmt::If {
            cond, then, else_, ..
        } => first_parameter_use_in_expr(cond, parameter).or_else(|| {
            conservative_branch_use([
                then.iter()
                    .find_map(|statement| first_parameter_use_in_stmt(statement, parameter)),
                else_.as_ref().and_then(|statements| {
                    statements
                        .iter()
                        .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
                }),
            ])
        }),
        Stmt::For {
            name, iter, body, ..
        } => first_parameter_use_in_expr(iter, parameter).or_else(|| {
            if name == parameter {
                Some(FirstParameterUse::Normal)
            } else {
                body.iter()
                    .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
            }
        }),
        Stmt::While { cond, body, .. } => {
            first_parameter_use_in_expr(cond, parameter).or_else(|| {
                body.iter()
                    .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
            })
        }
        Stmt::FunctionDef { .. } => None,
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|expression| first_parameter_use_in_expr(expression, parameter)),
    }
}

fn first_parameter_use_in_expr(expression: &Expr, parameter: &str) -> Option<FirstParameterUse> {
    match expression {
        Expr::Ident { name, .. } => (name == parameter).then_some(FirstParameterUse::Normal),
        Expr::Call { func, args, .. } => {
            let captured = captured_arguments(func, args);
            first_parameter_use_in_expr(func, parameter).or_else(|| {
                // Actual order is not promise force order. A normal use in
                // another actual must dominate a captured occurrence.
                conservative_branch_use(args.iter().enumerate().map(|(index, argument)| {
                    if is_parameter(&argument.value, parameter)
                        && (captured.get(index).copied().unwrap_or(false)
                            || matches!(
                                ident_name(func).map(bare_name),
                                Some("match.call" | "bquote")
                            ))
                    {
                        Some(FirstParameterUse::Defused)
                    } else {
                        first_parameter_use_in_expr(&argument.value, parameter)
                    }
                }))
            })
        }
        Expr::BinOp { lhs, rhs, .. } => first_parameter_use_in_expr(lhs, parameter)
            .or_else(|| first_parameter_use_in_expr(rhs, parameter)),
        Expr::UnaryOp { expr, .. } => first_parameter_use_in_expr(expr, parameter),
        Expr::Index { base, args, .. } => {
            first_parameter_use_in_expr(base, parameter).or_else(|| {
                args.iter()
                    .find_map(|argument| first_parameter_use_in_expr(&argument.value, parameter))
            })
        }
        Expr::Function { .. } => None,
        Expr::Block { body, .. } => {
            if embraced_symbol(body).is_some_and(|(name, _)| name == parameter) {
                Some(FirstParameterUse::Defused)
            } else {
                body.iter()
                    .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => first_parameter_use_in_expr(cond, parameter).or_else(|| {
            conservative_branch_use([
                first_parameter_use_in_expr(then, parameter),
                else_
                    .as_ref()
                    .and_then(|expression| first_parameter_use_in_expr(expression, parameter)),
            ])
        }),
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_)
        | Expr::Missing(_) => None,
    }
}

fn conservative_branch_use(
    uses: impl IntoIterator<Item = Option<FirstParameterUse>>,
) -> Option<FirstParameterUse> {
    let mut first = None;
    for use_ in uses.into_iter().flatten() {
        if use_ == FirstParameterUse::Normal {
            return Some(FirstParameterUse::Normal);
        }
        first = Some(FirstParameterUse::Defused);
    }
    first
}

fn string_literal(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::String(value, _) => Some(value),
        _ => None,
    }
}

fn s4_signature_class(expr: &Expr) -> Option<String> {
    match expr {
        Expr::String(class, _) => Some(class.clone()),
        Expr::Call { func, args, .. } if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "signature") => {
            args.first()
                .and_then(|argument| string_literal(&argument.value))
                .map(str::to_string)
        }
        _ => None,
    }
}

/// Return whether evaluating a block must force `name`, and whether control
/// always falls through it. Both answers come from the same statement walk:
/// force detection stops at the first forcing or non-falling statement, while
/// fall-through continues across the whole block.
fn block_force_flow(statements: &[Stmt], name: &str) -> (bool, bool) {
    let mut forces = false;
    let mut can_still_force = true;
    let mut falls_through = true;

    for statement in statements {
        let (statement_forces, statement_falls_through) = statement_force_flow(statement, name);
        if can_still_force {
            if statement_forces {
                forces = true;
                can_still_force = false;
            } else if !statement_falls_through {
                can_still_force = false;
            }
        }
        falls_through &= statement_falls_through;
    }

    (forces, falls_through)
}

fn statement_force_flow(statement: &Stmt, name: &str) -> (bool, bool) {
    match statement {
        Stmt::Assign { value, .. } | Stmt::Expr(value) => {
            if let Some(returned) = return_call_value(value) {
                (
                    returned.is_some_and(|value| expression_must_force(value, name)),
                    false,
                )
            } else {
                (expression_must_force(value, name), true)
            }
        }
        Stmt::If {
            cond, then, else_, ..
        } => {
            let (then_forces, then_falls) = block_force_flow(then, name);
            let (else_forces, else_falls) = else_
                .as_ref()
                .map(|statements| block_force_flow(statements, name))
                .unwrap_or((false, true));
            let forces = expression_must_force(cond, name) || (then_forces && else_forces);
            (forces, then_falls && else_falls)
        }
        // A `for` loop always falls through for force-flow purposes: an
        // empty iterable never runs the body, so a name forced only
        // inside the body is not guaranteed on any path (and the code
        // after the loop is always reachable, even when the body
        // `return`s on its first iteration). Reporting the body's
        // fall-through as the loop's own could wrongly mark a parameter
        // required.
        Stmt::For { iter, .. } => (expression_must_force(iter, name), true),
        Stmt::While { cond, .. } => (expression_must_force(cond, name), false),
        Stmt::Return { value, .. } => (
            value
                .as_ref()
                .is_some_and(|value| expression_must_force(value, name)),
            false,
        ),
        Stmt::FunctionDef { .. } => (false, true),
    }
}

fn return_call_value(expression: &Expr) -> Option<Option<&Expr>> {
    let Expr::Call { func, args, .. } = expression else {
        return None;
    };
    if !matches!(func.as_ref(), Expr::Ident { name, .. } if name == "return") {
        return None;
    }
    Some(args.first().map(|argument| &argument.value))
}

fn expression_must_force(expression: &Expr, name: &str) -> bool {
    match expression {
        Expr::Ident {
            name: identifier, ..
        } => identifier == name,
        Expr::BinOp { op, lhs, rhs, .. } => {
            expression_must_force(lhs, name)
                || (!matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr)
                    && expression_must_force(rhs, name))
        }
        Expr::UnaryOp { expr, .. } => expression_must_force(expr, name),
        Expr::Index { base, args, .. } => {
            expression_must_force(base, name)
                || args
                    .iter()
                    .any(|argument| expression_must_force(&argument.value, name))
        }
        Expr::Call { func, .. } => expression_must_force(func, name),
        Expr::Block { body, .. } => block_force_flow(body, name).0,
        Expr::If {
            cond, then, else_, ..
        } => {
            expression_must_force(cond, name)
                || (expression_must_force(then, name)
                    && else_
                        .as_ref()
                        .is_some_and(|else_| expression_must_force(else_, name)))
        }
        Expr::Function { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_)
        | Expr::Missing(_) => false,
    }
}

/// Pins the traversal policies of the collection walkers at the
/// behavior level: which subtrees each analysis enters, and which leaf
/// positions their enclosing construct classifies. The
/// `first_parameter_use` family stays hand-rolled; its pins document the
/// evaluation-order and branch semantics that keep it off the shared
/// walker.
#[cfg(test)]
mod collect_walker_tests {
    use super::*;

    fn parse_stmts(src: &str) -> Vec<Stmt> {
        crate::tests::parse_file("collect_walker_test.R", src).stmts
    }

    fn parameter_uses(src: &str, parameter: &str) -> ParameterUses {
        let mut uses = ParameterUses::default();
        for statement in &parse_stmts(src) {
            collect_parameter_uses_in_stmt(statement, parameter, &mut uses);
        }
        uses
    }

    fn first_use(src: &str, parameter: &str) -> Option<FirstParameterUse> {
        parse_stmts(src)
            .iter()
            .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
    }

    fn collect(src: &str) -> Checker {
        let file = crate::tests::parse_file("collect_walker_test.R", src);
        let mut checker = Checker::new("collect_walker_test.R");
        checker.collect_file_fns(&file);
        checker
    }

    #[test]
    fn caller_binding_installer_environment_requires_a_stable_local_frame() {
        for (source, may_replace_caller) in [
            (
                "install <- function(env) makeActiveBinding('x', function() 1L, env)",
                true,
            ),
            (
                "install <- function(env) { target <- get('env'); makeActiveBinding('x', function() 1L, target) }",
                true,
            ),
            (
                "install <- function(env) { target <- (function() env)(); makeActiveBinding('x', function() 1L, target) }",
                true,
            ),
            (
                "install <- function(env) { invisible(target <- env); makeActiveBinding('x', function() 1L, target) }",
                true,
            ),
            (
                "install <- function(env) makeActiveBinding('x', function() 1L, base::new.env())",
                false,
            ),
            (
                "install <- function(env) { target <- base::new.env(); makeActiveBinding('x', function() 1L, target) }",
                false,
            ),
            (
                "install <- function(env) { target <- base::new.env(); if (TRUE) target <- env; makeActiveBinding('x', function() 1L, target) }",
                true,
            ),
            (
                "install <- function(env) { target <- base::new.env(); get('target'); makeActiveBinding('x', function() 1L, target) }",
                true,
            ),
            (
                "install <- function(env) makeActiveBinding('x', function() 1L, new.env())",
                true,
            ),
            (
                "install <- function(env) makeActiveBinding('x', function() 1L, other::new.env())",
                true,
            ),
            (
                "install <- function(env) makeActiveBinding('x', function() 1L, base::new.env(parent=env))",
                false,
            ),
            (
                "install <- function(env, new.env = function() env) makeActiveBinding('x', function() 1L, new.env())",
                true,
            ),
            (
                "install <- function(env) { put <- delayedAssign; put('x', 1L, assign.env=env) }",
                true,
            ),
            (
                "install <- function(env) { put <- base::delayedAssign; put('x', 1L, assign.env=base::new.env()) }",
                false,
            ),
            (
                "install <- function(env) delayedAssign('x', 1L, env, eval.env=base::new.env())",
                true,
            ),
            (
                "install <- function(env) base::delayedAssign('x', 1L, base::new.env(), eval.env=base::new.env())",
                false,
            ),
            (
                "install <- function(env, act=function() makeActiveBinding('x', function() 1L, env)) act()",
                true,
            ),
            (
                "install <- function(env, act=function() makeActiveBinding('x', function() 1L, env)) base::invisible(NULL)",
                false,
            ),
            (
                "install <- function(env, act=function() base::invisible(NULL)) act()",
                false,
            ),
        ] {
            let checker = collect(source);
            assert_eq!(
                checker.fn_table.fns["install"].may_install_caller_binding, may_replace_caller,
                "{source}"
            );
        }
    }

    #[test]
    fn deep_called_default_chain_declines_a_binding_stability_proof() {
        let defaults: Vec<_> = (0..70)
            .map(|index| {
                if index == 0 {
                    "p0=function() base::invisible(NULL)".to_string()
                } else {
                    format!("p{index}=function() p{}()", index - 1)
                }
            })
            .collect();
        let source = format!("install <- function({}) p69()", defaults.join(", "));
        let checker = collect(&source);
        assert!(
            checker.fn_table.fns["install"].may_install_caller_binding,
            "an exhausted default-call walk must retain uncertainty"
        );
    }

    #[test]
    fn capture_actuals_follow_specific_formals() {
        for call in [
            "delayedAssign('held', p)",
            "base::delayedAssign(value=p, x='held')",
            "base:::delayedAssign(val=p, x='held')",
            "base::substitute(env=list(), expr=p)",
            "substitute(en=list(), ex=p)",
            "rlang::enquos(p, .named=FALSE)",
            "rlang::enquos(tag=p, .named=FALSE)",
            "rlang::enquo(p, ...)",
            "rlang::enquo(..., p)",
            "rlang::enquo(ar=p, ...)",
            "rlang::enquo(arg=p, ...)",
            r"rlang::enquo(`\x61rg`=p)",
            "delayedAssign(x='held', value=p, ...)",
        ] {
            let checker = collect(&format!("f <- function(p) {call}"));
            let param = &checker.fn_table.fns["f"].params[0];
            assert!(param.quoting && param.defused, "{call}: {param:?}");
            assert_eq!(
                parameter_uses(call, "p"),
                ParameterUses {
                    defused: true,
                    normal: false
                },
                "{call}"
            );
            assert_eq!(
                first_use(call, "p"),
                Some(FirstParameterUse::Defused),
                "{call}"
            );
        }
        for call in [
            "delayedAssign(p, 1L)",
            "delayedAssign('held', p, ...)",
            "delayedAssign('held', val=p, ...)",
            "base::delayedAssign('held', 1L, eval.env=p)",
            r"base::delayedAssign('held', 1L, `\x65val.env`=p)",
            "base::delayedAssign('held', 1L, assign.env=p)",
            "base::delayedAssign('held', 1L, p)",
            "substitute(expr=foo, env=p)",
            "base::substitute(en=p, ex=foo)",
            "substitute(e=p)",
            "substitute(expr=p, expr=p)",
            "rlang::enquos(.named=p)",
            "rlang::enquos(.ignore_empty=p)",
            "base::substitute(expr=p, env=p)",
            "base::substitute(env=p, expr=p)",
        ] {
            let checker = collect(&format!("f <- function(p) {call}"));
            let param = &checker.fn_table.fns["f"].params[0];
            assert!(!param.quoting && !param.defused, "{call}: {param:?}");
            assert!(parameter_uses(call, "p").normal, "{call}");
            assert_eq!(
                first_use(call, "p"),
                Some(FirstParameterUse::Normal),
                "{call}"
            );
        }
    }

    #[test]
    fn capture_inventory_remains_sparse() {
        let index = promise_capture_index();
        assert!(index.contains_key("substitute"));
        assert!(!index.contains_key("mean"));
        assert!(index.values().flatten().all(|sig| {
            sig.eval
                .values()
                .any(|mode| *mode == EvalMode::CapturesPromise)
        }));
    }

    /// A closure has its own formals: a same-named formal shadows the
    /// parameter, so uses in that body belong to the inner scope and
    /// count for neither flag. A nested function that does NOT re-declare
    /// the name still reads the enclosing formal.
    #[test]
    fn parameter_uses_skip_shadowing_closures_only() {
        assert_eq!(
            parameter_uses("function(p) p", "p"),
            ParameterUses {
                defused: false,
                normal: false
            }
        );
        assert_eq!(
            parameter_uses("function(q) p", "p"),
            ParameterUses {
                defused: false,
                normal: true
            }
        );
        assert_eq!(
            parameter_uses("function(q) { function(p) p; 0 }", "p"),
            ParameterUses {
                defused: false,
                normal: false
            }
        );
        assert_eq!(
            parameter_uses("function(q) { function(r) p; 0 }", "p"),
            ParameterUses {
                defused: false,
                normal: true
            }
        );
    }

    /// R never evaluates the left-hand side of a plain `name <- value`
    /// binding, so re-binding the parameter is not a use. A complex
    /// target's substructure IS evaluated (`arr[p] <- v` computes `arr`
    /// and `p`), so the subscript counts.
    #[test]
    fn parameter_uses_rebinding_is_not_a_use_complex_targets_are() {
        assert_eq!(
            parameter_uses("p <- 1", "p"),
            ParameterUses {
                defused: false,
                normal: false
            }
        );
        assert_eq!(
            parameter_uses("arr[p] <- 1", "p"),
            ParameterUses {
                defused: false,
                normal: true
            }
        );
    }

    /// A `for` loop that re-binds the parameter name counts as an
    /// ordinary use, and so does reading it in the iterator; both keep
    /// the parameter out of defused-only treatment.
    #[test]
    fn parameter_uses_for_loop_binding_and_iterator_are_normal() {
        assert_eq!(
            parameter_uses("for (p in xs) 0", "p"),
            ParameterUses {
                defused: false,
                normal: true
            }
        );
        assert_eq!(
            parameter_uses("for (i in p) 0", "p"),
            ParameterUses {
                defused: false,
                normal: true
            }
        );
    }

    /// Direct symbol arguments are classified by their call: a defusing
    /// call records a defused use (and not a normal one), `missing(p)`
    /// records neither because it only inspects whether a promise was
    /// supplied, an ordinary call records a normal use, and a non-symbol
    /// argument (`substitute(p + 1)`) is walked normally.
    #[test]
    fn parameter_uses_defusing_and_missing_classify_direct_arguments() {
        assert_eq!(
            parameter_uses("substitute(p)", "p"),
            ParameterUses {
                defused: true,
                normal: false
            }
        );
        assert_eq!(
            parameter_uses("missing(p)", "p"),
            ParameterUses {
                defused: false,
                normal: false
            }
        );
        assert_eq!(
            parameter_uses("print(p)", "p"),
            ParameterUses {
                defused: false,
                normal: true
            }
        );
        assert_eq!(
            parameter_uses("substitute(p + 1)", "p"),
            ParameterUses {
                defused: false,
                normal: true
            }
        );
    }

    /// `$` field subscripts and control-flow tests stay in the walk: the
    /// field is a synthesized identifier node in the visited tree, and
    /// an `if`/`while` condition reads the promise like any other
    /// expression position.
    #[test]
    fn parameter_uses_walk_dollar_fields_and_control_tests() {
        assert!(parameter_uses("x$p", "p").normal);
        assert!(parameter_uses("if (p) 1", "p").normal);
        assert!(parameter_uses("while (p) 1", "p").normal);
    }

    /// R evaluates the right-hand side before the target substructure of
    /// a complex assignment (R-lang, "Complex assignments"), so
    /// `arr[p] <- substitute(p)` reports the value side's Defused even
    /// though the target appears first in the source. A walker that
    /// descends target-first would answer Normal here.
    #[test]
    fn first_use_prefers_the_value_side_of_complex_assignments() {
        assert_eq!(
            first_use("arr[p] <- substitute(p)", "p"),
            Some(FirstParameterUse::Defused)
        );
    }

    /// `conservative_branch_use` walks both branches past their first
    /// hits: a Normal use in either branch dominates a Defused use in
    /// the other, because the analysis cannot know which branch runs. An
    /// early-exit walker would stop at the then-branch's Defused.
    #[test]
    fn first_use_normal_dominates_across_branches() {
        assert_eq!(
            first_use("if (c) substitute(p) else print(p)", "p"),
            Some(FirstParameterUse::Normal)
        );
        assert_eq!(
            first_use("if (c) substitute(p) else bquote(p)", "p"),
            Some(FirstParameterUse::Defused)
        );
        // A missing else branch contributes nothing, so a Defused
        // then-branch stands.
        assert_eq!(
            first_use("if (c) substitute(p)", "p"),
            Some(FirstParameterUse::Defused)
        );
    }

    /// A `for` loop's variable re-binding is checked strictly between
    /// the iterator walk and the body walk: the iterator's use wins over
    /// the re-binding, and the re-binding (a Normal use) wins over a
    /// defusing use inside the body.
    #[test]
    fn first_use_orders_iterator_rebinding_body() {
        assert_eq!(
            first_use("for (i in p) substitute(p)", "p"),
            Some(FirstParameterUse::Normal)
        );
        assert_eq!(
            first_use("for (p in i) substitute(p)", "p"),
            Some(FirstParameterUse::Normal)
        );
    }

    /// A bare `{{ p }}` embrace (rlang-style pronoun forwarding, as in
    /// `mean({{ var }})`) is the first use and is defused.
    /// Function-definition statements never contribute a use at all,
    /// shadowing or not — unlike the `collect_parameter_uses` policy,
    /// which enters a non-shadowing body.
    #[test]
    fn first_use_embraced_symbol_is_defused_function_defs_never_count() {
        assert_eq!(
            first_use("mean({ { p } })", "p"),
            Some(FirstParameterUse::Defused)
        );
        assert_eq!(first_use("function(q) p", "p"), None);
    }

    /// The declared-globals scan covers every position R evaluates:
    /// `if` conditions, `for` iterators, and nested function bodies all
    /// contribute call sites, so the per-file call-site index sees calls
    /// that the statement-level definition traversal skips.
    #[test]
    fn declared_globals_records_calls_in_tests_and_fn_bodies() {
        let checker = collect("if (cond_fn()) branch_fn()");
        assert!(checker.fn_table.call_sites.contains_key("cond_fn"));
        assert!(checker.fn_table.call_sites.contains_key("branch_fn"));
        let checker = collect("for (i in iter_fn()) body_fn()");
        assert!(checker.fn_table.call_sites.contains_key("iter_fn"));
        assert!(checker.fn_table.call_sites.contains_key("body_fn"));
        let checker = collect("h <- function() inner_fn()");
        assert!(checker.fn_table.call_sites.contains_key("inner_fn"));
    }

    /// `globalVariables(...)` strings and namespace-targeted
    /// `assign(..., envir = asNamespace(...))` declare known variables;
    /// an `assign` into any other environment does not.
    #[test]
    fn declared_globals_declare_only_namespace_targets() {
        let checker = collect("globalVariables(c(\"gv_one\", \"gv_two\"))");
        assert!(checker.fn_table.known_vars.contains("gv_one"));
        assert!(checker.fn_table.known_vars.contains("gv_two"));
        let checker = collect("assign(\"ns_var\", 1, envir = asNamespace(\"pkg\"))");
        assert!(checker.fn_table.known_vars.contains("ns_var"));
        let checker = collect("assign(\"global_var\", 1, envir = globalenv())");
        assert!(!checker.fn_table.known_vars.contains("global_var"));
    }

    /// Function definitions are collected wherever a binding statement
    /// can appear — `if` branches (taken or not, and `else` branches)
    /// and `for`/`while` bodies — while a definition in expression
    /// position has no binding statement and records nothing: neither a
    /// bare definition statement nor a function literal passed as an
    /// argument.
    #[test]
    fn fn_definitions_collected_from_branch_and_loop_bodies() {
        let checker = collect("if (c) { taken <- function() 1 } else { skipped <- function() 2 }");
        assert!(checker.fn_table.fns.contains_key("taken"));
        assert!(checker.fn_table.fns.contains_key("skipped"));
        let checker = collect("for (i in xs) { loop_fn <- function() 1 }");
        assert!(checker.fn_table.fns.contains_key("loop_fn"));
        let checker = collect("while (c) { while_fn <- function() 1 }");
        assert!(checker.fn_table.fns.contains_key("while_fn"));
        let checker = collect("function() 1");
        assert!(checker.fn_table.fns.is_empty());
        let checker = collect("out <- lapply(xs, function() 1)");
        assert!(!checker.fn_table.fns.contains_key("out"));
        // The callback skips Expr nodes, so a block-valued condition's
        // statements are never visited and contribute no definitions.
        // (`control_tests: false` only spares the walker those visits;
        // the callback's skip is the load-bearing rule, and this pin
        // fails if it is relaxed.)
        let checker = collect("if ({ fn_in_cond <- function() 1 }) 1");
        assert!(!checker.fn_table.fns.contains_key("fn_in_cond"));
    }

    /// Nested definitions record under the mangled `<outer>$<inner>`
    /// name at every nesting level, including inside `if` within a
    /// function body; a literal in expression position (an `lapply`
    /// argument) has no binding and records no mangled entry.
    #[test]
    fn nested_definitions_are_mangled_per_level() {
        let checker = collect(
            "outer <- function() {
  inner <- function() { deepest <- function() 1; 2 }
  3
}",
        );
        assert!(checker.fn_table.fns.contains_key("outer"));
        assert!(checker.fn_table.fns.contains_key("outer$inner"));
        assert!(checker.fn_table.fns.contains_key("outer$inner$deepest"));
        let checker = collect("f <- function() { if (c) { branched <- function() 1 } }");
        assert!(checker.fn_table.fns.contains_key("f$branched"));
        let checker = collect("f <- function() lapply(xs, function(q) q)");
        assert_eq!(checker.fn_table.fns.len(), 1);
        assert!(checker.fn_table.fns.contains_key("f"));
    }
}
