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
                    if bare == "assign"
                        && args.iter().any(|arg| {
                            arg.name.as_deref() == Some("envir")
                                && matches!(
                                    &arg.value,
                                    Expr::Call { func, .. }
                                        if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "asNamespace")
                                )
                        })
                        && let Some(first) = args.first()
                        && let Some(binding) = string_literal(&first.value)
                    {
                        Arc::make_mut(&mut self.fn_table)
                            .known_vars
                            .insert(binding.to_string());
                    }
                    self.collect_s4_call(bare, args);
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

    fn collect_s4_call(&mut self, name: &str, args: &[Arg]) {
        match name {
            "setClass" => {
                let Some(class) = args.first().and_then(|arg| string_literal(&arg.value)) else {
                    return;
                };
                let slots_expr = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("slots"))
                    .or_else(|| args.get(1))
                    .map(|arg| &arg.value);
                let slots = slots_expr.map(s4_slots).unwrap_or_default();
                Arc::make_mut(&mut self.fn_table)
                    .s4_classes
                    .insert(class.to_string(), slots);
            }
            "setGeneric" => {
                if let Some(generic) = args.first().and_then(|arg| string_literal(&arg.value)) {
                    Arc::make_mut(&mut self.fn_table)
                        .known_vars
                        .insert(generic.to_string());
                }
            }
            "setMethod" => {
                let Some(generic) = args.first().and_then(|arg| string_literal(&arg.value)) else {
                    return;
                };
                let Some(class) = args.get(1).and_then(|arg| s4_signature_class(&arg.value)) else {
                    return;
                };
                let Some(Expr::Function { params, body, .. }) = args
                    .iter()
                    .skip(2)
                    .find(|arg| matches!(arg.value, Expr::Function { .. }))
                    .map(|arg| &arg.value)
                else {
                    return;
                };
                let method_name = format!("__s4__{generic}__{class}");
                let slot = self.record_fn(method_name.clone(), params, body.clone());
                if let Some(first) = Arc::make_mut(&mut self.fn_table)
                    .fns
                    .get_mut(&method_name)
                    .and_then(|function| function.params.first_mut())
                {
                    first.type_ = RType::unknown().with_class(ClassVector::single(&class));
                }
                Arc::make_mut(&mut self.fn_table)
                    .s4_methods
                    .insert((generic.to_string(), class), slot);
            }
            _ => {}
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
                    p.name != "..." && p.default.is_none() && block_force_flow(&body, &p.name).0;
                UserParam {
                    name: p.name.clone(),
                    type_: t,
                    required,
                    defused: parameter_is_defused(&body, &p.name),
                    quoting: parameter_is_quoted(&body, params, &p.name),
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

        // Keep deferred expressions (notably `on.exit(expr)`) in the same
        // exit-time lexical context during fixpoint inference as during the
        // final diagnostic walk.
        self.deferred_captures
            .push(assigned_names_in_body(&body_clone));

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
        self.deferred_captures.pop();
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

/// Whether `parameter` is captured without evaluation by this function.
///
/// `match.call()`, `sys.call()`, and `sys.function()` capture the complete
/// call, so they make every formal quoting. `missing(p)` is deliberately not
/// included: it tests a promise without changing how an argument is evaluated.
fn parameter_is_quoted(body: &[Stmt], params: &[Param], parameter: &str) -> bool {
    body.iter().any(stmt_captures_all_arguments)
        || (params.iter().any(|formal| formal.name == parameter)
            && (body
                .iter()
                .any(|statement| stmt_quotes_parameter(statement, parameter))
                // Variadic promise-capture helpers capture the promises
                // stored in `...`; there is no single argument to match
                // syntactically.
                || (parameter == "..."
                    && body
                        .iter()
                .any(|statement| stmt_captures_promise_parameter(statement, parameter))))
            // Promise-capture helpers only make a promise safe to pass
            // unevaluated when that promise is not also used normally in
            // this function.  This preserves eager diagnostics for mixed
            // bodies such as a capture followed by `print(x)`.
            && !(body
                .iter()
                .any(|statement| stmt_captures_promise_parameter(statement, parameter))
                && parameter_has_normal_use(body, parameter)))
}

fn parameter_has_normal_use(body: &[Stmt], parameter: &str) -> bool {
    let mut uses = ParameterUses::default();
    for statement in body {
        collect_parameter_uses_in_stmt(statement, parameter, &mut uses);
    }
    uses.normal
}

fn stmt_captures_all_arguments(statement: &Stmt) -> bool {
    match statement {
        Stmt::Assign { target, value, .. } => {
            expr_captures_all_arguments(target) || expr_captures_all_arguments(value)
        }
        Stmt::Expr(expression) => expr_captures_all_arguments(expression),
        Stmt::If {
            cond, then, else_, ..
        } => {
            expr_captures_all_arguments(cond)
                || then.iter().any(stmt_captures_all_arguments)
                || else_.iter().flatten().any(stmt_captures_all_arguments)
        }
        Stmt::For { iter, body, .. } => {
            expr_captures_all_arguments(iter) || body.iter().any(stmt_captures_all_arguments)
        }
        Stmt::While { cond, body, .. } => {
            expr_captures_all_arguments(cond) || body.iter().any(stmt_captures_all_arguments)
        }
        Stmt::FunctionDef { .. } => false,
        Stmt::Return { value, .. } => value.as_ref().is_some_and(expr_captures_all_arguments),
    }
}

fn expr_captures_all_arguments(expression: &Expr) -> bool {
    match expression {
        Expr::Call { func, args, .. } => {
            matches!(
                bare_call_name(func),
                Some("match.call" | "sys.call" | "sys.function")
            ) || expr_captures_all_arguments(func)
                || args
                    .iter()
                    .any(|argument| expr_captures_all_arguments(&argument.value))
        }
        Expr::BinOp { lhs, rhs, .. } => {
            expr_captures_all_arguments(lhs) || expr_captures_all_arguments(rhs)
        }
        Expr::UnaryOp { expr, .. } => expr_captures_all_arguments(expr),
        Expr::Index { base, args, .. } => {
            expr_captures_all_arguments(base)
                || args
                    .iter()
                    .any(|argument| expr_captures_all_arguments(&argument.value))
        }
        Expr::Block { body, .. } => body.iter().any(stmt_captures_all_arguments),
        Expr::If {
            cond, then, else_, ..
        } => {
            expr_captures_all_arguments(cond)
                || expr_captures_all_arguments(then)
                || else_
                    .as_ref()
                    .is_some_and(|else_| expr_captures_all_arguments(else_))
        }
        Expr::Function { .. }
        | Expr::Ident { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => false,
    }
}

fn stmt_quotes_parameter(statement: &Stmt, parameter: &str) -> bool {
    match statement {
        Stmt::Assign { target, value, .. } => {
            expr_quotes_parameter(target, parameter) || expr_quotes_parameter(value, parameter)
        }
        Stmt::Expr(expression) => expr_quotes_parameter(expression, parameter),
        Stmt::If {
            cond, then, else_, ..
        } => {
            expr_quotes_parameter(cond, parameter)
                || then
                    .iter()
                    .any(|statement| stmt_quotes_parameter(statement, parameter))
                || else_
                    .iter()
                    .flatten()
                    .any(|statement| stmt_quotes_parameter(statement, parameter))
        }
        Stmt::For { iter, body, .. } => {
            expr_quotes_parameter(iter, parameter)
                || body
                    .iter()
                    .any(|statement| stmt_quotes_parameter(statement, parameter))
        }
        Stmt::While { cond, body, .. } => {
            expr_quotes_parameter(cond, parameter)
                || body
                    .iter()
                    .any(|statement| stmt_quotes_parameter(statement, parameter))
        }
        Stmt::FunctionDef { .. } => false,
        Stmt::Return { value, .. } => value
            .as_ref()
            .is_some_and(|expression| expr_quotes_parameter(expression, parameter)),
    }
}

fn expr_quotes_parameter(expression: &Expr, parameter: &str) -> bool {
    match expression {
        Expr::Call { func, args, .. } => {
            (matches!(bare_call_name(func), Some("substitute"))
                && args
                    .first()
                    .is_some_and(|argument| is_parameter(&argument.value, parameter)))
                || (is_single_promise_capture(func)
                    && args
                        .first()
                        .is_some_and(|argument| is_parameter(&argument.value, parameter)))
                || (matches!(bare_call_name(func), Some("bquote"))
                    && args
                        .iter()
                        .any(|argument| bquote_references_parameter(&argument.value, parameter)))
                || expr_quotes_parameter(func, parameter)
                || args
                    .iter()
                    .any(|argument| expr_quotes_parameter(&argument.value, parameter))
        }
        Expr::BinOp { lhs, rhs, .. } => {
            expr_quotes_parameter(lhs, parameter) || expr_quotes_parameter(rhs, parameter)
        }
        Expr::UnaryOp { expr, .. } => expr_quotes_parameter(expr, parameter),
        Expr::Index { base, args, .. } => {
            expr_quotes_parameter(base, parameter)
                || args
                    .iter()
                    .any(|argument| expr_quotes_parameter(&argument.value, parameter))
        }
        Expr::Block { body, .. } => body
            .iter()
            .any(|statement| stmt_quotes_parameter(statement, parameter)),
        Expr::If {
            cond, then, else_, ..
        } => {
            expr_quotes_parameter(cond, parameter)
                || expr_quotes_parameter(then, parameter)
                || else_
                    .as_ref()
                    .is_some_and(|else_| expr_quotes_parameter(else_, parameter))
        }
        Expr::Function { .. }
        | Expr::Ident { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => false,
    }
}

fn stmt_captures_promise_parameter(statement: &Stmt, parameter: &str) -> bool {
    match statement {
        Stmt::Assign { target, value, .. } => {
            expr_captures_promise_parameter(target, parameter)
                || expr_captures_promise_parameter(value, parameter)
        }
        Stmt::Expr(expression) => expr_captures_promise_parameter(expression, parameter),
        Stmt::If {
            cond, then, else_, ..
        } => {
            expr_captures_promise_parameter(cond, parameter)
                || then
                    .iter()
                    .any(|statement| stmt_captures_promise_parameter(statement, parameter))
                || else_
                    .iter()
                    .flatten()
                    .any(|statement| stmt_captures_promise_parameter(statement, parameter))
        }
        Stmt::For { iter, body, .. } => {
            expr_captures_promise_parameter(iter, parameter)
                || body
                    .iter()
                    .any(|statement| stmt_captures_promise_parameter(statement, parameter))
        }
        Stmt::While { cond, body, .. } => {
            expr_captures_promise_parameter(cond, parameter)
                || body
                    .iter()
                    .any(|statement| stmt_captures_promise_parameter(statement, parameter))
        }
        Stmt::FunctionDef { .. } => false,
        Stmt::Return { value, .. } => value
            .as_ref()
            .is_some_and(|expression| expr_captures_promise_parameter(expression, parameter)),
    }
}

fn expr_captures_promise_parameter(expression: &Expr, parameter: &str) -> bool {
    match expression {
        Expr::Call { func, args, .. } => {
            (is_single_promise_capture(func)
                && args
                    .first()
                    .is_some_and(|argument| is_parameter(&argument.value, parameter)))
                || (is_dots_promise_capture(func) && parameter == "...")
                || expr_captures_promise_parameter(func, parameter)
                || args
                    .iter()
                    .any(|argument| expr_captures_promise_parameter(&argument.value, parameter))
        }
        Expr::BinOp { lhs, rhs, .. } => {
            expr_captures_promise_parameter(lhs, parameter)
                || expr_captures_promise_parameter(rhs, parameter)
        }
        Expr::UnaryOp { expr, .. } => expr_captures_promise_parameter(expr, parameter),
        Expr::Index { base, args, .. } => {
            expr_captures_promise_parameter(base, parameter)
                || args
                    .iter()
                    .any(|argument| expr_captures_promise_parameter(&argument.value, parameter))
        }
        Expr::Block { body, .. } => body
            .iter()
            .any(|statement| stmt_captures_promise_parameter(statement, parameter)),
        Expr::If {
            cond, then, else_, ..
        } => {
            expr_captures_promise_parameter(cond, parameter)
                || expr_captures_promise_parameter(then, parameter)
                || else_
                    .as_ref()
                    .is_some_and(|else_| expr_captures_promise_parameter(else_, parameter))
        }
        Expr::Function { .. }
        | Expr::Ident { .. }
        | Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => false,
    }
}

/// Whether a package stub declares this callee as a promise-capture helper.
/// Collection happens before ordinary call-site resolution, so bare names are
/// recognized from the loaded stub inventory rather than lexical scope.
fn is_promise_capture(function: &Expr, dots: bool) -> bool {
    let Some(name) = ident_name(function) else {
        return false;
    };
    let (package, function) = name
        .rsplit_once("::")
        .map(|(package, function)| (Some(package.trim_end_matches(':')), function))
        .unwrap_or((None, name));
    let has_capture = |signature: &FunctionSig| {
        signature.eval.iter().any(|(parameter, mode)| {
            *mode == EvalMode::CapturesPromise && (parameter == "...") == dots
        })
    };
    match package {
        Some(package) => ry_typeshed::load_package(package)
            .and_then(|typeshed| typeshed.functions.get(function))
            .is_some_and(has_capture),
        None => {
            ry_typeshed::load_base_cached()
                .ok()
                .and_then(|typeshed| typeshed.functions.get(function))
                .is_some_and(has_capture)
                || ry_typeshed::known_packages().any(|package| {
                    ry_typeshed::load_package(package)
                        .and_then(|typeshed| typeshed.functions.get(function))
                        .is_some_and(has_capture)
                })
        }
    }
}

fn is_single_promise_capture(function: &Expr) -> bool {
    is_promise_capture(function, false)
}

fn is_dots_promise_capture(function: &Expr) -> bool {
    is_promise_capture(function, true)
}

fn bquote_references_parameter(expression: &Expr, parameter: &str) -> bool {
    match expression {
        Expr::Call { func, args, .. }
            if matches!(bare_call_name(func), Some("."))
                && args
                    .iter()
                    .any(|argument| is_parameter(&argument.value, parameter)) =>
        {
            true
        }
        Expr::Call { func, args, .. } => {
            bquote_references_parameter(func, parameter)
                || args
                    .iter()
                    .any(|argument| bquote_references_parameter(&argument.value, parameter))
        }
        Expr::BinOp { lhs, rhs, .. } => {
            bquote_references_parameter(lhs, parameter)
                || bquote_references_parameter(rhs, parameter)
        }
        Expr::UnaryOp { expr, .. } => bquote_references_parameter(expr, parameter),
        Expr::Index { base, args, .. } => {
            bquote_references_parameter(base, parameter)
                || args
                    .iter()
                    .any(|argument| bquote_references_parameter(&argument.value, parameter))
        }
        Expr::Block { body, .. } => body
            .iter()
            .any(|statement| stmt_quotes_parameter(statement, parameter)),
        Expr::If {
            cond, then, else_, ..
        } => {
            bquote_references_parameter(cond, parameter)
                || bquote_references_parameter(then, parameter)
                || else_
                    .as_ref()
                    .is_some_and(|else_| bquote_references_parameter(else_, parameter))
        }
        _ => false,
    }
}

fn bare_call_name(expression: &Expr) -> Option<&str> {
    ident_name(expression).map(|name| name.rsplit_once("::").map(|(_, bare)| bare).unwrap_or(name))
}

fn is_parameter(expression: &Expr, parameter: &str) -> bool {
    matches!(expression, Expr::Ident { name, .. } if name == parameter)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstParameterUse {
    Defused,
    Normal,
}

fn parameter_is_defused(body: &[Stmt], parameter: &str) -> bool {
    if parameter == "..." {
        let mut uses = ParameterUses::default();
        for statement in body {
            collect_parameter_uses_in_stmt(statement, parameter, &mut uses);
        }
        return uses.defused && !uses.normal;
    }
    body.iter()
        .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
        == Some(FirstParameterUse::Defused)
}

#[derive(Default)]
struct ParameterUses {
    defused: bool,
    normal: bool,
}

fn collect_parameter_uses_in_stmt(statement: &Stmt, parameter: &str, uses: &mut ParameterUses) {
    match statement {
        Stmt::Assign { target, value, .. } => {
            collect_parameter_uses_in_expr(value, parameter, uses);
            if !matches!(target, Expr::Ident { .. }) {
                collect_parameter_uses_in_expr(target, parameter, uses);
            }
        }
        Stmt::Expr(expression) => collect_parameter_uses_in_expr(expression, parameter, uses),
        Stmt::If {
            cond, then, else_, ..
        } => {
            collect_parameter_uses_in_expr(cond, parameter, uses);
            for statement in then {
                collect_parameter_uses_in_stmt(statement, parameter, uses);
            }
            for statement in else_.iter().flatten() {
                collect_parameter_uses_in_stmt(statement, parameter, uses);
            }
        }
        Stmt::For {
            name, iter, body, ..
        } => {
            collect_parameter_uses_in_expr(iter, parameter, uses);
            if name == parameter {
                uses.normal = true;
            }
            for statement in body {
                collect_parameter_uses_in_stmt(statement, parameter, uses);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_parameter_uses_in_expr(cond, parameter, uses);
            for statement in body {
                collect_parameter_uses_in_stmt(statement, parameter, uses);
            }
        }
        Stmt::FunctionDef { params, body, .. } => {
            if !params.iter().any(|formal| formal.name == parameter) {
                for statement in body {
                    collect_parameter_uses_in_stmt(statement, parameter, uses);
                }
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(expression) = value {
                collect_parameter_uses_in_expr(expression, parameter, uses);
            }
        }
    }
}

fn collect_parameter_uses_in_expr(expression: &Expr, parameter: &str, uses: &mut ParameterUses) {
    match expression {
        Expr::Ident { name, .. } => {
            if name == parameter {
                uses.normal = true;
            }
        }
        Expr::Call { func, args, .. } => {
            let probes_promise = matches!(bare_call_name(func), Some("missing"));
            let defuses_direct_argument = is_single_promise_capture(func)
                || is_dots_promise_capture(func)
                || matches!(bare_call_name(func), Some("match.call" | "substitute"));
            collect_parameter_uses_in_expr(func, parameter, uses);
            for argument in args {
                if matches!(&argument.value, Expr::Ident { name, .. } if name == parameter) {
                    if defuses_direct_argument {
                        uses.defused = true;
                    } else if !probes_promise {
                        // `missing(p)` inspects whether a promise was
                        // supplied without forcing it. It should therefore
                        // neither cancel a later NSE capture nor itself make
                        // the parameter quoted.
                        uses.normal = true;
                    }
                } else {
                    collect_parameter_uses_in_expr(&argument.value, parameter, uses);
                }
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_parameter_uses_in_expr(lhs, parameter, uses);
            collect_parameter_uses_in_expr(rhs, parameter, uses);
        }
        Expr::UnaryOp { expr, .. } => collect_parameter_uses_in_expr(expr, parameter, uses),
        Expr::Index { base, args, .. } => {
            collect_parameter_uses_in_expr(base, parameter, uses);
            for argument in args {
                collect_parameter_uses_in_expr(&argument.value, parameter, uses);
            }
        }
        Expr::Function { params, body, .. } => {
            if !params.iter().any(|formal| formal.name == parameter) {
                for statement in body {
                    collect_parameter_uses_in_stmt(statement, parameter, uses);
                }
            }
        }
        Expr::Block { body, .. } => {
            for statement in body {
                collect_parameter_uses_in_stmt(statement, parameter, uses);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            collect_parameter_uses_in_expr(cond, parameter, uses);
            collect_parameter_uses_in_expr(then, parameter, uses);
            if let Some(expression) = else_ {
                collect_parameter_uses_in_expr(expression, parameter, uses);
            }
        }
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => {}
    }
}

fn first_parameter_use_in_stmt(statement: &Stmt, parameter: &str) -> Option<FirstParameterUse> {
    match statement {
        Stmt::Assign { target, value, .. } => first_parameter_use_in_expr(value, parameter)
            .or_else(|| match target {
                Expr::Ident { .. } => None,
                target => first_parameter_use_in_expr(target, parameter),
            }),
        Stmt::Expr(expression) => first_parameter_use_in_expr(expression, parameter),
        Stmt::If {
            cond, then, else_, ..
        } => first_parameter_use_in_expr(cond, parameter).or_else(|| {
            conservative_branch_use([
                then.iter()
                    .find_map(|statement| first_parameter_use_in_stmt(statement, parameter)),
                else_.as_ref().and_then(|statements| {
                    statements
                        .iter()
                        .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
                }),
            ])
        }),
        Stmt::For {
            name, iter, body, ..
        } => first_parameter_use_in_expr(iter, parameter).or_else(|| {
            if name == parameter {
                Some(FirstParameterUse::Normal)
            } else {
                body.iter()
                    .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
            }
        }),
        Stmt::While { cond, body, .. } => {
            first_parameter_use_in_expr(cond, parameter).or_else(|| {
                body.iter()
                    .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
            })
        }
        Stmt::FunctionDef { .. } => None,
        Stmt::Return { value, .. } => value
            .as_ref()
            .and_then(|expression| first_parameter_use_in_expr(expression, parameter)),
    }
}

fn first_parameter_use_in_expr(expression: &Expr, parameter: &str) -> Option<FirstParameterUse> {
    match expression {
        Expr::Ident { name, .. } => (name == parameter).then_some(FirstParameterUse::Normal),
        Expr::Call { func, args, .. } => {
            let defuses_direct_argument = is_single_promise_capture(func)
                || is_dots_promise_capture(func)
                || matches!(
                    bare_call_name(func),
                    Some("substitute" | "match.call" | "bquote")
                );
            if defuses_direct_argument
                && args.iter().any(|argument| {
                    matches!(&argument.value, Expr::Ident { name, .. } if name == parameter)
                })
            {
                return Some(FirstParameterUse::Defused);
            }
            first_parameter_use_in_expr(func, parameter).or_else(|| {
                args.iter()
                    .find_map(|argument| first_parameter_use_in_expr(&argument.value, parameter))
            })
        }
        Expr::BinOp { lhs, rhs, .. } => first_parameter_use_in_expr(lhs, parameter)
            .or_else(|| first_parameter_use_in_expr(rhs, parameter)),
        Expr::UnaryOp { expr, .. } => first_parameter_use_in_expr(expr, parameter),
        Expr::Index { base, args, .. } => {
            first_parameter_use_in_expr(base, parameter).or_else(|| {
                args.iter()
                    .find_map(|argument| first_parameter_use_in_expr(&argument.value, parameter))
            })
        }
        Expr::Function { .. } => None,
        Expr::Block { body, .. } => {
            if embraced_symbol(body).is_some_and(|(name, _)| name == parameter) {
                Some(FirstParameterUse::Defused)
            } else {
                body.iter()
                    .find_map(|statement| first_parameter_use_in_stmt(statement, parameter))
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => first_parameter_use_in_expr(cond, parameter).or_else(|| {
            conservative_branch_use([
                first_parameter_use_in_expr(then, parameter),
                else_
                    .as_ref()
                    .and_then(|expression| first_parameter_use_in_expr(expression, parameter)),
            ])
        }),
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => None,
    }
}

fn conservative_branch_use(
    uses: impl IntoIterator<Item = Option<FirstParameterUse>>,
) -> Option<FirstParameterUse> {
    let mut first = None;
    for use_ in uses.into_iter().flatten() {
        if use_ == FirstParameterUse::Normal {
            return Some(FirstParameterUse::Normal);
        }
        first = Some(FirstParameterUse::Defused);
    }
    first
}

fn string_literal(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::String(value, _) => Some(value),
        _ => None,
    }
}

fn s4_signature_class(expr: &Expr) -> Option<String> {
    match expr {
        Expr::String(class, _) => Some(class.clone()),
        Expr::Call { func, args, .. } if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "signature") => {
            args.first()
                .and_then(|argument| string_literal(&argument.value))
                .map(str::to_string)
        }
        _ => None,
    }
}

fn s4_slots(expr: &Expr) -> HashMap<String, String> {
    let Expr::Call { func, args, .. } = expr else {
        return HashMap::new();
    };
    if !matches!(func.as_ref(), Expr::Ident { name, .. } if name == "representation" || name == "c")
    {
        return HashMap::new();
    }
    args.iter()
        .filter_map(|argument| {
            Some((
                semantic_argument_name(argument.name.as_deref()?).to_string(),
                string_literal(&argument.value)?.to_string(),
            ))
        })
        .collect()
}

/// Return whether evaluating a block must force `name`, and whether control
/// always falls through it. Both answers come from the same statement walk:
/// force detection stops at the first forcing or non-falling statement, while
/// fall-through continues across the whole block.
fn block_force_flow(statements: &[Stmt], name: &str) -> (bool, bool) {
    let mut forces = false;
    let mut can_still_force = true;
    let mut falls_through = true;

    for statement in statements {
        let (statement_forces, statement_falls_through) = statement_force_flow(statement, name);
        if can_still_force {
            if statement_forces {
                forces = true;
                can_still_force = false;
            } else if !statement_falls_through {
                can_still_force = false;
            }
        }
        falls_through &= statement_falls_through;
    }

    (forces, falls_through)
}

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
            let (then_forces, then_falls) = block_force_flow(then, name);
            let (else_forces, else_falls) = else_
                .as_ref()
                .map(|statements| block_force_flow(statements, name))
                .unwrap_or((false, true));
            let forces = expression_must_force(cond, name) || (then_forces && else_forces);
            (forces, then_falls && else_falls)
        }
        Stmt::For { iter, body, .. } => (
            expression_must_force(iter, name),
            block_force_flow(body, name).1,
        ),
        Stmt::While { cond, .. } => (expression_must_force(cond, name), false),
        Stmt::Return { value, .. } => (
            value
                .as_ref()
                .is_some_and(|value| expression_must_force(value, name)),
            false,
        ),
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
        Expr::Call { func, .. } => expression_must_force(func, name),
        Expr::Block { body, .. } => block_force_flow(body, name).0,
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
