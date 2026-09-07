//! Forwarded promises and guaranteed evaluation of lazy defaults.

use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr, walk_stmt, walk_stmts};
use std::ops::ControlFlow;

/// Walk `stmts` collecting calls inside `caller`'s body that forward its
/// `params` to nested calls. Skips nested function bodies: a nested
/// function has its own formals and is collected separately when it has
/// a binding.
pub(crate) fn collect_forwarded_calls_in_stmts(
    caller: &str,
    params: &[Param],
    stmts: &[Stmt],
    calls: &mut Vec<ForwardedCall>,
) {
    let _ = walk_stmts(
        stmts,
        Walk {
            fn_bodies: false,
            ..Walk::ALL
        },
        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
            if let AstNode::Expr(Expr::Call { func, args, .. }) = node
                && let Expr::Ident { name, .. } = func.as_ref()
                && args.iter().any(|argument| {
                    matches!(&argument.value, Expr::Ident { name, .. }
                        if params.iter().any(|parameter| parameter.name == *name))
                })
            {
                let callee = crate::semantic_lists::bare_name(name);
                calls.push(ForwardedCall {
                    caller: caller.to_string(),
                    callee: callee.to_string(),
                    stub_callee: name.clone(),
                    caller_params: params.to_vec(),
                    arguments: args
                        .iter()
                        .map(|argument| {
                            let forwarded = match &argument.value {
                                Expr::Ident { name, .. } => Some(name.clone()),
                                _ => None,
                            };
                            (argument.name.clone(), forwarded)
                        })
                        .collect(),
                });
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}

impl Checker {
    /// Diagnose the narrow, provable lazy-default ordering bug where a
    /// parameter is used by an earlier top-level statement than the direct
    /// body assignment needed by its default expression.
    pub(crate) fn check_lazy_default_reachability(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        assigned: &HashSet<String>,
        scope: &Scope,
    ) {
        if params.iter().all(|param| param.default.is_none()) {
            return;
        }
        // Syntax operators can be rebound to lazy closures. This narrow rule
        // does not resolve such closures, so abandon its primitive assumptions
        // when a visible binding or local assignment could replace one.
        let syntax_names = [
            "+", "`+`", "-", "`-`", "*", "`*`", "/", "`/`", "^", "`^`", "%%", "`%%`", "%/%",
            "`%/%`", ":", "`:`", "<", "`<`", "<=", "`<=`", ">", "`>`", ">=", "`>=`", "==", "`==`",
            "!=", "`!=`", "&", "`&`", "&&", "`&&`", "|", "`|`", "||", "`||`", "!", "`!`", "[",
            "`[`", "[[", "`[[`", "$", "`$`", "if", "`if`", "while", "`while`", "for", "`for`",
            "return", "`return`", "{", "`{`", "(", "`(`", "<-", "`<-`", "<<-", "`<<-`", "=", "`=`",
        ];
        let namespace_syntax = ["::", "`::`", ":::", "`:::`", "function", "`function`"];
        // These are literal binding names, including `::` itself, rather than
        // namespace-qualified identifiers accepted by resolves_to_base.
        if self.literal_bindings_may_be_shadowed(
            syntax_names.iter().chain(namespace_syntax.iter()).copied(),
            assigned,
            scope,
        ) {
            return;
        }
        let formals: HashSet<&str> = params.iter().map(|param| param.name.as_str()).collect();

        for param in params {
            let Some(default) = &param.default else {
                continue;
            };

            // A formal promise shadows every enclosing binding of the same
            // name. Diagnose a recursive default only when the body actually
            // forces that promise; defusing helpers such as enexpr()/enquo()
            // may deliberately capture `function(x = x)` without evaluating
            // the default.
            let forced_in_body = guaranteed_force_before_replacement(self, body, &param.name);
            if forced_in_body
                && let Some(span) = first_executed_identifier(self, default, &param.name)
            {
                self.emit(
                    Severity::Warning,
                    span,
                    "RY098",
                    format!(
                        "parameter `{}` has a self-referential default that recurses when forced",
                        param.name
                    ),
                );
                continue;
            }

            let mut references = std::collections::BTreeSet::new();
            self.collect_executed_identifiers(default, &mut references);

            for local in references
                .iter()
                .filter(|name| assigned.contains(name.as_str()) && !formals.contains(name.as_str()))
            {
                let Some(assign_index) = body.iter().position(|statement| {
                    matches!(statement, Stmt::Assign { target: Expr::Ident { name, .. }, .. } if name == local)
                }) else {
                    // Conditional and otherwise nested assignments are not a
                    // sufficiently precise guarantee for this rule.
                    continue;
                };

                let mut forced = None;
                for statement in &body[..assign_index] {
                    forced = definitely_forced_identifier_in_stmt(self, statement, &param.name);
                    if forced.is_some() || !statement_preserves_binding(statement, &param.name) {
                        break;
                    }
                }
                if let Some(span) = forced {
                    self.emit(
                        Severity::Warning,
                        span,
                        "RY098",
                        format!(
                            "parameter `{}` may force its default before body-local `{local}` is assigned",
                            param.name
                        ),
                    );
                    break;
                }
            }
        }
    }
}

// The force/identifier family below (`guaranteed_force_before_replacement`,
// `definitely_forced_identifier{,_in_stmt}`, `first_executed_identifier{,_in_stmt}`)
// is deliberately NOT expressed through the shared walker in
// `ry_core::walk`: its rules select individual children of a node —
// call arguments are skipped unless the callee has a reviewed forcing
// contract, an `if` with a literal condition visits only the taken
// branch, subscript promises are skipped while the dispatch object
// is kept — and the walk must stop at the first identifier forced in
// evaluation order. That is an evaluation-order analysis with
// per-child laziness rules, not a subtree-skip policy, so it keeps its
// hand-rolled recursion.
fn guaranteed_force_before_replacement(checker: &Checker, body: &[Stmt], wanted: &str) -> bool {
    for statement in body {
        if definitely_forced_identifier_in_stmt(checker, statement, wanted).is_some() {
            return true;
        }
        if !statement_preserves_binding(statement, wanted) {
            // Check the statement's own force before dropping certainty about
            // the binding. Unknown calls and other promise reads may replace it.
            return false;
        }
    }
    false
}

/// Find a force that is guaranteed when this statement executes. Conditional
/// branch bodies and loop bodies are not guaranteed to run; their conditions
/// (and a for-loop's iterator) are.
fn definitely_forced_identifier_in_stmt(
    checker: &Checker,
    statement: &Stmt,
    wanted: &str,
) -> Option<Span> {
    match statement {
        Stmt::Assign { value, .. } | Stmt::Expr(value) => {
            definitely_forced_identifier(checker, value, wanted)
        }
        Stmt::If {
            cond, then, else_, ..
        } => match cond {
            Expr::Logical(true, span) => {
                guaranteed_force_before_replacement(checker, then, wanted).then_some(*span)
            }
            Expr::Logical(false, span) => else_.as_ref().and_then(|statements| {
                guaranteed_force_before_replacement(checker, statements, wanted).then_some(*span)
            }),
            _ => definitely_forced_identifier(checker, cond, wanted),
        },
        Stmt::While { cond, .. } => definitely_forced_identifier(checker, cond, wanted),
        Stmt::For { iter, .. } => definitely_forced_identifier(checker, iter, wanted),
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|value| definitely_forced_identifier(checker, value, wanted)),
        Stmt::FunctionDef { .. } => None,
    }
}

// Exhaustive classification keeps newly parsed operators from silently gaining
// an eager evaluation contract. Pipes and `%in%` are calls with lazy arguments.
fn opaque_binary_operator(op: &BinOpKind) -> bool {
    match op {
        BinOpKind::PipeNative
        | BinOpKind::PipeForward
        | BinOpKind::PipeTee
        | BinOpKind::PipeAssign
        | BinOpKind::In => true,
        BinOpKind::Add
        | BinOpKind::Sub
        | BinOpKind::Mul
        | BinOpKind::Div
        | BinOpKind::Pow
        | BinOpKind::Mod
        | BinOpKind::IDiv
        | BinOpKind::Colon
        | BinOpKind::Lt
        | BinOpKind::Le
        | BinOpKind::Gt
        | BinOpKind::Ge
        | BinOpKind::Eq
        | BinOpKind::Ne
        | BinOpKind::And
        | BinOpKind::AndAnd
        | BinOpKind::Or
        | BinOpKind::OrOr
        | BinOpKind::Assign
        | BinOpKind::SuperAssign => false,
    }
}

fn definitely_forced_identifier(checker: &Checker, expr: &Expr, wanted: &str) -> Option<Span> {
    match expr {
        Expr::BinOp { op, .. } if opaque_binary_operator(op) => None,
        Expr::If {
            cond, then, else_, ..
        } => match cond.as_ref() {
            Expr::Logical(true, _) => definitely_forced_identifier(checker, then, wanted),
            Expr::Logical(false, _) => else_
                .as_ref()
                .and_then(|else_| definitely_forced_identifier(checker, else_, wanted)),
            _ => definitely_forced_identifier(checker, cond, wanted),
        },
        Expr::BinOp {
            lhs,
            rhs,
            op: op @ (BinOpKind::AndAnd | BinOpKind::OrOr),
            ..
        } => definitely_forced_identifier(checker, lhs, wanted).or_else(|| {
            // Only these literal operands guarantee evaluation of the RHS.
            matches!(
                (op, lhs.as_ref()),
                (BinOpKind::AndAnd, Expr::Logical(true, _))
                    | (BinOpKind::OrOr, Expr::Logical(false, _))
            )
            .then(|| definitely_forced_identifier(checker, rhs, wanted))
            .flatten()
        }),
        // Keep guaranteed traversal through wrappers; falling back to the
        // possible-dependency walker here would inspect untaken inner branches.
        Expr::UnaryOp { expr, .. } => definitely_forced_identifier(checker, expr, wanted),
        Expr::BinOp { lhs, rhs, op, .. } => {
            if matches!(op, BinOpKind::Assign | BinOpKind::SuperAssign) {
                definitely_forced_identifier(checker, rhs, wanted)
            } else {
                first_reference_in_order(
                    checker,
                    [lhs.as_ref(), rhs.as_ref()],
                    wanted,
                    definitely_forced_identifier,
                )
            }
        }
        // Index methods can ignore subscript promises. Only the object is
        // forced before dispatch, so subscripts supply no force guarantee.
        Expr::Index { base, .. } => definitely_forced_identifier(checker, base, wanted),
        Expr::Block { body, span } => {
            guaranteed_force_before_replacement(checker, body, wanted).then_some(*span)
        }
        _ => first_executed_identifier(checker, expr, wanted),
    }
}

fn first_reference_in_order<'a>(
    checker: &Checker,
    expressions: impl IntoIterator<Item = &'a Expr>,
    wanted: &str,
    visit: fn(&Checker, &Expr, &str) -> Option<Span>,
) -> Option<Span> {
    for expression in expressions {
        if let Some(span) = visit(checker, expression, wanted) {
            return Some(span);
        }
        if !expression_preserves_binding(expression, wanted) {
            return None;
        }
    }
    None
}

fn first_executed_identifier_in_stmts(
    checker: &Checker,
    body: &[Stmt],
    wanted: &str,
) -> Option<Span> {
    for statement in body {
        // An assignment can force its old value on the RHS before replacing
        // the binding. A possible replacement is enough to stop: later reads
        // cannot be attributed to the original promise without flow analysis.
        if let Some(span) = first_executed_identifier_in_stmt(checker, statement, wanted) {
            return Some(span);
        }
        if !statement_preserves_binding(statement, wanted) {
            return None;
        }
    }
    None
}

fn first_executed_identifier_in_stmt(
    checker: &Checker,
    statement: &Stmt,
    wanted: &str,
) -> Option<Span> {
    match statement {
        Stmt::Assign { value, .. } | Stmt::Expr(value) => {
            first_executed_identifier(checker, value, wanted)
        }
        Stmt::If {
            cond, then, else_, ..
        } => match cond {
            Expr::Logical(true, _) => first_executed_identifier_in_stmts(checker, then, wanted),
            Expr::Logical(false, _) => else_
                .as_ref()
                .and_then(|body| first_executed_identifier_in_stmts(checker, body, wanted)),
            _ => first_executed_identifier(checker, cond, wanted),
        },
        Stmt::For { iter, .. } => first_executed_identifier(checker, iter, wanted),
        Stmt::While { cond, .. } => first_executed_identifier(checker, cond, wanted),
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|value| first_executed_identifier(checker, value, wanted)),
        Stmt::FunctionDef { .. } => None,
    }
}

fn qualified_signature<'a>(
    checker: &'a Checker,
    func: &Expr,
) -> Option<&'a ry_typeshed::FunctionSig> {
    let name = ident_name(func)?;
    let (package, function) = name.rsplit_once("::")?;
    let package = package.trim_end_matches(':');
    // Bare names may be masked. Do not borrow base metadata for another
    // namespace merely because it shares the base database.
    let typeshed = if package == "base" {
        &checker.typeshed
    } else {
        checker.package_typeshed(package)?
    };
    typeshed.functions.get(function)
}

fn forced_argument<'a>(checker: &Checker, func: &Expr, args: &'a [Arg]) -> Option<&'a Expr> {
    let signature = qualified_signature(checker, func)?;
    let ry_typeshed::ForceSpec::SoleArgument { param, allow_named } = signature.force.as_ref()?;
    let [argument] = args else { return None };
    if argument
        .name
        .as_ref()
        .is_some_and(|name| !allow_named || name != param)
        || matches!(&argument.value, Expr::Unknown(_))
        || matches!(&argument.value, Expr::Ident { name, .. } if name == "...")
    {
        return None;
    }
    Some(&argument.value)
}

// Only cross prefixes that cannot replace the promise or invoke user code.
// Even another identifier can force a default or active binding that mutates
// the current frame. A forcing contract says nothing about effects after the
// guaranteed argument evaluation, so calls are barriers too.
fn plain_distinct_bindings(left: &str, right: &str) -> bool {
    // The parser retains backticks and escapes. Do not mistake alternate
    // spellings of the same R symbol for independent bindings.
    !left.starts_with('`') && !right.starts_with('`') && left != right
}

fn statement_preserves_binding(statement: &Stmt, wanted: &str) -> bool {
    match statement {
        Stmt::Assign {
            target: Expr::Ident { name, .. },
            value,
            ..
        } => plain_distinct_bindings(name, wanted) && expression_preserves_binding(value, wanted),
        Stmt::Expr(value) => expression_preserves_binding(value, wanted),
        Stmt::FunctionDef { .. } => true,
        Stmt::If {
            cond, then, else_, ..
        } => match cond {
            Expr::Logical(true, _) => then
                .iter()
                .all(|stmt| statement_preserves_binding(stmt, wanted)),
            Expr::Logical(false, _) => else_.as_ref().is_none_or(|body| {
                body.iter()
                    .all(|stmt| statement_preserves_binding(stmt, wanted))
            }),
            _ => false,
        },
        _ => false,
    }
}

fn expression_preserves_binding(expression: &Expr, wanted: &str) -> bool {
    match expression {
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Function { .. } => true,
        Expr::BinOp {
            lhs,
            rhs,
            op: BinOpKind::Assign,
            ..
        } => {
            matches!(lhs.as_ref(), Expr::Ident { name, .. } if plain_distinct_bindings(name, wanted))
                && expression_preserves_binding(rhs, wanted)
        }
        Expr::Block { body, .. } => body
            .iter()
            .all(|stmt| statement_preserves_binding(stmt, wanted)),
        Expr::If {
            cond, then, else_, ..
        } => match cond.as_ref() {
            Expr::Logical(true, _) => expression_preserves_binding(then, wanted),
            Expr::Logical(false, _) => else_
                .as_ref()
                .is_none_or(|branch| expression_preserves_binding(branch, wanted)),
            _ => false,
        },
        _ => false,
    }
}

fn first_executed_identifier(checker: &Checker, expr: &Expr, wanted: &str) -> Option<Span> {
    match expr {
        Expr::BinOp { op, .. } if opaque_binary_operator(op) => None,
        Expr::Ident { name, span } => (name == wanted).then_some(*span),
        Expr::Call { func, args, .. } => {
            first_executed_identifier(checker, func, wanted).or_else(|| {
                let argument = forced_argument(checker, func, args)?;
                definitely_forced_identifier(checker, argument, wanted)
            })
        }

        Expr::BinOp { lhs, rhs, op, .. } => {
            if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                definitely_forced_identifier(checker, expr, wanted)
            } else if matches!(op, BinOpKind::Assign | BinOpKind::SuperAssign) {
                first_executed_identifier(checker, rhs, wanted)
            } else {
                first_reference_in_order(
                    checker,
                    [lhs.as_ref(), rhs.as_ref()],
                    wanted,
                    first_executed_identifier,
                )
            }
        }
        Expr::UnaryOp { expr, .. } => first_executed_identifier(checker, expr, wanted),
        Expr::Index { base, .. } => first_executed_identifier(checker, base, wanted),
        Expr::Block { body, .. } => first_executed_identifier_in_stmts(checker, body, wanted),
        // An unknown condition can replace the binding through another promise.
        // Keep both branches opaque even when both syntactically read `wanted`.
        Expr::If {
            cond, then, else_, ..
        } => match cond.as_ref() {
            Expr::Logical(true, _) => first_executed_identifier(checker, then, wanted),
            Expr::Logical(false, _) => else_
                .as_ref()
                .and_then(|branch| first_executed_identifier(checker, branch, wanted)),
            _ => first_executed_identifier(checker, cond, wanted),
        },
        Expr::Function { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => None,
    }
}

impl Checker {
    /// Possible dependencies of a forced default. Only reviewed capture-only
    /// helpers can suppress their quoted arguments: other quoted-expression
    /// consumers, such as isolate(), capture and then execute their code.
    fn collect_executed_identifiers(
        &self,
        expr: &Expr,
        names: &mut std::collections::BTreeSet<String>,
    ) {
        let _ = walk_expr(
            expr,
            Walk {
                assign_targets: false,
                assign_operands: false,
                dollar_args: false,
                fn_bodies: false,
                ..Walk::ALL
            },
            |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                // Prune only code being evaluated. The separate capture walker
                // must still find injections under quoted, unreachable branches.
                match node {
                    AstNode::Expr(Expr::If {
                        cond, then, else_, ..
                    }) if matches!(cond.as_ref(), Expr::Logical(_, _)) => {
                        if matches!(cond.as_ref(), Expr::Logical(true, _)) {
                            self.collect_executed_identifiers(then, names);
                        } else if let Some(otherwise) = else_ {
                            self.collect_executed_identifiers(otherwise, names);
                        }
                        return ControlFlow::Continue(Descend::Skip);
                    }
                    AstNode::Expr(Expr::BinOp { lhs, op, .. })
                        if matches!(
                            (op, lhs.as_ref()),
                            (BinOpKind::AndAnd, Expr::Logical(false, _))
                                | (BinOpKind::OrOr, Expr::Logical(true, _))
                        ) =>
                    {
                        return ControlFlow::Continue(Descend::Skip);
                    }
                    _ => {}
                }
                if let AstNode::Expr(Expr::Call { func, args, .. }) = node
                    && let Some(callee) = ident_name(func)
                    && let Some(capture) = default_capture_mode(callee)
                    && let Some(signature) = self.resolve_typeshed_sig(callee)
                {
                    self.collect_executed_identifiers(func, names);
                    let bindings = match_params(&signature.params, args);
                    for (index, argument) in args.iter().enumerate() {
                        if matches!(
                            eval_mode_for_arg(&signature, &bindings, index),
                            Some(EvalMode::QuotedExpression | EvalMode::CapturesPromise)
                        ) {
                            if matches!(capture, DefaultCapture::Tidy) {
                                self.collect_possible_injection_dependencies(
                                    &argument.value,
                                    names,
                                );
                            }
                        } else {
                            self.collect_executed_identifiers(&argument.value, names);
                        }
                    }
                    return ControlFlow::Continue(Descend::Skip);
                }
                if let AstNode::Expr(Expr::Ident { name, .. }) = node {
                    names.insert(name.clone());
                }
                ControlFlow::Continue(Descend::Into)
            },
        );
    }

    fn collect_injection_dependencies(
        &self,
        expr: &Expr,
        names: &mut std::collections::BTreeSet<String>,
    ) {
        let Expr::Block { body, .. } = expr else {
            self.collect_executed_identifiers(expr, names);
            return;
        };
        let mut established = std::collections::BTreeSet::new();
        for statement in body {
            let mut dependencies = std::collections::BTreeSet::new();
            let assigned = match statement {
                Stmt::Assign { target, value, .. } => {
                    // R evaluates the RHS before establishing the local binding.
                    self.collect_injection_dependencies(value, &mut dependencies);
                    if let Expr::Ident { name, .. } = target {
                        Some(name)
                    } else {
                        self.collect_executed_identifiers(target, &mut dependencies);
                        None
                    }
                }
                Stmt::Expr(value) => {
                    self.collect_injection_dependencies(value, &mut dependencies);
                    None
                }
                Stmt::If {
                    cond: Expr::Logical(condition, _),
                    then,
                    else_,
                    span,
                } => {
                    let selected = if *condition {
                        then.as_slice()
                    } else {
                        else_.as_deref().unwrap_or_default()
                    };
                    self.collect_injection_dependencies(
                        &Expr::Block {
                            body: selected.to_vec(),
                            span: *span,
                        },
                        &mut dependencies,
                    );
                    None
                }
                _ => {
                    // Branches and loops do not establish a guaranteed binding.
                    let _ = walk_stmt(
                        statement,
                        Walk {
                            assign_targets: false,
                            assign_operands: false,
                            dollar_args: false,
                            fn_bodies: false,
                            ..Walk::ALL
                        },
                        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                            if let AstNode::Expr(value) = node {
                                self.collect_executed_identifiers(value, &mut dependencies);
                                return ControlFlow::Continue(Descend::Skip);
                            }
                            ControlFlow::Continue(Descend::Into)
                        },
                    );
                    None
                }
            };
            names.extend(dependencies.difference(&established).cloned());
            if let Some(name) = assigned {
                established.insert(name.clone());
            }
        }
    }

    /// The reviewed rlang helpers process tidy injection while capturing code.
    /// Retain dependencies of !!, !!!, and {{ }} payloads, including those inside
    /// quoted function bodies. Like rlang, the walker leaves defaults untouched.
    fn collect_possible_injection_dependencies(
        &self,
        expr: &Expr,
        names: &mut std::collections::BTreeSet<String>,
    ) {
        let _ = walk_expr(expr, Walk::ALL, |node: AstNode<'_>, _: usize| {
            match node {
                AstNode::Expr(Expr::UnaryOp {
                    op: UnaryOpKind::Not,
                    expr,
                    ..
                }) => {
                    if let Expr::UnaryOp {
                        op: UnaryOpKind::Not,
                        expr: payload,
                        ..
                    } = expr.as_ref()
                    {
                        let payload = match payload.as_ref() {
                            Expr::UnaryOp {
                                op: UnaryOpKind::Not,
                                expr,
                                ..
                            } => expr.as_ref(),
                            other => other,
                        };
                        self.collect_injection_dependencies(payload, names);
                        return ControlFlow::<(), Descend>::Continue(Descend::Skip);
                    }
                }
                AstNode::Expr(Expr::Block { body, .. }) => {
                    if let [Stmt::Expr(inner @ Expr::Block { .. })] = body.as_slice() {
                        self.collect_injection_dependencies(inner, names);
                        return ControlFlow::Continue(Descend::Skip);
                    }
                }
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        });
    }
}

#[derive(Clone, Copy)]
enum DefaultCapture {
    Literal,
    Tidy,
}

// EvalMode describes how an argument is received, not whether the callee
// later evaluates it. Keep this subset explicit until further helpers have
// runtime evidence; bquote(), for example, has different escape syntax.
fn default_capture_mode(name: &str) -> Option<DefaultCapture> {
    let (package, function) = name.rsplit_once("::")?;
    match (package.trim_end_matches(':'), function) {
        ("base", "quote" | "substitute" | "expression") => Some(DefaultCapture::Literal),
        ("rlang", "expr") => Some(DefaultCapture::Tidy),
        _ => None,
    }
}
