use super::*;
use crate::infer::*;

impl Checker {
    pub(crate) fn collect_fns(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.collect_fns_stmt(s);
        }
    }

    pub(crate) fn collect_fns_stmt(&mut self, s: &Stmt) {
        self.collect_declared_globals_stmt(s);
        match s {
            Stmt::Assign { target, value, .. } => {
                // Record every identifier-bound top-level assignment in
                // `known_vars`. This is independent of whether the RHS
                // is a function literal: regular variable assignments
                // (`my_const <- 42`, `GeomRect <- ggproto(...)`) need
                // to be resolvable from other files (and from later in
                // this same file) without triggering RY010.
                if let Some(name) = binding_name(target) {
                    Arc::make_mut(&mut self.fn_table)
                        .known_vars
                        .insert(name.to_string());
                }
                if let (Some(name), Expr::Function { params, body, .. }) =
                    (binding_name(target), value)
                {
                    // An S3 method named like `print.foo` is recorded both
                    // as a regular function (so the name resolves to its
                    // return type if called directly) and as an S3 method
                    // (so dispatch from `print(x)` on a classed value
                    // finds it). We record the body once and share the
                    // return slot between both entries.
                    //
                    // Group generics are unambiguous and may dispatch through
                    // `...` alone (notably `Summary.foo <- function(...)`).
                    // Other dotted names retain the first-parameter heuristic
                    // so ordinary helpers are not misregistered as methods.
                    let semantic_name = semantic_argument_name(name);
                    let looks_like_s3 =
                        split_s3_method_name(&semantic_name, &self.typeshed.globals)
                            .or_else(|| {
                                split_s3_operator_method_name(&semantic_name)
                                    .map(|(generic, class)| (generic.to_string(), class))
                            })
                            .filter(|(generic, _)| {
                                matches!(generic.as_str(), "Ops" | "Math" | "Summary" | "matrixOps")
                                    || params.first().is_some_and(|p| {
                                        p.name == "x"
                                            || (is_operator_generic(generic.as_str())
                                                && matches!(p.name.as_str(), "e1" | "e2"))
                                    })
                            });
                    if let Some((generic, class)) = looks_like_s3 {
                        let slot = self.record_fn(name.to_string(), params, body.clone());
                        Arc::make_mut(&mut self.fn_table)
                            .s3_methods
                            .insert((generic.to_string(), class), slot);
                    } else {
                        let _ = self.record_fn(name.to_string(), params, body.clone());
                    }
                    self.collect_forwarded_calls(name, params, body);
                    // Recurse into the function body so nested
                    // `inner <- function(...) ...` definitions are
                    // recorded with a mangled name. The mangled name is
                    // an internal implementation detail (not user-facing)
                    // used only so the fixpoint can refine the inner
                    // function's return type independently. Callers that
                    // close over the inner function via a captured
                    // `Function`-typed value go through `fn_sig` on the
                    // outer function's return type, not through this
                    // table entry.
                    self.collect_nested_fns_in_body(name, body);
                }
                // Non-function assignments: nothing further to record
                // (the name is already in `known_vars`).
            }
            Stmt::FunctionDef { name: Some(n), .. } => {
                // A bare top-level `function(params) body` literal in
                // statement position. If the parser gave it a name
                // (rare but possible for named-form function
                // definitions), record that name in `known_vars` so
                // cross-file references to it don't trigger RY010.
                Arc::make_mut(&mut self.fn_table)
                    .known_vars
                    .insert(n.clone());
            }
            Stmt::If { then, else_, .. } => {
                for s in then {
                    self.collect_fns_stmt(s);
                }
                if let Some(e) = else_ {
                    for s in e {
                        self.collect_fns_stmt(s);
                    }
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                // Loop bodies may contain function definitions (rare but
                // possible); recurse so we don't miss them.
                for s in body {
                    self.collect_fns_stmt(s);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn collect_declared_globals_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Assign { target, value, .. } => {
                self.collect_declared_globals_expr(target);
                self.collect_declared_globals_expr(value);
            }
            Stmt::Expr(e) => self.collect_declared_globals_expr(e),
            Stmt::If {
                cond, then, else_, ..
            } => {
                self.collect_declared_globals_expr(cond);
                for s in then {
                    self.collect_declared_globals_stmt(s);
                }
                if let Some(else_) = else_ {
                    for s in else_ {
                        self.collect_declared_globals_stmt(s);
                    }
                }
            }
            Stmt::For { iter, body, .. }
            | Stmt::While {
                cond: iter, body, ..
            } => {
                self.collect_declared_globals_expr(iter);
                for s in body {
                    self.collect_declared_globals_stmt(s);
                }
            }
            Stmt::FunctionDef { body, .. } => {
                for s in body {
                    self.collect_declared_globals_stmt(s);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    self.collect_declared_globals_expr(value);
                }
            }
        }
    }

    pub(crate) fn collect_declared_globals_expr(&mut self, e: &Expr) {
        match e {
            Expr::Call { func, args, .. } => {
                if let Expr::Ident { name, .. } = func.as_ref() {
                    let bare = name.rsplit_once("::").map(|(_, n)| n).unwrap_or(name);
                    Arc::make_mut(&mut self.fn_table)
                        .call_sites
                        .entry(bare.to_string())
                        .or_default()
                        .push(args.iter().map(|argument| argument.name.clone()).collect());
                    if bare == "globalVariables" {
                        if let Some(first) = args.first() {
                            for declared in string_literals(&first.value) {
                                Arc::make_mut(&mut self.fn_table)
                                    .known_vars
                                    .insert(declared);
                            }
                        }
                    }
                }
                self.collect_declared_globals_expr(func);
                for arg in args {
                    self.collect_declared_globals_expr(&arg.value);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.collect_declared_globals_expr(lhs);
                self.collect_declared_globals_expr(rhs);
            }
            Expr::UnaryOp { expr, .. } => self.collect_declared_globals_expr(expr),
            Expr::Index { base, args, .. } => {
                self.collect_declared_globals_expr(base);
                for arg in args {
                    self.collect_declared_globals_expr(&arg.value);
                }
            }
            Expr::Function { body, .. } => {
                for s in body {
                    self.collect_declared_globals_stmt(s);
                }
            }
            Expr::Block { body, .. } => {
                for s in body {
                    self.collect_declared_globals_stmt(s);
                }
            }
            Expr::If {
                cond, then, else_, ..
            } => {
                self.collect_declared_globals_expr(cond);
                self.collect_declared_globals_expr(then);
                if let Some(else_) = else_ {
                    self.collect_declared_globals_expr(else_);
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
    }

    pub(crate) fn collect_forwarded_calls(
        &mut self,
        caller: &str,
        params: &[Param],
        body: &[Stmt],
    ) {
        let mut calls = Vec::new();
        collect_forwarded_calls_in_stmts(caller, params, body, &mut calls);
        Arc::make_mut(&mut self.fn_table)
            .forwarded_calls
            .extend(calls);
    }

    // Walk a function body looking for `inner <- function(...) ...`
    // definitions and record them with the mangled name
    // `<outer>$<inner>`. The mangled name is internal: it exists so
    // the fixpoint can refine the inner function's return type, which
    // `refine_fn_return` reads back when building the outer function's
    // `fn_sig`. Users never see this name.
    //
    // Recursion is bounded by the AST's literal nesting (small in
    // practice). The inference depth is separately bounded by
    // `MAX_CLOSURE_DEPTH` in `build_function_signature`.
    pub(crate) fn collect_nested_fns_in_body(&mut self, outer: &str, body: &[Stmt]) {
        for s in body {
            self.collect_nested_fns_stmt(outer, s);
        }
    }

    // Per-statement helper for `collect_nested_fns_in_body`. Records
    // any `inner <- function(...) ...` under `<outer>$<inner>` and
    // recurses into compound statements so we catch nested defs
    // inside `if` / `for` / `while` blocks too.
    pub(crate) fn collect_nested_fns_stmt(&mut self, outer: &str, s: &Stmt) {
        match s {
            Stmt::Assign { target, value, .. } => {
                if let (
                    Expr::Ident { name: inner, .. },
                    Expr::Function {
                        params,
                        body: inner_body,
                        ..
                    },
                ) = (target, value)
                {
                    let mangled = format!("{}${}", outer, inner);
                    let next_outer = mangled.clone();
                    let _ = self.record_fn(mangled, params, inner_body.clone());
                    // Recurse one more level so doubly-nested factories
                    // are also collected.
                    self.collect_nested_fns_in_body(&next_outer, inner_body);
                }
            }
            Stmt::If { then, else_, .. } => {
                for s in then {
                    self.collect_nested_fns_stmt(outer, s);
                }
                if let Some(e) = else_ {
                    for s in e {
                        self.collect_nested_fns_stmt(outer, s);
                    }
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                for s in body {
                    self.collect_nested_fns_stmt(outer, s);
                }
            }
            _ => {}
        }
    }

    // Record a user-defined function. Returns the index of the
    // allocated return slot so callers can wire up S3 dispatch entries
    // that share the same slot.
    pub(crate) fn record_fn(&mut self, name: String, params: &[Param], body: Vec<Stmt>) -> usize {
        // We infer param types from defaults alone; params without a
        // default start as UNKNOWN (callers can refine them later).
        let params: Vec<UserParam> = params
            .iter()
            .map(|p| {
                let t = match &p.default {
                    // Defer inference to first fixpoint iteration by
                    // starting as UNKNOWN; if a literal default is present
                    // we can compute it now without a scope.
                    Some(e) => infer_literal_default(e),
                    None => RType::unknown(),
                };
                let required =
                    p.name != "..." && p.default.is_none() && block_must_force_name(&body, &p.name);
                UserParam {
                    name: p.name.clone(),
                    type_: t,
                    required,
                }
            })
            .collect();
        let slot = self.return_slots.0.len();
        Arc::make_mut(&mut self.return_slots).set(slot, RType::unknown());
        // Wrap the body in an Rc so the per-fixpoint clone in
        // refine_fn_return is a refcount bump, not a deep copy.
        let body: Arc<[Stmt]> = Arc::from(body);
        let prev = Arc::make_mut(&mut self.fn_table).fns.insert(
            name.clone(),
            UserFn {
                params,
                body,
                return_slot: slot,
            },
        );
        if let Some(prev) = prev {
            tracing::debug!(fn_name = %name, prev_slot = prev.return_slot, "shadowed earlier def");
        }
        slot
    }

    // Pass 2: refine one function's inferred return type by walking its
    // body once. Returns are collected from `return(...)` calls and from
    // the trailing expression of the body, then joined.
    pub(crate) fn refine_fn_return(&mut self, name: &str) {
        // Pull the body out by reference so we can re-borrow self during
        // the walk. We can't simply clone the body since that's expensive
        // for large functions; instead we snapshot the slot index.
        let (body_clone, params, slot) = match self.fn_table.fns.get(name) {
            Some(f) => (f.body.clone(), f.params.clone(), f.return_slot),
            None => return,
        };
        // Cycle detection: if this function is already on the inference
        // stack, leave its return as UNKNOWN and bail out. The fixpoint
        // will converge on subsequent iterations.
        if self.inferring.iter().any(|n| n == name) {
            return;
        }
        self.inferring.push(name.to_string());

        let mut scope = Scope::default();
        for parameter in &params {
            scope.insert(parameter.name.clone(), parameter.type_.clone());
        }
        // The function's own name is in scope as a function value, so
        // recursive calls resolve to a user-fn lookup.
        scope.insert(name.to_string(), RType::scalar(Mode::Function));
        insert_s3_dispatch_context(name, &mut scope, &self.typeshed.globals);

        let mut returns: Vec<RType> = Vec::new();
        // Walk the body via the unified walker in discarding mode, with
        // return collection enabled. The discarding flag is set by the
        // caller (refine_fn_return runs inside the fixpoint which sets
        // discarding=true at the run_fixpoint entry).
        for s in body_clone.iter() {
            self.walk_stmt(s, &mut scope, Some(&mut returns));
        }
        // Trailing expression of a braced body is the implicit return.
        // A trailing `Stmt::FunctionDef` is the implicit return value
        // for the `function() { function() { 1L } }` shape;
        // `trailing_return_type` handles both forms and attaches an
        // inferred `fn_sig` when the trailing expression is itself a
        // function literal (the closure-factory pattern).
        if let Some(t) = self.trailing_return_type(&body_clone[..], &mut scope, 0) {
            returns.push(t);
        }

        // Fold the collected return types. We start from the first
        // element rather than UNKNOWN because join() treats Opaque as
        // absorbing (correct for control-flow merge but wrong for an
        // empty-fold identity).
        let joined = if returns.is_empty() {
            RType::unknown()
        } else {
            let mut iter = returns.into_iter();
            let first = iter.next().unwrap_or(RType::unknown());
            iter.fold(first, |acc, t| acc.join(t))
        };
        Arc::make_mut(&mut self.return_slots).set(slot, joined);
        self.inferring.pop();
    }

    // The unified statement walker. Handles BOTH diagnostic emission (gated by
    // `self.discarding`) AND return-type collection (when `returns` is
    // `Some`).
    //
    // Callers:
    //   * `check_stmt` (pass 3): discarding=false, returns=None.
    //   * `refine_fn_return` (pass 2 fixpoint): discarding=true (set by
    //     caller), returns=Some.
    //   * `build_function_signature` (closure literals, both passes):
    //     discarding=true (set by caller), returns=Some.
    //
    // Approximations (documented):
    //   * `if` branches use `apply_narrowing` + separate child scopes
    //     (then/else); bindings leak into subsequent statements.
    //   * Loop bodies are walked once (not to fixpoint).
    //   * Indexed assignment (`x[i] <- v`) does not update the scope.
}

/// R permits a no-default formal to remain missing when the body never forces
/// it. Passing a formal onward is also not proof of forcing because the callee
/// may inspect `missing()` or ignore it. Required-argument diagnostics for user
/// functions therefore use this deliberately conservative must-force scan.
fn block_must_force_name(statements: &[Stmt], name: &str) -> bool {
    for statement in statements {
        let (forces, always_falls_through) = statement_force_flow(statement, name);
        if forces {
            return true;
        }
        if !always_falls_through {
            return false;
        }
    }
    false
}

/// Return `(must_force_name, always_falls_through)` for one statement.
fn statement_force_flow(statement: &Stmt, name: &str) -> (bool, bool) {
    match statement {
        Stmt::Assign { value, .. } | Stmt::Expr(value) => {
            if let Some(returned) = return_call_value(value) {
                (
                    returned.is_some_and(|value| expression_must_force(value, name)),
                    false,
                )
            } else {
                (expression_must_force(value, name), true)
            }
        }
        Stmt::If {
            cond, then, else_, ..
        } => {
            let then_forces = block_must_force_name(then, name);
            let else_forces = else_
                .as_ref()
                .is_some_and(|statements| block_must_force_name(statements, name));
            let forces = expression_must_force(cond, name) || (then_forces && else_forces);
            let then_falls = block_always_falls_through(then);
            let else_falls = else_
                .as_ref()
                .is_none_or(|statements| block_always_falls_through(statements));
            (forces, then_falls && else_falls)
        }
        Stmt::For { iter, body, .. } => (
            expression_must_force(iter, name),
            block_always_falls_through(body),
        ),
        // A while loop may not terminate, so later statements are not
        // guaranteed to execute even when its body contains no return.
        Stmt::While { cond, .. } => (expression_must_force(cond, name), false),
        Stmt::Return { value, .. } => (
            value
                .as_ref()
                .is_some_and(|value| expression_must_force(value, name)),
            false,
        ),
        // A nested closure may capture the formal without forcing it during
        // the outer call.
        Stmt::FunctionDef { .. } => (false, true),
    }
}

fn return_call_value(expression: &Expr) -> Option<Option<&Expr>> {
    let Expr::Call { func, args, .. } = expression else {
        return None;
    };
    if !matches!(func.as_ref(), Expr::Ident { name, .. } if name == "return") {
        return None;
    }
    Some(args.first().map(|argument| &argument.value))
}

fn block_always_falls_through(statements: &[Stmt]) -> bool {
    statements
        .iter()
        .all(|statement| statement_force_flow(statement, "\0").1)
}

fn expression_must_force(expression: &Expr, name: &str) -> bool {
    match expression {
        Expr::Ident {
            name: identifier, ..
        } => identifier == name,
        Expr::BinOp { op, lhs, rhs, .. } => {
            expression_must_force(lhs, name)
                || (!matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr)
                    && expression_must_force(rhs, name))
        }
        Expr::UnaryOp { expr, .. } => expression_must_force(expr, name),
        Expr::Index { base, args, .. } => {
            expression_must_force(base, name)
                || args
                    .iter()
                    .any(|argument| expression_must_force(&argument.value, name))
        }
        // R promises remain lazy across ordinary calls, so forwarding is not
        // sufficient evidence that this function requires the argument.
        Expr::Call { func, .. } => expression_must_force(func, name),
        Expr::Block { body, .. } => block_must_force_name(body, name),
        Expr::If {
            cond, then, else_, ..
        } => {
            expression_must_force(cond, name)
                || (expression_must_force(then, name)
                    && else_
                        .as_ref()
                        .is_some_and(|else_| expression_must_force(else_, name)))
        }
        Expr::Function { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => false,
    }
}
