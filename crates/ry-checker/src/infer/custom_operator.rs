use super::*;

impl Checker {
    /// Function-position lookup precedes promise creation/forcing. A custom
    /// operator may ignore both operands; primitive diagnostics cannot apply.
    pub(crate) fn infer_custom_operator(&self, op: BinOpKind, scope: &mut Scope) -> Option<RType> {
        // Short-circuit &&/||, colon, membership, pipes and unary syntax keep
        // their existing paths; this lookup covers the binary Ops families.
        if !op.is_arithmetic()
            && !is_comparison(op)
            && !matches!(op, BinOpKind::And | BinOpKind::Or)
        {
            return None;
        }
        let symbol = op_symbol(op);
        let quoted = match op {
            BinOpKind::Add => "`+`",
            BinOpKind::Sub => "`-`",
            BinOpKind::Mul => "`*`",
            BinOpKind::Div => "`/`",
            BinOpKind::Pow => "`^`",
            BinOpKind::Mod => "`%%`",
            BinOpKind::IDiv => "`%/%`",
            BinOpKind::Lt => "`<`",
            BinOpKind::Le => "`<=`",
            BinOpKind::Gt => "`>`",
            BinOpKind::Ge => "`>=`",
            BinOpKind::Eq => "`==`",
            BinOpKind::Ne => "`!=`",
            BinOpKind::And => "`&`",
            BinOpKind::Or => "`|`",
            _ => return None,
        };
        let names = [symbol, quoted];
        let escaped = self.fn_table.has_escaped_operator_names || self.escaped_operator_bindings;
        let local = names.iter().find_map(|name| scope.get(name));
        let project_function = names
            .iter()
            .any(|name| self.fn_table.fns.contains_key(*name));
        let external = names.iter().any(|name| {
            self.imported_from
                .get(*name)
                .is_some_and(|package| package != "base")
                || (self.external_bindings.contains(*name)
                    && !self.imported_from.contains_key(*name))
        });
        let known_project_binding = names
            .iter()
            .any(|name| self.fn_table.known_vars.contains(*name));
        let local_may_call = local
            .is_some_and(|ty| matches!(ty.mode, Mode::Function | Mode::Opaque | Mode::Union))
            || names.iter().any(|name| scope.is_parameter(name));
        // Concrete data is skipped in call position. A flat project definition
        // behind that data may instead be a stale same-frame definition: do not
        // resurrect its return type without lexical provenance.
        let explicit_mask = escaped
            || local_may_call
            || project_function
            || external
            || (known_project_binding && local.is_none());
        if !explicit_mask {
            return None;
        }
        if !escaped
            && !scope.ops_environment_unknown
            && !scope.data_mask_unknown
            && !scope.search_path_unknown
            && self.bare_loaded.is_empty()
            && !ops_chooser::syntax_rebound(self, scope)
            && let Some(result) = ops_chooser::literal_operator_return(symbol, scope)
        {
            // A proven constant body neither forces an operand nor changes the
            // caller. This also avoids effects from ignored assignment operands.
            return Some(result);
        }
        scope.invalidate_unknown_effects();
        Some(RType::unknown())
    }
}

/// Cache this during collection, including project refinement without source.
/// Raw escaped names need a binding decoder before they can prove base lookup.
pub(crate) fn has_escaped_names(stmts: &[Stmt]) -> bool {
    use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
    use std::ops::ControlFlow;
    walk_stmts(stmts, Walk::ALL, |node, _| {
        let escaped = match node {
            AstNode::Expr(Expr::Ident { name, .. }) | AstNode::Stmt(Stmt::For { name, .. }) => {
                escaped_name_may_mask_operator(name)
            }
            AstNode::Expr(Expr::Function { params, .. })
            | AstNode::Stmt(Stmt::FunctionDef { params, .. }) => params
                .iter()
                .any(|param| escaped_name_may_mask_operator(&param.name)),
            _ => false,
        };
        if escaped {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(Descend::Into)
        }
    })
    .is_break()
}

/// Escapes cannot erase a literal prefix. If no modeled operator starts with
/// that prefix, the name cannot replace one, regardless of how escapes decode.
/// Empty/compatible prefixes stay unknown; this is deliberately not a decoder.
pub(crate) fn escaped_name_may_mask_operator(name: &str) -> bool {
    let name = semantic_argument_name(name);
    let Some((prefix, _)) = name.split_once('\\') else {
        return false;
    };
    [
        "+", "-", "*", "/", "^", "%%", "%/%", "<", "<=", ">", ">=", "==", "!=", "&", "|",
    ]
    .iter()
    .any(|operator| operator.starts_with(prefix))
}
