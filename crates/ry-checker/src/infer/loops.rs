use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr, walk_stmts};
use std::ops::ControlFlow;

#[derive(Debug)]
struct LoopCallableInvocation {
    target: String,
    args: Vec<Arg>,
}

struct LoopSourceInputs<'a> {
    args: &'a [Arg],
    writes: &'a HashMap<String, Vec<HashSet<String>>>,
    scope: &'a Scope,
}

struct LoopSourceState {
    completed: HashMap<String, bool>,
    visiting: HashSet<String>,
    remaining: usize,
}

#[derive(Default)]
pub(super) struct LoopCallerBindingRisk {
    pub(super) targets: HashSet<String>,
    pub(super) immediate_targets: HashSet<String>,
    pub(super) unknown: bool,
}

/// A later iteration may invoke a value assigned after an earlier call.
/// The normal one-pass body walk only sees the initial callable at that call
/// site. Collect possible target names and assignment values with a bounded
/// walk, then use the same caller-binding source resolution as computed
/// call heads. This is an effect fact, not a general loop CFG or return type.
impl Checker {
    pub(super) fn loop_selected_immediate_assign(&self, name: &str, scope: &Scope) -> bool {
        let Some(index) = scope.loop_frame else {
            return false;
        };
        let mut seen = false;
        for frame in self.loop_frames.iter().take(index + 1) {
            if frame.risky_caller_binding_targets.contains(name) {
                if !frame.immediate_caller_binding_targets.contains(name) {
                    return false;
                }
                seen = true;
            }
        }
        seen
    }

    pub(super) fn loop_carried_binding_risk(
        &self,
        body: &[Stmt],
        repeated_condition: Option<&Expr>,
        scope: &Scope,
    ) -> LoopCallerBindingRisk {
        // A successful assertion may be established inside a repeated body.
        // A later iteration can select an installer before that assertion is
        // checked, so entry-only scalar markers cannot gate this prepass.
        let mut invocations = Vec::<LoopCallableInvocation>::new();
        let mut writes = HashMap::<String, Vec<HashSet<String>>>::new();
        let mut remaining = 4096;
        let mut exhausted = false;
        let policy = Walk {
            assign_targets: false,
            assign_operands: true,
            dollar_args: false,
            fn_bodies: false,
            control_tests: true,
        };
        let mut visit = |node: AstNode<'_>, _| {
            if remaining == 0 {
                exhausted = true;
                return ControlFlow::<(), Descend>::Break(());
            }
            remaining -= 1;
            if let AstNode::Expr(Expr::Call { func, args, .. }) = node {
                if ident_name(func)
                    .is_some_and(|name| crate::semantic_lists::bare_name(name) == "do.call")
                {
                    if let Some(matched) = crate::match_caller_binding_argument_names(
                        &["what", "args", "quote", "envir"],
                        args,
                    ) {
                        if let Some(target) =
                            matched.arg_for_param(0).and_then(|index| args.get(index))
                        {
                            let supplied = matched
                                .arg_for_param(1)
                                .and_then(|index| args.get(index))
                                .and_then(|arg| match &arg.value {
                                    Expr::Call { func, args, .. }
                                        if ident_name(func) == Some("base::list") =>
                                    {
                                        Some(args.clone())
                                    }
                                    _ => None,
                                })
                                .unwrap_or_default();
                            for source in crate::collect::global_caller_binding_value_sources(
                                &target.value,
                                64,
                            ) {
                                invocations.push(LoopCallableInvocation {
                                    target: source,
                                    args: supplied.clone(),
                                });
                            }
                        }
                    } else {
                        exhausted = true;
                    }
                }
                for source in crate::collect::global_caller_binding_value_sources(func, 64) {
                    invocations.push(LoopCallableInvocation {
                        target: source,
                        args: args.to_vec(),
                    });
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
                if let Some(name) = crate::caller_binding_identity(name) {
                    writes.entry(name).or_default().push(
                        crate::collect::global_caller_binding_value_sources(value, 64),
                    );
                } else {
                    exhausted = true;
                }
            }
            if invocations.len() + writes.len() > 256 {
                exhausted = true;
                return ControlFlow::<(), Descend>::Break(());
            }
            ControlFlow::<(), Descend>::Continue(Descend::Into)
        };
        if let Some(condition) = repeated_condition {
            let _ = walk_expr(condition, policy, &mut visit);
        }
        let _ = walk_stmts(body, policy, &mut visit);
        let mut risk = LoopCallerBindingRisk {
            unknown: exhausted,
            ..LoopCallerBindingRisk::default()
        };
        if exhausted {
            return risk;
        }
        for invocation in invocations {
            if !writes.contains_key(&invocation.target) {
                continue;
            }
            let inputs = LoopSourceInputs {
                args: &invocation.args,
                writes: &writes,
                scope,
            };
            let mut state = LoopSourceState {
                completed: HashMap::new(),
                visiting: HashSet::new(),
                remaining: 128,
            };
            let assigned_effect =
                self.loop_assigned_source_may_install(&invocation.target, &inputs, &mut state);
            let initial_effect = self.computed_source_may_replace_current_binding(
                &invocation.target,
                &invocation.args,
                scope,
                &mut HashMap::new(),
                &mut HashSet::new(),
                &mut 128,
            );
            if assigned_effect || initial_effect {
                // A call through `q <- p; q(...)` needs the carried fact on
                // `p` before the copy as well as on the eventual target `q`.
                // Otherwise that assignment would erase the marker on `q`
                // before its invocation. Follow only names actually written
                // by this repeated body, under the same finite source bound.
                let mut pending = vec![invocation.target];
                let mut related = HashSet::new();
                while let Some(target) = pending.pop() {
                    if related.len() >= 128 {
                        risk.unknown = true;
                        break;
                    }
                    if !related.insert(target.clone()) {
                        continue;
                    }
                    if let Some(alternatives) = writes.get(&target) {
                        for source in alternatives.iter().flat_map(|sources| sources.iter()) {
                            if writes.contains_key(source) {
                                pending.push(source.clone());
                            }
                        }
                    }
                }
                for target in related {
                    if self.loop_target_is_immediate_assign_or_inert(&target, &writes, scope) {
                        risk.immediate_targets.insert(target.clone());
                    }
                    risk.targets.insert(target);
                }
            }
        }
        risk
    }

    /// Only a copied, proven base `assign` value performs an immediate write
    /// without leaving a delayed or active binding behind. A successful later
    /// assertion may then establish a new scalar fact. The initial value and
    /// every loop-carried alternative must meet this bound; unknown aliases,
    /// promises, and active-binding installers keep persistent uncertainty.
    fn loop_target_is_immediate_assign_or_inert(
        &self,
        target: &str,
        writes: &HashMap<String, Vec<HashSet<String>>>,
        scope: &Scope,
    ) -> bool {
        if scope.dynamic_bindings_unknown
            || (!scope.inert_caller_binding_functions.contains(target)
                && !scope.function_alias(target).is_some_and(|alias| {
                    self.loop_source_is_immediate_assign_or_inert(
                        alias,
                        writes,
                        scope,
                        &mut HashSet::new(),
                        &mut 128,
                    )
                }))
        {
            return false;
        }
        writes.get(target).is_some_and(|alternatives| {
            alternatives.iter().all(|sources| {
                sources.iter().all(|source| {
                    self.loop_source_is_immediate_assign_or_inert(
                        source,
                        writes,
                        scope,
                        &mut HashSet::new(),
                        &mut 128,
                    )
                })
            })
        })
    }

    fn loop_source_is_immediate_assign_or_inert(
        &self,
        source: &str,
        writes: &HashMap<String, Vec<HashSet<String>>>,
        scope: &Scope,
        visiting: &mut HashSet<String>,
        remaining: &mut usize,
    ) -> bool {
        if *remaining == 0 || !visiting.insert(source.to_string()) {
            return false;
        }
        *remaining -= 1;
        let safe = if matches!(source, "base::assign" | "base:::assign") {
            true
        } else if let Some(alternatives) = writes.get(source) {
            alternatives.iter().all(|sources| {
                sources.iter().all(|value| {
                    self.loop_source_is_immediate_assign_or_inert(
                        value, writes, scope, visiting, remaining,
                    )
                })
            })
        } else if source == "assign" {
            self.resolves_to_base(source, scope)
        } else if scope.inert_caller_binding_functions.contains(source) {
            true
        } else if let Some(alias) = scope.function_alias(source) {
            self.loop_source_is_immediate_assign_or_inert(alias, writes, scope, visiting, remaining)
        } else {
            false
        };
        visiting.remove(source);
        safe
    }

    fn loop_assigned_source_may_install(
        &self,
        name: &str,
        inputs: &LoopSourceInputs<'_>,
        state: &mut LoopSourceState,
    ) -> bool {
        if let Some(effect) = state.completed.get(name) {
            return *effect;
        }
        if state.remaining == 0 || !state.visiting.insert(name.to_string()) {
            return true;
        }
        state.remaining -= 1;
        let effect = inputs.writes.get(name).is_some_and(|alternatives| {
            alternatives.iter().any(|sources| {
                sources.iter().any(|source| {
                    if inputs.writes.contains_key(source) {
                        self.loop_assigned_source_may_install(source, inputs, state)
                    } else {
                        self.computed_source_may_replace_current_binding(
                            source,
                            inputs.args,
                            inputs.scope,
                            &mut state.completed,
                            &mut state.visiting,
                            &mut state.remaining,
                        )
                    }
                })
            })
        });
        state.visiting.remove(name);
        state.completed.insert(name.to_string(), effect);
        effect
    }
}

pub(super) fn known_unclassed_vector(ty: &RType) -> bool {
    matches!(ty.length, Length::Known(n) if n > 1) && ty.class.known && ty.class.len == 0
}

#[derive(Default)]
pub(crate) struct LoopExitFrame {
    breaks: Option<Box<Scope>>,
    nexts: Option<Box<Scope>>,
    pub(super) risky_caller_binding_targets: HashSet<String>,
    pub(super) immediate_caller_binding_targets: HashSet<String>,
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
        .parameter_bindings
        .retain(|name| incoming.parameter_bindings.contains(name));
    joined
        .default_parameter_bindings
        .retain(|name| incoming.default_parameter_bindings.contains(name));
    joined
        .scalar_asserted_bindings
        .retain(|name| incoming.scalar_asserted_bindings.contains(name));
    joined
        .loop_vector_bindings
        .extend(incoming.loop_vector_bindings.iter().cloned());
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
                let parameter =
                    scope.is_parameter(&binding) && exit.parameter_bindings.contains(&binding);
                let default_parameter = parameter
                    && scope.is_default_parameter(&binding)
                    && exit.default_parameter_bindings.contains(&binding);
                let scalar_asserted = scalar_before.contains(&binding)
                    && exit.scalar_asserted_bindings.contains(&binding);
                let loop_vector = exit.loop_vector_bindings.contains(&binding)
                    || (!entered
                        && (vector_before.contains(&binding)
                            || scope.get(&binding).is_some_and(known_unclassed_vector)));
                if default_parameter {
                    scope.insert_parameter_default(binding.clone(), ty);
                } else if parameter {
                    scope.insert_parameter(binding.clone(), ty);
                } else {
                    scope.insert(binding.clone(), ty);
                }
                if list_origin {
                    scope.mark_list_origin(binding.clone());
                }
                if scalar_asserted {
                    scope.mark_scalar_asserted(&binding);
                }
                if loop_vector {
                    scope.mark_loop_vector(&binding);
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
                ..LoopExitFrame::default()
            });
            checker.finish_loop(&mut scope, inner, body_flag || break_flag, true);
            assert!(scope.has_escaped_slot_names);
            assert!(scope.bindings.is_empty());
            assert!(!checker.fn_table.has_escaped_slot_names);
            assert!(checker.has_explicit_operator_mask("@<-", "`@<-`", &scope));
        }
    }
}
