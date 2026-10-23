//! Values known within the current execution path: character literals and
//! boolean guard aliases.
use super::*;
use crate::scope_journal::Undo;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr};
use std::ops::ControlFlow;

/// A local bound to a guard predicate: the predicate, and every name it
/// reads (each in `semantic_argument_name` form).
#[derive(Debug)]
pub(crate) struct ConditionAlias {
    cond: Expr,
    reads: Vec<String>,
}

impl Scope {
    pub(crate) fn condition_alias(&self, name: &str) -> Option<&Expr> {
        self.condition_aliases
            .get(semantic_argument_name(name))
            .map(|alias| &alias.cond)
    }

    /// `name <- cond`, where `cond` is a guard predicate, so `if (name)` may
    /// narrow like `if (cond)`. The alias lives until `name` or a name that
    /// `cond` reads is written (directly or by a call's known writes), a call
    /// may rebind anything, effects become unknown, branches merge, or a loop
    /// starts or ends. A `cond` that reads `name` describes its old value and
    /// is not recorded.
    pub(crate) fn set_condition_alias(&mut self, name: &str, cond: &Expr) {
        let mut reads = Vec::new();
        let _ = walk_expr(cond, Walk::ALL, |node, _| {
            if let AstNode::Expr(Expr::Ident { name, .. }) = node {
                reads.push(semantic_argument_name(name).to_string());
            }
            ControlFlow::<(), _>::Continue(Descend::Into)
        });
        let name = semantic_argument_name(name).to_string();
        if reads.contains(&name) {
            return;
        }
        let alias = Arc::new(ConditionAlias {
            cond: cond.clone(),
            reads,
        });
        let previous = self.condition_aliases.insert(name.clone(), alias);
        if self.journal.depth > 0 {
            self.journal.undo.push(Undo::ConditionAlias(name, previous));
        }
    }

    /// Drop each alias whose own name, or a name its predicate reads, `keep`
    /// rejects. Names reach `keep` in their `semantic_argument_name` form.
    pub(crate) fn retain_condition_aliases(&mut self, keep: impl Fn(&str) -> bool) {
        if self.condition_aliases.is_empty() {
            return;
        }
        let lost: Vec<String> = self
            .condition_aliases
            .iter()
            .filter(|(name, alias)| !keep(name) || alias.reads.iter().any(|read| !keep(read)))
            .map(|(name, _)| name.clone())
            .collect();
        for name in lost {
            let previous = self.condition_aliases.remove(&name);
            if self.journal.depth > 0 {
                self.journal.undo.push(Undo::ConditionAlias(name, previous));
            }
        }
    }

    pub(crate) fn clear_condition_aliases(&mut self) {
        if self.journal.depth > 0 && !self.condition_aliases.is_empty() {
            self.journal
                .undo
                .push(Undo::ConditionAliases(std::mem::take(
                    &mut self.condition_aliases,
                )));
        } else {
            self.condition_aliases.clear();
        }
    }

    pub(crate) fn invalidate_literal_values(&mut self) {
        self.clear_known_strings();
        self.uncertainty.literal_values = true;
    }

    pub(crate) fn invalidate_literal_values_for_dispatch(&mut self, value: &RType) {
        if value.class.is_unknown()
            || value.class.has_known_class()
            || value.mode.is_unknown_or_non_vector()
            || value.mode == Mode::Union
        {
            self.invalidate_literal_values();
        }
    }

    pub(crate) fn known_string(&self, name: &str) -> Option<&str> {
        self.known_strings
            .get(semantic_argument_name(name))
            .map(AsRef::as_ref)
    }

    pub(crate) fn set_known_string(&mut self, name: &str, value: Arc<str>) {
        if self.uncertainty.effects || self.uncertainty.literal_values {
            return;
        }
        let name = semantic_argument_name(name).to_string();
        let previous = self.known_strings.insert(name.clone(), value);
        if self.journal.depth > 0 {
            self.journal.undo.push(Undo::KnownString(name, previous));
        }
    }

    pub(crate) fn clear_known_string(&mut self, name: &str) {
        if self.known_strings.is_empty() {
            return;
        }
        let name = semantic_argument_name(name);
        if let Some(value) = self.known_strings.remove(name)
            && self.journal.depth > 0
        {
            self.journal
                .undo
                .push(Undo::KnownString(name.to_string(), Some(value)));
        }
    }

    pub(crate) fn clear_known_strings(&mut self) {
        if self.journal.depth > 0 && !self.known_strings.is_empty() {
            self.journal
                .undo
                .push(Undo::KnownStrings(std::mem::take(&mut self.known_strings)));
        } else {
            self.known_strings.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expr(src: &str) -> Expr {
        let file = ry_core::RParser::new().unwrap().parse("a.R", src).unwrap();
        let Some(Stmt::Expr(expr)) = file.stmts.into_iter().next() else {
            panic!("expected an expression");
        };
        expr
    }

    #[test]
    fn an_alias_that_reads_its_own_name_is_not_recorded() {
        let mut scope = Scope::default();
        scope.set_condition_alias("ok", &expr("is.na(x) && identical(y, ok)"));
        assert!(scope.condition_alias("ok").is_none());
        scope.set_condition_alias("has_x", &expr("!is.null(x)"));
        assert!(scope.condition_alias("has_x").is_some());
    }
}
