//! The shared Stmt/Expr recursion skeleton for collection-style AST
//! analyses (name collection, call indexing, binding scans).
//!
//! The walkers this replaces differ on purpose: some treat nested
//! function bodies as opaque, some skip assignment targets because R
//! does not evaluate them, some skip the synthesized identifier a `$`
//! subscript stores in the AST. Those differences are policy knobs on
//! one walker ([`Walk`]) plus a per-node "don't descend" answer from
//! the callback — not a dozen hand-rolled recursions.
//!
//! Evaluation-order analyses (does this call force its arguments?
//! which `if` branch runs?) do not fit this shape: their rules select
//! individual children of a node (call arguments, one branch) rather
//! than whole subtrees. They stay hand-rolled at their call sites.

use crate::ast::{BinOpKind, Expr, IndexKind, Stmt};
use std::ops::ControlFlow;

/// One node handed to a [`walk_stmts`](fn@walk_stmts) callback.
#[derive(Debug, Copy, Clone)]
pub enum AstNode<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
}

/// What the walker does with a node's children after the callback saw
/// the node itself (pre-order: the callback always runs first).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Descend {
    /// Recurse into the node's children (subject to the [`Walk`] policy).
    Into,
    /// Skip this node's children; the callback's own logic is the last
    /// word on this subtree.
    Skip,
}

/// Which well-known subtrees a walk enters. Every field defaults to
/// "visit"; a family opts out of exactly the edges its hand-rolled
/// predecessor skipped.
#[derive(Debug, Copy, Clone)]
pub struct Walk {
    /// Descend into `Stmt::Assign` targets. R does not evaluate the
    /// target of an assignment, so identifier-execution and
    /// binding-collection walks leave this off.
    pub assign_targets: bool,
    /// Descend into the left operand of `<-` / `<<-` in expression
    /// position (`y <- x <- 1L`). Walks that only record the bound
    /// name leave this off.
    pub assign_operands: bool,
    /// Descend into `$field` subscript arguments. The parser stores the
    /// field as a synthesized identifier node, but R never evaluates it
    /// as an expression; identifier-execution walks leave this off.
    pub dollar_args: bool,
    /// Descend into nested function bodies (`Expr::Function` literals
    /// and `Stmt::FunctionDef`). A closure has its own scope and its
    /// own formals, so scope- and quoting-sensitive walks leave this
    /// off.
    pub fn_bodies: bool,
    /// Descend into `if`/`while` conditions and `for` iterators.
    /// Binding-collection walks that model only the body leave this
    /// off.
    pub control_tests: bool,
}

impl Walk {
    /// Visit every subtree. The policy of walkers with no skip rules.
    pub const ALL: Walk = Walk {
        assign_targets: true,
        assign_operands: true,
        dollar_args: true,
        fn_bodies: true,
        control_tests: true,
    };
}

/// Walk a statement slice in source order, pre-order per node. Returns
/// the first [`ControlFlow::Break`] payload from the callback, or
/// `Continue(())` after the last node.
///
/// The callback receives each statement and expression node together
/// with `fn_depth`, the number of function bodies entered so far (0 at
/// the top level of the walk; braced blocks do not count).
pub fn walk_stmts<B>(
    stmts: &[Stmt],
    policy: Walk,
    mut visit: impl FnMut(AstNode<'_>, usize) -> ControlFlow<B, Descend>,
) -> ControlFlow<B> {
    for stmt in stmts {
        stmt_step(stmt, policy, 0, &mut visit)?;
    }
    ControlFlow::Continue(())
}

/// Walk one statement (and its subtree). See [`walk_stmts`].
pub fn walk_stmt<B>(
    stmt: &Stmt,
    policy: Walk,
    mut visit: impl FnMut(AstNode<'_>, usize) -> ControlFlow<B, Descend>,
) -> ControlFlow<B> {
    stmt_step(stmt, policy, 0, &mut visit)
}

/// Walk one expression (and its subtree). See [`walk_stmts`].
pub fn walk_expr<B>(
    expr: &Expr,
    policy: Walk,
    mut visit: impl FnMut(AstNode<'_>, usize) -> ControlFlow<B, Descend>,
) -> ControlFlow<B> {
    expr_step(expr, policy, 0, &mut visit)
}

fn stmt_step<B>(
    stmt: &Stmt,
    policy: Walk,
    fn_depth: usize,
    visit: &mut impl FnMut(AstNode<'_>, usize) -> ControlFlow<B, Descend>,
) -> ControlFlow<B> {
    match visit(AstNode::Stmt(stmt), fn_depth) {
        ControlFlow::Break(b) => return ControlFlow::Break(b),
        ControlFlow::Continue(Descend::Skip) => return ControlFlow::Continue(()),
        ControlFlow::Continue(Descend::Into) => {}
    }
    match stmt {
        Stmt::Assign { target, value, .. } => {
            if policy.assign_targets {
                expr_step(target, policy, fn_depth, visit)?;
            }
            expr_step(value, policy, fn_depth, visit)?;
        }
        Stmt::Expr(expression) => expr_step(expression, policy, fn_depth, visit)?,
        Stmt::If {
            cond, then, else_, ..
        } => {
            if policy.control_tests {
                expr_step(cond, policy, fn_depth, visit)?;
            }
            for stmt in then {
                stmt_step(stmt, policy, fn_depth, visit)?;
            }
            if let Some(else_) = else_ {
                for stmt in else_ {
                    stmt_step(stmt, policy, fn_depth, visit)?;
                }
            }
        }
        Stmt::For { iter, body, .. } => {
            if policy.control_tests {
                expr_step(iter, policy, fn_depth, visit)?;
            }
            for stmt in body {
                stmt_step(stmt, policy, fn_depth, visit)?;
            }
        }
        Stmt::While { cond, body, .. } => {
            if policy.control_tests {
                expr_step(cond, policy, fn_depth, visit)?;
            }
            for stmt in body {
                stmt_step(stmt, policy, fn_depth, visit)?;
            }
        }
        Stmt::FunctionDef { body, .. } => {
            if policy.fn_bodies {
                for stmt in body {
                    stmt_step(stmt, policy, fn_depth + 1, visit)?;
                }
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                expr_step(value, policy, fn_depth, visit)?;
            }
        }
    }
    ControlFlow::Continue(())
}

fn expr_step<B>(
    expr: &Expr,
    policy: Walk,
    fn_depth: usize,
    visit: &mut impl FnMut(AstNode<'_>, usize) -> ControlFlow<B, Descend>,
) -> ControlFlow<B> {
    match visit(AstNode::Expr(expr), fn_depth) {
        ControlFlow::Break(b) => return ControlFlow::Break(b),
        ControlFlow::Continue(Descend::Skip) => return ControlFlow::Continue(()),
        ControlFlow::Continue(Descend::Into) => {}
    }
    match expr {
        Expr::Call { func, args, .. } => {
            expr_step(func, policy, fn_depth, visit)?;
            for argument in args {
                expr_step(&argument.value, policy, fn_depth, visit)?;
            }
        }
        Expr::BinOp { op, lhs, rhs, .. } => {
            let assignment = matches!(op, BinOpKind::Assign | BinOpKind::SuperAssign);
            if !(assignment && !policy.assign_operands) {
                expr_step(lhs, policy, fn_depth, visit)?;
            }
            expr_step(rhs, policy, fn_depth, visit)?;
        }
        Expr::UnaryOp { expr, .. } => expr_step(expr, policy, fn_depth, visit)?,
        Expr::Index {
            base, kind, args, ..
        } => {
            expr_step(base, policy, fn_depth, visit)?;
            if !(*kind == IndexKind::Dollar && !policy.dollar_args) {
                for argument in args {
                    expr_step(&argument.value, policy, fn_depth, visit)?;
                }
            }
        }
        Expr::Function { body, .. } => {
            if policy.fn_bodies {
                for stmt in body {
                    stmt_step(stmt, policy, fn_depth + 1, visit)?;
                }
            }
        }
        Expr::Block { body, .. } => {
            for stmt in body {
                stmt_step(stmt, policy, fn_depth, visit)?;
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            if policy.control_tests {
                expr_step(cond, policy, fn_depth, visit)?;
            }
            expr_step(then, policy, fn_depth, visit)?;
            if let Some(else_) = else_ {
                expr_step(else_, policy, fn_depth, visit)?;
            }
        }
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Ident { .. }
        | Expr::Unknown(_) => {}
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RParser, SourceFile};
    use std::collections::HashSet;

    fn parse(src: &str) -> SourceFile {
        RParser::new()
            .expect("parser")
            .parse("walk_test.R", src)
            .expect("parse")
    }

    fn visited(src: &str, policy: Walk) -> Vec<String> {
        let file = parse(src);
        let mut seen = Vec::new();
        let _ = walk_stmts(&file.stmts, policy, |node, _| {
            match node {
                AstNode::Stmt(Stmt::Assign { .. }) => seen.push("stmt-assign".into()),
                AstNode::Expr(Expr::Ident { name, .. }) => seen.push(name.clone()),
                _ => {}
            }
            ControlFlow::<(), Descend>::Continue(Descend::Into)
        });
        seen
    }

    /// The policy knobs skip exactly their documented subtrees: the
    /// `$field` identifier, the `<-` LHS and `Stmt::Assign` target, the
    /// function body, and the `if` condition.
    #[test]
    fn policy_flags_skip_exactly_their_subtrees() {
        let src = "if (cond(x)) { d$field <- f(d$other, function(y) z) }";
        let all = visited(src, Walk::ALL);
        for name in ["cond", "x", "d", "field", "f", "other", "z"] {
            assert!(
                all.contains(&name.to_string()),
                "ALL must visit {name}: {all:?}"
            );
        }
        let skipped = visited(
            src,
            Walk {
                assign_targets: false,
                assign_operands: false,
                dollar_args: false,
                fn_bodies: false,
                control_tests: false,
            },
        );
        for name in ["cond", "x", "field", "other", "z"] {
            assert!(
                !skipped.contains(&name.to_string()),
                "policy must skip {name}: {skipped:?}"
            );
        }
        // The callee and a `$` index base in value position are still
        // evaluated, so both remain visited.
        for name in ["d", "f"] {
            assert!(
                skipped.contains(&name.to_string()),
                "kept {name}: {skipped:?}"
            );
        }
    }

    /// `fn_depth` counts entered function bodies; braced blocks do not.
    #[test]
    fn fn_depth_counts_function_bodies_not_blocks() {
        let file = parse("{ f(function() g(function() h)) }");
        let mut depths = HashSet::new();
        let _ = walk_stmts(&file.stmts, Walk::ALL, |node, depth| {
            if let AstNode::Expr(Expr::Ident { name, .. }) = node {
                depths.insert((name.clone(), depth));
            }
            ControlFlow::<(), Descend>::Continue(Descend::Into)
        });
        assert!(depths.contains(&("f".to_string(), 0)));
        assert!(depths.contains(&("g".to_string(), 1)));
        assert!(depths.contains(&("h".to_string(), 2)));
    }

    /// `Break` stops the walk with its payload; `Skip` prunes a subtree
    /// the callback has fully handled.
    #[test]
    fn break_stops_and_skip_prunes() {
        let file = parse("f(a, { b <- g(c); h(d) })");
        let mut seen = Vec::new();
        let hit = walk_stmts(&file.stmts, Walk::ALL, |node, _| match node {
            AstNode::Expr(Expr::Call { func, .. }) if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "g") => {
                ControlFlow::Continue(Descend::Skip)
            }
            AstNode::Expr(Expr::Ident { name, .. }) if name == "h" => {
                ControlFlow::Break(name.clone())
            }
            AstNode::Expr(Expr::Ident { name, .. }) => {
                seen.push(name.clone());
                ControlFlow::Continue(Descend::Into)
            }
            _ => ControlFlow::Continue(Descend::Into),
        });
        // `c` was pruned with the `g` call, and `d` never ran: `h` broke.
        assert_eq!(hit, ControlFlow::Break("h".to_string()));
        assert_eq!(
            seen,
            vec!["f".to_string(), "a".to_string(), "b".to_string()]
        );
    }
}
