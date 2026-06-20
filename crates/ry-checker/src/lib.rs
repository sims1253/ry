//! Local type inference + diagnostics.
//!
//! v1 scope: single-file, inference-only, NSE-opaque. We walk statements
//! top-down, maintaining a per-scope binding table `name -> RType`.
//!
//! v2 additions: interprocedural function-return inference via a
//! module-level FnTable and a fixpoint loop. The first pass collects
//! function definitions; subsequent passes refine each function's
//! inferred return type until stable (or the depth cap is hit).

pub mod rules;
pub mod format;

use ry_core::ast::*;
use ry_core::types::{Length, Mode, RType};
use ry_core::Span;
use ry_typeshed::{load_base, FunctionSig, JsonRType, ReturnSpec, Typeshed};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub path: String,
    pub code: &'static str,
    pub message: String,
}

impl Diagnostic {
    pub fn new(severity: Severity, span: Span, path: &str, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity,
            span,
            path: path.to_string(),
            code,
            message: message.into(),
        }
    }

    /// Look up the rule metadata for this diagnostic's code, if any.
    pub fn rule(&self) -> Option<&'static rules::Rule> {
        rules::find(self.code)
    }
}

/// Severity overrides that a caller (typically the CLI) wants to apply.
/// Matches ty's `--error` / `--warn` / `--ignore` semantics.
#[derive(Debug, Clone, Default)]
pub struct SeverityFilter {
    pub errors: Vec<String>,
    pub warns: Vec<String>,
    pub ignores: Vec<String>,
}

impl SeverityFilter {
    /// Resolve a user-provided token (rule code, rule name, or "all")
    /// into the list of matching codes.
    fn expand(token: &str) -> Vec<&'static str> {
        if token == "all" {
            return rules::all_codes();
        }
        match rules::find(token) {
            Some(r) => vec![r.code],
            None => Vec::new(),
        }
    }

    /// Add a token (code / name / "all") to one of the buckets.
    pub fn add_error(&mut self, token: &str) {
        self.errors.push(token.to_string());
    }
    pub fn add_warn(&mut self, token: &str) {
        self.warns.push(token.to_string());
    }
    pub fn add_ignore(&mut self, token: &str) {
        self.ignores.push(token.to_string());
    }

    /// Returns the effective severity for a code, or None to suppress it.
    /// Precedence (highest to lowest): ignore > error > warn > default.
    pub fn effective(&self, code: &str, default: Severity) -> Option<Severity> {
        for tok in &self.ignores {
            if Self::expand(tok).contains(&code) {
                return None;
            }
        }
        for tok in &self.errors {
            if Self::expand(tok).contains(&code) {
                return Some(Severity::Error);
            }
        }
        for tok in &self.warns {
            if Self::expand(tok).contains(&code) {
                return Some(Severity::Warning);
            }
        }
        Some(default)
    }
}

/// A single scope's binding table.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub bindings: HashMap<String, RType>,
}

impl Scope {
    pub fn get(&self, name: &str) -> Option<&RType> {
        self.bindings.get(name)
    }

    pub fn insert(&mut self, name: impl Into<String>, t: RType) {
        self.bindings.insert(name.into(), t);
    }
}

/// A user-defined function recorded for interprocedural inference.
/// We store the AST nodes by index into a side-table the checker owns,
/// avoiding lifetime entanglement with the SourceFile.
#[derive(Debug, Clone)]
struct UserFn {
    /// Parameter names with their inferred-or-default types.
    params: Vec<(String, RType)>,
    /// Indices into the body Vec<Stmt>. Stored as a snapshot we can
    /// re-walk on each fixpoint iteration.
    body: Vec<Stmt>,
    /// Currently-inferred return type. Starts as UNKNOWN, refined by
    /// each fixpoint iteration. Stored as a slot index so all calls
    /// observe the latest refinement without rebuilding the table.
    return_slot: usize,
}

/// Side-table of inferred return types, indexed by `UserFn::return_slot`.
/// Stored separately so we can clone the table cheaply when entering a
/// nested inference pass without deep-cloning the function bodies.
#[derive(Debug, Clone, Default)]
struct ReturnSlots(Vec<RType>);

impl ReturnSlots {
    fn get(&self, i: usize) -> RType {
        self.0.get(i).copied().unwrap_or(RType::UNKNOWN)
    }
    fn set(&mut self, i: usize, t: RType) {
        if i >= self.0.len() {
            self.0.resize(i + 1, RType::UNKNOWN);
        }
        self.0[i] = t;
    }
}

/// Map from function name to its recorded definition. A name shadows
/// earlier entries (later definitions win), mirroring R's own semantics
/// for top-level rebinding.
#[derive(Debug, Clone, Default)]
struct FnTable {
    fns: HashMap<String, UserFn>,
}

/// Maximum fixpoint depth before we give up and freeze as Opaque.
/// Conservative cap; well-typed programs converge in 2-3 iterations.
const MAX_FIXPOINT_DEPTH: usize = 8;

pub struct Checker {
    typeshed: Typeshed,
    diagnostics: Vec<Diagnostic>,
    path: String,
    /// User-defined functions collected in pass 1.
    fn_table: FnTable,
    /// Inferred return types, refined by the fixpoint loop.
    return_slots: ReturnSlots,
    /// Stack of function names currently being inferred (cycle detection).
    inferring: Vec<String>,
}

impl Checker {
    pub fn new(path: &str) -> Self {
        let typeshed = load_base().expect("typeshed must load");
        Self {
            typeshed,
            diagnostics: Vec::new(),
            path: path.to_string(),
            fn_table: FnTable::default(),
            return_slots: ReturnSlots::default(),
            inferring: Vec::new(),
        }
    }

    pub fn check(&mut self, file: &SourceFile) -> &[Diagnostic] {
        self.path = file.path.clone();

        // Pass 1: collect function definitions into the FnTable. We don't
        // emit diagnostics yet - the body's `return` types depend on the
        // table being fully populated.
        self.collect_fns(&file.stmts);

        // Pass 2 (fixpoint): refine each function's inferred return type
        // until the table stabilizes or we hit MAX_FIXPOINT_DEPTH. We
        // snapshot between iterations to detect convergence.
        for _ in 0..MAX_FIXPOINT_DEPTH {
            let before = self.return_slots.clone();
            let names: Vec<String> = self.fn_table.fns.keys().cloned().collect();
            for name in names {
                self.refine_fn_return(&name);
            }
            if self.return_slots.0 == before.0 {
                break;
            }
        }

        // Pass 3: final walk, emitting all diagnostics. Function calls
        // now resolve against the refined FnTable.
        let mut scope = Scope::default();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
        &self.diagnostics
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Apply a `SeverityFilter` to the diagnostics collected so far,
    /// mutating severities (or dropping suppressed ones) in place.
    pub fn apply_filter(&mut self, filter: &SeverityFilter) {
        let mut out: Vec<Diagnostic> = Vec::with_capacity(self.diagnostics.len());
        for d in self.diagnostics.drain(..) {
            let default = d
                .rule()
                .map(|r| r.default_severity)
                .unwrap_or(Severity::Warning);
            if let Some(sev) = filter.effective(d.code, default) {
                let mut d = d;
                d.severity = sev;
                out.push(d);
            }
        }
        self.diagnostics = out;
    }

    fn emit(&mut self, severity: Severity, span: Span, code: &'static str, msg: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(
            severity,
            span,
            &self.path,
            code,
            msg,
        ));
    }

    /// Pass 1: walk top-level (and only top-level) statements, collecting
    /// function definitions of the form `name <- function(...) body` into
    /// the FnTable. Nested function definitions are recorded only if they
    /// are themselves bound to a name at their enclosing scope; this is
    /// sufficient for v2 since R-style nested defs typically close over
    /// locals and are tricky to type without proper closure analysis.
    fn collect_fns(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.collect_fns_stmt(s);
        }
    }

    fn collect_fns_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Assign { target, value, .. } => {
                if let (Expr::Ident { name, .. }, Expr::Function { params, body, .. }) =
                    (target, value)
                {
                    self.record_fn(name.clone(), params, body.clone());
                }
                // Recurse into compound statements so we catch
                // function-returning-function patterns at top level.
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
            _ => {}
        }
    }

    fn record_fn(&mut self, name: String, params: &[Param], body: Vec<Stmt>) {
        // We infer param types from defaults alone; params without a
        // default start as UNKNOWN (callers can refine them later).
        let params: Vec<(String, RType)> = params
            .iter()
            .map(|p| {
                let t = match &p.default {
                    // Defer inference to first fixpoint iteration by
                    // starting as UNKNOWN; if a literal default is present
                    // we can compute it now without a scope.
                    Some(e) => infer_literal_default(e),
                    None => RType::UNKNOWN,
                };
                (p.name.clone(), t)
            })
            .collect();
        let slot = self.return_slots.0.len();
        self.return_slots.set(slot, RType::UNKNOWN);
        let prev = self.fn_table.fns.insert(
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
    }

    /// Pass 2: refine one function's inferred return type by walking its
    /// body once. Returns are collected from `return(...)` calls and from
    /// the trailing expression of the body, then joined.
    fn refine_fn_return(&mut self, name: &str) {
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
        for (n, t) in &params {
            scope.insert(n.clone(), *t);
        }
        // The function's own name is in scope as a function value, so
        // recursive calls resolve to a user-fn lookup.
        scope.insert(name.to_string(), RType::scalar(Mode::Function, false));

        let mut returns: Vec<RType> = Vec::new();
        for s in &body_clone {
            self.collect_returns_stmt(s, &scope, &mut returns);
        }
        // Trailing expression of a braced body is the implicit return.
        if let Some(Stmt::Expr(e)) = body_clone.last() {
            if !is_return_call(e) {
                returns.push(self.infer(e, &mut scope.clone()));
            }
        }

        // Fold the collected return types. We start from the first
        // element rather than UNKNOWN because join() treats Opaque as
        // absorbing (correct for control-flow merge but wrong for an
        // empty-fold identity).
        let joined = if returns.is_empty() {
            RType::UNKNOWN
        } else {
            let mut iter = returns.into_iter();
            let first = iter.next().unwrap_or(RType::UNKNOWN);
            iter.fold(first, |acc, t| acc.join(t))
        };
        self.return_slots.set(slot, joined);
        self.inferring.pop();
    }

    /// Walk a statement collecting the types of any values that flow out
    /// of the function via `return(...)` or `invisible(...)`.
    fn collect_returns_stmt(
        &mut self,
        s: &Stmt,
        scope: &Scope,
        returns: &mut Vec<RType>,
    ) {
        match s {
            Stmt::Expr(e) => {
                if let Some(rt) = self.try_infer_return_call(e, scope) {
                    returns.push(rt);
                }
            }
            Stmt::If { cond, then, else_, .. } => {
                let _ = cond;
                for s in then {
                    self.collect_returns_stmt(s, scope, returns);
                }
                if let Some(e) = else_ {
                    for s in e {
                        self.collect_returns_stmt(s, scope, returns);
                    }
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                // next/return inside loops are still real returns; we
                // approximate by walking unconditionally. FALSE POSITIVES
                // for early-exit returns are possible but rare in idiomatic R.
                for s in body {
                    self.collect_returns_stmt(s, scope, returns);
                }
            }
            _ => {}
        }
    }

    /// If `e` is a call to `return(...)` or `invisible(...)`, infer and
    /// return the type of its argument; otherwise None.
    fn try_infer_return_call(&self, e: &Expr, scope: &Scope) -> Option<RType> {
        if let Expr::Call { func, args, .. } = e {
            if let Expr::Ident { name, .. } = func.as_ref() {
                if name == "return" || name == "invisible" {
                    return Some(args.first().map(|a| self.infer_pure(&a.value, scope)).unwrap_or(RType::new(Mode::Null, Length::Zero, false)));
                }
            }
        }
        None
    }

    /// Non-mutating variant of `infer`, used during pass 2 refinement so
    /// we don't double-emit diagnostics. Diagnostics are produced in pass
    /// 3 against the fully refined FnTable.
    fn infer_pure(&self, e: &Expr, scope: &Scope) -> RType {
        match e {
            Expr::Logical(_, _) => RType::scalar(Mode::Logical, false),
            Expr::Integer(_, _) => RType::scalar(Mode::Integer, false),
            Expr::Double(_, _) => RType::scalar(Mode::Double, false),
            Expr::String(_, _) => RType::scalar(Mode::Character, false),
            Expr::Null(_) => RType::new(Mode::Null, Length::Zero, false),
            Expr::Na(t, _) => *t,
            Expr::Ident { name, .. } => scope.get(name).copied().unwrap_or(RType::UNKNOWN),
            Expr::BinOp { op, lhs, rhs, .. } => {
                let lt = self.infer_pure(lhs, scope);
                let rt = self.infer_pure(rhs, scope);
                self.infer_binop_pure(*op, lt, rt)
            }
            Expr::UnaryOp { op, expr, .. } => {
                let t = self.infer_pure(expr, scope);
                match op {
                    UnaryOpKind::Neg => t,
                    UnaryOpKind::Not => RType::new(Mode::Logical, t.length, t.na.0),
                }
            }
            Expr::Call { func, args, .. } => {
                if let Expr::Ident { name, .. } = func.as_ref() {
                    // Direct recursion: read the current best estimate
                    // from the return slot table.
                    if let Some(f) = self.fn_table.fns.get(name) {
                        return self.return_slots.get(f.return_slot);
                    }
                    if name == "c" {
                        let arg_types: Vec<RType> =
                            args.iter().map(|a| self.infer_pure(&a.value, scope)).collect();
                        return self.infer_c_pure(&arg_types);
                    }
                    if name == "list" {
                        return RType::new(Mode::List, Length::Known(args.len()), false);
                    }
                    if let Some(sig) = self.typeshed.functions.get(name) {
                        let arg_types: Vec<RType> =
                            args.iter().map(|a| self.infer_pure(&a.value, scope)).collect();
                        return self.apply_sig_pure(sig, &arg_types);
                    }
                }
                RType::UNKNOWN
            }
            Expr::Index { base, kind, .. } => {
                let bt = self.infer_pure(base, scope);
                match kind {
                    IndexKind::Single => bt,
                    IndexKind::Double | IndexKind::Dollar => {
                        RType::new(bt.mode, Length::One, bt.na.0)
                    }
                }
            }
            Expr::Function { .. } => RType::scalar(Mode::Function, false),
            Expr::Unknown(_) => RType::UNKNOWN,
        }
    }

    fn infer_binop_pure(&self, op: BinOpKind, lt: RType, rt: RType) -> RType {
        match op {
            BinOpKind::Colon => lt.seq(rt),
            BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div
            | BinOpKind::Pow | BinOpKind::Mod | BinOpKind::IDiv => {
                lt.arith(rt).unwrap_or(RType::UNKNOWN)
            }
            BinOpKind::Lt | BinOpKind::Le | BinOpKind::Gt | BinOpKind::Ge
            | BinOpKind::Eq | BinOpKind::Ne | BinOpKind::In | BinOpKind::NotIn => {
                lt.compare(rt).unwrap_or(RType::UNKNOWN)
            }
            BinOpKind::And | BinOpKind::AndAnd | BinOpKind::Or | BinOpKind::OrOr => {
                let length = if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                    Length::One
                } else {
                    lt.length.binary(rt.length)
                };
                RType::new(Mode::Logical, length, true)
            }
            BinOpKind::Assign | BinOpKind::SuperAssign | BinOpKind::PipeForward
            | BinOpKind::PipeTee | BinOpKind::PipeAssign | BinOpKind::PipeBind => RType::UNKNOWN,
        }
    }

    fn infer_c_pure(&self, arg_types: &[RType]) -> RType {
        if arg_types.is_empty() {
            return RType::new(Mode::Null, Length::Zero, false);
        }
        let mut mode = Mode::Null;
        let mut total_len: usize = 0;
        let mut any_na = false;
        for t in arg_types {
            mode = if mode.coerce_rank() >= t.mode.coerce_rank() {
                mode
            } else {
                t.mode
            };
            any_na = any_na || t.na.0;
            total_len = total_len.saturating_add(match t.length {
                Length::Zero => 0,
                Length::One => 1,
                Length::Known(n) => n,
                Length::Unknown => return RType::new(mode, Length::Unknown, any_na),
            });
        }
        RType::new(mode, Length::Known(total_len), any_na)
    }

    fn apply_sig_pure(&self, sig: &FunctionSig, arg_types: &[RType]) -> RType {
        let first = arg_types.first().copied().unwrap_or(RType::UNKNOWN);
        match &sig.return_ {
            ReturnSpec::Slot(s) => match s.as_str() {
                "arg0" => first,
                s if s.starts_with("arg") => {
                    let idx: usize = s[3..].parse().unwrap_or(0);
                    arg_types.get(idx).copied().unwrap_or(RType::UNKNOWN)
                }
                _ => RType::UNKNOWN,
            },
            ReturnSpec::Concrete(c) => {
                let mode = match c.mode.as_str() {
                    "logical" => Mode::Logical,
                    "integer" => Mode::Integer,
                    "double" => Mode::Double,
                    "character" => Mode::Character,
                    "complex" => Mode::Complex,
                    "raw" => Mode::Raw,
                    "list" => Mode::List,
                    "null" => Mode::Null,
                    "function" => Mode::Function,
                    "opaque" => Mode::Opaque,
                    "double_or_int" => {
                        if matches!(first.mode, Mode::Integer) {
                            Mode::Integer
                        } else {
                            Mode::Double
                        }
                    }
                    _ => Mode::Opaque,
                };
                let length = match c.length.as_str() {
                    "0" => Length::Zero,
                    "1" => Length::One,
                    "unknown" => Length::Unknown,
                    "arg0" => first.length,
                    _ => Length::Unknown,
                };
                RType::new(mode, length, c.na)
            }
        }
    }

    fn check_stmt(&mut self, s: &Stmt, scope: &mut Scope) {
        match s {
            Stmt::Assign { target, value, .. } => {
                let vt = self.infer(value, scope);
                self.assign_target(target, vt, scope);
            }
            Stmt::Expr(e) => {
                self.infer(e, scope);
            }
            Stmt::If { cond, then, else_, .. } => {
                let ct = self.infer(cond, scope);
                if ct.invalid_condition() {
                    self.emit(
                        Severity::Error,
                        span_of(cond),
                        "RY001",
                        format!("`if` condition is `{}`, expected length-1 logical", ct),
                    );
                } else if !matches!(ct.mode, Mode::Logical | Mode::Opaque) {
                    // R coerces silently but this is almost always a bug.
                    self.emit(
                        Severity::Warning,
                        span_of(cond),
                        "RY001",
                        format!(
                            "`if` condition is `{}` (not logical); will be silently coerced",
                            ct.mode
                        ),
                    );
                } else if matches!(ct.mode, Mode::Logical) && !matches!(ct.length, Length::One) {
                    self.emit(
                        Severity::Warning,
                        span_of(cond),
                        "RY002",
                        format!("`if` condition has length {:?}, will only use first element", ct.length),
                    );
                }
                for s in then {
                    self.check_stmt(s, scope);
                }
                if let Some(else_) = else_ {
                    for s in else_ {
                        self.check_stmt(s, scope);
                    }
                }
            }
            Stmt::For { name, iter, body, .. } => {
                let iter_t = self.infer(iter, scope);
                let mut inner = scope.clone();
                // The loop variable gets the element type of the iterator:
                // a length-1 value of the iterator's mode (or opaque if
                // we couldn't infer). This means `for (i in 1:10)` now
                // gives `i : integer<1>` instead of opaque.
                inner.insert(name.clone(), iter_t.element());
                for s in body {
                    self.check_stmt(s, &mut inner);
                }
            }
            Stmt::While { cond, body, .. } => {
                let ct = self.infer(cond, scope);
                if ct.invalid_condition() {
                    self.emit(
                        Severity::Error,
                        span_of(cond),
                        "RY001",
                        format!("loop condition is `{}`, expected length-1 logical", ct),
                    );
                }
                for s in body {
                    self.check_stmt(s, scope);
                }
            }
            Stmt::FunctionDef { name, params, body, .. } => {
                // Install the function as opaque in the surrounding scope.
                if let Some(n) = name {
                    scope.insert(n.clone(), RType::scalar(Mode::Function, false));
                }
                // Infer the body in a fresh scope populated with params.
                let mut fn_scope = scope.clone();
                for p in params {
                    let t = match &p.default {
                        Some(e) => self.infer(e, &mut fn_scope),
                        None => RType::UNKNOWN,
                    };
                    fn_scope.insert(p.name.clone(), t);
                }
                for s in body {
                    self.check_stmt(s, &mut fn_scope);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.infer(v, scope);
                }
            }
        }
    }

    fn assign_target(&mut self, target: &Expr, vt: RType, scope: &mut Scope) {
        match target {
            Expr::Ident { name, .. } => {
                scope.insert(name.clone(), vt);
            }
            _ => {
                // Indexed assignment `x[i] <- v` etc. is too dynamic for v1.
                self.infer(target, scope);
            }
        }
    }

    /// Infer the type of an expression, emitting diagnostics for misuse.
    fn infer(&mut self, e: &Expr, scope: &mut Scope) -> RType {
        match e {
            Expr::Logical(_, _) => RType::scalar(Mode::Logical, false),
            Expr::Integer(_, _) => RType::scalar(Mode::Integer, false),
            Expr::Double(_, _) => RType::scalar(Mode::Double, false),
            Expr::String(_, _) => RType::scalar(Mode::Character, false),
            Expr::Null(_) => RType::new(Mode::Null, Length::Zero, false),
            Expr::Na(t, _) => *t,
            Expr::Ident { name, span } => match scope.get(name) {
                Some(t) => *t,
                None => {
                    // Built-in dataset? (mtcars, iris, ...) Resolve before
                    // flagging the identifier as unbound.
                    if let Some(jt) = self.typeshed.datasets.get(name) {
                        return json_rtype_to_rtype(jt);
                    }
                    self.emit(
                        Severity::Warning,
                        *span,
                        "RY010",
                        format!("variable `{}` is not bound in this scope", name),
                    );
                    RType::UNKNOWN
                }
            },
            Expr::BinOp { op, lhs, rhs, span } => {
                // Pipes need structural access to `rhs` (to build a
                // desugared call), so they bypass `infer_binop`'s
                // type-only signature.
                if matches!(*op, BinOpKind::PipeForward | BinOpKind::PipeAssign) {
                    return self.infer_pipe(lhs, rhs, *span, scope);
                }
                if matches!(*op, BinOpKind::PipeTee) {
                    return self.infer_pipe_tee(lhs, rhs, scope);
                }
                let lt = self.infer(lhs, scope);
                let rt = self.infer(rhs, scope);
                self.infer_binop(*op, lt, rt, *span)
            }
            Expr::UnaryOp { op, expr, span } => {
                let t = self.infer(expr, scope);
                match op {
                    UnaryOpKind::Neg => {
                        if matches!(t.mode, Mode::Character | Mode::List | Mode::Function) {
                            self.emit(
                                Severity::Error,
                                *span,
                                "RY020",
                                format!("cannot apply unary `-` to `{}`", t.mode),
                            );
                        }
                        t
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
                        RType::new(Mode::Logical, t.length, t.na.0)
                    }
                }
            }
            Expr::Call { func, args, span } => {
                self.infer_call(func, args, scope, *span)
            }
            Expr::Index { base, kind, args, span } => {
                let bt = self.infer(base, scope);
                for a in args {
                    self.infer(&a.value, scope);
                }
                self.infer_index(bt, *kind, *span)
            }
            Expr::Function { .. } => RType::scalar(Mode::Function, false),
            Expr::Unknown(_) => RType::UNKNOWN,
        }
    }

    fn infer_binop(&mut self, op: BinOpKind, lt: RType, rt: RType, span: Span) -> RType {
        // `:` sequence operator. Always produces a vector; mode depends
        // on operand modes per R's coercion (int:int -> int, otherwise
        // double). If both operands are integer literals we can even
        // pin the length exactly.
        if matches!(op, BinOpKind::Colon) {
            let length = match (&lt.length, &rt.length) {
                (Length::One, Length::One) => {
                    // The actual length is |b - a| + 1, but without
                    // runtime values we can only say "at least 1".
                    Length::Unknown
                }
                _ => Length::Unknown,
            };
            let mode = if matches!(lt.mode, Mode::Integer | Mode::Logical)
                && matches!(rt.mode, Mode::Integer | Mode::Logical)
            {
                Mode::Integer
            } else if matches!(lt.mode, Mode::Opaque) || matches!(rt.mode, Mode::Opaque) {
                Mode::Opaque
            } else {
                Mode::Double
            };
            return RType::new(mode, length, false);
        }
        let is_compare = matches!(
            op,
            BinOpKind::Lt | BinOpKind::Le | BinOpKind::Gt | BinOpKind::Ge
                | BinOpKind::Eq | BinOpKind::Ne | BinOpKind::In | BinOpKind::NotIn
        );
        let is_logic = matches!(
            op,
            BinOpKind::And | BinOpKind::AndAnd | BinOpKind::Or | BinOpKind::OrOr
        );
        if is_compare {
            if let Some(t) = lt.compare(rt) {
                if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                    return RType::new(Mode::Logical, Length::One, t.na.0);
                }
                return t;
            }
            self.emit(
                Severity::Error,
                span,
                "RY030",
                format!("cannot compare `{}` with `{}`", lt.mode, rt.mode),
            );
            return RType::UNKNOWN;
        }
        if is_logic {
            if matches!(lt.mode, Mode::Character | Mode::List | Mode::Function)
                || matches!(rt.mode, Mode::Character | Mode::List | Mode::Function)
            {
                self.emit(
                    Severity::Error,
                    span,
                    "RY031",
                    format!(
                        "logical op applied to `{}` and `{}`",
                        lt.mode, rt.mode
                    ),
                );
                return RType::UNKNOWN;
            }
            let length = if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                Length::One
            } else {
                lt.length.binary(rt.length)
            };
            return RType::new(Mode::Logical, length, true);
        }
        // Arithmetic.
        if let Some(t) = lt.arith(rt) {
            return t;
        }
        self.emit(
            Severity::Error,
            span,
            "RY040",
            format!("cannot apply arithmetic op to `{}` and `{}`", lt.mode, rt.mode),
        );
        RType::UNKNOWN
    }

    /// Desugar `lhs %>% rhs` (and `lhs |> rhs`, `lhs %<>% rhs`) into a
    /// call to `rhs` with `lhs` injected into the argument list.
    ///
    /// Magrittr `%>%` semantics: if `rhs` is a call, prepend `lhs` as
    /// the first positional argument - unless one of the args is the
    /// bare placeholder `.` (or base-R `_`), in which case the first
    /// such occurrence is replaced with `lhs`. Bare `rhs` (e.g. `x %>% abs`)
    /// becomes a one-arg call.
    ///
    /// `%<>%` (assignment pipe) shares the result type with `%>%` at v1.
    /// The assignment side-effect (`x <- ...`) is handled by the caller
    /// when it appears in an `Assign` statement; for a bare binop we
    /// cannot reassign without a target expression, so we leave that to
    /// a future pass.
    fn infer_pipe(&mut self, lhs: &Expr, rhs: &Expr, span: Span, scope: &mut Scope) -> RType {
        // Infer the LHS so diagnostics fire on it (e.g. unbound name).
        let lhs_t = self.infer(lhs, scope);
        let result = match rhs {
            Expr::Call {
                func,
                args,
                span: call_span,
            } => {
                let mut new_args: Vec<Arg> = Vec::with_capacity(args.len() + 1);
                let mut placeholder_seen = false;
                for a in args {
                    if !placeholder_seen && is_pipe_placeholder(&a.value) {
                        new_args.push(Arg {
                            name: a.name.clone(),
                            value: lhs.clone(),
                            span: a.span,
                        });
                        placeholder_seen = true;
                    } else {
                        new_args.push(a.clone());
                    }
                }
                if !placeholder_seen {
                    new_args.insert(
                        0,
                        Arg {
                            name: None,
                            value: lhs.clone(),
                            span,
                        },
                    );
                }
                self.infer_call(func, &new_args, scope, *call_span)
            }
            Expr::Ident { .. } => {
                let new_args = vec![Arg {
                    name: None,
                    value: lhs.clone(),
                    span,
                }];
                self.infer_call(rhs, &new_args, scope, span)
            }
            _ => {
                // Unknown rhs form: infer rhs for diagnostics, give up on type.
                let _ = self.infer(rhs, scope);
                RType::UNKNOWN
            }
        };
        let _ = lhs_t;
        result
    }

    /// Tee pipe `%T>%`: run both sides for diagnostics, return the LHS type.
    /// The RHS side-effect (e.g. `print`, `plot`) is discarded at runtime;
    /// the value flows through as the LHS.
    fn infer_pipe_tee(&mut self, lhs: &Expr, rhs: &Expr, scope: &mut Scope) -> RType {
        let lhs_t = self.infer(lhs, scope);
        // Still walk the RHS so any diagnostics on its body fire.
        let _ = self.infer_pipe(lhs, rhs, span_of(rhs), scope);
        lhs_t
    }

    fn infer_call(&mut self, func: &Expr, args: &[Arg], scope: &mut Scope, span: Span) -> RType {
        // Only model direct calls `name(...)`. Pipelines and indirect calls
        // return opaque.
        let name = match func {
            Expr::Ident { name, .. } => name.clone(),
            _ => {
                self.infer(func, scope);
                for a in args {
                    self.infer(&a.value, scope);
                }
                return RType::UNKNOWN;
            }
        };

        // NSE-opaque functions whose arguments are not regular values:
        // `library(foo)` and `require(foo)` take a package name as a bare
        // symbol, not an expression. Inferring their args would trigger
        // spurious RY010 on every `library(magrittr)` etc. Return NULL
        // (these functions return invisible(NULL) at runtime).
        if name == "library" || name == "require" {
            return RType::new(Mode::Null, Length::Zero, false);
        }

        // Infer arg types.
        let mut arg_types: Vec<RType> = Vec::with_capacity(args.len());
        for a in args {
            arg_types.push(self.infer(&a.value, scope));
        }

        // Built-in: `c(...)` concatenates and produces the common mode.
        if name == "c" {
            return self.infer_c(args, &arg_types, span);
        }
        if name == "list" {
            return RType::new(Mode::List, Length::Known(args.len()), false);
        }

        // User-defined functions: read from the refined FnTable. We
        // intentionally do NOT refine on demand here - that would risk
        // exponential blowup on deep call chains. The fixpoint loop in
        // `check()` already stabilized the table.
        if let Some(f) = self.fn_table.fns.get(&name) {
            return self.return_slots.get(f.return_slot);
        }

        // Look up in the typeshed.
        if let Some(sig) = self.typeshed.functions.get(&name).cloned() {
            return self.apply_sig(&name, &sig, &arg_types, args, span);
        }

        // Unknown function: opaque.
        RType::UNKNOWN
    }

    fn infer_c(&mut self, args: &[Arg], arg_types: &[RType], _span: Span) -> RType {
        if arg_types.is_empty() {
            return RType::new(Mode::Null, Length::Zero, false);
        }
        let mut mode = Mode::Null;
        let mut total_len: usize = 0;
        let mut any_na = false;
        for t in arg_types {
            mode = if mode.coerce_rank() >= t.mode.coerce_rank() {
                mode
            } else {
                t.mode
            };
            any_na = any_na || t.na.0;
            total_len = total_len.saturating_add(match t.length {
                Length::Zero => 0,
                Length::One => 1,
                Length::Known(n) => n,
                Length::Unknown => {
                    return RType::new(mode, Length::Unknown, any_na);
                }
            });
        }
        let length = if args.iter().any(|a| matches!(a.value, Expr::Unknown(_))) {
            Length::Unknown
        } else {
            Length::Known(total_len)
        };
        RType::new(mode, length, any_na || matches!(mode, Mode::Character | Mode::Double))
    }

    fn apply_sig(
        &mut self,
        name: &str,
        sig: &FunctionSig,
        arg_types: &[RType],
        args: &[Arg],
        span: Span,
    ) -> RType {
        // For v1, a very small set of signatures is interpreted precisely.
        // Everything else just gets an opaque type.
        let first = arg_types.first().copied().unwrap_or(RType::UNKNOWN);
        match &sig.return_ {
            ReturnSpec::Slot(s) => {
                match s.as_str() {
                    "arg0" => first,
                    "concat_of_args" => self.infer_c(args, arg_types, span),
                    s if s.starts_with("arg") => {
                        let idx: usize = s[3..].parse().unwrap_or(0);
                        arg_types.get(idx).copied().unwrap_or(RType::UNKNOWN)
                    }
                    _ => RType::UNKNOWN,
                }
            }
            ReturnSpec::Concrete(c) => {
                let mode = match c.mode.as_str() {
                    "logical" => Mode::Logical,
                    "integer" => Mode::Integer,
                    "double" => Mode::Double,
                    "character" => Mode::Character,
                    "complex" => Mode::Complex,
                    "raw" => Mode::Raw,
                    "list" => Mode::List,
                    "null" => Mode::Null,
                    "function" => Mode::Function,
                    "opaque" => Mode::Opaque,
                    // Compound specs that pick by arg type. For v1 we
                    // approximate "double_or_int" as the first arg's mode if
                    // it's already integer, else double.
                    "double_or_int" => {
                        if matches!(first.mode, Mode::Integer) {
                            Mode::Integer
                        } else {
                            Mode::Double
                        }
                    }
                    _ => Mode::Opaque,
                };
                let length = match c.length.as_str() {
                    "0" => Length::Zero,
                    "1" => Length::One,
                    "unknown" => Length::Unknown,
                    "arg0" => first.length,
                    "test" => arg_types.first().copied().unwrap_or(RType::UNKNOWN).length,
                    _ => Length::Unknown,
                };
                let _ = name;
                RType::new(mode, length, c.na)
            }
        }
    }

    fn infer_index(&mut self, bt: RType, kind: IndexKind, _span: Span) -> RType {
        // Subset preserves element type. `x[[i]]` and `x$i` are scalar.
        match kind {
            IndexKind::Single => bt,
            IndexKind::Double | IndexKind::Dollar => RType::new(bt.mode, Length::One, bt.na.0),
        }
    }
}

/// Quick literal-only inference for function parameter defaults. We
/// don't have a scope yet at the point of `record_fn`, but for typed
/// defaults (`x = 1L`, `trim = 0`, `verbose = TRUE`) the literal
/// carries enough information.
fn infer_literal_default(e: &Expr) -> RType {
    match e {
        Expr::Logical(_, _) => RType::scalar(Mode::Logical, false),
        Expr::Integer(_, _) => RType::scalar(Mode::Integer, false),
        Expr::Double(_, _) => RType::scalar(Mode::Double, false),
        Expr::String(_, _) => RType::scalar(Mode::Character, false),
        Expr::Null(_) => RType::new(Mode::Null, Length::Zero, false),
        Expr::Na(t, _) => *t,
        // Anything more complex (call, ident, binop) needs a scope; defer
        // to the first fixpoint iteration by starting as UNKNOWN.
        _ => RType::UNKNOWN,
    }
}

/// True if `e` is syntactically a `return(...)` or `invisible(...)` call.
fn is_return_call(e: &Expr) -> bool {
    matches!(e, Expr::Call { func, .. }
        if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "return" || name == "invisible"))
}

fn span_of(e: &Expr) -> Span {
    match e {
        Expr::Logical(_, s) => *s,
        Expr::Integer(_, s) => *s,
        Expr::Double(_, s) => *s,
        Expr::String(_, s) => *s,
        Expr::Null(s) => *s,
        Expr::Na(_, s) => *s,
        Expr::Ident { span, .. } => *span,
        Expr::Call { span, .. } => *span,
        Expr::BinOp { span, .. } => *span,
        Expr::UnaryOp { span, .. } => *span,
        Expr::Index { span, .. } => *span,
        Expr::Function { span, .. } => *span,
        Expr::Unknown(s) => *s,
    }
}

/// True if `e` is a magrittr (`.`) or base-R (`_`) pipe placeholder.
/// These are bare identifier references used inside a piped call to
/// mark where the LHS value should be substituted.
fn is_pipe_placeholder(e: &Expr) -> bool {
    matches!(e, Expr::Ident { name, .. } if name == "." || name == "_")
}

/// Convert a typeshed `JsonRType` to the checker's `RType`. Mirrors the
/// inline conversion in `apply_sig` for `ReturnSpec::Concrete` - kept
/// here in ry-checker (not ry-typeshed) so that crate stays free of any
/// dependency on ry-core's type definitions.
fn json_rtype_to_rtype(jt: &JsonRType) -> RType {
    let mode = match jt.mode.as_str() {
        "logical" => Mode::Logical,
        "integer" => Mode::Integer,
        "double" => Mode::Double,
        "character" => Mode::Character,
        "complex" => Mode::Complex,
        "raw" => Mode::Raw,
        "list" => Mode::List,
        "null" => Mode::Null,
        "function" => Mode::Function,
        "opaque" => Mode::Opaque,
        _ => Mode::Opaque,
    };
    let length = match jt.length.as_str() {
        "0" => Length::Zero,
        "1" => Length::One,
        s if s.parse::<usize>().is_ok() => {
            Length::Known(s.parse::<usize>().unwrap_or(0))
        }
        _ => Length::Unknown,
    };
    RType::new(mode, length, jt.na)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_core::RParser;

    fn check(src: &str) -> Vec<Diagnostic> {
        let mut p = RParser::new().unwrap();
        let f = p.parse("test.R", src).unwrap();
        let mut c = Checker::new("test.R");
        c.check(&f);
        c.take_diagnostics()
    }

    #[test]
    fn detects_char_plus_int() {
        let diags = check(r#""a" + 1L"#);
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040, got {:?}", diags
        );
    }

    #[test]
    fn allows_int_plus_double() {
        let diags = check("1L + 2.0\n");
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn detects_if_on_character() {
        let diags = check(r#"if ("x") print(1)"#);
        assert!(diags.iter().any(|d| d.code == "RY001"));
    }

    #[test]
    fn detects_long_condition_warning() {
        let diags = check("if (c(TRUE, FALSE)) print(1)\n");
        assert!(diags.iter().any(|d| d.code == "RY002"));
    }

    #[test]
    fn detects_unbound_var() {
        let diags = check("y <- undefined_thing\n");
        assert!(diags.iter().any(|d| d.code == "RY010"));
    }

    #[test]
    fn function_param_inference_no_diag() {
        // `f` has a default-typed param `x = 1L` (integer), so `x + 1`
        // is integer + double = double. Well-typed; no diagnostics.
        let diags = check("f <- function(x = 1L) { x + 1 }\ng <- f(2L)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "got false positive: {:?}",
            diags
        );
    }

    #[test]
    fn user_fn_return_type_inferred() {
        // `text` returns a string literal, so `text()` is character and
        // the arithmetic use must error.
        let diags = check("text <- function() { \"hello\" }\ny <- text() + 1L\n");
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from character-returning fn used arithmetically, got {:?}",
            diags
        );
    }

    #[test]
    fn user_fn_return_explicit_return() {
        let diags = check("f <- function(x = 1L) { return(x * 2) }\ny <- f() + \"bad\"\n");
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from integer-returning fn + character, got {:?}",
            diags
        );
    }

    #[test]
    fn recursive_fn_terminates() {
        // The fixpoint must converge on fact()'s return type (integer)
        // without infinite descent. We don't assert any specific diag,
        // just that the checker terminates and doesn't crash.
        let diags = check(
            "fact <- function(n = 1L) { if (n <= 1L) return(1L); n * fact(n - 1L) }\ny <- fact(5)\n",
        );
        // The result is integer; arithmetic with another integer is fine.
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "false positive on recursive fn: {:?}",
            diags
        );
    }

    #[test]
    fn seq_operator_produces_integer() {
        // `1:10` is integer, so `i` in the loop is integer, so `i + 1L`
        // is well-typed.
        let diags = check("total <- 0L\nfor (i in 1:10) { total <- total + i }\n");
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn for_loop_var_is_element_type() {
        // Iterating over a character vector makes the loop variable
        // character; using it arithmetically should error.
        let diags = check("for (s in c(\"a\", \"b\")) { total <- s + 1 }\n");
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from character loop var + int, got {:?}",
            diags
        );
    }

    #[test]
    fn pipe_desugars_to_call() {
        // `c(1,2,3) %>% mean()` desugars to `mean(c(1,2,3))`, which is
        // well-typed: no diagnostics.
        let diags = check("result <- c(1, 2, 3) %>% mean()\n");
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn pipe_chain_infers() {
        // A two-step pipe composes: `mean() -> double_or_int<1>`, then
        // `round(<double>, digits = 2)` resolves against the typeshed.
        let diags = check("a <- c(1, 2, 3) %>% mean() %>% round(2)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn pipe_base_r_infers() {
        // Base-R `|>` desugars identically to magrittr `%>%`.
        let diags = check("a <- c(1, 2, 3) |> mean()\n");
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn pipe_bare_function() {
        // Bare `rhs` becomes a one-arg call: `x %>% abs` -> `abs(x)`.
        let diags = check("x <- 1L\ny <- x %>% abs\n");
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn pipe_placeholder_substitutes() {
        // The first `.` is replaced with the LHS; `round(., digits = 2)`
        // becomes `round(c(1,2,3), digits = 2)`.
        let diags = check("result <- c(1, 2, 3) %>% round(., digits = 2)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn pipe_tee_returns_lhs_type() {
        // `%T>%` returns the LHS; the RHS is walked for diagnostics only.
        // `c(1,2,3) %T>% print()` should be a length-3 double vector.
        let diags = check("result <- c(1, 2, 3) %T>% print()\n");
        assert!(diags.is_empty(), "got {:?}", diags);
    }

    #[test]
    fn dataset_resolves_mtcars() {
        // `mtcars` is in the typeshed's datasets table; using it must
        // not emit RY010 (unbound variable).
        let diags = check("df <- mtcars\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "expected no RY010 for mtcars, got {:?}",
            diags
        );
    }

    #[test]
    fn dataset_resolves_iris() {
        let diags = check("df <- iris\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "expected no RY010 for iris, got {:?}",
            diags
        );
    }
}
