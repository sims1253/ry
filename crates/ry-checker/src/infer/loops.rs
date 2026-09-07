use super::*;

#[derive(Default)]
pub(crate) struct LoopExitFrame {
    breaks: Option<Box<Scope>>,
    nexts: Option<Box<Scope>>,
}

/// Accumulate alternative paths. Allocate a scope snapshot only for the first
/// transfer; later transfers join their binding types into that snapshot.
fn join_path(paths: &mut Option<Box<Scope>>, incoming: &Scope) {
    let Some(joined) = paths else {
        *paths = Some(Box::new(incoming.clone()));
        return;
    };
    for (name, ty) in &mut joined.bindings {
        *ty = incoming
            .get(name)
            .map_or_else(RType::unknown, |other| ty.clone().join(other.clone()));
    }
    for name in incoming.bindings.keys() {
        joined
            .bindings
            .entry(name.clone())
            .or_insert_with(RType::unknown);
    }
    joined
        .list_origin_bindings
        .retain(|name| incoming.has_list_origin(name));
    joined.ops_environment_unknown |= incoming.ops_environment_unknown;
    joined.effects_unknown |= incoming.effects_unknown;
}

impl Checker {
    fn unmasked_loop_transfer(&self, name: &str, scope: &Scope) -> bool {
        matches!(name, "break" | "next") && !scope.effects_unknown && {
            let escaped = format!("`{name}`");
            !self.literal_bindings_may_be_shadowed([name, escaped.as_str()], &HashSet::new(), scope)
        }
    }

    fn ends_loop_iteration(&self, statement: &Stmt, scope: &Scope) -> bool {
        match statement {
            Stmt::Expr(Expr::Ident { name, .. }) => self.unmasked_loop_transfer(name, scope),
            Stmt::Expr(Expr::Block { body, .. }) => {
                body.iter().any(|s| self.ends_loop_iteration(s, scope))
            }
            Stmt::If {
                then,
                else_: Some(other),
                ..
            } => {
                then.iter().any(|s| self.ends_loop_iteration(s, scope))
                    && other.iter().any(|s| self.ends_loop_iteration(s, scope))
            }
            _ => false,
        }
    }

    pub(crate) fn reachable_loop_assignments(
        &self,
        body: &[Stmt],
        scope: &Scope,
    ) -> HashSet<String> {
        let Some(end) = body.iter().position(|s| self.ends_loop_iteration(s, scope)) else {
            return assigned_names_in_body(body);
        };
        let mut names = assigned_names_in_body(&body[..end]);
        match &body[end] {
            Stmt::Expr(Expr::Block { body, .. }) => {
                names.extend(self.reachable_loop_assignments(body, scope))
            }
            statement => names.extend(assigned_names_in_body(std::slice::from_ref(statement))),
        }
        names
    }

    pub(crate) fn begin_loop(&mut self, inner: &mut Scope) {
        inner.loop_frame = Some(self.loop_frames.len());
        self.loop_frames.push(LoopExitFrame::default());
    }

    pub(crate) fn record_loop_transfer(&mut self, name: &str, scope: &mut Scope) -> bool {
        if !self.unmasked_loop_transfer(name, scope) {
            return false;
        }
        let Some(index) = scope.loop_frame else {
            return false;
        };
        let frame = &mut self.loop_frames[index];
        join_path(
            if name == "break" {
                &mut frame.breaks
            } else {
                &mut frame.nexts
            },
            scope,
        );
        scope.unreachable = true;
        true
    }

    pub(crate) fn finish_loop(
        &mut self,
        scope: &mut Scope,
        inner: Scope,
        always_true: bool,
        entered: bool,
    ) {
        let frame = self.loop_frames.pop().expect("active loop frame");
        let has_transfer = frame.breaks.is_some() || frame.nexts.is_some();
        let body_unreachable = inner.unreachable;
        // An unmodelled path may mutate bindings or control syntax before
        // leaving the loop. A previously recorded safe break cannot erase it.
        scope.ops_environment_unknown |= inner.ops_environment_unknown;
        scope.effects_unknown |= inner.effects_unknown;
        let mut exits = frame.breaks;
        if has_transfer && inner.effects_unknown {
            join_path(&mut exits, &inner);
        }
        if has_transfer {
            // A literal-TRUE loop can leave only through break. Finite loops
            // can also finish after the body or a next; their initial state
            // remains possible unless entry was established.
            if !always_true {
                if !inner.unreachable {
                    join_path(&mut exits, &inner);
                }
                if let Some(next) = &frame.nexts {
                    join_path(&mut exits, next);
                }
                if !entered {
                    join_path(&mut exits, scope);
                }
            }
        }
        let exits = if has_transfer {
            exits.map(|exit| *exit)
        } else {
            Some(inner)
        };
        let reaches = exits.is_some() && !(always_true && !has_transfer && body_unreachable);
        if let Some(exit) = exits {
            scope.ops_environment_unknown |= exit.ops_environment_unknown;
            scope.effects_unknown |= exit.effects_unknown;
            for (binding, ty) in exit.bindings {
                let list_origin = exit.list_origin_bindings.contains(&binding);
                scope.insert(binding.clone(), ty);
                if list_origin {
                    scope.mark_list_origin(binding);
                }
            }
        }
        if always_true && !reaches {
            scope.unreachable = true;
        }
    }
}
