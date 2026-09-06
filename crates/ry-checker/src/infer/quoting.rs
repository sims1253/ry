//! Forwarded promises and guaranteed evaluation of lazy defaults.

use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr, walk_stmts};
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
    ) {
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
            let forced_in_body = guaranteed_force_before_replacement(body, &param.name);
            if forced_in_body && let Some(span) = first_executed_identifier(default, &param.name) {
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

            let mut references = HashSet::new();
            collect_executed_identifiers(default, &mut references);

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

                let forced = body[..assign_index].iter().find_map(|statement| {
                    definitely_forced_identifier_in_stmt(statement, &param.name)
                });
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
// call arguments are skipped unless the callee is a known strict
// builtin, an `if` with a literal condition visits only the taken
// branch, a `$` subscript's synthesized ident is skipped while the base
// is kept — and the walk must stop at the first identifier forced in
// evaluation order. That is an evaluation-order analysis with
// per-child laziness rules, not a subtree-skip policy, so it keeps its
// hand-rolled recursion.
fn guaranteed_force_before_replacement(body: &[Stmt], wanted: &str) -> bool {
    for statement in body {
        match statement {
            Stmt::Assign {
                target: Expr::Ident { name, .. },
                value,
                ..
            } if name == wanted => {
                return definitely_forced_identifier(value, wanted).is_some();
            }
            _ => {}
        }
        if definitely_forced_identifier_in_stmt(statement, wanted).is_some() {
            return true;
        }
        if matches!(statement, Stmt::If { .. }) {
            // A non-literal condition may take a diverging branch, so later
            // statements are not guaranteed to execute.
            return false;
        }
        let explicit_return = matches!(statement, Stmt::Return { .. })
            || matches!(
                statement,
                Stmt::Expr(Expr::Call { func, .. })
                    if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "return")
            );
        if explicit_return {
            return false;
        }
    }
    false
}

/// Find a force that is guaranteed when this statement executes. Conditional
/// branch bodies and loop bodies are not guaranteed to run; their conditions
/// (and a for-loop's iterator) are.
fn definitely_forced_identifier_in_stmt(statement: &Stmt, wanted: &str) -> Option<Span> {
    match statement {
        Stmt::Assign { value, .. } | Stmt::Expr(value) => {
            definitely_forced_identifier(value, wanted)
        }
        Stmt::If {
            cond, then, else_, ..
        } => match cond {
            Expr::Logical(true, span) => {
                guaranteed_force_before_replacement(then, wanted).then_some(*span)
            }
            Expr::Logical(false, span) => else_.as_ref().and_then(|statements| {
                guaranteed_force_before_replacement(statements, wanted).then_some(*span)
            }),
            _ => definitely_forced_identifier(cond, wanted),
        },
        Stmt::While { cond, .. } => definitely_forced_identifier(cond, wanted),
        Stmt::For { iter, .. } => definitely_forced_identifier(iter, wanted),
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|value| definitely_forced_identifier(value, wanted)),
        Stmt::FunctionDef { .. } => None,
    }
}

fn definitely_forced_identifier(expr: &Expr, wanted: &str) -> Option<Span> {
    match expr {
        Expr::If {
            cond, then, else_, ..
        } => match cond.as_ref() {
            Expr::Logical(true, _) => definitely_forced_identifier(then, wanted),
            Expr::Logical(false, _) => else_
                .as_ref()
                .and_then(|else_| definitely_forced_identifier(else_, wanted)),
            _ => definitely_forced_identifier(cond, wanted),
        },
        Expr::BinOp {
            lhs,
            op: BinOpKind::AndAnd | BinOpKind::OrOr,
            ..
        } => definitely_forced_identifier(lhs, wanted),
        Expr::Block { body, span } => {
            guaranteed_force_before_replacement(body, wanted).then_some(*span)
        }
        _ => first_executed_identifier(expr, wanted),
    }
}

fn first_executed_identifier_in_stmt(statement: &Stmt, wanted: &str) -> Option<Span> {
    match statement {
        Stmt::Assign { value, .. } => first_executed_identifier(value, wanted),
        Stmt::Expr(expr) => first_executed_identifier(expr, wanted),
        Stmt::If {
            cond: Expr::Logical(taken, _),
            then,
            else_,
            ..
        } => {
            let branch = if *taken { Some(then) } else { else_.as_ref() };
            branch.and_then(|statements| {
                statements
                    .iter()
                    .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
            })
        }
        Stmt::If {
            cond, then, else_, ..
        } => first_executed_identifier(cond, wanted)
            .or_else(|| {
                then.iter()
                    .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
            })
            .or_else(|| {
                else_.as_ref().and_then(|statements| {
                    statements
                        .iter()
                        .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
                })
            }),
        Stmt::For { iter, body, .. } => first_executed_identifier(iter, wanted).or_else(|| {
            body.iter()
                .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
        }),
        Stmt::While { cond, body, .. } => first_executed_identifier(cond, wanted).or_else(|| {
            body.iter()
                .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted))
        }),
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|value| first_executed_identifier(value, wanted)),
        // Defining a closure does not evaluate its body or force captures.
        Stmt::FunctionDef { .. } => None,
    }
}

fn first_executed_identifier(expr: &Expr, wanted: &str) -> Option<Span> {
    match expr {
        Expr::Ident { name, span } => (name == wanted).then_some(*span),
        Expr::Call { func, args, .. } => {
            // identity forces its sole argument after argument matching. Bare
            // names can be masked; malformed calls fail before forcing x.
            if matches!(ident_name(func), Some("base::identity" | "base:::identity")) {
                return match args.as_slice() {
                    [argument] if argument.name.as_deref().is_none_or(|name| name == "x") => {
                        definitely_forced_identifier(&argument.value, wanted)
                    }
                    _ => None,
                };
            }
            // Only explicitly qualified strict builtins establish
            // guaranteed argument forcing. Bare names may be shadowed by
            // lazy user functions, and any other call may defuse
            // (`enquo(x)`, `join_by(x == y)`), so neither walks arguments.
            if ident_name(func).is_some_and(|name| {
                name.rsplit_once("::").is_some_and(|(package, bare)| {
                    matches!(package.trim_end_matches(':'), "base" | "rlang")
                        && matches!(bare, "abort" | "stop" | "warning" | "message")
                })
            }) {
                first_executed_identifier(func, wanted).or_else(|| {
                    args.iter()
                        .find_map(|argument| first_executed_identifier(&argument.value, wanted))
                })
            } else {
                first_executed_identifier(func, wanted)
            }
        }
        Expr::BinOp { lhs, rhs, op, .. } => {
            // A literal short-circuit operand can make the recursive name
            // in the RHS unreachable even though the default is forced.
            let skips_rhs = matches!(
                (op, lhs.as_ref()),
                (BinOpKind::AndAnd, Expr::Logical(false, _))
                    | (BinOpKind::OrOr, Expr::Logical(true, _))
            );
            if skips_rhs {
                first_executed_identifier(lhs, wanted)
            } else if matches!(op, BinOpKind::Assign | BinOpKind::SuperAssign) {
                first_executed_identifier(rhs, wanted)
            } else {
                first_executed_identifier(lhs, wanted)
                    .or_else(|| first_executed_identifier(rhs, wanted))
            }
        }
        Expr::UnaryOp { expr, .. } => first_executed_identifier(expr, wanted),
        Expr::Index {
            base, kind, args, ..
        } => first_executed_identifier(base, wanted).or_else(|| {
            // `$field` stores `field` as a synthesized identifier in the AST,
            // but R does not evaluate it as an expression. Counting that name
            // would turn `vars = parent$vars` into a self-reference.
            (!matches!(kind, IndexKind::Dollar)).then(|| {
                args.iter()
                    .find_map(|argument| first_executed_identifier(&argument.value, wanted))
            })?
        }),
        Expr::Block { body, .. } => body
            .iter()
            .find_map(|statement| first_executed_identifier_in_stmt(statement, wanted)),
        Expr::If {
            cond, then, else_, ..
        } if matches!(cond.as_ref(), Expr::Logical(_, _)) => {
            if matches!(cond.as_ref(), Expr::Logical(true, _)) {
                first_executed_identifier(then, wanted)
            } else {
                else_
                    .as_ref()
                    .and_then(|else_| first_executed_identifier(else_, wanted))
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => first_executed_identifier(cond, wanted)
            .or_else(|| first_executed_identifier(then, wanted))
            .or_else(|| {
                else_
                    .as_ref()
                    .and_then(|else_| first_executed_identifier(else_, wanted))
            }),
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

/// Identifiers the expression evaluates when forced. Skips assignment
/// targets (R does not evaluate them), `$` subscript idents, and nested
/// function bodies.
fn collect_executed_identifiers(expr: &Expr, names: &mut HashSet<String>) {
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
            if let AstNode::Expr(Expr::Ident { name, .. }) = node {
                names.insert(name.clone());
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}
