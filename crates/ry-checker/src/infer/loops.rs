use super::*;

pub(super) fn known_unclassed_vector(ty: &RType) -> bool {
    matches!(ty.length, Length::Known(n) if n > 1) && ty.class.known && ty.class.len == 0
}

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
    joined
        .scalar_asserted_bindings
        .retain(|name| incoming.scalar_asserted_bindings.contains(name));
    joined
        .loop_vector_bindings
        .extend(incoming.loop_vector_bindings.iter().cloned());
    // A lexical function on either path may shadow a same-spelled flat
    // table entry. Its exact declaration target survives only when both
    // paths agree on the same definition span.
    joined
        .lexical_functions
        .extend(incoming.lexical_functions.iter().cloned());
    joined
        .lexical_definitions
        .retain(|name, definition| incoming.lexical_definition(name) == Some(*definition));
    joined.ops_environment_unknown |= incoming.ops_environment_unknown;
    joined.effects_unknown |= incoming.effects_unknown;
    joined.dynamic_bindings_unknown |= incoming.dynamic_bindings_unknown;
    joined.literal_values_unknown |= incoming.literal_values_unknown;
    joined.has_escaped_slot_names |= incoming.has_escaped_slot_names;
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
        inner.clear_known_strings();
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
        scope.clear_known_strings();
        let frame = self.loop_frames.pop().expect("active loop frame");
        let has_transfer = frame.breaks.is_some() || frame.nexts.is_some();
        let body_unreachable = inner.unreachable;
        // An unmodelled path may mutate bindings or control syntax before
        // leaving the loop. A previously recorded safe break cannot erase it.
        scope.ops_environment_unknown |= inner.ops_environment_unknown;
        scope.effects_unknown |= inner.effects_unknown;
        scope.dynamic_bindings_unknown |= inner.dynamic_bindings_unknown;
        scope.literal_values_unknown |= inner.literal_values_unknown;
        scope.has_escaped_slot_names |= inner.has_escaped_slot_names;
        let mut exits = frame.breaks;
        if has_transfer && inner.effects_unknown {
            join_path(&mut exits, &inner);
        }
        if has_transfer {
            // A literal-TRUE loop can leave only through break. Finite loops
            // can also finish after the body or a next; their initial state
            // remains possible unless entry was established.
            if !always_true {
                // Unknown-effect paths were already included above.
                if !inner.unreachable && !inner.effects_unknown {
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
            let scalar_before = scope.scalar_asserted_bindings.clone();
            let vector_before = scope.loop_vector_bindings.clone();
            scope.ops_environment_unknown |= exit.ops_environment_unknown;
            scope.effects_unknown |= exit.effects_unknown;
            scope.dynamic_bindings_unknown |= exit.dynamic_bindings_unknown;
            scope.literal_values_unknown |= exit.literal_values_unknown;
            scope.has_escaped_slot_names |= exit.has_escaped_slot_names;
            for (binding, ty) in exit.bindings {
                let list_origin = exit.list_origin_bindings.contains(&binding);
                let scalar_asserted = scalar_before.contains(&binding)
                    && exit.scalar_asserted_bindings.contains(&binding);
                // A possibly empty loop keeps the entry value's vector path.
                let loop_vector = exit.loop_vector_bindings.contains(&binding)
                    || (!entered
                        && (vector_before.contains(&binding)
                            || scope.get(&binding).is_some_and(known_unclassed_vector)));
                let lexical = exit.lexical_functions.contains(&binding)
                    || (!entered && scope.is_lexical_function(&binding));
                let definition =
                    exit.lexical_definitions
                        .get(&binding)
                        .copied()
                        .filter(|definition| {
                            entered || scope.lexical_definition(&binding) == Some(*definition)
                        });
                scope.insert(binding.clone(), ty);
                if list_origin {
                    scope.mark_list_origin(binding.clone());
                }
                if scalar_asserted {
                    scope.mark_scalar_asserted(&binding);
                }
                if loop_vector {
                    scope.mark_loop_vector(&binding);
                }
                if let Some(definition) = definition {
                    scope.mark_lexical_function(binding, definition);
                } else if lexical {
                    scope.mark_lexical_callable(binding);
                }
            }
        }
        if always_true && !reaches {
            scope.unreachable = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escaped_slot_scope() -> Scope {
        let mut scope = Scope::default();
        scope.insert(r"`@\x3c-`", RType::unknown());
        // Sticky evidence remains after removal, independently of a source AST.
        scope.bindings.clear();
        assert!(scope.has_escaped_slot_names);
        scope
    }

    #[test]
    fn escaped_slot_flag_survives_loop_path_joins_without_bindings() {
        let mut paths = None;
        join_path(&mut paths, &Scope::default());
        join_path(&mut paths, &escaped_slot_scope());
        join_path(&mut paths, &Scope::default());
        let joined = paths.unwrap();
        assert!(joined.has_escaped_slot_names);
        assert!(joined.bindings.is_empty());
    }

    #[test]
    fn escaped_slot_flag_survives_loop_body_break_and_next_exits() {
        for (body_flag, break_flag, next_flag) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let mut checker = Checker::new("slot-loop-state.R");
            let mut scope = Scope::default();
            let inner = if body_flag {
                escaped_slot_scope()
            } else {
                Scope::default()
            };
            checker.loop_frames.push(LoopExitFrame {
                // In the body case, a separate unmarked break is the selected
                // exit. The inner flag must survive even when its state is not.
                breaks: (body_flag || break_flag).then(|| {
                    Box::new(if break_flag {
                        escaped_slot_scope()
                    } else {
                        Scope::default()
                    })
                }),
                nexts: next_flag.then(|| Box::new(escaped_slot_scope())),
            });
            checker.finish_loop(&mut scope, inner, body_flag || break_flag, true);
            assert!(scope.has_escaped_slot_names);
            assert!(scope.bindings.is_empty());
            assert!(!checker.fn_table.has_escaped_slot_names);
            assert!(checker.has_explicit_operator_mask("@<-", "`@<-`", &scope));
        }
    }
}
