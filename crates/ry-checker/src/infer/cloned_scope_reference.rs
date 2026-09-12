//! Test-only reference implementation for journal storage parity.
use super::*;

impl Checker {
    pub(super) fn walk_cloned_if(
        &mut self,
        scope: &mut Scope,
        narrowing: &Narrowing,
        then: &[Stmt],
        else_: Option<&[Stmt]>,
        mut returns: Option<&mut Vec<RType>>,
    ) {
        let has_else = else_.is_some();
        let (mut then_scope, mut else_scope, narrowed) = apply_narrowing(scope, narrowing);
        for s in then {
            self.walk_stmt(s, &mut then_scope, returns.as_deref_mut());
        }
        if let Some(else_) = else_ {
            for s in else_ {
                self.walk_stmt(s, &mut else_scope, returns.as_deref_mut());
            }
        }
        // Merge branch bindings back into the parent scope. In R,
        // assignments inside an `if` branch leak to the enclosing
        // scope, so a name bound conditionally must still be visible
        // after the `if` (otherwise uses fire RY010 false positives).
        self.merge_branch_bindings(scope, &then_scope, &else_scope, (then, else_), &narrowed);
        // Refinements normally remain branch-local (see
        // `apply_narrowing`). A diverging arm is the exception: the
        // continuation is reachable only through its sibling, so its
        // recorded refinements are facts in the parent scope.
        let then_diverges =
            (scope.loop_frame.is_some() && then_scope.unreachable) || self.block_diverges(then);
        let else_diverges = (scope.loop_frame.is_some() && else_scope.unreachable)
            || else_.is_some_and(|statements| self.block_diverges(statements));
        let continuation = match (then_diverges, else_, else_diverges) {
            (true, Some(_), false) | (true, None, _) => Some(&else_scope),
            (false, Some(_), true) => Some(&then_scope),
            _ => None,
        };
        if let Some(continuation) = continuation {
            self.copy_continuation_narrowing(scope, continuation, &narrowed);
        }
        // When both explicit arms throw, no route reaches the
        // enclosing block's continuation.
        if has_else && then_scope.unreachable && else_scope.unreachable {
            scope.unreachable = true;
        }
    }
    pub(crate) fn merge_branch_bindings(
        &self,
        scope: &mut Scope,
        then_scope: &Scope,
        else_scope: &Scope,
        branches: (&[Stmt], Option<&[Stmt]>),
        narrowed: &HashSet<String>,
    ) {
        let has_else = branches.1.is_some();
        // Types can compare equal after replacing a literal function. Do not
        // carry its identity or constant result across a branch merge.
        scope.clear_ops_facts();
        scope.clear_known_strings();
        scope.ops_environment_unknown |=
            then_scope.ops_environment_unknown || else_scope.ops_environment_unknown;
        scope.effects_unknown |= then_scope.effects_unknown || else_scope.effects_unknown;
        scope.literal_values_unknown |=
            then_scope.literal_values_unknown || else_scope.literal_values_unknown;
        scope.has_escaped_slot_names |=
            then_scope.has_escaped_slot_names || else_scope.has_escaped_slot_names;
        // A diverging branch contributes no state to the continuation. Treat
        // its live sibling as the only arm, while retaining the parent path
        // for a one-arm `if` whose then branch can continue.
        let then_reaches = !then_scope.unreachable;
        let else_reaches = has_else && !else_scope.unreachable;
        if scope.loop_frame.is_some() && !then_reaches && !has_else {
            return;
        }
        if has_else && then_reaches != else_reaches {
            let continuation = if then_reaches { then_scope } else { else_scope };
            for (name, ty) in &continuation.bindings {
                if narrowed.contains(name) && continuation.narrowed_bindings.contains(name) {
                    continue;
                }
                if scope.get(name) != Some(ty) {
                    // The continuation is the only route forward, so its
                    // provenance is a fact in the parent (same capture-
                    // insert-remark shape as `assign_replacement_target`).
                    let had_list_origin = continuation.has_list_origin(name);
                    scope.insert(name.clone(), ty.clone());
                    if had_list_origin {
                        scope.mark_list_origin(name.clone());
                    }
                }
            }
            return;
        }

        // Collect the candidate names (only those that differ from the
        // parent) without holding a borrow of `scope` while we mutate it.
        let mut branch_types: HashMap<&str, (Option<&RType>, Option<&RType>)> = HashMap::new();
        for (name, t) in &then_scope.bindings {
            // Only the marker installed by `apply_narrowing` is
            // branch-local. An ordinary `Scope::insert` clears that marker,
            // so a rebinding of a narrowed name is always merged even when
            // its type is opaque during an early fixpoint iteration.
            if narrowed.contains(name) && then_scope.narrowed_bindings.contains(name) {
                continue;
            }
            match scope.get(name) {
                Some(existing) if existing == t => {}
                _ => {
                    branch_types.entry(name).or_insert((None, None)).0 = Some(t);
                }
            }
        }
        if has_else {
            for (name, t) in &else_scope.bindings {
                // See the then-branch loop: only a pure narrowing
                // refinement is branch-local.
                if narrowed.contains(name) && else_scope.narrowed_bindings.contains(name) {
                    continue;
                }
                match scope.get(name) {
                    Some(existing) if existing == t => {}
                    _ => {
                        branch_types.entry(name).or_insert((None, None)).1 = Some(t);
                    }
                }
            }
        }
        // Scope differences alone do not prove assignment: loop inference
        // can expose a body write even when the loop executes zero times.
        let definitely_rebound = if branch_types
            .values()
            .any(|(a, b)| a.is_some() && b.is_some())
        {
            let mut names = definitely_assigned_names(branches.0);
            let other = definitely_assigned_names(branches.1.unwrap_or_default());
            names.retain(|name| other.contains(name));
            names
        } else {
            HashSet::new()
        };
        for (name, (then_t, else_t)) in branch_types {
            let merged = match (then_t, else_t) {
                (Some(a), Some(b)) => {
                    let joined = a.clone().join(b.clone());
                    match scope.get(name) {
                        Some(parent) if !definitely_rebound.contains(name) => {
                            parent.clone().join(joined)
                        }
                        _ => joined,
                    }
                }
                (Some(a), None) | (None, Some(a)) => {
                    // One path keeps the parent binding, including an
                    // unchanged assignment or an implicit no-else path.
                    // If no parent binding exists, the name may be missing.
                    match scope.get(name) {
                        Some(parent) => parent.clone().join(a.clone()),
                        None => RType::unknown(),
                    }
                }
                (None, None) => continue,
            };
            // A branch scope that does not bind the name (an implicit
            // no-`else`, or an arm that never assigns it) cannot supply a
            // non-list value at any later use — reading the name on that
            // path errors first — so absence is vacuous agreement. A branch
            // that keeps the parent binding (inherited into the clone)
            // demands the parent's marker instead.
            let then_origin =
                !then_scope.bindings.contains_key(name) || then_scope.has_list_origin(name);
            let else_origin =
                !else_scope.bindings.contains_key(name) || else_scope.has_list_origin(name);
            let keeps_list_origin = then_origin && else_origin;
            scope.insert(name, merged);
            if keeps_list_origin {
                scope.mark_list_origin(name);
            }
        }
    }

    /// Copy only the type facts produced by `apply_narrowing` from a branch
    /// known to be the sole route to the continuation. Assignments continue
    /// to use `merge_branch_bindings`; this avoids changing its established
    /// branch-merge and lazy-default semantics.
    fn copy_continuation_narrowing(
        &self,
        scope: &mut Scope,
        continuation: &Scope,
        narrowed: &HashSet<String>,
    ) {
        for name in narrowed {
            if let Some(ty) = continuation.get(name) {
                if continuation.is_default_parameter(name) {
                    scope.insert_parameter_default(name.clone(), ty.clone());
                } else {
                    scope.insert(name.clone(), ty.clone());
                }
            }
        }
    }
}
