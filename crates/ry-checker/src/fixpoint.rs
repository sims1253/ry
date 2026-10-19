use super::*;

/// Resolve method identities once; only their parameter metadata changes
/// during refinement. Include every dot prefix because generic names may
/// themselves contain dots.
pub(crate) fn s3_evaluation_methods(table: &FnTable) -> HashMap<String, Vec<String>> {
    let mut by_generic: HashMap<&str, HashSet<usize>> = HashMap::new();
    for (name, function) in table.functions.live() {
        let name = semantic_argument_name(name);
        for (dot, _) in name.match_indices('.') {
            if dot + 1 < name.len() {
                by_generic
                    .entry(&name[..dot])
                    .or_default()
                    .insert(function.return_slot);
            }
        }
    }
    for ((generic, _), slot) in &table.s3_methods {
        by_generic.entry(generic).or_default().insert(*slot);
    }
    let by_slot: HashMap<_, _> = table
        .functions
        .live()
        .map(|(name, function)| (function.return_slot, name))
        .collect();
    table
        .functions
        .live()
        .filter_map(|(name, function)| {
            let dispatch = usemethod_generic_name(&function.body)?;
            if semantic_argument_name(name) != dispatch {
                return None;
            }
            let methods = by_generic
                .get(dispatch.as_str())
                .into_iter()
                .flatten()
                .filter_map(|slot| by_slot.get(slot).map(|name| (*name).clone()))
                .collect();
            Some((name.clone(), methods))
        })
        .collect()
}

/// Strongly connected components of a dependency graph, each listed after
/// every component it depends on. Iterative Tarjan: call chains in large
/// packages are deeper than a recursive walk should go.
fn components_after_dependencies(dependencies: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let unvisited = usize::MAX;
    let mut order = vec![unvisited; dependencies.len()];
    let mut low = vec![0; dependencies.len()];
    let mut on_stack = vec![false; dependencies.len()];
    let mut stack = Vec::new();
    let mut components = Vec::new();
    let mut next = 0;
    for root in 0..dependencies.len() {
        if order[root] != unvisited {
            continue;
        }
        let mut frames = vec![(root, 0)];
        order[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(frame) = frames.last_mut() {
            let node = frame.0;
            if let Some(&target) = dependencies[node].get(frame.1) {
                frame.1 += 1;
                if order[target] == unvisited {
                    order[target] = next;
                    low[target] = next;
                    next += 1;
                    stack.push(target);
                    on_stack[target] = true;
                    frames.push((target, 0));
                } else if on_stack[target] {
                    low[node] = low[node].min(order[target]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == order[node] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                component.sort_unstable();
                components.push(component);
            }
        }
    }
    components
}

impl Checker {
    // Pass 2: refine all function return types until convergence.
    // Safe to call once, after all files have been collected.
    //
    // S3 methods (`print.foo`, etc.) sit in `fns` under their full
    // name, with `s3_methods` pointing at the same return slot, so
    // iterating `fns` refines their bodies alongside regular
    // functions; dispatch reads the refined slot via `s3_methods`.
    pub(crate) fn run_fixpoint(&mut self) {
        self.run_fixpoint_scoped(None);
    }

    /// Run the fixpoint, but only refine functions in `scope` when given.
    /// Functions outside the scope keep their current (seeded) return type.
    /// Used by `Project` for incremental checks where only a subset of
    /// functions can have changed.
    ///
    /// The set must include every function whose definition or callees
    /// changed; functions outside the set are assumed stable. The fixpoint
    /// still iterates until convergence *within the scope* — a scoped
    /// function whose return type changes can still affect other scoped
    /// functions that call it.
    ///
    /// Scoped returns start at bottom (not computed): a call to such a
    /// function makes its path unreachable, so recursion keeps its base
    /// case. The first pass visits functions in name order to observe
    /// their reads; later passes visit strongly connected components of
    /// that read graph callee-first, so acyclic chains settle in one pass
    /// and only recursive components iterate.
    pub(crate) fn run_fixpoint_scoped(&mut self, scope: Option<&HashSet<String>>) {
        if scope.is_some_and(|s| s.is_empty()) {
            return;
        }
        let prev_discarding = self.discarding;
        self.discarding = true;
        let mut names: Vec<_> = self.fn_table.functions.names().cloned().collect();
        names.sort_unstable();
        let slots: Vec<_> = names
            .iter()
            .map(|name| self.fn_table.functions[name].return_slot)
            .collect();
        let active: Vec<_> = names
            .iter()
            .map(|name| scope.is_none_or(|scope| scope.contains(name)))
            .collect();
        let mut owners = vec![Vec::new(); self.return_slots.0.len()];
        for (index, slot) in slots.iter().enumerate() {
            owners[*slot].push(index);
        }
        let mut pending: std::collections::BTreeSet<_> =
            (0..names.len()).filter(|&index| active[index]).collect();
        self.uncomputed_returns = pending.iter().map(|&index| slots[index]).collect();
        let mut callers: Vec<HashSet<usize>> = vec![HashSet::new(); self.return_slots.0.len()];
        let methods = s3_evaluation_methods(&self.fn_table);
        let mut reads = Vec::new();
        let mut evaluation_pending = true;
        let mut passes = 0;
        while passes < MAX_FIXPOINT_DEPTH && (!pending.is_empty() || evaluation_pending) {
            passes += 1;
            let attached_before = (self.loaded.len(), self.bare_loaded.len());
            let components: Vec<Vec<usize>> = if passes == 1 {
                pending.iter().map(|&index| vec![index]).collect()
            } else {
                let mut dependencies = vec![Vec::new(); names.len()];
                for (slot, readers) in callers.iter().enumerate() {
                    for &reader in readers {
                        dependencies[reader].extend(&owners[slot]);
                    }
                }
                components_after_dependencies(&dependencies)
            };
            let mut component_of = vec![usize::MAX; names.len()];
            for (id, component) in components.iter().enumerate() {
                for &index in component {
                    component_of[index] = id;
                }
            }
            for (id, component) in components.iter().enumerate() {
                let mut queue: std::collections::BTreeSet<_> = component
                    .iter()
                    .copied()
                    .filter(|index| pending.remove(index))
                    .collect();
                let mut budget = MAX_FIXPOINT_DEPTH * component.len();
                loop {
                    let mut changed = Vec::new();
                    // Termination bound for a recursive component.
                    let widen = budget == 0 && !queue.is_empty();
                    if widen {
                        queue.clear();
                        for &member in component {
                            if self.store_return(slots[member], RType::unknown()) {
                                changed.push(slots[member]);
                            }
                        }
                    } else if let Some(index) = queue.pop_first() {
                        budget -= 1;
                        *self.refinement_reads.get_mut() = Some(std::mem::take(&mut reads));
                        let result = self.refine_fn_return(&names[index]);
                        reads = self.refinement_reads.get_mut().take().unwrap();
                        let read_bottom = reads
                            .iter()
                            .any(|slot| self.uncomputed_returns.contains(slot));
                        for dependency in reads.drain(..) {
                            callers[dependency].insert(index);
                        }
                        // Without a returning path through computed callees,
                        // the return stays bottom.
                        if let Some(result) = result.or_else(|| (!read_bottom).then(RType::unknown))
                            && self.store_return(slots[index], result)
                        {
                            changed.push(slots[index]);
                        }
                    } else {
                        // Members still at bottom only reach each other.
                        // Before the read graph is known, revisit them in the
                        // next pass; afterwards no path returns a known value.
                        let stuck = component
                            .iter()
                            .copied()
                            .filter(|&member| self.uncomputed_returns.contains(&slots[member]));
                        if passes == 1 {
                            pending.extend(stuck);
                            break;
                        }
                        changed.extend(stuck.map(|member| slots[member]));
                        if changed.is_empty() {
                            break;
                        }
                        for &slot in &changed {
                            self.store_return(slot, RType::unknown());
                        }
                    }
                    for slot in changed {
                        for &caller in &callers[slot] {
                            if component_of[caller] != id {
                                pending.insert(caller);
                            } else if !widen {
                                queue.insert(caller);
                            }
                        }
                    }
                    if widen {
                        break;
                    }
                }
            }
            let attachments_changed =
                attached_before != (self.loaded.len(), self.bare_loaded.len());
            let metadata_changed = if evaluation_pending || attachments_changed {
                let mut changed = self.propagate_s3_generic_evaluation(&methods);
                changed.extend(self.propagate_forwarded_evaluation());
                changed
            } else {
                HashSet::new()
            };
            // Returns do not affect argument evaluation metadata. Once the
            // latter converges, only an attachment can invalidate it.
            evaluation_pending = !metadata_changed.is_empty();
            // A parameter's evaluation mode can affect its own body as well
            // as callers, even when its return slot has not changed yet.
            for slot in metadata_changed {
                pending.extend(owners[slot].iter().filter(|&&index| active[index]));
                pending.extend(callers[slot].iter().copied());
            }
            // Inference can discover library()/require() through an alias.
            // Attaching a package changes bare-name resolution for every body.
            if attachments_changed {
                pending.extend((0..names.len()).filter(|&index| active[index]));
            }
        }
        tracing::debug!(
            passes,
            scoped = scope.is_some(),
            converged = pending.is_empty() && !evaluation_pending,
            "fixpoint finished"
        );
        // Bottom slots are stored as unknown; only the marker is dropped.
        self.uncomputed_returns.clear();
        if let Some(dependencies) = &mut self.refinement_dependencies {
            // Slots are local to the rebuilt table. Persist owning names, and
            // retain reads from every round rather than only the final pass.
            for (index, name) in names.iter().enumerate() {
                if active[index] {
                    dependencies.insert(name.clone(), HashSet::new());
                }
            }
            for (slot, readers) in callers.iter().enumerate() {
                for reader in readers {
                    let reads = dependencies.get_mut(&names[*reader]).unwrap();
                    reads.extend(owners[slot].iter().map(|index| names[*index].clone()));
                }
            }
        }
        self.discarding = prev_discarding;
    }

    /// Store a computed return. Leaving bottom counts as a change even when
    /// the stored placeholder already equals the result.
    fn store_return(&mut self, slot: usize, result: RType) -> bool {
        let was_bottom = self.uncomputed_returns.remove(&slot);
        if self.return_slots.0.get(slot) == Some(&result) {
            return was_bottom;
        }
        Arc::make_mut(&mut self.return_slots).set(slot, result);
        true
    }

    pub(crate) fn record_signature_read(&self, slot: usize) {
        if let Some(reads) = self.refinement_reads.borrow_mut().as_mut() {
            reads.push(slot);
        }
    }

    pub(crate) fn read_return_slot(&self, slot: usize) -> RType {
        self.record_signature_read(slot);
        self.return_slots.get(slot)
    }

    /// A generic must allow the quoting and injection behavior of its known
    /// methods. Union their parameter metadata because any selected method
    /// may capture the supplied arguments.
    fn propagate_s3_generic_evaluation(
        &mut self,
        methods: &HashMap<String, Vec<String>>,
    ) -> HashSet<usize> {
        let mut inherited = Vec::new();
        for (name, method_names) in methods {
            let generic = &self.fn_table.functions[name];
            let dots = generic
                .params
                .iter()
                .position(|parameter| parameter.name == "...");
            for method_name in method_names {
                let method = &self.fn_table.functions[method_name];
                for parameter in &method.params {
                    if !parameter.quoting && parameter.injection.is_none() {
                        continue;
                    }
                    let target = match generic
                        .params
                        .iter()
                        .position(|generic_parameter| generic_parameter.name == parameter.name)
                    {
                        // A method formal with the same name is matched by
                        // that generic formal, regardless of its position.
                        Some(position) => Some(position),
                        // A named method formal absent from the generic is
                        // supplied through the generic's dots just like a
                        // method dots formal.  This is the common S3 shape
                        // `generic(x, ...)` / `generic.class(x, column, ...)`.
                        None => dots,
                    };
                    if let Some(target) = target {
                        inherited.push((
                            name.clone(),
                            target,
                            parameter.quoting,
                            parameter.injection,
                        ));
                    }
                }
            }
        }

        let table = Arc::make_mut(&mut self.fn_table);
        let mut changed = HashSet::new();
        for (generic, position, quoting, injection) in inherited {
            let slot = table.functions[&generic].return_slot;
            if let Some(parameter) = table
                .functions
                .live_mut(&generic)
                .and_then(|function| function.params.get_mut(position))
            {
                if quoting && !parameter.quoting {
                    parameter.quoting = true;
                    changed.insert(slot);
                }
                if injection > parameter.injection {
                    parameter.injection = injection;
                    changed.insert(slot);
                }
            }
        }
        changed
    }

    /// Propagate user-NSE metadata across direct formal forwarding.
    ///
    /// `ForwardedCall` is collected syntactically, so an argument is present
    /// here only when its value was an identifier.  This deliberately excludes
    /// expressions such as `callee(p + 1)` and nested calls such as
    /// `callee(f(p))`, which evaluate `p` before the callee can capture it.
    fn propagate_forwarded_evaluation(&mut self) -> HashSet<usize> {
        let mut inherited = Vec::new();

        for call in &self.fn_table.forwarded_calls {
            let Some(caller) = self.fn_table.functions.live_named(&call.caller) else {
                continue;
            };

            // Forwarding and direct calls use the same package provenance
            // when selecting a collected definition.
            let user_callee = self
                .fn_table
                .functions
                .select(Callee::parse(&call.stub_callee));
            let stub_callee = self.resolve_typeshed_sig(&call.stub_callee);
            if user_callee.is_none() && stub_callee.is_none() {
                for (_, source) in &call.arguments {
                    if let Some(source) = source
                        && caller.params.iter().any(|param| param.name == *source)
                    {
                        inherited.push((
                            call.caller.clone(),
                            source.clone(),
                            false,
                            false,
                            Some(InjectionMode::Full),
                        ));
                    }
                }
                continue;
            }

            let names: Vec<&str> = if let Some(callee) = user_callee {
                callee
                    .params
                    .iter()
                    .map(|param| param.name.as_str())
                    .collect()
            } else {
                stub_callee
                    .as_ref()
                    .unwrap()
                    .params
                    .iter()
                    .map(|param| param.name.as_str())
                    .collect()
            };
            let bindings = infer::match_argument_names(
                &names,
                call.arguments.iter().map(|(name, _)| name.as_deref()),
            );
            for (index, (_, source)) in call.arguments.iter().enumerate() {
                let Some(source) = source else {
                    continue;
                };
                let target = if source == "..." {
                    bindings.dots
                } else {
                    bindings.param_for_arg[index].or(bindings.dots)
                };
                let Some(target) = target else {
                    continue;
                };
                // Capturing a promise does not imply quoting every caller expression:
                // mixed-use wrappers still need diagnostics inside their arguments.
                let inherits_quoting = if let Some(callee) = user_callee {
                    callee.params[target].quoting
                } else {
                    stub_callee.as_ref().is_some_and(|sig| {
                        matches!(
                            sig.eval.get(names[target]),
                            Some(EvalMode::QuotedExpression | EvalMode::QuotedSymbol)
                        )
                    })
                };
                // Dots capture is already modeled as defusing (rather than
                // quoting) so its direct arguments remain opaque.  Preserve
                // that stronger behavior while forwarding `...` to another
                // dots-capturing user function.
                let inherits_defusing = source == "..."
                    && if let Some(callee) = user_callee {
                        callee.params[target].defused
                    } else {
                        stub_callee.as_ref().is_some_and(|sig| {
                            matches!(
                                sig.eval.get(names[target]),
                                Some(EvalMode::DataMask | EvalMode::TidySelect)
                            )
                        })
                    };
                let inherits_injection = if let Some(callee) = user_callee {
                    callee.params[target].injection
                } else {
                    stub_callee
                        .as_ref()
                        .and_then(|sig| sig.injection.get(names[target]).copied())
                };
                if (inherits_quoting || inherits_defusing || inherits_injection.is_some())
                    && caller.params.iter().any(|param| param.name == *source)
                {
                    inherited.push((
                        call.caller.clone(),
                        source.clone(),
                        inherits_quoting,
                        inherits_defusing,
                        inherits_injection,
                    ));
                }
            }
        }

        let table = Arc::make_mut(&mut self.fn_table);
        let mut changed = HashSet::new();
        for (caller, parameter, quoting, defused, injection) in inherited {
            let slot = table.functions[&caller].return_slot;
            if let Some(parameter) = table
                .functions
                .live_mut(&caller)
                .and_then(|function| function.params.iter_mut().find(|p| p.name == parameter))
            {
                if quoting && !parameter.quoting {
                    parameter.quoting = true;
                    changed.insert(slot);
                }
                if defused && !parameter.defused {
                    parameter.defused = true;
                    changed.insert(slot);
                }
                if injection > parameter.injection {
                    parameter.injection = injection;
                    changed.insert(slot);
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_core::RParser;

    fn collected(source: &str) -> Checker {
        let file = RParser::new().unwrap().parse("fixpoint.R", source).unwrap();
        let mut checker = Checker::new("fixpoint.R");
        checker.collect_file_fns(&file);
        checker
    }

    // Full sweeps to convergence are an independent scheduling oracle.
    // Comparing complete signatures catches skipped metadata edges as well
    // as returns.
    fn full_sweep(checker: &mut Checker) {
        checker.discarding = true;
        let mut names: Vec<_> = checker.fn_table.functions.names().cloned().collect();
        names.sort_unstable();
        let methods = s3_evaluation_methods(&checker.fn_table);
        checker.uncomputed_returns = names
            .iter()
            .map(|name| checker.fn_table.functions[name].return_slot)
            .collect();
        for _ in 0..100 {
            let mut changed = false;
            for name in &names {
                *checker.refinement_reads.get_mut() = Some(Vec::new());
                let result = checker.refine_fn_return(name);
                let reads = checker.refinement_reads.get_mut().take().unwrap();
                let bottom = reads
                    .iter()
                    .any(|slot| checker.uncomputed_returns.contains(slot));
                if let Some(result) = result.or_else(|| (!bottom).then(RType::unknown)) {
                    changed |=
                        checker.store_return(checker.fn_table.functions[name].return_slot, result);
                }
            }
            changed |= !checker.propagate_s3_generic_evaluation(&methods).is_empty();
            changed |= !checker.propagate_forwarded_evaluation().is_empty();
            if !changed {
                // Returns still at bottom never produce a known value.
                if checker.uncomputed_returns.is_empty() {
                    checker.discarding = false;
                    return;
                }
                checker.uncomputed_returns.clear();
            }
        }
        panic!("full sweep did not converge");
    }

    fn compare(source: &str) -> (Checker, Checker) {
        let mut scheduled = collected(source);
        let mut swept = collected(source);
        scheduled.run_fixpoint();
        full_sweep(&mut swept);
        assert_eq!(scheduled.return_slots.0, swept.return_slots.0);
        for (name, function) in scheduled.fn_table.functions.live() {
            assert_eq!(
                function.params, swept.fn_table.functions[name].params,
                "{name}"
            );
        }
        (scheduled, swept)
    }

    #[test]
    fn stable_functions_are_not_refined_again() {
        let mut source = String::new();
        for index in 0..100 {
            source.push_str(&format!("stable{index} <- function() 1L\n"));
        }
        source.push_str("aa <- function() bb()\nbb <- function() cc()\ncc <- function() dd()\ndd <- function() 1L\n");
        let (scheduled, swept) = compare(&source);
        assert_eq!(scheduled.refinement_counts["stable0"], 1);
        assert_eq!(scheduled.refinement_counts["aa"], 2);
        let visits = |checker: &Checker| checker.refinement_counts.values().sum::<usize>();
        assert_eq!(visits(&scheduled), 107);
        assert_eq!(visits(&swept), 520);
    }

    #[test]
    fn scoped_refinement_finishes_metadata_outside_its_body_scope() {
        let mut checker = collected(
            "a <- function(x) b(x)\nb <- function(x) cc(x)\ncc <- function(x) substitute(x)\nstable <- function() 1L",
        );
        checker.run_fixpoint_scoped(Some(&HashSet::from(["stable".to_string()])));
        assert!(checker.fn_table.functions["a"].params[0].quoting);
        assert!(!checker.refinement_counts.contains_key("a"));
    }

    #[test]
    fn replacing_a_function_discards_its_old_forwarding() {
        let first = "capture <- function(x) substitute(x)\nf <- function(x) capture(x)\n";
        let second = "f <- function(x) x";
        let mut same_file = collected(&format!("{first}{second}"));
        same_file.run_fixpoint();
        assert!(!same_file.fn_table.functions["f"].params[0].quoting);

        let mut combined = collected(first);
        let replacement = collected(second);
        Arc::make_mut(&mut combined.fn_table).append_collected(
            &replacement.fn_table,
            Arc::make_mut(&mut combined.return_slots),
            &replacement.return_slots,
        );
        combined.run_fixpoint();
        assert!(!combined.fn_table.functions["f"].params[0].quoting);
    }

    #[test]
    fn deep_chains_converge_in_either_name_order() {
        for descending in [false, true] {
            let name = |index: usize| {
                let index = if descending { 20 - index } else { index };
                format!("f{index:02}")
            };
            let mut source = String::new();
            for index in 0..20 {
                source.push_str(&format!(
                    "{} <- function() {}()\n",
                    name(index),
                    name(index + 1)
                ));
            }
            source.push_str(&format!("{} <- function() 1L\n", name(20)));
            let (scheduled, _) = compare(&source);
            let top = &scheduled.fn_table.functions[&name(0)];
            assert_eq!(
                scheduled.return_slots.get(top.return_slot).mode,
                Mode::Integer
            );
            // One discovery visit, then one callee-first visit.
            assert!(
                scheduled
                    .refinement_counts
                    .values()
                    .all(|&count| count <= 2)
            );
        }
    }

    #[test]
    fn recursion_keeps_its_base_case() {
        let (scheduled, _) = compare(
            "f <- function(n) if (n == 0L) 'oops' else f(n - 1L)\n\
             g <- function(n) { if (n == 0L) return(1L); h(n - 1L) }\n\
             h <- function(n) g(n)\n\
             loop <- function() loop()\n\
             uses_loop <- function(x) if (x) loop() else 1L\n\
             count <- function(n) if (n == 0L) 0L else count(n - 1L) + 1L",
        );
        let returns = |name: &str| {
            scheduled
                .return_slots
                .get(scheduled.fn_table.functions[name].return_slot)
        };
        assert_eq!(returns("f").mode, Mode::Character);
        assert_eq!(returns("g").mode, Mode::Integer);
        assert_eq!(returns("h").mode, Mode::Integer);
        assert_eq!(returns("count").mode, Mode::Integer);
        // A component with no returning path leaves bottom as unknown.
        assert_eq!(returns("loop"), RType::unknown());
        assert_eq!(returns("uses_loop"), RType::unknown());
        assert!(scheduled.uncomputed_returns.is_empty());
    }

    #[test]
    fn non_converging_recursion_widens_to_unknown() {
        // Each round nests the column schema one level deeper.
        let mut checker = collected("grow <- function(x) if (x) list(a = grow(x)) else 1L");
        checker.run_fixpoint();
        let grow = &checker.fn_table.functions["grow"];
        assert_eq!(checker.return_slots.get(grow.return_slot), RType::unknown());
        assert_eq!(checker.refinement_counts["grow"], MAX_FIXPOINT_DEPTH);
    }

    #[test]
    fn metadata_callbacks_dispatch_and_cycles_match_full_sweeps() {
        for source in [
            "setClass('Widget', slots = c(value = 'numeric'))\nsetMethod('labels', signature('Widget'), function(object) zz_leaf())\naa <- function() labels(new('Widget'))\nzz_leaf <- function() 'ok'",
            "aa <- function() map_int(1L, function(x) 1L)\nzz <- function() library(purrr)",
            "a <- function(x) b(x)\nb <- function(x) cc(x)\ncc <- function(x) substitute(x)",
            "a <- function(...) b(...)\nb <- function(...) cc(...)\ncc <- function(...) dplyr::mutate(df, ...)",
            "a <- function(x) b(x)\nb <- function(x) cc(x)\ncc <- function(x) rlang::inject(x)",
            "a <- function() lapply(1:3, b)\nb <- function(x) cc()\ncc <- function() 1L",
            "a <- function() g(structure(1L, class = 'foo'))\ng <- function(x) UseMethod('g')\ng.foo <- function(x) z()\nz <- function() 1L",
            "a <- function(x) g(x)\ng <- function(x) UseMethod('g')\ng.foo <- function(x) z(x)\nz <- function(x) substitute(x)",
            "a <- function(x) b(x)\nb <- function(x) if (x) a(x) else 1L",
            "a <- function() b()()\nb <- function() function() cc()\ncc <- function() 1L",
            "a <- function() b(1L)\nb <- function(x) if (missing(x)) 1L else cc()\ncc <- function() 1L",
        ] {
            compare(source);
        }
    }
}
