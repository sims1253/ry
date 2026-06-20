//! Local type inference + diagnostics.
//!
//! v1 scope: single-file, inference-only, NSE-opaque. We walk statements
//! top-down, maintaining a per-scope binding table `name -> RType`.
//! Function definitions install a fresh scope and infer the body once
//! (no recursion, no interprocedural flow).

pub mod rules;
pub mod format;

use ry_core::ast::*;
use ry_core::types::{Length, Mode, RType};
use ry_core::Span;
use ry_typeshed::{load_base, FunctionSig, ReturnSpec, Typeshed};
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

pub struct Checker {
    typeshed: Typeshed,
    diagnostics: Vec<Diagnostic>,
    path: String,
}

impl Checker {
    pub fn new(path: &str) -> Self {
        let typeshed = load_base().expect("typeshed must load");
        Self {
            typeshed,
            diagnostics: Vec::new(),
            path: path.to_string(),
        }
    }

    pub fn check(&mut self, file: &SourceFile) -> &[Diagnostic] {
        self.path = file.path.clone();
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
                let _ = self.infer(iter, scope);
                let mut inner = scope.clone();
                // The loop variable gets the element type of the iterator.
                // For v1, conservatively mark as opaque unknown.
                inner.insert(name.clone(), RType::UNKNOWN);
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
        let diags = check("f <- function(x = 1L) { x + 1 }\ng <- f(2L)\n");
        // We don't yet model the return type of user functions, so `g` is
        // opaque. We just want no false positives here.
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "got false positive: {:?}",
            diags
        );
    }
}
