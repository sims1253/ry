use super::*;
pub(crate) use index::*;
pub(crate) use misc::*;
pub(crate) mod binop;
pub(crate) mod call;
pub(crate) mod construct;
pub(crate) mod index;
pub(crate) mod misc;
pub(crate) mod pipe;

/// The diagnostic family appropriate for a known condition type. Opaque
/// conditions deliberately remain silent: the runtime value may be logical.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConditionDiagnostic {
    Invalid,
    Numeric,
}

/// Classify a condition without losing the member-level information carried
/// by unions. In particular, `integer | double` is numeric truthiness, while
/// `integer | character` can still fail at runtime and is invalid.
pub(crate) fn condition_diagnostic(t: &RType) -> Option<ConditionDiagnostic> {
    if matches!(t.length, Length::Zero) {
        return Some(ConditionDiagnostic::Invalid);
    }

    match t.mode {
        Mode::Logical => None,
        Mode::Integer | Mode::Double => Some(ConditionDiagnostic::Numeric),
        Mode::Opaque => None,
        Mode::Union => {
            let members = t.members.as_ref()?;
            let mut numeric = false;
            let mut invalid = false;
            for member in members.iter() {
                match condition_diagnostic(member) {
                    Some(ConditionDiagnostic::Invalid) => {
                        invalid = true;
                    }
                    Some(ConditionDiagnostic::Numeric) => numeric = true,
                    // A logical branch is already an explicit condition, so
                    // preserve the existing silence for logical|numeric
                    // unions. An opaque branch is unknown for the same
                    // reason.
                    None if matches!(member.mode, Mode::Logical | Mode::Opaque) => return None,
                    None => {}
                }
            }
            if invalid {
                Some(ConditionDiagnostic::Invalid)
            } else {
                numeric.then_some(ConditionDiagnostic::Numeric)
            }
        }
        _ => Some(ConditionDiagnostic::Invalid),
    }
}

// The T7b "mutually-exclusive branch" loop refinement was removed after two
// rounds of corpus regressions (it ended up flagging loop iterators inside
// their own bodies). Loop bodies simply pre-bind every name assigned anywhere
// in the body before walking; a use-before-first-assignment inside a loop is
// statically indistinguishable from a legitimate loop-carried binding.

impl Checker {
    pub(crate) fn walk_stmt(
        &mut self,
        s: &Stmt,
        scope: &mut Scope,
        mut returns: Option<&mut Vec<RType>>,
    ) {
        if scope.unreachable {
            // Function bodies remain independent analysis units even when
            // their definition is textually located in dead code. Walk the
            // definition with reachability restored, then keep the enclosing
            // block unreachable.
            if matches!(
                s,
                Stmt::FunctionDef { .. }
                    | Stmt::Assign {
                        value: Expr::Function { .. },
                        ..
                    }
            ) {
                scope.unreachable = false;
                self.walk_stmt(s, scope, returns);
                scope.unreachable = true;
            }
            return;
        }
        match s {
            Stmt::Assign { target, value, .. } => {
                let vt = self.infer(value, scope);
                let function_alias = self.function_alias_target(value, scope);
                if !self.assign_class_attribute(target, value, scope)
                    && !self.assign_replacement_target(target, scope)
                {
                    self.assign_target(target, vt, scope);
                    if let (Some(name), Some(alias)) = (binding_name(target), function_alias) {
                        scope.set_function_alias(name.to_string(), alias);
                    }
                }
                // Named function bodies (`f <- function(...) body`) must
                // be walked for diagnostics. The function-value inference
                // path (`Expr::Function` -> `function_value_from_literal`)
                // runs in discarding mode and emits nothing on its own, so
                // without this walk almost all real R code would go
                // unchecked.
                if let Expr::Function { params, body, .. } = value {
                    let mut fn_scope = scope.clone();
                    if let Some(captures) = self.deferred_captures.last() {
                        for capture in captures {
                            if fn_scope.get(capture).is_none() {
                                fn_scope.insert(capture.clone(), RType::unknown());
                            }
                        }
                    }
                    if let Some(name) = binding_name(target) {
                        insert_s3_dispatch_context(name, &mut fn_scope, &self.typeshed.globals);
                    }
                    for parameter in params {
                        fn_scope.insert(parameter.name.clone(), RType::unknown());
                    }
                    let assigned = assigned_names_in_body(body);
                    self.check_lazy_default_reachability(params, body, &assigned);
                    let mut default_scope = fn_scope.clone();
                    for name in &assigned {
                        default_scope.insert(name.clone(), RType::unknown());
                    }
                    for p in params {
                        let t = match &p.default {
                            Some(e) => {
                                let _ = self.infer(e, &mut default_scope);
                                binding_name(target)
                                    .map(|function| {
                                        self.diagnostic_parameter_type(function, p, params, e)
                                    })
                                    .unwrap_or_else(RType::unknown)
                            }
                            None => RType::unknown(),
                        };
                        if p.default.is_some() {
                            fn_scope.insert_parameter_default(p.name.clone(), t);
                        } else {
                            fn_scope.insert(p.name.clone(), t);
                        }
                    }
                    self.deferred_captures.push(assigned);
                    self.push_enclosing_formals(params);
                    for s in body {
                        self.walk_stmt(s, &mut fn_scope, None);
                    }
                    self.enclosing_formals.pop();
                    self.deferred_captures.pop();
                }
            }
            Stmt::Expr(e) => {
                if let Expr::Call { func, args, .. } = e
                    && self.destructuring_operator_in_scope()
                    && args.len() == 2
                {
                    let op = ident_name(func);
                    let pattern = match op {
                        Some("%<-%") => Some((&args[0].value, &args[1].value)),
                        Some("%->%") => Some((&args[1].value, &args[0].value)),
                        _ => None,
                    };
                    if let Some((pattern, value)) = pattern {
                        self.infer(value, scope);
                        bind_destructure_pattern(pattern, scope);
                        return;
                    }
                }
                // Detect `return(...)` / `invisible(...)` calls and collect
                // the argument type (the function's return type).
                if let Expr::Call { func, args, .. } = e {
                    if let Expr::Ident { name, .. } = func.as_ref() {
                        if name == "return" || name == "invisible" {
                            let t = args
                                .first()
                                .map(|a| self.infer(&a.value, scope))
                                .unwrap_or_else(|| RType::new(Mode::Null, Length::Zero));
                            if let Some(r) = returns {
                                r.push(t);
                            }
                            return;
                        }
                    }
                }
                self.infer(e, scope);
            }
            Stmt::If {
                cond, then, else_, ..
            } => {
                let diagnostic_start = self.diagnostics.len();
                let ct = self.infer(cond, scope);
                let has_ry100 = self.diagnostics[diagnostic_start..]
                    .iter()
                    .any(|diagnostic| diagnostic.code == "RY100");
                if matches!(
                    condition_diagnostic(&ct),
                    Some(ConditionDiagnostic::Invalid)
                ) && !has_ry100
                {
                    self.emit(
                        Severity::Error,
                        span_of(cond),
                        "RY001",
                        format!("`if` condition is `{}`, expected length-1 logical", ct),
                    );
                } else if matches!(
                    condition_diagnostic(&ct),
                    Some(ConditionDiagnostic::Numeric)
                ) && !has_ry100
                    && !is_numeric_truthiness_idiom(cond, scope)
                {
                    self.emit(
                        Severity::Info,
                        span_of(cond),
                        "RY003",
                        format!("`if` condition is `{}`; R coerces nonzero to TRUE", ct.mode),
                    );
                } else if matches!(ct.mode, Mode::Logical) {
                    if let Length::Known(n) = ct.length {
                        if n > 1 {
                            self.emit(
                                Severity::Warning,
                                span_of(cond),
                                "RY002",
                                format!(
                                    "`if` condition has length {}; R requires a length-1 condition",
                                    n
                                ),
                            );
                        }
                    }
                }
                let narrowing = extract_type_narrowing(cond);
                let has_else = else_.is_some();
                let (then_scope, else_scope, narrowed) =
                    apply_narrowing(scope, &narrowing, has_else);
                let mut then_scope = then_scope;
                let mut else_scope = else_scope;
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
                self.merge_branch_bindings(
                    scope,
                    then_scope.clone(),
                    else_scope.clone(),
                    has_else,
                    &narrowed,
                );
                // Refinements normally remain branch-local (see
                // `apply_narrowing`). A diverging arm is the exception: the
                // continuation is reachable only through its sibling, so its
                // recorded refinements are facts in the parent scope.
                let then_diverges = self.block_diverges(then);
                let else_diverges = else_
                    .as_ref()
                    .is_some_and(|statements| self.block_diverges(statements));
                let continuation = match (then_diverges, else_.as_ref(), else_diverges) {
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
            Stmt::For {
                name, iter, body, ..
            } => {
                let iter_t = self.infer(iter, scope);
                let mut inner = scope.clone();
                inner.insert(name.clone(), iter_t.element());
                self.insert_loop_carried_bindings(body, &mut inner);
                for s in body {
                    self.walk_stmt(s, &mut inner, returns.as_deref_mut());
                }
                // R loop bodies execute in the enclosing environment. Carry
                // schema mutations and assignments forward; retaining the
                // iterator binding also matches R when at least one iteration
                // occurs. Static analysis cannot prove the zero-iteration
                // case, so opaque downstream behavior is preferable to
                // claiming a field definitely does not exist.
                for (binding, ty) in inner.bindings {
                    scope.insert(binding, ty);
                }
            }
            Stmt::While { cond, body, .. } => {
                let diagnostic_start = self.diagnostics.len();
                let ct = self.infer(cond, scope);
                let has_ry100 = self.diagnostics[diagnostic_start..]
                    .iter()
                    .any(|diagnostic| diagnostic.code == "RY100");
                if matches!(
                    condition_diagnostic(&ct),
                    Some(ConditionDiagnostic::Invalid)
                ) && !has_ry100
                {
                    self.emit(
                        Severity::Error,
                        span_of(cond),
                        "RY001",
                        format!("loop condition is `{}`, expected length-1 logical", ct),
                    );
                } else if matches!(
                    condition_diagnostic(&ct),
                    Some(ConditionDiagnostic::Numeric)
                ) && !has_ry100
                    && !is_numeric_truthiness_idiom(cond, scope)
                {
                    self.emit(
                        Severity::Info,
                        span_of(cond),
                        "RY003",
                        format!("loop condition is `{}`; R coerces nonzero to TRUE", ct.mode),
                    );
                }
                let mut inner = scope.clone();
                self.insert_loop_carried_bindings(body, &mut inner);
                for s in body {
                    self.walk_stmt(s, &mut inner, returns.as_deref_mut());
                }
                // As with `for`, assignments made by `while` and `repeat`
                // bodies remain visible in R's enclosing environment.
                let body_unreachable = inner.unreachable;
                for (binding, ty) in inner.bindings {
                    scope.insert(binding, ty);
                }
                // The parser represents `repeat` as `while (TRUE)`. If its
                // body cannot continue, neither can the enclosing block.
                if matches!(cond, Expr::Logical(true, _)) && body_unreachable {
                    scope.unreachable = true;
                }
            }
            Stmt::FunctionDef {
                name, params, body, ..
            } => {
                let vt = self.function_value_from_literal(params, body, scope, 0);
                if let Some(n) = name {
                    scope.insert(n.clone(), vt);
                }
                let mut fn_scope = scope.clone();
                if let Some(captures) = self.deferred_captures.last() {
                    for capture in captures {
                        if fn_scope.get(capture).is_none() {
                            fn_scope.insert(capture.clone(), RType::unknown());
                        }
                    }
                }
                for parameter in params {
                    fn_scope.insert(parameter.name.clone(), RType::unknown());
                }
                let assigned = assigned_names_in_body(body);
                self.check_lazy_default_reachability(params, body, &assigned);
                let mut default_scope = fn_scope.clone();
                for name in &assigned {
                    default_scope.insert(name.clone(), RType::unknown());
                }
                for p in params {
                    let t = match &p.default {
                        Some(e) => {
                            let _ = self.infer(e, &mut default_scope);
                            name.as_deref()
                                .map(|function| {
                                    self.diagnostic_parameter_type(function, p, params, e)
                                })
                                .unwrap_or_else(RType::unknown)
                        }
                        None => RType::unknown(),
                    };
                    if p.default.is_some() {
                        fn_scope.insert_parameter_default(p.name.clone(), t);
                    } else {
                        fn_scope.insert(p.name.clone(), t);
                    }
                }
                self.deferred_captures.push(assigned);
                self.push_enclosing_formals(params);
                for s in body {
                    self.walk_stmt(s, &mut fn_scope, None);
                }
                self.enclosing_formals.pop();
                self.deferred_captures.pop();
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    let t = self.infer(v, scope);
                    if let Some(r) = returns {
                        r.push(t);
                    }
                } else if let Some(r) = returns {
                    r.push(RType::new(Mode::Null, Length::Zero));
                }
            }
        }
    }

    /// Bind names assigned by a loop body before walking it. A binding may
    /// have been established by a previous iteration, even when its first
    /// assignment is textually later than its use in the body.
    fn insert_loop_carried_bindings(&self, body: &[Stmt], scope: &mut Scope) -> HashSet<String> {
        let mut prebound = HashSet::new();
        for name in assigned_names_in_body(body) {
            if scope.get(&name).is_none() {
                scope.insert(name.clone(), RType::unknown());
                prebound.insert(name);
            }
        }
        prebound
    }

    /// Merge bindings introduced inside the two `if` branches back into the
    /// parent `scope`.
    ///
    /// A name that is newly bound in BOTH branches gets the join of the two
    /// branch types. A name bound in only one branch (or when there is no
    /// `else`) is inserted into the parent as [`RType::unknown`]: there is no
    /// sound type for "possibly missing" in the current model, and the goal
    /// here is solely to stop RY010 false positives on the conditional
    /// assignment idiom. Modeling "definitely unbound" as a diagnostic is a
    /// separate future rule and intentionally out of scope.
    ///
    /// "Newly bound" means present in the branch scope but absent from the
    /// parent (or bound to a different type): names that already existed in
    /// the parent with the same type are left untouched.
    pub(crate) fn merge_branch_bindings(
        &self,
        scope: &mut Scope,
        then_scope: Scope,
        else_scope: Scope,
        has_else: bool,
        narrowed: &HashSet<String>,
    ) {
        // Collect the candidate names (only those that differ from the
        // parent) without holding a borrow of `scope` while we mutate it.
        let mut branch_types: HashMap<String, (Option<RType>, Option<RType>)> =
            HashMap::with_capacity(then_scope.bindings.len());
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
                    branch_types.entry(name.clone()).or_insert((None, None)).0 = Some(t.clone());
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
                        branch_types.entry(name.clone()).or_insert((None, None)).1 =
                            Some(t.clone());
                    }
                }
            }
        }
        for (name, (then_t, else_t)) in branch_types {
            let merged = match (then_t, else_t) {
                (Some(a), Some(b)) => a.join(b),
                (Some(a), None) | (None, Some(a)) => a.join(RType::unknown()),
                (None, None) => continue,
            };
            // If the name already existed in the parent with a *different*
            // type, fold that prior type into the merge so a branch
            // reassignment doesn't silently degrade a precise parent type
            // to unknown (e.g. `s <- 1L; if (c) { s <- "x" }` keeps `s` as
            // union[integer, character] rather than collapsing to unknown).
            let merged = match scope.get(&name) {
                Some(p) => p.clone().join(merged),
                None => merged,
            };
            scope.insert(name, merged);
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

    /// Whether every path through a statement block stops executing the
    /// surrounding block. This is intentionally syntactic and conservative:
    /// failing to recognize divergence only misses a narrowing opportunity.
    fn block_diverges(&self, stmts: &[Stmt]) -> bool {
        self.block_diverges_with_visited(stmts, &mut HashSet::new())
    }

    fn block_diverges_with_visited(&self, stmts: &[Stmt], visited: &mut HashSet<String>) -> bool {
        stmts
            .iter()
            .any(|statement| self.stmt_diverges(statement, visited))
    }

    fn stmt_diverges(&self, statement: &Stmt, visited: &mut HashSet<String>) -> bool {
        match statement {
            Stmt::Return { .. } => true,
            Stmt::Expr(expression) => self.expr_diverges(expression, visited),
            Stmt::If { then, else_, .. } => else_.as_ref().is_some_and(|else_| {
                self.block_diverges_with_visited(then, visited)
                    && self.block_diverges_with_visited(else_, visited)
            }),
            _ => false,
        }
    }

    fn expr_diverges(&self, expression: &Expr, visited: &mut HashSet<String>) -> bool {
        match expression {
            // tree-sitter lowers `break` and `next` as identifier statements.
            Expr::Ident { name, .. } if matches!(name.as_str(), "break" | "next") => true,
            Expr::Call { func, .. } => {
                let Some(name) = ident_name(func) else {
                    return false;
                };
                if name == "UseMethod"
                    || name
                        .rsplit_once("::")
                        .is_some_and(|(_, bare)| bare == "UseMethod")
                {
                    return true;
                }
                if self
                    .resolve_typeshed_sig(name)
                    .is_some_and(|signature| signature.no_return)
                {
                    return true;
                }
                // A collected helper whose body itself diverges is a known
                // never-returning function. The visited set keeps recursive
                // helpers conservative rather than recursing forever.
                if !visited.insert(name.to_string()) {
                    return false;
                }
                let diverges = self.fn_table.fns.get(name).is_some_and(|function| {
                    self.block_diverges_with_visited(&function.body, visited)
                });
                visited.remove(name);
                diverges
            }
            Expr::Block { body, .. } => self.block_diverges_with_visited(body, visited),
            Expr::If { then, else_, .. } => else_.as_ref().is_some_and(|else_| {
                self.expr_diverges(then, visited) && self.expr_diverges(else_, visited)
            }),
            _ => false,
        }
    }

    /// runs the single diagnostic `infer` with `discarding` enabled, so
    /// the type computation (including the full `Expr::Ident` resolution
    /// ladder, all `Expr::Call` cases, narrowing, etc.) is shared between
    /// the pure and the diagnostic walks.
    pub(crate) fn infer_discarding(&mut self, e: &Expr, scope: &mut Scope) -> RType {
        let prev = self.discarding;
        self.discarding = true;
        let t = self.infer(e, scope);
        self.discarding = prev;
        t
    }

    /// Build a `Mode::Function` `RType` (with `fn_sig` when we can
    /// infer it) for a `function(params) body` literal. `captured_scope`
    /// is the scope at the point where the literal appears; the inner
    /// function's params are layered on top so it can reference both.
    ///
    /// `depth` is the current closure-nesting depth (0 at the top
    /// level). Once `depth >= MAX_CLOSURE_DEPTH` we stop building
    /// nested signatures and return an opaque `Function` value, as
    /// documented in the closure-support scope limits.
    pub(crate) fn function_value_from_literal(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        captured_scope: &Scope,
        depth: usize,
    ) -> RType {
        let base = RType::scalar(Mode::Function);
        if depth >= MAX_CLOSURE_DEPTH {
            return base;
        }
        match self.build_function_signature(params, body, captured_scope, depth) {
            Some(sig) => base.with_fn_sig(sig),
            None => base,
        }
    }

    /// Build an interned `FunctionSignature` for a function literal by
    /// walking its body's returns with a scope that layers the inner
    /// params on top of the captured enclosing scope. Returns `None`
    /// when we have no information (empty body, depth cap exceeded on
    /// nested literals, etc.); the caller falls back to an opaque
    /// `Function` value.
    ///
    /// Captured bindings are snapshotted here by reading
    /// `captured_scope`. We do NOT track per-binding mutation in v1, so
    /// a closure that closes over mutable state (a binding reassigned
    /// in the body) sees the captured value rather than the final
    /// mutated value. This is the documented approximation.
    pub(crate) fn build_function_signature(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        captured_scope: &Scope,
        depth: usize,
    ) -> Option<Arc<FunctionSignature>> {
        if body.is_empty() {
            return None;
        }
        // Signature building is a PURE return-type computation: it must
        // never emit diagnostics (the diagnostic walk of a function body
        // happens via check_stmt's function-body arm in pass 3). Force
        // discarding mode for this walk regardless of the caller's mode.
        let prev_discarding = self.discarding;
        self.discarding = true;
        let result = self.build_function_signature_inner(params, body, captured_scope, depth);
        self.discarding = prev_discarding;
        result
    }

    pub(crate) fn build_function_signature_inner(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        captured_scope: &Scope,
        depth: usize,
    ) -> Option<Arc<FunctionSignature>> {
        // Layer the inner function's params on top of the captured
        // scope. We start from a clone of the captured scope so the
        // body can reference enclosing bindings (`make_adder`'s `x`).
        let mut scope = captured_scope.clone();
        let mut param_types: Vec<RType> = Vec::with_capacity(params.len());
        for p in params {
            let t = match &p.default {
                Some(e) => infer_literal_default(e),
                None => RType::unknown(),
            };
            if p.default.is_some() {
                scope.insert_parameter_default(p.name.clone(), t.clone());
            } else {
                scope.insert(p.name.clone(), t.clone());
            }
            param_types.push(t);
        }
        // Walk the body in source order, simulating each statement's
        // effect on the scope so later statements (notably the trailing
        // return expression) can reference bindings established earlier
        // in the body. This is what lets us resolve the named-return
        // closure pattern:
        //     f <- function() { g <- function() { 1L }; g }
        // Here the trailing `g` must see the `g <- function() { 1L }`
        // binding to pick up its inferred `fn_sig`.
        //
        // We collect explicit `return(...)` types as we go; the trailing
        // statement's value is added separately below. Branches in `if`
        // are walked without splitting the scope (v1 approximation).
        let mut returns: Vec<RType> = Vec::new();
        // Walk the body via the unified walker (discarding mode, return
        // collection enabled).
        for s in body {
            self.walk_stmt(s, &mut scope, Some(&mut returns));
        }
        // Trailing expression of a braced body is the implicit return.
        // A trailing `Stmt::FunctionDef` (a bare function literal in
        // statement position) is also the implicit return value - this
        // is the closure-factory pattern: `function() { function() { 1L } }`
        // has a `Stmt::FunctionDef` as its body's last statement.
        if let Some(t) = self.trailing_return_type(body, &mut scope, depth + 1) {
            returns.push(t);
        }
        if returns.is_empty() {
            return None;
        }
        let mut iter = returns.into_iter();
        let first = iter.next().unwrap_or(RType::unknown());
        let joined = iter.fold(first, |acc, t| acc.join(t));
        // If we couldn't infer anything useful (joined is UNKNOWN),
        // there's no point attaching an empty signature.
        if matches!(joined.mode, Mode::Opaque) {
            return None;
        }
        Some(Arc::new(FunctionSignature {
            params: param_types,
            return_type: Box::new(joined),
        }))
    }

    /// Extract the implicit return type of a function body's trailing
    /// statement. Handles both `Stmt::Expr(e)` (a bare expression) and
    /// `Stmt::FunctionDef` (a bare function literal in statement
    /// position, which is how the parser represents the trailing
    /// function in `function() { function() { 1L } }`).
    ///
    /// Returns `None` when the body is empty, the last statement is not
    /// an expression-like form, or the trailing expression is a
    /// `return(...)` call (which `collect_returns_stmt_at_depth`
    /// already counted).
    pub(crate) fn trailing_return_type(
        &mut self,
        body: &[Stmt],
        scope: &mut Scope,
        depth: usize,
    ) -> Option<RType> {
        let last = body.last()?;
        match last {
            Stmt::Expr(e) => {
                if is_return_call(e) {
                    None
                } else {
                    Some(self.infer_discarding(e, scope))
                }
            }
            Stmt::FunctionDef { params, body, .. } => {
                // A trailing bare function definition is the implicit
                // return value. Build it as a function literal so the
                // signature is attached.
                Some(self.function_value_from_literal(params, body, scope, depth))
            }
            _ => None,
        }
    }

    /// Pass-3 entry point: walk a top-level statement for diagnostics.
    /// Thin wrapper over `walk_stmt` (the unified walker) with return
    /// collection disabled and emission enabled.
    pub(crate) fn check_stmt(&mut self, s: &Stmt, scope: &mut Scope) {
        self.walk_stmt(s, scope, None);
    }

    pub(crate) fn diagnostic_parameter_type(
        &self,
        function: &str,
        parameter: &Param,
        parameters: &[Param],
        default: &Expr,
    ) -> RType {
        // A default is selected only when the argument is omitted. An observed
        // omitted call proves that execution path even when other calls supply
        // the argument; without such evidence, the parameter stays opaque.
        let Some(index) = parameters
            .iter()
            .position(|candidate| candidate.name == parameter.name)
        else {
            return RType::unknown();
        };
        let Some(call_sites) = self.fn_table.call_sites.get(function) else {
            return RType::unknown();
        };
        if call_sites.is_empty() {
            return RType::unknown();
        }
        let omitted_somewhere = call_sites.iter().any(|arguments| {
            let exact = arguments
                .iter()
                .flatten()
                .any(|name| name == &parameter.name);
            let positional = arguments.iter().filter(|name| name.is_none()).count() > index;
            !exact && !positional
        });
        if omitted_somewhere {
            infer_literal_default(default)
        } else if let Some(forwarded) =
            self.forwarded_default_type(function, &parameter.name, index)
        {
            forwarded
        } else {
            RType::unknown()
        }
    }

    pub(crate) fn forwarded_default_type(
        &self,
        function: &str,
        parameter: &str,
        index: usize,
    ) -> Option<RType> {
        self.fn_table.forwarded_calls.iter().find_map(|call| {
            if call.callee != function {
                return None;
            }
            let argument = call
                .arguments
                .iter()
                .find(|(name, _)| name.as_deref() == Some(parameter))
                .or_else(|| {
                    call.arguments
                        .iter()
                        .filter(|(name, _)| name.is_none())
                        .nth(index)
                })?;
            let source = argument.1.as_deref()?;
            let (source_index, source_parameter) = call
                .caller_params
                .iter()
                .enumerate()
                .find(|(_, candidate)| candidate.name == source)?;
            let source_default = source_parameter.default.as_ref()?;
            let caller_sites = self.fn_table.call_sites.get(&call.caller)?;
            let omitted = caller_sites.iter().any(|arguments| {
                let exact = arguments.iter().flatten().any(|name| name == source);
                let positional =
                    arguments.iter().filter(|name| name.is_none()).count() > source_index;
                !exact && !positional
            });
            omitted.then(|| infer_literal_default(source_default))
        })
    }

    pub(crate) fn assign_target(&mut self, target: &Expr, vt: RType, scope: &mut Scope) {
        match target {
            Expr::Ident { name, .. } | Expr::String(name, _) => {
                scope.insert(name.clone(), vt);
            }
            Expr::Index {
                base,
                kind,
                args,
                span,
            } => {
                if self.assign_nested_record_path(target, vt.clone(), scope) {
                    return;
                }
                if self.assign_index_target(base, *kind, args, vt, *span, scope) {
                    return;
                }
                // Other indexed assignments `x[i] <- v` etc. are too
                // dynamic for v1; still infer the target so diagnostics on
                // the base expression fire.
                self.infer(target, scope);
            }
            _ => {
                // Indexed assignment `x[i] <- v` etc. is too dynamic for v1.
                self.infer(target, scope);
            }
        }
    }

    pub(crate) fn assign_class_attribute(
        &mut self,
        target: &Expr,
        value: &Expr,
        scope: &mut Scope,
    ) -> bool {
        let Expr::Call { func, args, .. } = target else {
            return false;
        };
        if !matches!(func.as_ref(), Expr::Ident { name, .. } if name == "class") {
            return false;
        }
        let Some(Expr::Ident { name, .. }) = args.first().map(|arg| &arg.value) else {
            return false;
        };
        let Some(base) = scope.get(name).cloned() else {
            return false;
        };
        let class = match parse_class_literal(value) {
            ClassLiteral::Single(class) => ClassVector::single(&class),
            ClassLiteral::Multi(classes) => {
                let classes: Vec<&str> = classes.iter().map(String::as_str).collect();
                ClassVector::from_slice(&classes)
            }
            ClassLiteral::Unknown => ClassVector::unknown(),
        };
        scope.insert(name.clone(), base.with_class(class));
        true
    }

    /// Replacement calls such as `dimnames(x) <- value` mutate `x`; the
    /// accessor expression is not an ordinary read/call that should be
    /// argument-checked. Keep the binding but conservatively forget its shape.
    pub(crate) fn assign_replacement_target(&mut self, target: &Expr, scope: &mut Scope) -> bool {
        let Expr::Call { func, args, .. } = target else {
            return false;
        };
        let Expr::Ident { name: accessor, .. } = func.as_ref() else {
            return false;
        };
        if !matches!(
            accessor.as_str(),
            "names" | "dimnames" | "colnames" | "rownames" | "attr" | "levels" | "environment"
        ) {
            return false;
        }
        let Some(Expr::Ident { name, .. }) = args.first().map(|arg| &arg.value) else {
            return false;
        };
        scope.insert(name.clone(), RType::unknown());
        true
    }

    pub(crate) fn assign_nested_record_path(
        &self,
        target: &Expr,
        value: RType,
        scope: &mut Scope,
    ) -> bool {
        fn path(expr: &Expr, fields: &mut Vec<String>) -> Option<String> {
            match expr {
                Expr::Ident { name, .. } => Some(name.clone()),
                Expr::Index {
                    base, kind, args, ..
                } => {
                    let root = path(base, fields)?;
                    if matches!(kind, IndexKind::Dollar | IndexKind::Double) {
                        if let Some(field) = assigned_column_name(*kind, args) {
                            fields.push(field.to_string());
                        }
                    }
                    Some(root)
                }
                _ => None,
            }
        }
        fn write(base: RType, fields: &[String], value: RType) -> RType {
            let Some((field, rest)) = fields.split_first() else {
                return value;
            };
            if rest.is_empty() {
                return type_with_assigned_column(base, field, value);
            }
            let child = base
                .columns
                .as_ref()
                .and_then(|schema| schema.get(field))
                .filter(|ty| matches!(ty.mode, Mode::List | Mode::Opaque))
                .unwrap_or_else(|| {
                    RType::new(Mode::List, Length::Unknown).with_columns(Arc::new(ColumnSchema {
                        columns: Vec::new(),
                        complete: false,
                        locally_constructed: false,
                    }))
                });
            let child = write(child, rest, value);
            type_with_assigned_column(base, field, child)
        }

        let mut fields = Vec::new();
        let Some(root) = path(target, &mut fields) else {
            return false;
        };
        if fields.len() < 2 {
            return false;
        }
        let Some(root_type) = scope.get(&root).cloned() else {
            return false;
        };
        scope.insert(root, write(root_type, &fields, value));
        true
    }

    pub(crate) fn assign_index_target(
        &mut self,
        base: &Expr,
        kind: IndexKind,
        args: &[Arg],
        vt: RType,
        span: Span,
        scope: &mut Scope,
    ) -> bool {
        if let Expr::Index {
            base: root,
            kind: field_kind,
            args: field_args,
            ..
        } = base
        {
            let Expr::Ident {
                name: root_name, ..
            } = root.as_ref()
            else {
                return false;
            };
            let Some(field) = assigned_column_name(*field_kind, field_args) else {
                return false;
            };
            let Some(root_type) = scope.get(root_name).cloned() else {
                return false;
            };
            if root_type.class.contains("data.frame") {
                if let Some(schema) = &root_type.columns {
                    if schema.complete
                        && schema.get(field).is_none()
                        && schema.names().iter().any(|name| name.starts_with(field))
                    {
                        self.emit_undefined_column(field, schema, span);
                    }
                }
            }
            let field_type = root_type
                .columns
                .as_ref()
                .and_then(|schema| schema.get(field))
                .unwrap_or(vt);
            scope.insert(
                root_name.clone(),
                type_with_assigned_column(root_type, field, field_type),
            );
            return true;
        }
        let Expr::Ident {
            name: base_name, ..
        } = base
        else {
            return false;
        };
        let Some(base_t) = scope.get(base_name).cloned() else {
            let _ = self.infer(base, scope);
            return true;
        };
        if matches!(kind, IndexKind::Single) {
            let names = args
                .first()
                .map(|arg| string_literals(&arg.value))
                .unwrap_or_default();
            if !names.is_empty() {
                let mut updated = base_t;
                for name in names {
                    updated = type_with_assigned_column(updated, &name, vt.clone());
                }
                scope.insert(base_name.clone(), updated);
                return true;
            }
        }
        let Some(col) = assigned_column_name(kind, args) else {
            // A dynamic `$`/`[[` write proves that the record may contain
            // additional fields. Preserve known fields but mark the schema
            // incomplete so a later unknown-field read degrades to opaque
            // instead of emitting RY060.
            if matches!(
                kind,
                IndexKind::Dollar | IndexKind::Double | IndexKind::Single
            ) && matches!(base_t.mode, Mode::List | Mode::Null)
            {
                let mut schema = base_t
                    .columns
                    .as_ref()
                    .map(|schema| (**schema).clone())
                    .unwrap_or_default();
                schema.complete = false;
                scope.insert(base_name.clone(), base_t.with_columns(Arc::new(schema)));
                return true;
            }
            return false;
        };
        if matches!(
            base_t.mode,
            Mode::Integer
                | Mode::Double
                | Mode::Character
                | Mode::Logical
                | Mode::Complex
                | Mode::Raw
        ) && base_t.columns.is_none()
        {
            self.emit(
                Severity::Error,
                span,
                "RY061",
                format!(
                    "$ operator is invalid for atomic vectors of mode `{}`",
                    base_t.mode
                ),
            );
            return true;
        }
        scope.insert(
            base_name.clone(),
            type_with_assigned_column(base_t, col, vt),
        );
        true
    }

    pub(crate) fn infer_block_expr(&mut self, body: &[Stmt], scope: &mut Scope) -> RType {
        let Some((last, prefix)) = body.split_last() else {
            return RType::new(Mode::Null, Length::Zero);
        };
        for s in prefix {
            self.walk_stmt(s, scope, None);
        }
        self.infer_stmt_value(last, scope)
    }

    pub(crate) fn infer_stmt_value(&mut self, stmt: &Stmt, scope: &mut Scope) -> RType {
        if scope.unreachable {
            return RType::unknown();
        }
        match stmt {
            Stmt::Assign { target, value, .. } => {
                let vt = self.infer(value, scope);
                if !self.assign_class_attribute(target, value, scope)
                    && !self.assign_replacement_target(target, scope)
                {
                    self.assign_target(target, vt.clone(), scope);
                }
                vt
            }
            Stmt::Expr(e) => self.infer(e, scope),
            Stmt::Return { value, .. } => value
                .as_ref()
                .map(|v| self.infer(v, scope))
                .unwrap_or_else(|| RType::new(Mode::Null, Length::Zero)),
            Stmt::If {
                cond,
                then,
                else_,
                span,
            } => {
                let then_expr = Expr::Block {
                    body: then.clone(),
                    span: *span,
                };
                let else_expr = else_.as_ref().map(|body| {
                    Box::new(Expr::Block {
                        body: body.clone(),
                        span: *span,
                    })
                });
                self.infer_if_expr(cond, &then_expr, &else_expr, *span, scope)
            }
            Stmt::For { .. } | Stmt::While { .. } | Stmt::FunctionDef { .. } => {
                self.walk_stmt(stmt, scope, None);
                RType::unknown()
            }
        }
    }

    /// Infer the type of an expression, emitting diagnostics for misuse.
    pub(crate) fn infer(&mut self, e: &Expr, scope: &mut Scope) -> RType {
        // `infer_pipe` has already inferred the expression it injects into
        // its desugared call. The entry exists only while that call is being
        // inferred, so its type is valid for this exact scope.
        if let Some(t) = self.pipe_argument_types.get(&span_of(e)) {
            return t.clone();
        }
        match e {
            Expr::Logical(_, _) => RType::scalar(Mode::Logical),
            Expr::Integer(_, _) => RType::scalar(Mode::Integer),
            Expr::Double(_, _) => RType::scalar(Mode::Double),
            Expr::String(_, _) => RType::scalar(Mode::Character),
            Expr::Null(_) => RType::new(Mode::Null, Length::Zero),
            Expr::Na(t, _) => t.clone(),
            Expr::Ident { name, span } => match scope.get(name) {
                Some(t) => {
                    let is_lexical_binding_under_unknown_mask = scope.data_mask_unknown
                        && scope.get(crate::nse::DATA_MASK_ACTIVE).is_some()
                        && scope
                            .get(&format!("{}{name}", crate::nse::DATA_MASK_ENV_PREFIX))
                            .is_some()
                        && scope
                            .get(&format!("{}{name}", crate::nse::DATA_MASK_COLUMN_PREFIX))
                            .is_none();
                    if is_lexical_binding_under_unknown_mask {
                        RType::unknown()
                    } else {
                        t.clone()
                    }
                }
                None => {
                    if self.external_bindings.contains(name) {
                        return RType::unknown();
                    }
                    if self.external_bindings.iter().any(|binding| {
                        binding.strip_prefix("\0useDynLib:").is_some_and(|prefix| {
                            name.strip_prefix(prefix)
                                .is_some_and(|rest| !rest.is_empty())
                        })
                    }) {
                        return RType::unknown();
                    }
                    // Built-in dataset? (mtcars, iris, ...) Resolve before
                    // flagging the identifier as unbound.
                    if let Some(jt) = self.typeshed.datasets.get(name) {
                        return json_rtype_to_rtype(jt);
                    }
                    // Known typeshed function used as a value (e.g.
                    // `sapply(x, sqrt)` passes `sqrt` as a bare
                    // identifier)? Return an opaque function value
                    // rather than flagging it as unbound. The higher-
                    // order call handlers resolve the signature when
                    // the callback is invoked.
                    if self.typeshed.functions.contains_key(name) {
                        return RType::scalar(Mode::Function);
                    }
                    // A function from a loaded package (e.g. purrr's
                    // `map` used as a value) resolves to a function too.
                    if self.bare_loaded.iter().any(|pkg| {
                        self.package_is_known(pkg)
                            && self
                                .package_typeshed(pkg)
                                .map(|t| t.functions.contains_key(name))
                                .unwrap_or(false)
                    }) {
                        return RType::scalar(Mode::Function);
                    }
                    // User-defined function in the FnTable used as a
                    // value? Same treatment.
                    if self.fn_table.fns.contains_key(name) {
                        return RType::scalar(Mode::Function);
                    }
                    // Cross-file variable defined in another file of
                    // the project (or a top-level assignment later in
                    // this same file)? Return opaque rather than
                    // flagging it as unbound. Without this, multi-file
                    // projects like ggplot2 (where `GeomRect <-
                    // ggproto(...)` is a CALL, not a function literal)
                    // generate hundreds of false-positive RY010
                    // warnings for references to symbols defined in
                    // sibling files.
                    if self.known_vars.contains(name) {
                        return RType::unknown();
                    }
                    if self
                        .typeshed
                        .globals
                        .ambient_functions
                        .iter()
                        .any(|function| function == name)
                    {
                        // Value-position uses of ambient functions are
                        // overwhelmingly legitimate higher-order idioms
                        // (`lapply(x, enc2utf8)`, `do.call(rbind, z)`), so
                        // resolve silently as a function value. The typo
                        // class (`col`, `oldClass` misused as data) is still
                        // caught downstream when the function type flows
                        // into comparisons or arithmetic (RY030/RY033).
                        return RType::scalar(Mode::Function);
                    }
                    // Existence-only standard and ambient globals are a
                    // fallback after typed datasets, functions, and project
                    // bindings so inventory overlap cannot erase precision.
                    if self
                        .typeshed
                        .globals
                        .ambient
                        .iter()
                        .any(|global| global == name)
                    {
                        return RType::unknown();
                    }
                    // Namespace-qualified reference (`pkg::name`),
                    // including the bare reexport pattern
                    // (`rlang::set_names` or `magrittr::`%>%`` in
                    // statement position) and qualified values
                    // (`x <- S7::class_any`). We don't model other
                    // packages' export tables, so we treat these as
                    // opaque cross-package references and never emit
                    // RY010. The `contains("::")` test matches both
                    // `::` and `:::`.
                    if name.contains("::") {
                        return RType::unknown();
                    }
                    // Special/operator names referenced via backticks
                    // (e.g. `` `%+%` ``, `` `+` ``) or bare operator
                    // symbols. The parser preserves the surrounding
                    // backticks in the identifier name, so a leading
                    // backtick is the primary signal. These are commonly
                    // user-defined operators or package reexports that
                    // we cannot resolve against any scope, typeshed, or
                    // FnTable -- suppressing RY010 here avoids
                    // false positives on code like ggplot2's `` `%+%` ``
                    // operator. We return opaque rather than flagging.
                    if name.starts_with('`') || name.contains('%') || is_operator_symbol(name) {
                        return RType::unknown();
                    }
                    if scope.data_mask_unknown || scope.search_path_unknown {
                        return RType::unknown();
                    }
                    self.emit(
                        Severity::Warning,
                        *span,
                        "RY010",
                        format!("variable `{}` is not bound in this scope", name),
                    );
                    RType::unknown()
                }
            },
            Expr::BinOp { op, lhs, rhs, span } => {
                // Pipes need structural access to `rhs` (to build a
                // desugared call), so they bypass `infer_binop`'s
                // type-only signature.
                if matches!(*op, BinOpKind::PipeForward) {
                    return self.infer_pipe(lhs, rhs, *span, scope);
                }
                if matches!(*op, BinOpKind::PipeAssign) {
                    // `%<>%` (assignment pipe): like `%>%` but also
                    // rebinds the LHS identifier to the result, so
                    // `x %<>% f()` is `x <- x %>% f()`.
                    let result = self.infer_pipe(lhs, rhs, *span, scope);
                    if let Expr::Ident { name, .. } = lhs.as_ref() {
                        scope.insert(name.clone(), result.clone());
                    }
                    return result;
                }
                if matches!(*op, BinOpKind::PipeTee) {
                    return self.infer_pipe_tee(lhs, rhs, scope);
                }
                // Assignment in expression position (e.g. the inner
                // assignment in `a <- b <- 1L`): bind the LHS and
                // return the RHS type. R's `<-` returns the assigned
                // value (invisibly).
                if matches!(*op, BinOpKind::Assign | BinOpKind::SuperAssign) {
                    let rt = self.infer(rhs, scope);
                    if !self.assign_class_attribute(lhs, rhs, scope)
                        && !self.assign_replacement_target(lhs, scope)
                    {
                        self.assign_target(lhs, rt.clone(), scope);
                    }
                    return rt;
                }
                // `:` sequence operator: when both operands are
                // integer-valued literals we can pin the result length
                // exactly as `|b - a| + 1` (R always returns integer
                // for whole-number endpoints). We need the raw AST to
                // read the literal values, so this case lives here in
                // the `Expr::BinOp` arm rather than in the type-only
                // `infer_binop`. Non-literal operands fall through to
                // `infer_binop`'s lattice-based `seq` (Unknown length).
                if matches!(*op, BinOpKind::Colon) {
                    if let (Some(a), Some(b)) = (extract_literal_int(lhs), extract_literal_int(rhs))
                    {
                        let len = (b - a).unsigned_abs() as usize;
                        let len = len.saturating_add(1);
                        if len > 0 {
                            return RType::new(Mode::Integer, Length::Known(len));
                        }
                    }
                }
                if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                    return self.infer_short_circuit_binop(*op, lhs, rhs, scope, *span);
                }
                if matches!(op, BinOpKind::Eq | BinOpKind::Ne)
                    && (is_na_literal(lhs) || is_na_literal(rhs))
                {
                    self.emit(
                        Severity::Warning,
                        *span,
                        "RY034",
                        "comparison with `NA` always produces `NA`; use `is.na()` instead",
                    );
                }
                let lt = self.infer(lhs, scope);
                let rt = self.infer(rhs, scope);
                self.infer_binop(
                    *op,
                    lt,
                    rt,
                    *span,
                    known_null_arithmetic_operand(lhs, scope)
                        || known_null_arithmetic_operand(rhs, scope),
                )
            }
            Expr::UnaryOp { op, expr, span } => {
                // Detect tidyeval `!!` (unquote) and `!!!` (splice)
                // operators BEFORE inferring the inner expression.
                // tree-sitter parses these as nested unary `!`:
                // `!!x` -> `!(!x)`, `!!!x` -> `!(!(!x))`.
                // These are NSE operators, not actual negation. We must
                // strip ALL nested `!` operators and only infer the
                // innermost operand, so RY021 doesn't fire on the
                // intermediate `!` applied to a list/function.
                if matches!(op, UnaryOpKind::Not) {
                    if let Expr::UnaryOp {
                        op: UnaryOpKind::Not,
                        ..
                    } = expr.as_ref()
                    {
                        // Strip all consecutive `!` operators to find
                        // the innermost real expression.
                        let mut innermost = expr.as_ref();
                        while let Expr::UnaryOp {
                            op: UnaryOpKind::Not,
                            expr: next,
                            ..
                        } = innermost
                        {
                            innermost = next.as_ref();
                        }
                        let _ = self.infer(innermost, scope);
                        return RType::unknown();
                    }
                }
                let t = self.infer(expr, scope);
                // Base R's `Math.data.frame`/`Ops.data.frame` apply unary
                // operators column-wise. Preserve the frame rather than
                // treating its list storage mode as primitive evidence.
                if t.class.contains("data.frame") {
                    return t;
                }
                if let Some(dispatched) = self.try_s3_unary_dispatch(*op, &t) {
                    return dispatched;
                }
                match op {
                    UnaryOpKind::Neg => {
                        if matches!(
                            t.mode,
                            Mode::Character | Mode::Raw | Mode::List | Mode::Function
                        ) {
                            self.emit(
                                Severity::Error,
                                *span,
                                "RY020",
                                format!("cannot apply unary `-` to `{}`", t.mode),
                            );
                            RType::unknown()
                        } else {
                            let mode = match t.mode {
                                Mode::Logical | Mode::Null => Mode::Integer,
                                other => other,
                            };
                            RType::new(mode, t.length)
                        }
                    }
                    UnaryOpKind::Not => {
                        if matches!(t.mode, Mode::Character | Mode::List | Mode::Function) {
                            self.emit(
                                Severity::Error,
                                *span,
                                "RY021",
                                format!("cannot apply `!` to `{}`", t.mode),
                            );
                        }
                        RType::new(Mode::Logical, t.length)
                    }
                }
            }
            Expr::Call { func, args, span } => self.infer_call(func, args, scope, *span),
            Expr::Index {
                base,
                kind,
                args,
                span,
            } => {
                let receiver_name = ident_name(base);
                if receiver_name == Some(".env")
                    && scope.get(crate::nse::DATA_MASK_ACTIVE).is_some()
                {
                    let name = match kind {
                        IndexKind::Dollar => args.first().and_then(|arg| arg.name.as_deref()),
                        IndexKind::Double => args.first().and_then(|arg| match &arg.value {
                            Expr::String(name, _) => Some(name.as_str()),
                            _ => None,
                        }),
                        IndexKind::Single => None,
                    };
                    if let Some(name) = name {
                        let key = format!("{}{name}", crate::nse::DATA_MASK_ENV_PREFIX);
                        if let Some(ty) = scope.get(&key) {
                            return ty.clone();
                        }
                        self.emit(
                            Severity::Warning,
                            *span,
                            "RY010",
                            format!("variable `{name}` is not bound in this scope"),
                        );
                        return RType::unknown();
                    }
                }
                let default_null_receiver = matches!(
                    base.as_ref(),
                    Expr::Ident { name, .. }
                        if scope.is_default_parameter(name)
                            && matches!(scope.get(name).map(|ty| ty.mode), Some(Mode::Null))
                );
                let bt = self.infer(base, scope);
                self.infer_index(bt, *kind, args, *span, default_null_receiver, scope)
            }
            Expr::Function { params, body, .. } => {
                // Pass 3: build a `Mode::Function` value with an
                // inferred `fn_sig` when we can. This mirrors the
                // non-emitting inference path so a function literal in a
                // top-level expression (`g <- f(); v <- (function() 1L)()`)
                // resolves the same way as one inside a return slot.
                self.function_value_from_literal(params, body, scope, 0)
            }
            Expr::Block { body, .. } => {
                if let Some((name, span)) = embraced_symbol(body) {
                    let key = format!("{}{name}", crate::nse::DATA_MASK_ENV_PREFIX);
                    let binding = if scope.get(crate::nse::DATA_MASK_ACTIVE).is_some() {
                        scope.get(&key)
                    } else {
                        scope.get(name)
                    };
                    if binding.is_none() {
                        self.emit(
                            Severity::Warning,
                            span,
                            "RY010",
                            format!("variable `{name}` is not bound in this scope"),
                        );
                    }
                    RType::unknown()
                } else {
                    self.infer_block_expr(body, scope)
                }
            }
            Expr::If {
                cond,
                then,
                else_,
                span,
            } => self.infer_if_expr(cond, then, else_, *span, scope),
            Expr::Unknown(_) => RType::unknown(),
        }
    }

    fn push_enclosing_formals(&mut self, params: &[Param]) {
        self.enclosing_formals.push(EnclosingFormals {
            names: params
                .iter()
                .filter(|parameter| parameter.name != "...")
                .map(|parameter| parameter.name.clone())
                .collect(),
            has_dots: params.iter().any(|parameter| parameter.name == "..."),
        });
    }
}

impl Checker {
    const MAX_FUNCTION_ALIAS_DEPTH: usize = 8;

    fn function_alias_target(&self, value: &Expr, scope: &Scope) -> Option<String> {
        let Expr::Ident { name, .. } = value else {
            return None;
        };

        let mut target = name.as_str();
        for _ in 0..Self::MAX_FUNCTION_ALIAS_DEPTH {
            let Some(next) = scope.function_alias(target) else {
                break;
            };
            target = next;
        }

        self.is_aliasable_function(target)
            .then(|| target.to_string())
    }

    fn is_aliasable_function(&self, name: &str) -> bool {
        matches!(name, "~" | "expression" | "vars")
            || is_nse_symbol_fn(name)
            || self.resolve_typeshed_sig(name).is_some()
            || self
                .typeshed
                .globals
                .ambient_functions
                .iter()
                .any(|function| function == name)
    }

    fn destructuring_operator_in_scope(&self) -> bool {
        self.loaded
            .iter()
            .chain(self.imported_from.values())
            .any(|package| {
                self.package_typeshed(package).is_some_and(|typeshed| {
                    typeshed.functions.contains_key("%<-%")
                        || typeshed.functions.contains_key("%->%")
                })
            })
    }
}

fn bind_destructure_pattern(pattern: &Expr, scope: &mut Scope) {
    match pattern {
        Expr::Ident { name, .. } => scope.insert(name.clone(), RType::unknown()),
        Expr::Call { func, args, .. } if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "c") => {
            for argument in args {
                bind_destructure_pattern(&argument.value, scope);
            }
        }
        Expr::Call { func, args, .. } if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "%<-%") => {
            for argument in args {
                bind_destructure_pattern(&argument.value, scope);
            }
        }
        _ => {}
    }
}

pub(crate) fn embraced_symbol(body: &[Stmt]) -> Option<(&str, Span)> {
    let [Stmt::Expr(Expr::Block { body: inner, .. })] = body else {
        return None;
    };
    let [Stmt::Expr(Expr::Ident { name, span })] = inner.as_slice() else {
        return None;
    };
    Some((name, *span))
}
