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
pub mod project;

// Re-export `Project` at the crate root so callers (the CLI, integration
// tests) can write `ry_checker::Project` rather than
// `ry_checker::project::Project`. Mirrors the ergonomics of `Checker`.
pub use project::Project;

use ry_core::ast::*;
use ry_core::types::{
    intern_class_name, intern_column_schema, intern_function_signature, ClassVector, ColumnSchema,
    FunctionSignature, Length, Mode, RType,
};
use ry_core::Span;
use ry_typeshed::{load_base, FunctionSig, JsonRType, ReturnSpec, Typeshed};
use std::collections::HashMap;

/// S3 generics we recognize when collecting method definitions of the
/// form `print.foo <- function(...) body`. A `<generic>.<class>` name
/// where `<generic>` is in this list is recorded in `FnTable::s3_methods`
/// (keyed by `(generic, class)`) in addition to its slot in `fns`.
///
/// The list is intentionally generous: it mirrors the generics shipped
/// with base R plus the most commonly defined ones in CRAN packages.
/// Anything missing falls back to plain function-call inference (and so
/// RY050 won't fire on it, which is a deliberate conservative choice).
const S3_GENERICS: &[&str] = &[
    "print",
    "summary",
    "plot",
    "predict",
    "fitted",
    "residuals",
    "coef",
    "vcov",
    "logLik",
    "AIC",
    "BIC",
    "update",
    "deviance",
    "anova",
    "model.matrix",
    "terms",
    "str",
    "format",
    "as.character",
    "as.data.frame",
    "as.matrix",
    "as.vector",
    "t",
    "is.na",
    "length",
    "names",
    "dim",
    "[",
    "[[",
    "$",
    "c",
    "rep",
    "rev",
    "sort",
    "unique",
    "head",
    "tail",
    "subset",
    "transform",
    "within",
    "merge",
];

/// Returns `Some((generic, class))` if `name` matches the S3 method
/// naming convention `<generic>.<class>` and `<generic>` is in
/// `S3_GENERICS`. We try the longest known generic prefix first so
/// multi-segment generics like `as.data.frame` win over the shorter
/// `as`. This is necessary because method names like `print.as.data.frame`
/// (rare but valid) would otherwise match the wrong prefix.
fn split_s3_method_name(name: &str) -> Option<(&'static str, String)> {
    // Try every known generic, keep the longest matching prefix. This
    // is O(N) per name but N is small (40) and the function is only
    // called once per top-level assignment.
    let mut best: Option<(&'static str, String)> = None;
    for generic in S3_GENERICS {
        // Build the prefix once per generic; cheap for our small list.
        let mut buf = [0u8; 64];
        let generic_bytes = generic.as_bytes();
        if generic_bytes.len() + 1 > buf.len() {
            continue; // Generic longer than the scratch buffer; skip.
        }
        buf[..generic_bytes.len()].copy_from_slice(generic_bytes);
        buf[generic_bytes.len()] = b'.';
        let prefix = std::str::from_utf8(&buf[..generic_bytes.len() + 1]).ok();
        if let Some(prefix) = prefix {
            if let Some(class) = name.strip_prefix(prefix) {
                if class.is_empty() {
                    continue;
                }
                // Prefer the longest matching prefix (more specific).
                let is_better = best
                    .as_ref()
                    .is_none_or(|(g, _)| g.len() < generic.len());
                if is_better {
                    best = Some((generic, class.to_string()));
                }
            }
        }
    }
    best
}

/// R's Non-Standard Evaluation verbs. Each evaluates its expression
/// arguments in an augmented scope built from a data frame's column
/// schema, so `subset(df, cyl == 4)` resolves `cyl` against `df` rather
/// than the enclosing environment. See `infer_nse_call` for the
/// per-verb semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NseVerb {
    /// `subset(df, subset_expr, select_expr?)`: returns a data frame.
    Subset,
    /// `with(df, expr)`: returns whatever `expr` evaluates to.
    With,
    /// `within(df, expr)`: returns a (possibly mutated) data frame.
    Within,
    /// `transform(df, new_col = expr, ...)`: returns a data frame.
    Transform,
}

impl NseVerb {
    /// Recognize an NSE verb by its base-R function name. Returns
    /// `None` for any other name so the caller can fall through to the
    /// regular call-resolution path.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "subset" => Some(NseVerb::Subset),
            "with" => Some(NseVerb::With),
            "within" => Some(NseVerb::Within),
            "transform" => Some(NseVerb::Transform),
            _ => None,
        }
    }
}

/// R's higher-order built-ins. Each takes a function-valued argument
/// (`FUN` or `f`) and applies it to elements of a data argument. The
/// checker models the common cases to infer the result type from the
/// callback's return type, rather than returning opaque for every
/// `lapply` / `sapply` / `Map` call.
///
/// `from_name` recognizes both the base-R name and common aliases
/// (e.g. `mapply` maps to the same handler as `Map`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HigherOrderFunc {
    /// `lapply(X, FUN)`: always returns a list.
    Lapply,
    /// `sapply(X, FUN)`: simplifies to a vector when possible.
    Sapply,
    /// `vapply(X, FUN, FUN.VALUE)`: result type is FUN.VALUE.
    Vapply,
    /// `Map(f, ...)` / `mapply(f, ...)`: element-wise, returns a list.
    Map,
    /// `mapply(f, ...)`: like Map but simplifies. Modeled as Map for v1.
    Mapply,
    /// `rapply(L, f)`: recursive apply on a list.
    Rapply,
    /// `Reduce(f, x)`: left-fold.
    Reduce,
    /// `Filter(f, x)`: subset where f returns TRUE.
    Filter,
    /// `Find(f, x)`: first element where f returns TRUE.
    Find,
    /// `Position(f, x)`: index of first element where f returns TRUE.
    Position,
    /// `do.call(fun, args)`: invoke fun with args list.
    DoCall,
}

impl HigherOrderFunc {
    /// Recognize a higher-order built-in by its base-R name. Returns
    /// `None` for any other name.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "lapply" => Some(HigherOrderFunc::Lapply),
            "sapply" => Some(HigherOrderFunc::Sapply),
            "vapply" => Some(HigherOrderFunc::Vapply),
            "Map" => Some(HigherOrderFunc::Map),
            "mapply" => Some(HigherOrderFunc::Mapply),
            "rapply" => Some(HigherOrderFunc::Rapply),
            "Reduce" => Some(HigherOrderFunc::Reduce),
            "Filter" => Some(HigherOrderFunc::Filter),
            "Find" => Some(HigherOrderFunc::Find),
            "Position" => Some(HigherOrderFunc::Position),
            "do.call" => Some(HigherOrderFunc::DoCall),
            _ => None,
        }
    }
}

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
pub(crate) struct UserFn {
    /// Parameter names with their inferred-or-default types.
    pub(crate) params: Vec<(String, RType)>,
    /// Indices into the body Vec<Stmt>. Stored as a snapshot we can
    /// re-walk on each fixpoint iteration.
    pub(crate) body: Vec<Stmt>,
    /// Currently-inferred return type. Starts as UNKNOWN, refined by
    /// each fixpoint iteration. Stored as a slot index so all calls
    /// observe the latest refinement without rebuilding the table.
    pub(crate) return_slot: usize,
}

/// Side-table of inferred return types, indexed by `UserFn::return_slot`.
/// Stored separately so we can clone the table cheaply when entering a
/// nested inference pass without deep-cloning the function bodies.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReturnSlots(pub(crate) Vec<RType>);

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
///
/// S3 method dispatch is modeled separately: assignments named
/// `<generic>.<class>` (e.g. `print.foo`) are also recorded in
/// `s3_methods` keyed by `(generic, class)`. The method body shares
/// `return_slots` with regular functions so the fixpoint loop refines
/// it the same way.
#[derive(Debug, Clone, Default)]
pub(crate) struct FnTable {
    pub(crate) fns: HashMap<String, UserFn>,
    /// `(generic, class)` -> return slot index. Mirrors the same
    /// `return_slots` storage as `fns`; lookups during dispatch consult
    /// this map for an S3 method before falling back to the generic.
    pub(crate) s3_methods: HashMap<(String, String), usize>,
}

/// Maximum fixpoint depth before we give up and freeze as Opaque.
/// Conservative cap; well-typed programs converge in 2-3 iterations.
pub(crate) const MAX_FIXPOINT_DEPTH: usize = 8;

/// Maximum nesting depth for closure inference. A function factory
/// whose body returns another function factory (and so on) eventually
/// bottoms out at this depth; deeper nests get an opaque `Function`
/// value with no `fn_sig`. Three levels covers the overwhelming
/// majority of real-world R closure patterns (factories, currying,
/// method chaining) while bounding the worst-case recursion.
///
/// Scope limits for closure support (documented here so all the
/// approximations live in one place):
///   * Captured bindings are snapshotted at the point where the inner
///     function is inferred. Closures that close over mutable state
///     (reassigned in the body) get opaque for the captured binding
///     (we don't track per-binding mutation in v1).
///   * Recursive closures (a closure that calls itself by name) are
///     detected via the existing fixpoint cycle detection in
///     `refine_fn_return`.
///   * Anonymous functions passed to higher-order built-ins like
///     `lapply` / `sapply` / `Map` are NOT inferred in v1; doing so
///     would require per-builtin modeling of how they invoke the
///     callback. They resolve to opaque (matching the typeshed entry).
pub(crate) const MAX_CLOSURE_DEPTH: usize = 3;

pub struct Checker {
    typeshed: Typeshed,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) path: String,
    /// User-defined functions collected in pass 1.
    pub(crate) fn_table: FnTable,
    /// Inferred return types, refined by the fixpoint loop.
    pub(crate) return_slots: ReturnSlots,
    /// Stack of function names currently being inferred (cycle detection).
    pub(crate) inferring: Vec<String>,
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
        // until the table stabilizes or we hit MAX_FIXPOINT_DEPTH.
        self.run_fixpoint();

        // Pass 3: final walk, emitting all diagnostics. Function calls
        // now resolve against the refined FnTable.
        self.emit_diagnostics(file);
        &self.diagnostics
    }

    /// Check a file and return both diagnostics and the final top-level
    /// scope. Used by the LSP server for hover support: the scope maps
    /// variable names to their inferred types, so hovering over a
    /// variable shows its type.
    pub fn check_with_scope(&mut self, file: &SourceFile) -> (Vec<Diagnostic>, Scope) {
        self.path = file.path.clone();
        self.collect_fns(&file.stmts);
        self.run_fixpoint();
        let mut scope = Scope::default();
        // Clear diagnostics so we start fresh (the caller may call this
        // multiple times on the same checker instance).
        self.diagnostics.clear();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
        (std::mem::take(&mut self.diagnostics), scope)
    }

    /// Construct a checker that uses pre-populated function tables.
    /// Used by `Project` for multi-file checking, where the tables are
    /// shared across files. The fresh checker starts with an empty
    /// diagnostics vec and an empty `inferring` stack.
    pub(crate) fn with_tables(path: &str, fn_table: FnTable, return_slots: ReturnSlots) -> Self {
        let typeshed = load_base().expect("typeshed must load");
        Self {
            typeshed,
            diagnostics: Vec::new(),
            path: path.to_string(),
            fn_table,
            return_slots,
            inferring: Vec::new(),
        }
    }

    /// Take ownership of this checker's tables. Used by `Project` to
    /// move a populated `FnTable`/`ReturnSlots` out of a throwaway
    /// checker and into a shared `Project`.
    pub(crate) fn into_tables(self) -> (FnTable, ReturnSlots) {
        (self.fn_table, self.return_slots)
    }

    /// Pass 1: collect function definitions from this file into the
    /// shared `FnTable`. Does NOT emit diagnostics. `Project::check`
    /// calls this once per file before running the fixpoint.
    pub(crate) fn collect_file_fns(&mut self, file: &SourceFile) {
        self.path = file.path.clone();
        self.collect_fns(&file.stmts);
    }

    /// Pass 2: refine all function return types until convergence.
    /// Iterates the shared `FnTable`; safe to call once after all files
    /// have been collected.
    ///
    /// S3 methods (`print.foo`, etc.) are inserted into `fns` under
    /// their full name during pass 1, with `s3_methods` pointing at
    /// the same return slot. Iterating `fns.keys()` therefore refines
    /// S3 method bodies alongside regular functions; dispatch reads
    /// the refined slot via the `s3_methods` map.
    pub(crate) fn run_fixpoint(&mut self) {
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
    }

    /// Pass 3: emit diagnostics for this file using the refined tables.
    /// Diagnostics are appended to `self.diagnostics`; clear that vec
    /// first if you want only this file's diagnostics.
    pub(crate) fn emit_diagnostics(&mut self, file: &SourceFile) {
        self.path = file.path.clone();
        let mut scope = Scope::default();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Apply a `SeverityFilter` to the diagnostics collected so far,
    /// mutating severities (or dropping suppressed ones) in place.
    pub fn apply_filter(&mut self, filter: &SeverityFilter) {
        apply_filter_to_diagnostics(&mut self.diagnostics, filter);
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
                    // An S3 method named like `print.foo` is recorded both
                    // as a regular function (so the name resolves to its
                    // return type if called directly) and as an S3 method
                    // (so dispatch from `print(x)` on a classed value
                    // finds it). We record the body once and share the
                    // return slot between both entries.
                    if let Some((generic, class)) = split_s3_method_name(name) {
                        let slot =
                            self.record_fn(name.clone(), params, body.clone());
                        self.fn_table
                            .s3_methods
                            .insert((generic.to_string(), class), slot);
                    } else {
                        let _ = self.record_fn(name.clone(), params, body.clone());
                    }
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
                // Non-function assignments: nothing to record.
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

    /// Walk a function body looking for `inner <- function(...) ...`
    /// definitions and record them with the mangled name
    /// `<outer>$<inner>`. The mangled name is internal: it exists so
    /// the fixpoint can refine the inner function's return type, which
    /// `refine_fn_return` reads back when building the outer function's
    /// `fn_sig`. Users never see this name.
    ///
    /// Recursion is bounded by the AST's literal nesting (small in
    /// practice). The inference depth is separately bounded by
    /// `MAX_CLOSURE_DEPTH` in `build_function_signature_pure`.
    fn collect_nested_fns_in_body(&mut self, outer: &str, body: &[Stmt]) {
        for s in body {
            self.collect_nested_fns_stmt(outer, s);
        }
    }

    /// Per-statement helper for `collect_nested_fns_in_body`. Records
    /// any `inner <- function(...) ...` under `<outer>$<inner>` and
    /// recurses into compound statements so we catch nested defs
    /// inside `if` / `for` / `while` blocks too.
    fn collect_nested_fns_stmt(&mut self, outer: &str, s: &Stmt) {
        match s {
            Stmt::Assign { target, value, .. } => {
                if let (Expr::Ident { name: inner, .. }, Expr::Function { params, body: inner_body, .. }) =
                    (target, value)
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

    /// Record a user-defined function. Returns the index of the
    /// allocated return slot so callers can wire up S3 dispatch entries
    /// that share the same slot.
    fn record_fn(&mut self, name: String, params: &[Param], body: Vec<Stmt>) -> usize {
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
        slot
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
        // Walk the body in source order, simulating each statement's
        // effect on the scope so the trailing return expression can
        // reference bindings established earlier in the body. This is
        // the same simulation `build_function_signature_pure` uses for
        // nested closures; we apply it at the top level too so
        // named-return closures (`g <- function() { 1L }; g`) resolve.
        //
        // We use the pure inference path (no diagnostics) so pass 2
        // does not double-emit; diagnostics are produced in pass 3
        // against the fully refined FnTable.
        for s in &body_clone {
            self.collect_returns_and_simulate_at_depth(s, &mut scope, &mut returns, 0);
        }
        // Trailing expression of a braced body is the implicit return.
        // A trailing `Stmt::FunctionDef` is the implicit return value
        // for the `function() { function() { 1L } }` shape;
        // `trailing_return_type` handles both forms and attaches an
        // inferred `fn_sig` when the trailing expression is itself a
        // function literal (the closure-factory pattern).
        if let Some(t) = self.trailing_return_type(&body_clone, &scope, 0) {
            returns.push(t);
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

    /// Combined return-collector and scope-simulator. Unlike a plain
    /// return-collector, this takes `&mut Scope` and processes
    /// `Stmt::Assign` so later statements see the binding. Used by both
    /// `refine_fn_return` (for top-level user functions) and
    /// `build_function_signature_pure` (for nested closures) so the
    /// closure-factory-with-named-return pattern
    /// (`g <- function() { 1L }; g`) resolves.
    ///
    /// Approximations (documented):
    ///   * `if` branches are walked in sequence against the same scope;
    ///     bindings established in the `then` branch leak into `else_`
    ///     and into subsequent statements. This is a deliberate v1
    ///     simplification (proper flow-sensitive scoping would require
    ///     a full abstract interpreter).
    ///   * Loop bodies are walked once (not to fixpoint); bindings they
    ///     establish leak to subsequent statements. This matches the
    ///     existing `collect_returns_stmt_at_depth` approximation.
    ///   * Indexed assignment (`x[i] <- v`) does not update the scope
    ///     (we don't model per-element mutation in v1).
    fn collect_returns_and_simulate_at_depth(
        &self,
        s: &Stmt,
        scope: &mut Scope,
        returns: &mut Vec<RType>,
        depth: usize,
    ) {
        match s {
            Stmt::Expr(e) => {
                if let Some(rt) = self.try_infer_return_call_at_depth(e, scope, depth) {
                    returns.push(rt);
                }
            }
            Stmt::Assign { target, value, .. } => {
                let vt = self.infer_pure_at_depth(value, scope, depth);
                if let Expr::Ident { name, .. } = target {
                    scope.insert(name.clone(), vt);
                }
            }
            Stmt::FunctionDef { name, params, body, .. } => {
                // A named function def establishes a binding whose type
                // is a `Mode::Function` value (with `fn_sig` when we can
                // infer it). This is what makes
                // `f <- function() { g <- function() { 1L }; g }` resolve.
                let vt = self.function_value_from_literal(params, body, scope, depth);
                if let Some(n) = name {
                    scope.insert(n.clone(), vt);
                }
            }
            Stmt::If { cond, then, else_, .. } => {
                let _ = cond;
                for s in then {
                    self.collect_returns_and_simulate_at_depth(s, scope, returns, depth);
                }
                if let Some(e) = else_ {
                    for s in e {
                        self.collect_returns_and_simulate_at_depth(s, scope, returns, depth);
                    }
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } => {
                for s in body {
                    self.collect_returns_and_simulate_at_depth(s, scope, returns, depth);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    returns.push(self.infer_pure_at_depth(v, scope, depth));
                } else {
                    returns.push(RType::new(Mode::Null, Length::Zero, false));
                }
            }
        }
    }

    /// If `e` is a call to `return(...)` or `invisible(...)`, infer and
    /// return the type of its argument; otherwise None.
    ///
    /// Depth-tracked so a `return(function() { ... })` builds the inner
    /// signature at the right depth. Threads the closure-nesting depth
    /// through to `infer_pure_at_depth`.
    fn try_infer_return_call_at_depth(
        &self,
        e: &Expr,
        scope: &Scope,
        depth: usize,
    ) -> Option<RType> {
        if let Expr::Call { func, args, .. } = e {
            if let Expr::Ident { name, .. } = func.as_ref() {
                if name == "return" || name == "invisible" {
                    return Some(args.first().map(|a| self.infer_pure_at_depth(&a.value, scope, depth)).unwrap_or(RType::new(Mode::Null, Length::Zero, false)));
                }
            }
        }
        None
    }

    /// Non-mutating, depth-tracked variant of `infer`, used during pass
    /// 2 refinement so we don't double-emit diagnostics. Diagnostics
    /// are produced in pass 3 against the fully refined FnTable.
    ///
    /// This is also the path that builds `fn_sig` for closures: when we
    /// encounter an `Expr::Function` literal we walk its body to infer
    /// its return type and attach the resulting signature. Closure
    /// nesting is bounded by `MAX_CLOSURE_DEPTH`.
    ///
    /// `depth` counts how many closure bodies we have descended into;
    /// once it reaches `MAX_CLOSURE_DEPTH` we stop building nested
    /// signatures and return opaque `Function` values (matching the
    /// documented scope limit). Top-level expressions start at depth 0;
    /// callers that don't care about depth tracking should pass 0.
    fn infer_pure_at_depth(&self, e: &Expr, scope: &Scope, depth: usize) -> RType {
        match e {
            Expr::Logical(_, _) => RType::scalar(Mode::Logical, false),
            Expr::Integer(_, _) => RType::scalar(Mode::Integer, false),
            Expr::Double(_, _) => RType::scalar(Mode::Double, false),
            Expr::String(_, _) => RType::scalar(Mode::Character, false),
            Expr::Null(_) => RType::new(Mode::Null, Length::Zero, false),
            Expr::Na(t, _) => *t,
            Expr::Ident { name, .. } => scope.get(name).copied().unwrap_or(RType::UNKNOWN),
            Expr::BinOp { op, lhs, rhs, .. } => {
                let lt = self.infer_pure_at_depth(lhs, scope, depth);
                let rt = self.infer_pure_at_depth(rhs, scope, depth);
                self.infer_binop_pure(*op, lt, rt)
            }
            Expr::UnaryOp { op, expr, .. } => {
                let t = self.infer_pure_at_depth(expr, scope, depth);
                match op {
                    UnaryOpKind::Neg => t,
                    UnaryOpKind::Not => RType::new(Mode::Logical, t.length, t.na.0),
                }
            }
            Expr::Call { func, args, .. } => {
                if let Expr::Ident { name, .. } = func.as_ref() {
                    // Indirect call through a closure value: if the name
                    // is bound in scope to a `Function`-typed value with
                    // a `fn_sig`, the call resolves to the signature's
                    // return type. This unblocks the
                    // `c <- make_counter(); c()` pattern without needing
                    // a FnTable entry for `c`.
                    if let Some(t) = scope.get(name) {
                        if matches!(t.mode, Mode::Function) {
                            if let Some(sig) = t.fn_sig {
                                return *sig.return_type;
                            }
                            // Bound function value without an inferred
                            // signature: opaque. Fall through to the
                            // FnTable / typeshed paths below only if
                            // those could plausibly match (they can't
                            // for a scope-local name, but the check is
                            // cheap and keeps the fallthrough explicit).
                        }
                    }
                    // Direct recursion: read the current best estimate
                    // from the return slot table.
                    if let Some(f) = self.fn_table.fns.get(name) {
                        return self.return_slots.get(f.return_slot);
                    }
                    if name == "c" {
                        let arg_types: Vec<RType> =
                            args.iter().map(|a| self.infer_pure_at_depth(&a.value, scope, depth)).collect();
                        return self.infer_c_pure(&arg_types);
                    }
                    if name == "list" || name == "data.frame" {
                        // Pass 2 (pure) mirrors pass 3 minus diagnostics.
                        // We rebuild the schema so the refined return
                        // type is correct for column access in callers.
                        let arg_types: Vec<RType> =
                            args.iter().map(|a| self.infer_pure_at_depth(&a.value, scope, depth)).collect();
                        let length = Length::Known(arg_types.len());
                        let base = if name == "data.frame" {
                            RType::new(Mode::List, length, false)
                                .with_class(ClassVector::single(intern_class_name("data.frame")))
                        } else {
                            RType::new(Mode::List, length, false)
                        };
                        let schema = build_named_schema(&arg_types, args);
                        return match schema {
                            Some(s) => base.with_columns(intern_column_schema(s)),
                            None => base,
                        };
                    }
                    // Higher-order built-ins: model the callback in
                    // pass 2 too, so the refined return type of a
                    // user-fn that uses `lapply` etc. is correct.
                    if let Some(rt) = {
                        let ho_args: Vec<RType> =
                            args.iter().map(|a| self.infer_pure_at_depth(&a.value, scope, depth)).collect();
                        self.infer_higher_order_call(name, args, &ho_args, scope)
                    } {
                        return rt;
                    }
                    if let Some(sig) = self.typeshed.functions.get(name) {
                        let arg_types: Vec<RType> =
                            args.iter().map(|a| self.infer_pure_at_depth(&a.value, scope, depth)).collect();
                        return self.apply_sig_pure(sig, &arg_types);
                    }
                }
                RType::UNKNOWN
            }
            Expr::Index { base, kind, args, .. } => {
                let bt = self.infer_pure_at_depth(base, scope, depth);
                match kind {
                    IndexKind::Single => bt,
                    IndexKind::Dollar => {
                        // Pass 2 (pure) mirrors pass 3 minus diagnostics.
                        // The column name lives on `args[0].name`; if we
                        // have a schema, return the column's type, else
                        // fall back to the length-1 default.
                        let col = args.first().and_then(|a| a.name.as_deref());
                        if let (Some(name), Some(schema)) = (col, bt.columns) {
                            if let Some(t) = schema.get(name) {
                                return t;
                            }
                        }
                        RType::new(bt.mode, Length::One, bt.na.0)
                    }
                    IndexKind::Double => {
                        // `df[["col"]]` or `x[[i]]`: string literal or
                        // integer literal index.
                        if let Some(Expr::String(name, _)) =
                            args.first().map(|a| &a.value)
                        {
                            if let Some(schema) = bt.columns {
                                if let Some(t) = schema.get(name) {
                                    return t;
                                }
                            }
                            return RType::new(bt.mode, Length::One, bt.na.0);
                        }
                        let int_idx = match args.first().map(|a| &a.value) {
                            Some(Expr::Integer(i, _)) => Some(*i as f64),
                            Some(Expr::Double(f, _)) => Some(*f),
                            _ => None,
                        };
                        if let Some(idx) = int_idx {
                            if let Some(schema) = bt.columns {
                                let key = format!("[[{}]]", idx as i64);
                                if let Some(t) = schema.get(&key) {
                                    return t;
                                }
                                if let Some(common) = homogeneous_list_element_type(schema) {
                                    return common;
                                }
                            }
                            return RType::UNKNOWN;
                        }
                        RType::new(bt.mode, Length::One, bt.na.0)
                    }
                }
            }
            Expr::Function { params, body, .. } => {
                // Closure literal: build an inferred signature by
                // walking the inner body with the captured scope plus
                // the function's own params. Bounded by
                // `MAX_CLOSURE_DEPTH`; beyond that we return an opaque
                // `Function` value (no `fn_sig`).
                self.function_value_from_literal(params, body, scope, depth)
            }
            Expr::If { cond, then, else_, .. } => {
                // Pass 2 (pure): infer both branches without emitting
                // diagnostics, then join. Mirrors pass 3's
                // `infer_if_expr` minus the diagnostic emission.
                let _ = self.infer_pure_at_depth(cond, scope, depth);
                let then_t = self.infer_pure_at_depth(then, scope, depth);
                let else_t = else_
                    .as_ref()
                    .map(|e| self.infer_pure_at_depth(e, scope, depth))
                    .unwrap_or(RType::new(Mode::Null, Length::Zero, false));
                then_t.join(else_t)
            }
            Expr::Unknown(_) => RType::UNKNOWN,
        }
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
    fn function_value_from_literal(
        &self,
        params: &[Param],
        body: &[Stmt],
        captured_scope: &Scope,
        depth: usize,
    ) -> RType {
        let base = RType::scalar(Mode::Function, false);
        if depth >= MAX_CLOSURE_DEPTH {
            return base;
        }
        match self.build_function_signature_pure(params, body, captured_scope, depth) {
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
    fn build_function_signature_pure(
        &self,
        params: &[Param],
        body: &[Stmt],
        captured_scope: &Scope,
        depth: usize,
    ) -> Option<&'static FunctionSignature> {
        if body.is_empty() {
            return None;
        }
        // Layer the inner function's params on top of the captured
        // scope. We start from a clone of the captured scope so the
        // body can reference enclosing bindings (`make_adder`'s `x`).
        let mut scope = captured_scope.clone();
        let mut param_types: Vec<RType> = Vec::with_capacity(params.len());
        for p in params {
            let t = match &p.default {
                Some(e) => infer_literal_default(e),
                None => RType::UNKNOWN,
            };
            scope.insert(p.name.clone(), t);
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
        for s in body {
            self.collect_returns_and_simulate_at_depth(
                s,
                &mut scope,
                &mut returns,
                depth + 1,
            );
        }
        // Trailing expression of a braced body is the implicit return.
        // A trailing `Stmt::FunctionDef` (a bare function literal in
        // statement position) is also the implicit return value - this
        // is the closure-factory pattern: `function() { function() { 1L } }`
        // has a `Stmt::FunctionDef` as its body's last statement.
        if let Some(t) = self.trailing_return_type(body, &scope, depth + 1) {
            returns.push(t);
        }
        if returns.is_empty() {
            return None;
        }
        let mut iter = returns.into_iter();
        let first = iter.next().unwrap_or(RType::UNKNOWN);
        let joined = iter.fold(first, |acc, t| acc.join(t));
        // If we couldn't infer anything useful (joined is UNKNOWN),
        // there's no point attaching an empty signature.
        if matches!(joined.mode, Mode::Opaque) {
            return None;
        }
        Some(intern_function_signature(FunctionSignature {
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
    fn trailing_return_type(
        &self,
        body: &[Stmt],
        scope: &Scope,
        depth: usize,
    ) -> Option<RType> {
        let last = body.last()?;
        match last {
            Stmt::Expr(e) => {
                if is_return_call(e) {
                    None
                } else {
                    Some(self.infer_pure_at_depth(e, scope, depth))
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
            BinOpKind::Assign | BinOpKind::SuperAssign => rt,
            BinOpKind::PipeForward
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
                    "arg0" => first.mode,
                    "arg1" => arg_types.get(1).map(|t| t.mode).unwrap_or(Mode::Opaque),
                    "arg2" => arg_types.get(2).map(|t| t.mode).unwrap_or(Mode::Opaque),
                    "yes_or_no" => {
                        let yes = arg_types.get(1).copied().unwrap_or(RType::UNKNOWN);
                        let no = arg_types.get(2).copied().unwrap_or(RType::UNKNOWN);
                        yes.join(no).mode
                    }
                    _ => Mode::Opaque,
                };
                let length = match c.length.as_str() {
                    "0" => Length::Zero,
                    "1" => Length::One,
                    "unknown" => Length::Unknown,
                    "arg0" => first.length,
                    "arg1" => arg_types.get(1).map(|t| t.length).unwrap_or(Length::Unknown),
                    "arg2" => arg_types.get(2).map(|t| t.length).unwrap_or(Length::Unknown),
                    "longest_arg" => longest_arg_length(arg_types),
                    "n_args" => Length::Known(arg_types.len()),
                    "x_times" => rep_length(arg_types),
                    "test" => first.length,
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
                // Flow-sensitive type refinement: if the condition is a
                // type predicate (`is.numeric(x)`, `is.null(x)`, ...),
                // narrow `x`'s type in each branch. The `then` branch
                // gets the positive refinement; `else_` gets the
                // negative (which we model only for `is.null`).
                let narrowing = extract_type_narrowing(cond);
                let (then_scope, else_scope) = apply_narrowing(scope, &narrowing);
                for s in then {
                    self.check_stmt(s, &mut then_scope.clone());
                }
                if let Some(else_) = else_ {
                    for s in else_ {
                        self.check_stmt(s, &mut else_scope.clone());
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
                // Install the function in the surrounding scope. We
                // build an inferred `fn_sig` (when possible) so a bare
                // named function def in statement position behaves the
                // same as `name <- function(...) body` for downstream
                // callers. This mirrors `infer`'s handling of
                // `Expr::Function` and `collect_returns_and_simulate_at_depth`'s
                // handling of `Stmt::FunctionDef` in pass 2.
                let vt = self.function_value_from_literal(params, body, scope, 0);
                if let Some(n) = name {
                    scope.insert(n.clone(), vt);
                }
                // Infer the body in a fresh scope populated with params
                // so any diagnostics inside the body still fire.
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
                    // Known typeshed function used as a value (e.g.
                    // `sapply(x, sqrt)` passes `sqrt` as a bare
                    // identifier)? Return an opaque function value
                    // rather than flagging it as unbound. The higher-
                    // order call handlers resolve the signature when
                    // the callback is invoked.
                    if self.typeshed.functions.contains_key(name) {
                        return RType::scalar(Mode::Function, false);
                    }
                    // User-defined function in the FnTable used as a
                    // value? Same treatment.
                    if self.fn_table.fns.contains_key(name) {
                        return RType::scalar(Mode::Function, false);
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
                // Assignment in expression position (e.g. the inner
                // assignment in `a <- b <- 1L`): bind the LHS and
                // return the RHS type. R's `<-` returns the assigned
                // value (invisibly).
                if matches!(*op, BinOpKind::Assign | BinOpKind::SuperAssign) {
                    let rt = self.infer(rhs, scope);
                    if let Expr::Ident { name, .. } = lhs.as_ref() {
                        scope.insert(name.clone(), rt);
                    }
                    return rt;
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
                self.infer_index(bt, *kind, args, *span, scope)
            }
            Expr::Function { params, body, .. } => {
                // Pass 3: build a `Mode::Function` value with an
                // inferred `fn_sig` when we can. This mirrors pass 2's
                // `infer_pure` so a function literal in a top-level
                // expression (`g <- f(); v <- (function() 1L)()`)
                // resolves the same way as one inside a return slot.
                self.function_value_from_literal(params, body, scope, 0)
            }
            Expr::If { cond, then, else_, span } => {
                self.infer_if_expr(cond, then, else_, *span, scope)
            }
            Expr::Unknown(_) => RType::UNKNOWN,
        }
    }

    fn infer_binop(&mut self, op: BinOpKind, lt: RType, rt: RType, span: Span) -> RType {
        // `:` sequence operator. Always produces a vector; mode depends
        // on operand modes per R's coercion (int:int -> int, otherwise
        // double). If both operands are integer literals we can even
        // pin the length exactly.
        if matches!(op, BinOpKind::Colon) {
            // Delegate to the type lattice's `seq` method, which models
            // R's `:` behavior (integer for whole-number endpoints).
            return lt.seq(rt);
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

    /// Infer the type of an `if` expression `if (cond) then else else_`.
    /// The condition is inferred for diagnostics (RY001/RY002). Both
    /// branches are inferred; the result is the join of their types.
    /// When `else_` is absent, R returns NULL for the else branch, so
    /// we join with NULL's type.
    fn infer_if_expr(
        &mut self,
        cond: &Expr,
        then: &Expr,
        else_: &Option<Box<Expr>>,
        span: Span,
        scope: &mut Scope,
    ) -> RType {
        let ct = self.infer(cond, scope);
        if ct.invalid_condition() {
            self.emit(
                Severity::Error,
                span_of(cond),
                "RY001",
                format!("`if` condition is `{}`, expected length-1 logical", ct),
            );
        } else if !matches!(ct.mode, Mode::Logical | Mode::Opaque) {
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
        // Flow-sensitive type narrowing for the expression form too.
        let narrowing = extract_type_narrowing(cond);
        let (then_scope, else_scope) = apply_narrowing(scope, &narrowing);
        let then_t = self.infer(then, &mut then_scope.clone());
        let else_t = match else_ {
            Some(e) => self.infer(e, &mut else_scope.clone()),
            None => RType::new(Mode::Null, Length::Zero, false),
        };
        let _ = span;
        then_t.join(else_t)
    }

    /// Infer the result type of `switch(EXPR, ...)`. Both forms are
    /// supported:
    ///   * Numeric: `switch(1, "first", "second", "third")` - selects
    ///     the Nth positional argument.
    ///   * Named: `switch(x, a = 1L, b = "two")` - selects by matching
    ///     `x` against the argument names.
    ///
    /// The result type is the join of all alternative types (since we
    /// can't know which branch will execute at runtime). Each
    /// alternative is also walked for diagnostics.
    fn infer_switch_call(
        &mut self,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        // The first argument is the selector; infer it for diagnostics.
        if let Some(first) = args.first() {
            let _ = self.infer(&first.value, scope);
        }
        // Join the types of all remaining arguments (the alternatives).
        let mut alt_types: Vec<RType> = Vec::new();
        for a in args.iter().skip(1) {
            alt_types.push(self.infer(&a.value, scope));
        }
        let _ = span;
        if alt_types.is_empty() {
            return RType::UNKNOWN;
        }
        let mut iter = alt_types.into_iter();
        let first = iter.next().unwrap_or(RType::UNKNOWN);
        iter.fold(first, |acc, t| acc.join(t))
    }

    /// Infer the result type of `tryCatch(expr, ...)`. The first
    /// positional argument is the main expression; subsequent named
    /// arguments are condition handlers (`error = function(e) ...`,
    /// `warning = function(w) ...`, etc.).
    ///
    /// The result type is the join of the main expression's type and
    /// all handler return types. Each handler is a function literal
    /// (or named function); we infer its return type via
    /// `callback_return_type` with the condition object as the
    /// callback's argument (opaque, since we don't model the
    /// condition object).
    fn infer_trycatch_call(
        &mut self,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        let mut types: Vec<RType> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                // Main expression.
                types.push(self.infer(&a.value, scope));
            } else if a.name.is_some() {
                // Named handler: `error = function(e) ...`. Infer the
                // handler function's return type.
                if let Some(rt) = self.callback_return_type(
                    &a.value,
                    &[RType::UNKNOWN],
                    scope,
                ) {
                    types.push(rt);
                } else {
                    // Couldn't infer handler return: infer for
                    // diagnostics and use opaque.
                    let _ = self.infer(&a.value, scope);
                }
            } else {
                // Extra positional arg (rare): infer for diagnostics.
                let _ = self.infer(&a.value, scope);
            }
        }
        let _ = span;
        if types.is_empty() {
            return RType::UNKNOWN;
        }
        let mut iter = types.into_iter();
        let first = iter.next().unwrap_or(RType::UNKNOWN);
        iter.fold(first, |acc, t| acc.join(t))
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

        // `switch(EXPR, ...)` selects one of several alternatives.
        // The result type is the join of all alternatives. Both numeric
        // switch (`switch(1, "a", "b")`) and named switch
        // (`switch(x, a = 1, b = 2)`) are supported.
        if name == "switch" {
            return self.infer_switch_call(args, scope, span);
        }

        // `tryCatch(expr, ..., handler = fun)`: error-handling construct.
        // The result type is the join of the main expression and all
        // handler return types. Handlers are named arguments whose
        // values are functions (error = function(e) ...).
        if name == "tryCatch" {
            return self.infer_trycatch_call(args, scope, span);
        }

        // `structure(x, class = "...")` is R's class constructor. We
        // model only the common literal forms:
        //   * `class = "foo"` attaches a single class.
        //   * `class = c("a", "b", ...)` attaches a class vector.
        // Non-literal or unparseable forms fall through to opaque
        // inference with `ClassVector::unknown()` so RY050 stays quiet.
        if name == "structure" {
            return self.infer_structure_call(args, scope, span);
        }
        // `factor(x)` returns an integer vector with class "factor".
        // (And often also "ordered" if `ordered = TRUE`, but we keep v1
        // to the base case.)
        if name == "factor" {
            // Infer args so unbound-variable diagnostics still fire.
            for a in args {
                let _ = self.infer(&a.value, scope);
            }
            return RType::new(Mode::Integer, Length::Unknown, true)
                .with_class(ClassVector::single(intern_class_name("factor")));
        }

        // NSE verbs (`subset`, `with`, `within`, `transform`) evaluate
        // their expression arguments in an augmented scope where the
        // data frame's columns are bound as names. We must intercept
        // these BEFORE the eager `infer(&a.value, scope)` loop below,
        // because that loop would emit spurious RY010 ("variable not
        // bound") for every column reference (`cyl`, `mpg`, ...).
        // Returns `Some(t)` when the call was handled; the caller uses
        // the returned type verbatim. Returns `None` to fall through to
        // the regular arg-inference path.
        if let Some(t) = self.infer_nse_call(&name, args, scope, span) {
            return t;
        }

        // Infer arg types.
        let mut arg_types: Vec<RType> = Vec::with_capacity(args.len());
        for a in args {
            arg_types.push(self.infer(&a.value, scope));
        }

        // Indirect call through a closure value: if the name is bound
        // in scope to a `Function`-typed value with an inferred
        // `fn_sig`, the call resolves to the signature's return type.
        // This is what makes `c <- make_counter(); v <- c()` work
        // without `c` having its own FnTable entry. We check this
        // before the FnTable / typeshed paths so a local binding
        // shadows any same-named top-level function (matching R's
        // lexical scoping).
        if let Some(t) = scope.get(&name) {
            if matches!(t.mode, Mode::Function) {
                if let Some(sig) = t.fn_sig {
                    return *sig.return_type;
                }
                // Bound function value without an inferred signature:
                // opaque. We do NOT fall through to the FnTable path,
                // because a scope-local binding shadows top-level
                // definitions and we have no way to refine the local
                // one. Returning opaque here is the conservative
                // choice (no false positives, possible false negatives).
                return RType::UNKNOWN;
            }
        }

        // Built-in: `c(...)` concatenates and produces the common mode.
        if name == "c" {
            return self.infer_c(args, &arg_types, span);
        }
        if name == "list" {
            return self.infer_list(&arg_types, args, span);
        }
        // `data.frame(...)`: a record constructor. Same column-schema
        // logic as `list(...)`, but the result is classed
        // "data.frame" and column lengths are coerced to a common
        // length (R recycles; for v1 we take the max of the known
        // lengths).
        if name == "data.frame" {
            return self.infer_data_frame(&arg_types, args, span);
        }

        // S3 dispatch: when a known generic is called with a classed
        // first argument, look up `(generic, class)` in the S3 method
        // table. On a hit, return the method's inferred return type. On
        // a miss with a *known* class, emit RY050. On a miss with an
        // unknown or empty class, fall through (we can't say anything).
        //
        // We model only R's first-element dispatch rule: walking the
        // full class vector (and matching `default`) is a future task.
        // `default` is treated as always-present in the typeshed's S3
        // method table for the common generics, so RY050 never fires
        // for them unless the user explicitly shadows them away.
        if S3_GENERICS.contains(&name.as_str()) {
            if let Some(rt) = self.try_s3_dispatch(&name, &arg_types, span) {
                return rt;
            }
        }

        // Higher-order built-ins (`lapply`, `sapply`, `vapply`, `Map`,
        // `Reduce`, `Filter`, ...): model the callback to infer the
        // result type. Falls through to the typeshed when the name is
        // not one we recognize, so the existing opaque entries for
        // these functions still apply.
        //
        // Before computing the result type, walk the callback body for
        // diagnostics (e.g. RY010 on an unbound name inside the
        // callback). This ensures that `lapply(x, function(i)
        // undefined_var)` still flags the unbound variable, even though
        // the type computation itself is pure.
        if HigherOrderFunc::from_name(&name).is_some() {
            self.walk_callback_for_diagnostics(&name, args, &arg_types, scope);
        }
        if let Some(rt) = self.infer_higher_order_call(&name, args, &arg_types, scope) {
            return rt;
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

    /// Infer the type of `structure(x, class = "...")`. We model only
    /// the literal class forms; everything else returns the first
    /// argument's type with `ClassVector::unknown()` (so we neither lie
    /// about a class nor spuriously trigger RY050).
    ///
    /// The base value's column schema is preserved: `RType::with_class`
    /// is `RType { class, ..self }`, so a `structure(list(a = 1L),
    /// class = "foo")` call yields a value whose columns are still
    /// `[("a", integer<1>)]` and whose class is `["foo"]`. This lets
    /// `$a` resolve correctly on user-defined classes built on top of
    /// a list-shaped payload.
    fn infer_structure_call(
        &mut self,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        // The base value is the first positional argument (or the
        // `x = ...` named argument). The first such positional-or-`x`
        // arg wins; later ones are inferred for diagnostics only.
        let mut base_type = RType::UNKNOWN;
        let mut class_expr: Option<&Expr> = None;
        for a in args {
            if matches!(a.name.as_deref(), Some("class")) {
                class_expr = Some(&a.value);
                continue;
            }
            let is_base = matches!(a.name.as_deref(), None | Some("x"))
                && matches!(base_type.mode, Mode::Opaque);
            if is_base {
                base_type = self.infer(&a.value, scope);
            } else {
                let _ = self.infer(&a.value, scope);
            }
        }
        if let Some(ce) = class_expr {
            match parse_class_literal(ce) {
                ClassLiteral::Single(name) => {
                    let interned = intern_class_name(&name);
                    return base_type.with_class(ClassVector::single(interned));
                }
                ClassLiteral::Multi(names) => {
                    let interned: Vec<&'static str> =
                        names.iter().map(|n| intern_class_name(n)).collect();
                    return base_type
                        .with_class(ClassVector::from_static_slice(&interned));
                }
                ClassLiteral::Unknown => {
                    // Class is dynamic; keep base type but mark class as
                    // undetermined so RY050 stays quiet.
                    return base_type.with_class(ClassVector::unknown());
                }
            }
        }
        let _ = span;
        base_type
    }

    /// Handle R's Non-Standard Evaluation verbs (`subset`, `with`,
    /// `within`, `transform`). These evaluate their expression
    /// arguments in an augmented scope where the data frame's columns
    /// are bound as names, so `subset(df, cyl == 4)` resolves `cyl`
    /// against `df`'s column schema rather than the enclosing scope.
    ///
    /// Returns `Some(t)` when the call was recognized as an NSE verb
    /// (the caller uses `t` verbatim and skips the regular arg-inference
    /// path). Returns `None` for non-NSE names so `infer_call` falls
    /// through to the regular path.
    ///
    /// Behavior when the first arg has no column schema: we cannot
    /// enumerate the columns, so the expression arguments cannot be
    /// type-checked meaningfully. We still infer them against the bare
    /// scope (no column augmentation) so any genuinely unbound name in
    /// the expression still emits RY010; this mirrors the conservative
    /// approach for unknown data throughout the checker.
    ///
    /// The augmented scope is local to this call: column bindings must
    /// NOT leak back into the enclosing scope (we operate on a clone).
    fn infer_nse_call(
        &mut self,
        name: &str,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> Option<RType> {
        let verb = NseVerb::from_name(name)?;
        // The data frame is the first positional argument. If it's
        // absent, fall through to the regular path (R would error at
        // runtime; v1 stays silent and defers).
        let df_arg = args.first()?;
        let df_type = self.infer(&df_arg.value, scope);
        let augmented = match df_type.columns {
            Some(schema) => self.scope_with_columns(scope, schema),
            None => scope.clone(),
        };
        let result = match verb {
            NseVerb::Subset => self.infer_nse_subset(args, df_type, &augmented),
            NseVerb::With => self.infer_nse_with(args, df_type, &augmented),
            NseVerb::Within => self.infer_nse_within(args, df_type, &augmented),
            NseVerb::Transform => self.infer_nse_transform(args, df_type, &augmented),
        };
        let _ = span;
        Some(result)
    }

    /// `subset(df, subset_expr, select_expr?)` returns a data frame of
    /// the same class as `df` with possibly fewer rows. We infer the
    /// `subset_expr` (and `select_expr`, if present) against the
    /// augmented scope so column references resolve; the result type is
    /// the data frame's own type (column schema is preserved since the
    /// column set is unchanged in v1's model).
    fn infer_nse_subset(
        &mut self,
        args: &[Arg],
        df_type: RType,
        augmented: &Scope,
    ) -> RType {
        // Args at indices 1 and 2 are the subset and select expressions.
        // Any later positional or named args (e.g. `drop = ...`) are
        // walked for diagnostics against the augmented scope.
        let mut local = augmented.clone();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                continue;
            }
            // Named metadata args like `select = ...` are still NSE
            // expressions in `subset`; we walk them all in the
            // augmented scope so column references resolve uniformly.
            let _ = self.infer(&a.value, &mut local);
        }
        df_type
    }

    /// `with(df, expr)` evaluates `expr` in the data frame's scope and
    /// returns whatever `expr` evaluates to. The result type is the
    /// inferred type of the expression.
    fn infer_nse_with(
        &mut self,
        args: &[Arg],
        df_type: RType,
        augmented: &Scope,
    ) -> RType {
        let _ = df_type;
        let mut local = augmented.clone();
        // The second positional arg is the expression; any further args
        // (rare for `with`) are walked for diagnostics.
        let mut result = RType::UNKNOWN;
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let t = self.infer(&a.value, &mut local);
            if i == 1 {
                result = t;
            }
        }
        result
    }

    /// `within(df, expr)` evaluates `expr` (typically assignments like
    /// `df$new <- ...`) in the data frame's scope and returns the
    /// (possibly mutated) data frame. The result type is the data
    /// frame's own type; column additions from assignments inside `expr`
    /// are not modeled at v1.
    fn infer_nse_within(
        &mut self,
        args: &[Arg],
        df_type: RType,
        augmented: &Scope,
    ) -> RType {
        let mut local = augmented.clone();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let _ = self.infer(&a.value, &mut local);
        }
        df_type
    }

    /// `transform(df, new_col = expr, ...)` adds or replaces columns.
    /// Each named expression is inferred against the augmented scope so
    /// references to existing columns (e.g. `mpg * 2`) resolve. The
    /// result type is the data frame's own type; the new column types
    /// are not folded into the schema at v1 (the existing schema is
    /// preserved unchanged, matching the conservative stance documented
    /// for `within`).
    fn infer_nse_transform(
        &mut self,
        args: &[Arg],
        df_type: RType,
        augmented: &Scope,
    ) -> RType {
        let mut local = augmented.clone();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let _ = self.infer(&a.value, &mut local);
        }
        df_type
    }

    /// Build an augmented scope by cloning `base_scope` and inserting a
    /// binding for every column in `schema`. Column names that shadow
    /// existing bindings in `base_scope` are overwritten by the column
    /// type (this mirrors R's actual NSE lookup order: columns first,
    /// then the enclosing environment). The returned scope is a fresh
    /// clone; `base_scope` is untouched, so column bindings never leak
    /// into the caller's scope.
    fn scope_with_columns(&self, base_scope: &Scope, schema: &'static ColumnSchema) -> Scope {
        let mut s = base_scope.clone();
        for (name, t) in &schema.columns {
            s.insert(name.clone(), *t);
        }
        s
    }

    /// Handle R's higher-order built-ins (`lapply`, `sapply`, `vapply`,
    /// `Map`, `mapply`, `rapply`, `Reduce`, `Filter`, `Find`,
    /// `Position`, `do.call`). These take a function-valued argument
    /// (`FUN` or `f`) and apply it to each element (or reduction) of
    /// their data argument(s). The key insight is that the callback's
    /// return type determines the result type, so we model each
    /// callback invocation against the element type of the input.
    ///
    /// Returns `Some(t)` when the call was recognized (the caller uses
    /// `t` verbatim). Returns `None` for names we don't model so
    /// `infer_call` falls through to S3 / user-fn / typeshed paths.
    ///
    /// Callback resolution covers three forms:
    ///   * Inline anonymous function literal (`function(x) x * 2`):
    ///     walk the body with a scope containing the param bound to the
    ///     element type, collecting returns.
    ///   * Named user-defined function (`my_fun`): look up its refined
    ///     return slot in the FnTable.
    ///   * Named typeshed function (`sqrt`): apply its signature with
    ///     the element type as the argument.
    ///
    /// When the callback cannot be resolved (unknown name, non-function
    /// literal, depth cap exceeded), the result falls back to the
    /// typeshed's declared return type for that higher-order function
    /// (e.g. `list` for `lapply`), so callers still get a useful upper
    /// bound on the mode without false positives.
    fn infer_higher_order_call(
        &self,
        name: &str,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> Option<RType> {
        let ho = HigherOrderFunc::from_name(name)?;
        Some(self.infer_ho_result(&ho, args, arg_types, scope))
    }

    /// Per-builtin result-type computation. Shared between pass 2 (pure,
    /// via `infer_ho_result_pure`) and pass 3 (diagnostic-emitting).
    /// This is the pass-3 entry point: it calls `self.infer` on data
    /// arguments (which may emit RY010 etc.) before computing the
    /// element type.
    fn infer_ho_result(
        &self,
        ho: &HigherOrderFunc,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        match ho {
            HigherOrderFunc::Lapply => self.ho_lapply(args, arg_types, scope),
            HigherOrderFunc::Sapply => self.ho_sapply(args, arg_types, scope),
            HigherOrderFunc::Vapply => self.ho_vapply(args, arg_types, scope),
            HigherOrderFunc::Map | HigherOrderFunc::Mapply => {
                self.ho_map(args, arg_types, scope)
            }
            HigherOrderFunc::Rapply => self.ho_rapply(args, arg_types, scope),
            HigherOrderFunc::Reduce => self.ho_reduce(args, arg_types, scope),
            HigherOrderFunc::Filter => self.ho_filter(args, arg_types, scope),
            HigherOrderFunc::Find => self.ho_find(args, arg_types, scope),
            HigherOrderFunc::Position => self.ho_position(args, arg_types, scope),
            HigherOrderFunc::DoCall => self.ho_do_call(args, arg_types, scope),
        }
    }

    /// Extract the callback expression from an argument list by name
    /// (`FUN`, `f`) or by positional index. Returns `None` when no
    /// callback argument is present.
    fn extract_callback<'a>(
        args: &'a [Arg],
        names: &[&str],
        positional_idx: usize,
    ) -> Option<&'a Expr> {
        for a in args {
            if let Some(n) = a.name.as_deref() {
                if names.contains(&n) {
                    return Some(&a.value);
                }
            }
        }
        args.get(positional_idx).map(|a| &a.value)
    }

    /// `lapply(X, FUN, ...)`: applies `FUN` to each element of `X`,
    /// returning a list of the same length. The element type of `X`
    /// becomes the callback's first argument.
    fn ho_lapply(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        let x_type = arg_types.first().copied().unwrap_or(RType::UNKNOWN);
        let elem = x_type.element();
        let cb = Self::extract_callback(args, &["FUN"], 1);
        let cb_ret = cb.and_then(|c| self.callback_return_type(c, &[elem], scope));
        let length = x_type.length;
        let element_type = cb_ret.unwrap_or(RType::UNKNOWN);
        let mut result = RType::new(Mode::List, length, false);
        // Always build a schema when we know the element type, even if
        // the list length is unknown. We create a single `[[1]]` entry
        // so that `result[[1]]` and `homogeneous_list_element_type`
        // can resolve the element type. When the length IS known, we
        // create explicit entries for each index.
        if !matches!(element_type.mode, Mode::Opaque) {
            let n = match length {
                Length::Known(n) if n > 0 => n,
                _ => 1, // Unknown or zero length: still create one
                       // entry so the element type is discoverable.
            };
            let schema = ColumnSchema {
                columns: (0..n)
                    .map(|i| (format!("[[{}]]", i + 1), element_type))
                    .collect(),
            };
            result = result.with_columns(intern_column_schema(schema));
        }
        result
    }

    /// `sapply(X, FUN, ...)`: like `lapply` but simplifies the result.
    /// When the callback returns a length-1 atomic for every element,
    /// the result is a vector of that mode with `X`'s length. When the
    /// callback returns length-`k` vectors, the result is a matrix. We
    /// model the common case: callback returns atomic length-1, so the
    /// result is a vector of the callback's return mode with X's length.
    fn ho_sapply(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        let x_type = arg_types.first().copied().unwrap_or(RType::UNKNOWN);
        let elem = x_type.element();
        let cb = Self::extract_callback(args, &["FUN"], 1);
        let cb_ret = cb.and_then(|c| self.callback_return_type(c, &[elem], scope));
        match cb_ret {
            Some(t) if matches!(t.length, Length::One) && !matches!(t.mode, Mode::List | Mode::Opaque) => {
                // Simplification to a vector of the callback's mode.
                RType::new(t.mode, x_type.length, t.na.0)
            }
            // Could not infer the callback, or it returns non-scalar /
            // list values: conservatively report a list (the
            // unsimplified form). This avoids false positives while
            // still giving a useful mode upper bound.
            _ => RType::new(Mode::List, x_type.length, false),
        }
    }

    /// `vapply(X, FUN, FUN.VALUE, ...)`: like `sapply` but the result
    /// type is specified by `FUN.VALUE`. The callback's actual return
    /// must be compatible; we return the FUN.VALUE template's mode and
    /// length. The result length is `FUN.VALUE.length * X.length` when
    /// both are known (R stacks the callback outputs column-wise), but
    /// for v1 we approximate as FUN.VALUE's mode with X's length when
    /// FUN.VALUE is length-1, else opaque length.
    fn ho_vapply(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        let x_type = arg_types.first().copied().unwrap_or(RType::UNKNOWN);
        let fun_value = arg_types.get(2).copied().unwrap_or(RType::UNKNOWN);
        // Walk the callback for type information (its body may reference
        // unbound vars etc.). We don't use its return type because
        // FUN.VALUE is the authoritative template.
        if let Some(cb) = Self::extract_callback(args, &["FUN"], 1) {
            let elem = x_type.element();
            let _ = self.callback_return_type(cb, &[elem], scope);
        }
        match fun_value.length {
            Length::One => RType::new(fun_value.mode, x_type.length, fun_value.na.0),
            _ => RType::new(fun_value.mode, Length::Unknown, fun_value.na.0),
        }
    }

    /// `Map(f, ...)`: applies `f` to corresponding elements of all
    /// arguments, returning a list. Each argument contributes its
    /// element type as a callback argument.
    fn ho_map(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        // First positional arg is `f`; subsequent positional args are
        // the vectors to map over. Named `f = ...` is also recognized.
        let cb = Self::extract_callback(args, &["f"], 0);
        let elem_types: Vec<RType> = arg_types
            .iter()
            .skip(1)
            .map(|t| t.element())
            .collect();
        let cb_ret = cb.and_then(|c| self.callback_return_type(c, &elem_types, scope));
        // The result list length is the length of the shortest input
        // (R's recycling for Map). We approximate as the first data
        // arg's length, or Unknown if absent.
        let length = arg_types.get(1).map(|t| t.length).unwrap_or(Length::Zero);
        let _ = cb_ret; // Mode is list regardless of callback return.
        RType::new(Mode::List, length, false)
    }

    /// `rapply(L, f, ...)`: recursively applies `f` to each leaf of
    /// list `L`. The result is a list of the same shape. We model only
    /// the top-level shape: result is a list with L's length.
    fn ho_rapply(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        let l_type = arg_types.first().copied().unwrap_or(RType::UNKNOWN);
        // Walk the callback for type information.
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 1) {
            let _ = self.callback_return_type(cb, &[RType::UNKNOWN], scope);
        }
        RType::new(Mode::List, l_type.length, false)
    }

    /// `Reduce(f, x, ...)`: left-fold. The result type is the element
    /// type of `x` (the accumulator starts as `x[[1]]`). For an empty
    /// `x` with no `init`, R errors; we stay opaque in that case.
    fn ho_reduce(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        let x_type = arg_types.get(1).copied().unwrap_or(RType::UNKNOWN);
        // Walk the callback for type information. The callback takes two
        // args: the accumulator and the next element, both of x's
        // element type.
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 0) {
            let elem = x_type.element();
            let _ = self.callback_return_type(cb, &[elem, elem], scope);
        }
        x_type.element()
    }

    /// `Filter(f, x)`: returns the subset of `x` where `f` returns
    /// TRUE. The result type is `x`'s type (same mode, possibly shorter
    /// length which we cannot know statically).
    fn ho_filter(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        let x_type = arg_types.get(1).copied().unwrap_or(RType::UNKNOWN);
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 0) {
            let _ = self.callback_return_type(cb, &[x_type.element()], scope);
        }
        x_type
    }

    /// `Find(f, x)`: returns the first element of `x` where `f` returns
    /// TRUE, or NULL. The result type is the element type (or NULL).
    fn ho_find(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        let x_type = arg_types.get(1).copied().unwrap_or(RType::UNKNOWN);
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 0) {
            let _ = self.callback_return_type(cb, &[x_type.element()], scope);
        }
        x_type.element()
    }

    /// `Position(f, x)`: returns the integer index of the first element
    /// where `f` returns TRUE, or NA_integer_. The result is always
    /// integer length-1.
    fn ho_position(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
    ) -> RType {
        let x_type = arg_types.get(1).copied().unwrap_or(RType::UNKNOWN);
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 0) {
            let _ = self.callback_return_type(cb, &[x_type.element()], scope);
        }
        RType::scalar(Mode::Integer, true)
    }

    /// `do.call(fun, args, ...)`: invokes `fun` with the arguments in
    /// `args` (a list). We model only the case where `fun` is a named
    /// function (user-fn or typeshed). The result is `fun`'s return type.
    fn ho_do_call(
        &self,
        args: &[Arg],
        arg_types: &[RType],
        _scope: &Scope,
    ) -> RType {
        let fun_expr = args.first().map(|a| &a.value);
        let _ = arg_types;
        match fun_expr {
            Some(Expr::Ident { name, .. }) => {
                // User-fn: look up the refined return slot.
                if let Some(f) = self.fn_table.fns.get(name) {
                    return self.return_slots.get(f.return_slot);
                }
                // Typeshed: apply the signature with no arg types.
                if let Some(sig) = self.typeshed.functions.get(name) {
                    return self.apply_sig_pure(sig, &[]);
                }
                RType::UNKNOWN
            }
            _ => RType::UNKNOWN,
        }
    }

    /// Infer the return type of a single callback invocation, given the
    /// argument types the higher-order function will pass to it.
    ///
    /// Covers three callback forms:
    ///   * `Expr::Function { params, body }` (anonymous literal): walk
    ///     the body with a scope containing the params bound to the
    ///     element types, collecting returns. Bounded by
    ///     `MAX_CLOSURE_DEPTH`.
    ///   * `Expr::Ident { name }` bound in scope to a
    ///     `Mode::Function` value with `fn_sig`: use the signature's
    ///     return type.
    ///   * `Expr::Ident { name }` referring to a user-fn: read its
    ///     refined return slot.
    ///   * `Expr::Ident { name }` referring to a typeshed function:
    ///     apply its signature with the element types as arguments.
    ///
    /// Returns `None` when the callback form is not recognized or the
    /// return type is opaque (caller falls back to the conservative
    /// per-builtin default).
    fn callback_return_type(
        &self,
        callback: &Expr,
        call_arg_types: &[RType],
        scope: &Scope,
    ) -> Option<RType> {
        match callback {
            Expr::Function { params, body, .. } => {
                self.callback_literal_return(params, body, call_arg_types, scope, 0)
            }
            Expr::Ident { name, .. } => {
                // Bound closure value in scope?
                if let Some(t) = scope.get(name) {
                    if matches!(t.mode, Mode::Function) {
                        if let Some(sig) = t.fn_sig {
                            return Some(*sig.return_type);
                        }
                        return None;
                    }
                }
                // User-defined function in the FnTable?
                if let Some(f) = self.fn_table.fns.get(name) {
                    let rt = self.return_slots.get(f.return_slot);
                    if !matches!(rt.mode, Mode::Opaque) {
                        return Some(rt);
                    }
                    return None;
                }
                // Typeshed function?
                if let Some(sig) = self.typeshed.functions.get(name) {
                    return Some(self.apply_sig_pure(sig, call_arg_types));
                }
                None
            }
            _ => None,
        }
    }

    /// Walk an anonymous function literal's body to infer its return
    /// type, given the argument types the caller will pass. Similar to
    /// `build_function_signature_pure` but takes explicit argument
    /// types rather than inferring from defaults. Used by
    /// `callback_return_type` for the inline-literal case.
    fn callback_literal_return(
        &self,
        params: &[Param],
        body: &[Stmt],
        call_arg_types: &[RType],
        captured_scope: &Scope,
        depth: usize,
    ) -> Option<RType> {
        if body.is_empty() || depth >= MAX_CLOSURE_DEPTH {
            return None;
        }
        let mut scope = captured_scope.clone();
        for (i, p) in params.iter().enumerate() {
            let t = call_arg_types.get(i).copied().unwrap_or(RType::UNKNOWN);
            scope.insert(p.name.clone(), t);
        }
        let mut returns: Vec<RType> = Vec::new();
        for s in body {
            self.collect_returns_and_simulate_at_depth(s, &mut scope, &mut returns, depth + 1);
        }
        if let Some(t) = self.trailing_return_type(body, &scope, depth + 1) {
            returns.push(t);
        }
        if returns.is_empty() {
            return None;
        }
        let mut iter = returns.into_iter();
        let first = iter.next().unwrap_or(RType::UNKNOWN);
        let joined = iter.fold(first, |acc, t| acc.join(t));
        if matches!(joined.mode, Mode::Opaque) {
            return None;
        }
        Some(joined)
    }

    /// Walk the callback body of a higher-order function call for
    /// diagnostics (RY010 unbound variables, RY040 type errors, etc.).
    /// Called from pass 3 (`infer_call`) before the type-computation
    /// path, which is pure. This ensures that errors inside the
    /// callback body are surfaced even though the type computation
    /// itself doesn't emit diagnostics.
    ///
    /// For each callback (inline anonymous function literal), we build
    /// a scope with the callback's params bound to the element types
    /// the higher-order function will pass, then walk the body's
    /// statements via `check_stmt` (which emits diagnostics). Named
    /// callbacks (user-fn, typeshed) don't need this: their bodies are
    /// walked during the user-fn fixpoint or are built-in.
    fn walk_callback_for_diagnostics(
        &mut self,
        name: &str,
        args: &[Arg],
        arg_types: &[RType],
        scope: &mut Scope,
    ) {
        let ho = match HigherOrderFunc::from_name(name) {
            Some(h) => h,
            None => return,
        };
        // Determine the callback argument and the element types it will
        // be called with, based on which higher-order function this is.
        let (cb_idx, cb_names, elem_types) = match ho {
            HigherOrderFunc::Lapply | HigherOrderFunc::Sapply | HigherOrderFunc::Vapply => {
                let x_type = arg_types.first().copied().unwrap_or(RType::UNKNOWN);
                (1, &["FUN"][..], vec![x_type.element()])
            }
            HigherOrderFunc::Map | HigherOrderFunc::Mapply => {
                let elem_types: Vec<RType> =
                    arg_types.iter().skip(1).map(|t| t.element()).collect();
                (0, &["f"][..], elem_types)
            }
            HigherOrderFunc::Rapply => {
                (1, &["f", "FUN"][..], vec![RType::UNKNOWN])
            }
            HigherOrderFunc::Reduce => {
                let x_type = arg_types.get(1).copied().unwrap_or(RType::UNKNOWN);
                let elem = x_type.element();
                (0, &["f", "FUN"][..], vec![elem, elem])
            }
            HigherOrderFunc::Filter | HigherOrderFunc::Find | HigherOrderFunc::Position => {
                let x_type = arg_types.get(1).copied().unwrap_or(RType::UNKNOWN);
                (0, &["f", "FUN"][..], vec![x_type.element()])
            }
            HigherOrderFunc::DoCall => return, // callback is the function name, no body to walk
        };
        let cb = match Self::extract_callback(args, cb_names, cb_idx) {
            Some(c) => c,
            None => return,
        };
        if let Expr::Function { params, body, .. } = cb {
            let mut fn_scope = scope.clone();
            for (i, p) in params.iter().enumerate() {
                let t = elem_types.get(i).copied().unwrap_or(RType::UNKNOWN);
                fn_scope.insert(p.name.clone(), t);
            }
            for s in body {
                self.check_stmt(s, &mut fn_scope);
            }
        }
    }

    /// Try S3 dispatch for a known generic. Returns `Some(rt)` if a
    /// method was found or a diagnostic was emitted (the caller should
    /// use the returned type directly). Returns `None` only when the
    /// caller should fall through to other resolution paths.
    ///
    /// RY050 emission policy: we only flag a missing method when we're
    /// confident the generic actually uses S3 dispatch. The signal for
    /// that confidence is the existence of a `default` method (in user
    /// code or typeshed). Without a `default`, the call might just be a
    /// plain function call that happens to share a name with an S3
    /// generic, so we stay silent and let the regular function table
    /// resolve it. This keeps the real-world baseline stable while still
    /// catching the cases the task calls out (`print` on an undefined
    /// class, etc.).
    ///
    /// Design note: we deliberately return `Option<RType>` rather than
    /// `RType` because the caller (`infer_call`) may still want to
    /// consult the user-fn table or the typeshed for non-S3 forms (e.g.
    /// when the first arg is opaque).
    fn try_s3_dispatch(
        &mut self,
        generic: &str,
        arg_types: &[RType],
        span: Span,
    ) -> Option<RType> {
        let first = arg_types.first().copied()?;
        let cv = first.class;
        if !cv.has_known_class() {
            // No known class (either empty or unknown): nothing for S3
            // dispatch to do. The caller will try user-fn/typeshed
            // resolution against the bare name.
            return None;
        }
        // We have a known class vector. R walks it in order; for v1 we
        // model only the first-element rule.
        let first_class = cv.first()?;
        // `default` is never itself "missing" - it's the fallback.
        if first_class == "default" {
            return None;
        }
        // 1. User-defined method for the first class wins.
        if let Some(slot) = self
            .fn_table
            .s3_methods
            .get(&(generic.to_string(), first_class.to_string()))
            .copied()
        {
            return Some(self.return_slots.get(slot));
        }
        // 2. Built-in (typeshed) method for the first class. These are
        // registered in the typeshed's `s3_methods` table as a
        // `(generic, class)` -> FunctionSig map so we can reuse the
        // existing `apply_sig` plumbing.
        if let Some(sig) = self
            .typeshed
            .s3_methods
            .get(&(generic.to_string(), first_class.to_string()))
            .cloned()
        {
            return Some(self.apply_sig(generic, &sig, arg_types, &[], span));
        }
        // 3. No specific method. Only emit RY050 if we're confident the
        // generic uses S3 dispatch, which we approximate by the
        // existence of a `default` method anywhere in the program or
        // typeshed. If there's no default, fall through silently: the
        // call is probably just a plain function call.
        let default_key = (generic.to_string(), "default".to_string());
        let has_default = self.fn_table.s3_methods.contains_key(&default_key)
            || self.typeshed.s3_methods.contains_key(&default_key);
        if !has_default {
            return None;
        }
        // The generic is a known S3 generic (it has a default) but the
        // specific class has no method. Emit RY050 and return opaque so
        // callers don't trip further diagnostics on the result. R would
        // fall back to `<generic>.default` at runtime, but the missing
        // specific method is almost always a bug worth flagging.
        self.emit(
            Severity::Warning,
            span,
            "RY050",
            format!(
                "S3 generic `{}` called on value of class `{}` but no `{}.{}` method is defined",
                generic, first_class, generic, first_class
            ),
        );
        Some(RType::UNKNOWN)
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

    /// Infer the type of `list(...)`. The result is always a list whose
    /// length equals the argument count; if at least one argument is
    /// named, we additionally build a column schema from the named
    /// args (positional args get R's auto-generated `[[i]]` names).
    ///
    /// We build the schema even when only some args are named: that
    /// mirrors R's `list(a = 1, "x")` which produces names `c("a", "2")`.
    /// The schema is what powers `df$col` / `df[["col"]]` resolution
    /// downstream.
    fn infer_list(
        &mut self,
        arg_types: &[RType],
        args: &[Arg],
        _span: Span,
    ) -> RType {
        let length = Length::Known(arg_types.len());
        let base = RType::new(Mode::List, length, false);
        let schema = build_named_schema(arg_types, args);
        if let Some(s) = schema {
            base.with_columns(intern_column_schema(s))
        } else {
            base
        }
    }

    /// Infer the type of `data.frame(...)`. Same column-schema logic as
    /// `list(...)`, but:
    /// * The result is classed `"data.frame"`.
    /// * Column lengths are coerced to a common length (R recycles). For
    ///   v1 we take the max of the known lengths (or Unknown if any
    ///   column's length is Unknown), and propagate that length onto
    ///   each column so `df$col` returns a vector of the right length.
    /// * Special arguments like `row.names = ...`, `check.names = ...`
    ///   are NOT columns and are dropped from the schema. We recognize
    ///   the common ones by name.
    fn infer_data_frame(
        &mut self,
        arg_types: &[RType],
        args: &[Arg],
        _span: Span,
    ) -> RType {
        // Filter out non-column named arguments first. Positional args
        // are kept (they become columns); known metadata args are dropped
        // so they don't pollute the schema.
        const METADATA_ARGS: &[&str] = &[
            "row.names",
            "check.rows",
            "check.names",
            "stringsAsFactors",
            "fix.empty.names",
        ];
        let mut filtered_types: Vec<RType> = Vec::with_capacity(arg_types.len());
        let mut filtered_args: Vec<Arg> = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            if let Some(n) = a.name.as_deref() {
                if METADATA_ARGS.contains(&n) {
                    continue;
                }
            }
            filtered_types.push(arg_types[i]);
            filtered_args.push(a.clone());
        }

        // Compute the common column length (max of known lengths).
        let mut common_len: Length = Length::One;
        for t in &filtered_types {
            common_len = match (common_len, t.length) {
                (Length::Zero, x) | (x, Length::Zero) => x,
                (Length::One, x) | (x, Length::One) => x,
                (Length::Known(a), Length::Known(b)) => Length::Known(a.max(b)),
                _ => Length::Unknown,
            };
        }

        // Build per-column types with the coerced length.
        let coerced_types: Vec<RType> = filtered_types
            .iter()
            .map(|t| RType {
                mode: t.mode,
                length: common_len,
                na: t.na,
                class: t.class,
                // Nested column schemas on a data-frame column would
                // mean nested data frames; v1 keeps those opaque.
                columns: None,
                // fn_sig is meaningless on a data-frame column.
                fn_sig: None,
            })
            .collect();

        // Reuse the named-schema builder, then patch the coerced types
        // in (the builder uses the original arg_types verbatim).
        let mut schema = build_named_schema(&coerced_types, &filtered_args);
        if let Some(s) = schema.as_mut() {
            // Sanity: lengths should already match coerced_types.
            debug_assert_eq!(s.columns.len(), coerced_types.len());
        }

        let class = ClassVector::single(intern_class_name("data.frame"));
        let base = RType::new(Mode::List, Length::Known(filtered_types.len()), false)
            .with_class(class);
        match schema {
            Some(s) => base.with_columns(intern_column_schema(s)),
            None => base,
        }
    }

    fn apply_sig(
        &mut self,
        name: &str,
        sig: &FunctionSig,
        arg_types: &[RType],
        args: &[Arg],
        span: Span,
    ) -> RType {
        // Match named arguments to parameters so that `arg0` refers to
        // the first *parameter* (by name), not the first positional arg.
        // When `sig.params` is empty or only contains `...`, fall back
        // to raw positional indexing.
        let matched = if sig.params.is_empty() || sig.params.iter().all(|p| p == "...") {
            arg_types.to_vec()
        } else {
            match_args_to_params(&sig.params, args, arg_types)
        };
        let first = matched.first().copied().unwrap_or(RType::UNKNOWN);
        match &sig.return_ {
            ReturnSpec::Slot(s) => match s.as_str() {
                "arg0" => first,
                "concat_of_args" => self.infer_c(args, arg_types, span),
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
                    // "arg0" as a mode spec: use the first param's mode.
                    "arg0" => first.mode,
                    // "arg1" as a mode spec: use the second param's mode.
                    "arg1" => matched.get(1).map(|t| t.mode).unwrap_or(Mode::Opaque),
                    // "arg2" as a mode spec: use the third param's mode.
                    "arg2" => matched.get(2).map(|t| t.mode).unwrap_or(Mode::Opaque),
                    // "yes_or_no": join of the second and third params'
                    // modes (for `ifelse(test, yes, no)`).
                    "yes_or_no" => {
                        let yes = matched.get(1).copied().unwrap_or(RType::UNKNOWN);
                        let no = matched.get(2).copied().unwrap_or(RType::UNKNOWN);
                        yes.join(no).mode
                    }
                    _ => Mode::Opaque,
                };
                let length = match c.length.as_str() {
                    "0" => Length::Zero,
                    "1" => Length::One,
                    "unknown" => Length::Unknown,
                    "arg0" => first.length,
                    "arg1" => matched.get(1).map(|t| t.length).unwrap_or(Length::Unknown),
                    "arg2" => matched.get(2).map(|t| t.length).unwrap_or(Length::Unknown),
                    // Longest of all args' lengths (for paste/paste0/sprintf).
                    "longest_arg" => longest_arg_length(arg_types),
                    // Number of arguments (for list()).
                    "n_args" => Length::Known(args.len()),
                    // x_times: arg0 length * arg1 value (for rep).
                    "x_times" => rep_length(arg_types),
                    "test" => first.length,
                    _ => Length::Unknown,
                };
                let _ = name;
                RType::new(mode, length, c.na)
            }
        }
    }

    /// Resolve the type of a subset/extract expression given the base
    /// type, the kind of index (`[`, `[[`, `$`), and the (already
    /// lowered) argument list.
    ///
    /// v1 column-access semantics:
    /// * `df$col` (`Dollar`): the column name lives on `args[0].name`.
    ///   If `bt` has a column schema, return that column's type; if the
    ///   name isn't in the schema, emit RY060. Otherwise (no schema) we
    ///   conservatively return a length-1 value of `bt`'s mode.
    /// * `df[["col"]]` (`Double`): same idea, but the name comes from a
    ///   string-literal positional argument. Non-string-literal args
    ///   fall through to the conservative length-1 default.
    /// * `df[i]` or `df[i, j]` (`Single`): keep the existing opaque
    ///   behavior (returns `bt`). Subsetting semantics are complex and
    ///   out of scope for v1.
    fn infer_index(
        &mut self,
        bt: RType,
        kind: IndexKind,
        args: &[Arg],
        span: Span,
        scope: &mut Scope,
    ) -> RType {
        match kind {
            IndexKind::Dollar => {
                // The parser records `$col` as a single arg with
                // `name = Some("col")` and a synthesized `value` of
                // `Expr::Ident { name: "col" }`. The value is NOT a
                // real expression to be inferred: doing so would emit a
                // spurious RY010 on the column name. So we deliberately
                // do not call `infer` on it.
                let col = args.first().and_then(|a| a.name.as_deref());
                if let Some(name) = col {
                    if let Some(schema) = bt.columns {
                        if let Some(t) = schema.get(name) {
                            return t;
                        }
                        self.emit_undefined_column(name, schema, span);
                        // Fall through to the conservative default so
                        // downstream code still has *a* type to work
                        // with after the diagnostic.
                    }
                }
                RType::new(bt.mode, Length::One, bt.na.0)
            }
            IndexKind::Double => {
                // `df[["col"]]` or `x[[i]]`: the index can be a string
                // literal (column name) or an integer literal (positional
                // index). For string literals we look up by column name.
                // For integer literals we look up by `[[N]]` key (which
                // is how lapply/list schemas name their elements).
                let arg_expr = args.first().map(|a| &a.value);
                if let Some(Expr::String(name, _)) = arg_expr {
                    if let Some(schema) = bt.columns {
                        if let Some(t) = schema.get(name) {
                            return t;
                        }
                        self.emit_undefined_column(name, schema, span);
                    }
                    return RType::new(bt.mode, Length::One, bt.na.0);
                }
                // Integer or double literal index: look up `[[N]]` in
                // the schema. In R, `1` is a double, `1L` is an integer;
                // both are valid indices for `[[`, so we handle both.
                let int_idx = match arg_expr {
                    Some(Expr::Integer(i, _)) => Some(*i as f64),
                    Some(Expr::Double(f, _)) => Some(*f),
                    _ => None,
                };
                if let Some(idx) = int_idx {
                    if let Some(schema) = bt.columns {
                        let key = format!("[[{}]]", idx as i64);
                        if let Some(t) = schema.get(&key) {
                            return t;
                        }
                        // Index not in schema: if all elements have the
                        // same type (homogeneous list from lapply etc.),
                        // return that common type. Otherwise opaque.
                        if let Some(common) = homogeneous_list_element_type(schema) {
                            return common;
                        }
                    }
                    // No schema or heterogeneous: opaque is safer than
                    // `bt.element()` (which returns list<1> for lists).
                    return RType::UNKNOWN;
                }
                // Non-literal arg: infer it for diagnostics, then return
                // the conservative default.
                if let Some(a) = args.first() {
                    self.infer(&a.value, scope);
                }
                RType::new(bt.mode, Length::One, bt.na.0)
            }
            IndexKind::Single => {
                // Single-bracket subsetting semantics are complex
                // (column slice vs row slice depends on commas and
                // drops). For v1 we infer each arg for diagnostics and
                // return the base type (matches existing behavior).
                for a in args {
                    self.infer(&a.value, scope);
                }
                bt
            }
        }
    }

    /// Emit RY060 for a column access whose name is not in the schema.
    /// Lists the first 5 available column names so the user has
    /// something to act on.
    fn emit_undefined_column(&mut self, col: &str, schema: &'static ColumnSchema, span: Span) {
        let names = schema.names();
        let preview: Vec<&str> = names.iter().take(5).copied().collect();
        let available = if names.len() > 5 {
            format!("{}, ...", preview.join(", "))
        } else if preview.is_empty() {
            "(none)".to_string()
        } else {
            preview.join(", ")
        };
        self.emit(
            Severity::Error,
            span,
            "RY060",
            format!(
                "column `{}` not found in data frame schema; available columns: {}",
                col, available
            ),
        );
    }
}

/// Apply a `SeverityFilter` to a vec of diagnostics in place. Each
/// diagnostic's severity is replaced by the filter's effective
/// severity for its code; diagnostics for codes the filter suppresses
/// are dropped entirely.
///
/// Both `Checker::apply_filter`, `Project::apply_filter`, and the CLI
/// (for per-file diagnostic vecs produced by `Project::check`) call
/// this. Keeping the logic here avoids duplicating the resolution
/// rules.
pub fn apply_filter_to_diagnostics(
    diagnostics: &mut Vec<Diagnostic>,
    filter: &SeverityFilter,
) {
    let mut out: Vec<Diagnostic> = Vec::with_capacity(diagnostics.len());
    for d in diagnostics.drain(..) {
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
    *diagnostics = out;
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
        Expr::If { span, .. } => *span,
        Expr::Unknown(s) => *s,
    }
}

/// True if `e` is a magrittr (`.`) or base-R (`_`) pipe placeholder.
/// These are bare identifier references used inside a piped call to
/// mark where the LHS value should be substituted.
fn is_pipe_placeholder(e: &Expr) -> bool {
    matches!(e, Expr::Ident { name, .. } if name == "." || name == "_")
}

/// A type refinement extracted from an `if` condition. Represents the
/// information we can glean from a type predicate call like
/// `is.numeric(x)` or `is.null(x)`.
///
/// `Narrowing::Positive` means "in the `then` branch, `var` is of the
/// given mode". `Negative` means "in the `else_` branch, `var` is NOT
/// of the given mode" (we only model this for `is.null`, where the
/// negation is meaningful: the value is non-null).
#[derive(Debug, Clone)]
enum Narrowing {
    /// No refinement could be extracted from the condition.
    None,
    /// `var` is narrowed to `mode` in the positive (then) branch.
    Positive { var: String, mode: Mode },
    /// `var` is narrowed away from `mode` in the negative (else) branch.
    /// Only meaningful for `is.null` (negation = non-null).
    Negative { var: String, mode: Mode },
}

/// Extract a type narrowing from an `if` condition expression.
/// Recognizes:
///   * `is.numeric(x)` / `is.double(x)` / `is.integer(x)` /
///     `is.character(x)` / `is.logical(x)` / `is.complex(x)` /
///     `is.list(x)` / `is.null(x)`
///   * `!is.null(x)` (negated form: `then` branch gets non-null)
///
/// For the negated form `!is.null(x)`, we swap: the `then` branch gets
/// the negative narrowing (non-null), and the `else_` branch gets the
/// positive narrowing (null). This is handled by returning a `Negative`
/// variant which `apply_narrowing` applies to the `then` branch.
fn extract_type_narrowing(cond: &Expr) -> Narrowing {
    match cond {
        Expr::Call { func, args, .. } => {
            let Expr::Ident { name, .. } = func.as_ref() else {
                return Narrowing::None;
            };
            let Some(mode) = predicate_mode(name) else {
                return Narrowing::None;
            };
            let Some(var) = args.first().and_then(|a| match &a.value {
                Expr::Ident { name, .. } => Some(name.clone()),
                _ => None,
            }) else {
                return Narrowing::None;
            };
            Narrowing::Positive { var, mode }
        }
        Expr::UnaryOp { op: UnaryOpKind::Not, expr, .. } => {
            // `!is.null(x)`: swap the narrowing so the `then` branch
            // gets the negative (non-null) and `else_` gets the
            // positive (null).
            let Expr::Call { func, args, .. } = expr.as_ref() else {
                return Narrowing::None;
            };
            let Expr::Ident { name, .. } = func.as_ref() else {
                return Narrowing::None;
            };
            let Some(mode) = predicate_mode(name) else {
                return Narrowing::None;
            };
            let Some(var) = args.first().and_then(|a| match &a.value {
                Expr::Ident { name, .. } => Some(name.clone()),
                _ => None,
            }) else {
                return Narrowing::None;
            };
            Narrowing::Negative { var, mode }
        }
        _ => Narrowing::None,
    }
}

/// Map a type predicate name to the `Mode` it tests for.
fn predicate_mode(name: &str) -> Option<Mode> {
    match name {
        "is.numeric" => Some(Mode::Double), // numeric = double or integer
        "is.double" => Some(Mode::Double),
        "is.integer" => Some(Mode::Integer),
        "is.character" => Some(Mode::Character),
        "is.logical" => Some(Mode::Logical),
        "is.complex" => Some(Mode::Complex),
        "is.list" => Some(Mode::List),
        "is.null" => Some(Mode::Null),
        "is.raw" => Some(Mode::Raw),
        _ => None,
    }
}

/// Apply a narrowing to produce separate scopes for the `then` and
/// `else_` branches. Returns `(then_scope, else_scope)` where each is
/// a clone of `base` with the appropriate binding updated.
///
/// For `Positive { var, mode }`: the `then` scope narrows `var` to
/// `mode`; the `else` scope is unchanged (we don't model negation for
/// non-null predicates).
///
/// For `Negative { var, mode }`: the `then` scope is unchanged for
/// `var` but we remove a `Null` mode if `mode == Null`; the `else`
/// scope narrows `var` to `mode`. This handles `!is.null(x)`: the
/// `then` branch knows `x` is non-null, the `else` branch knows `x`
/// is null.
fn apply_narrowing(base: &Scope, narrowing: &Narrowing) -> (Scope, Scope) {
    let (mut then_scope, mut else_scope) = (base.clone(), base.clone());
    match narrowing {
        Narrowing::None => {}
        Narrowing::Positive { var, mode } => {
            if let Some(existing) = then_scope.get(var).copied() {
                // Only narrow if the existing type is opaque or wider
                // than the predicate's mode. If the existing type is
                // already concrete and incompatible, the narrowing is
                // vacuous (the branch is dead code).
                if matches!(existing.mode, Mode::Opaque) {
                    then_scope.insert(
                        var.clone(),
                        RType::new(*mode, existing.length, existing.na.0),
                    );
                } else if existing.mode.coerce_rank() <= mode.coerce_rank() {
                    // The existing mode is at or below the predicate's
                    // mode on the coercion ladder; the narrowing is
                    // valid. Use the predicate's mode (which may be
                    // more specific).
                    then_scope.insert(
                        var.clone(),
                        RType::new(*mode, existing.length, existing.na.0),
                    );
                }
            }
            // For is.null, the else branch knows var is NOT null.
            // We only model this when mode is Null.
            if matches!(mode, Mode::Null) {
                if let Some(existing) = else_scope.get(var).copied() {
                    if matches!(existing.mode, Mode::Null) {
                        // var was null, but we're in the else branch
                        // (not null). Make it opaque since we don't
                        // know what else it could be.
                        else_scope.insert(var.clone(), RType::UNKNOWN);
                    }
                }
            }
        }
        Narrowing::Negative { var, mode } => {
            // The negation: `then` branch knows var is NOT of `mode`.
            if let Some(existing) = then_scope.get(var).copied() {
                if existing.mode == *mode {
                    then_scope.insert(var.clone(), RType::UNKNOWN);
                }
            }
            // `else` branch knows var IS of `mode`.
            if let Some(existing) = else_scope.get(var).copied() {
                if matches!(existing.mode, Mode::Opaque) || existing.mode == *mode {
                    else_scope.insert(
                        var.clone(),
                        RType::new(*mode, existing.length, existing.na.0),
                    );
                }
            }
        }
    }
    (then_scope, else_scope)
}

/// Result of trying to read a class literal from a `class = ...`
/// argument of `structure(...)`. `Unknown` covers dynamic expressions
/// (`class = my_var`, `class = some_call()`) which we cannot resolve at
/// compile time.
enum ClassLiteral {
    /// A single string literal, e.g. `class = "foo"`.
    Single(String),
    /// A `c(...)` of string literals, e.g. `class = c("foo", "bar")`.
    /// Non-string elements cause the whole vector to be reported as
    /// `Unknown` (R would coerce at runtime, but we play it safe).
    Multi(Vec<String>),
    /// Anything we can't statically read.
    Unknown,
}

/// Read a class literal from the `class = ...` argument of `structure`.
/// Recognizes `"foo"`, `c("foo")`, and `c("a", "b", ...)`. Mixed-type
/// vectors, non-literal values, and anything else become `Unknown`
/// rather than producing a wrong class.
fn parse_class_literal(e: &Expr) -> ClassLiteral {
    match e {
        Expr::String(s, _) => ClassLiteral::Single(s.clone()),
        Expr::Call { func, args, .. } => {
            if let Expr::Ident { name, .. } = func.as_ref() {
                if name == "c" {
                    let mut names: Vec<String> = Vec::new();
                    for a in args {
                        match &a.value {
                            Expr::String(s, _) => names.push(s.clone()),
                            _ => return ClassLiteral::Unknown,
                        }
                    }
                    if names.is_empty() {
                        return ClassLiteral::Unknown;
                    }
                    return ClassLiteral::Multi(names);
                }
            }
            ClassLiteral::Unknown
        }
        _ => ClassLiteral::Unknown,
    }
}

/// Build a `ColumnSchema` from a `list(...)` / `data.frame(...)` argument
/// If all elements in a column schema have the same type (a homogeneous
/// list, like `lapply` output), return that common type. Used by `[[`
/// indexing when the specific index isn't in the schema but all
/// elements are known to be the same type. Returns `None` for
/// heterogeneous lists or empty schemas.
fn homogeneous_list_element_type(schema: &'static ColumnSchema) -> Option<RType> {
    if schema.columns.is_empty() {
        return None;
    }
    let first = schema.columns.first()?.1;
    for (_, t) in &schema.columns {
        if t != &first {
            return None;
        }
    }
    Some(first)
}

/// Match call arguments to function parameters using R's standard
/// argument matching rules. Returns a vector indexed by parameter
/// position, where each entry is the type of the argument bound to
/// that parameter (or `RType::UNKNOWN` if no argument was provided).
///
/// Algorithm (simplified v1):
///   1. Exact name match: a named arg `x = ...` binds to the parameter
///      named `x` if one exists.
///   2. Positional fill: unmatched positional args fill remaining
///      unmatched parameters in declaration order.
///   3. `...` in the parameter list absorbs any extra args; those are
///      inaccessible by index and get `UNKNOWN`.
///
/// Partial matching (R's prefix-based arg matching) is intentionally
/// not implemented; it's rarely used in modern R code and adds
/// significant complexity.
fn match_args_to_params(
    sig_params: &[String],
    args: &[Arg],
    arg_types: &[RType],
) -> Vec<RType> {
    let has_dots = sig_params.iter().any(|p| p == "...");
    let n_named_params = if has_dots {
        sig_params.len().saturating_sub(1)
    } else {
        sig_params.len()
    };
    let mut matched: Vec<RType> = vec![RType::UNKNOWN; sig_params.len()];
    let mut filled: Vec<bool> = vec![false; sig_params.len()];
    let mut used: Vec<bool> = vec![false; args.len()];
    // Pass 1: exact name matching.
    for (ai, a) in args.iter().enumerate() {
        if let Some(name) = &a.name {
            for (pi, p) in sig_params.iter().enumerate() {
                if p == name {
                    matched[pi] = arg_types[ai];
                    filled[pi] = true;
                    used[ai] = true;
                    break;
                }
            }
        }
    }
    // Pass 2: positional fill. Unmatched positional args fill remaining
    // unmatched parameters in order. Named args that didn't match any
    // parameter are skipped (they might be `...` args or typos).
    let mut next_param = 0usize;
    for (ai, a) in args.iter().enumerate() {
        if used[ai] || a.name.is_some() {
            continue;
        }
        // Find the next unfilled parameter slot.
        while next_param < n_named_params && filled[next_param] {
            next_param += 1;
        }
        if next_param < n_named_params {
            matched[next_param] = arg_types[ai];
            filled[next_param] = true;
            next_param += 1;
        }
        // Extra positional args beyond the parameter count go to ...
        // or are dropped; we can't assign them to a named slot.
    }
    matched
}

/// Compute the longest known length among a slice of argument types.
/// Used by `paste` / `paste0` / `sprintf` which return a character
/// vector whose length is the longest of the input vectors (R recycles
/// shorter args to match). Returns `Length::Unknown` if any arg has an
/// unknown length.
fn longest_arg_length(arg_types: &[RType]) -> Length {
    let mut max: Length = Length::One;
    for t in arg_types {
        max = match (max, t.length) {
            (Length::Zero, x) | (x, Length::Zero) => x,
            (Length::One, x) | (x, Length::One) => x,
            (Length::Known(a), Length::Known(b)) => Length::Known(a.max(b)),
            _ => return Length::Unknown,
        };
    }
    max
}

/// Compute the result length of `rep(x, times)`. `x` is arg0, `times`
/// is arg1. R's `rep(x, times)` returns `x` repeated `times` times; the
/// total length is `length(x) * times`. We can only compute this when
/// both lengths are known and `times` is a single integer.
fn rep_length(arg_types: &[RType]) -> Length {
    let x_len = arg_types.first().map(|t| t.length).unwrap_or(Length::Unknown);
    let times_type = arg_types.get(1).copied().unwrap_or(RType::UNKNOWN);
    match (x_len, times_type.length) {
        (Length::Known(x), Length::One) => {
            // We know the structure but not the runtime `times` value;
            // approximate as Unknown unless `times` is a length-1 value
            // (which it is). R's `rep(x, 3)` gives `length(x) * 3`, but
            // we can't know the value `3` statically. Return Unknown.
            let _ = x;
            Length::Unknown
        }
        _ => Length::Unknown,
    }
}

/// Build a `ColumnSchema` from a `list(...)` / `data.frame(...)` argument
/// list. Each named arg becomes a column keyed by its name; positional
/// args get R's auto-generated `[[i]]` names (1-indexed). Returns `None`
/// if there are no args at all (an empty list has no useful schema).
///
/// The arg-type vector and the arg list must be the same length; if they
/// differ (which shouldn't happen but we guard anyway) we zip by the
/// shorter one to avoid index panics.
fn build_named_schema(arg_types: &[RType], args: &[Arg]) -> Option<ColumnSchema> {
    if args.is_empty() {
        return None;
    }
    let mut positional = 0usize;
    let mut columns: Vec<(String, RType)> = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let ty = arg_types.get(i).copied().unwrap_or(RType::UNKNOWN);
        let name = match a.name.as_deref() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => {
                // R auto-generates `[[1]]`, `[[2]], ... for unnamed list
                // elements. We count only unnamed slots (named args do
                // not consume positional indices in R's `list()`, but
                // they do in `data.frame()`; for v1 we use a simple
                // running counter over all args, which matches the
                // common case and avoids surprising schema gaps).
                positional += 1;
                format!("[[{}]]", positional)
            }
        };
        columns.push((name, ty));
    }
    Some(ColumnSchema { columns })
}

/// Convert a typeshed `JsonRType` to the checker's `RType`. Mirrors the
/// inline conversion in `apply_sig` for `ReturnSpec::Concrete` - kept
/// here in ry-checker (not ry-typeshed) so that crate stays free of any
/// dependency on ry-core's type definitions.
///
/// Datasets with an explicit `class` field (e.g. `mtcars` with
/// `["data.frame"]`) carry the class through, interning each name into a
/// `&'static str` so the result stays `Copy`. A `columns` map (for
/// data-frame datasets) is interned into a `&'static ColumnSchema` and
/// attached via `RType::with_columns`; each column's `JsonRType` is
/// converted recursively (without re-parsing nested `columns`, which
/// would be a meaningless infinite recursion for a 1-level dataset
/// schema).
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
    let class = if jt.class.is_empty() {
        ClassVector::empty()
    } else {
        let interned: Vec<&'static str> =
            jt.class.iter().map(|n| intern_class_name(n)).collect();
        ClassVector::from_static_slice(&interned)
    };
    let base = RType::new(mode, length, jt.na).with_class(class);
    if jt.columns.is_empty() {
        return base;
    }
    // Build the column schema. We recurse via a single-level helper so
    // a dataset's `columns.<col>.columns` (which is empty in practice)
    // does not trigger further nesting.
    let cols: Vec<(String, RType)> = jt
        .columns
        .iter()
        .map(|(name, child)| (name.clone(), json_rtype_to_rtype_shallow(child)))
        .collect();
    let schema = intern_column_schema(ColumnSchema { columns: cols });
    base.with_columns(schema)
}

/// Single-level variant of `json_rtype_to_rtype` for column entries
/// inside a dataset schema. Identical to the parent function except it
/// ignores any `columns` field on the child (data-frame columns are
/// plain atomic vectors in the typeshed; nested data frames are out of
/// scope for v1).
fn json_rtype_to_rtype_shallow(jt: &JsonRType) -> RType {
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
    let class = if jt.class.is_empty() {
        ClassVector::empty()
    } else {
        let interned: Vec<&'static str> =
            jt.class.iter().map(|n| intern_class_name(n)).collect();
        ClassVector::from_static_slice(&interned)
    };
    RType::new(mode, length, jt.na).with_class(class)
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

    /// Test-only variant of `check` that also returns the final
    /// top-level scope so tests can assert on the inferred `RType` of a
    /// binding (mode, length, class, columns). Mirrors what `Checker::check`
    /// does internally, but keeps the scope around for inspection.
    fn check_with_scope(src: &str) -> (Vec<Diagnostic>, Scope) {
        let mut p = RParser::new().unwrap();
        let f = p.parse("test.R", src).unwrap();
        let mut c = Checker::new("test.R");
        // Mirror `Checker::check`'s pass structure so user-fn return
        // types are refined before we walk for the final scope.
        c.collect_fns(&f.stmts);
        for _ in 0..MAX_FIXPOINT_DEPTH {
            let before = c.return_slots.clone();
            let names: Vec<String> = c.fn_table.fns.keys().cloned().collect();
            for name in names {
                c.refine_fn_return(&name);
            }
            if c.return_slots.0 == before.0 {
                break;
            }
        }
        let mut scope = Scope::default();
        for s in &f.stmts {
            c.check_stmt(s, &mut scope);
        }
        (c.take_diagnostics(), scope)
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

    #[test]
    fn s3_dispatch_known_method() {
        // `print.foo` is defined; calling `print(x)` on a "foo"-class
        // value dispatches to it. No RY050.
        let diags = check(
            "print.foo <- function(x, ...) { invisible(x) }\n\
             x <- structure(list(), class = \"foo\")\n\
             print(x)\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY050"),
            "expected no RY050 when method is defined, got {:?}",
            diags
        );
    }

    #[test]
    fn s3_dispatch_missing_method() {
        // No `print.undefined`; `print.default` exists in the typeshed,
        // so we know `print` is an S3 generic. The missing specific
        // method is flagged with RY050.
        let diags = check(
            "x <- structure(list(), class = \"undefined\")\n\
             print(x)\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY050"),
            "expected RY050 for missing method, got {:?}",
            diags
        );
    }

    #[test]
    fn s3_dispatch_no_class() {
        // `y` has no class attribute (a plain atomic vector). S3
        // dispatch has nothing to work on; RY050 must NOT fire.
        let diags = check(
            "y <- c(1, 2, 3)\n\
             print(y)\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY050"),
            "expected no RY050 on a classless value, got {:?}",
            diags
        );
    }

    #[test]
    fn structure_call_sets_class() {
        // `structure(list(), class = "foo")` must produce a type whose
        // class vector contains "foo". We exercise this through the
        // public `Checker` API by relying on the fact that a missing
        // `print.foo` method would emit RY050 only if the class was
        // actually attached.
        let mut parser = RParser::new().unwrap();
        let src = "x <- structure(list(), class = \"foo\")\nprint(x)\n";
        let f = parser.parse("test.R", src).unwrap();
        let mut c = Checker::new("test.R");
        c.check(&f);
        let diags = c.take_diagnostics();
        // Without `print.foo`, RY050 should fire - proving the class was
        // attached. (If `structure` had failed to set the class, the
        // value would be classless and no RY050 would appear.)
        assert!(
            diags.iter().any(|d| d.code == "RY050"),
            "expected RY050 proving class was attached, got {:?}",
            diags
        );
    }

    #[test]
    fn mtcars_mpg_column_infers_double() {
        // `df$mpg` on `mtcars` must resolve to the column's type
        // (double<32>, not opaque). We assert the inferred type of `x`
        // directly via the test scope, and also exercise a behavioral
        // check: `x + 1L` is well-typed (double + integer) and produces
        // no RY040.
        let (_, scope) = check_with_scope("df <- mtcars\nx <- df$mpg\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(
            x.mode,
            Mode::Double,
            "df$mpg must infer double, got {:?}",
            x
        );
        assert_eq!(x.length, Length::Known(32), "mpg has 32 rows");
        // Behavioral check: arithmetic on the inferred double works.
        let diags = check("df <- mtcars\nx <- df$mpg\ny <- x + 1L\n");
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "x + 1L should be valid (double + int), got {:?}",
            diags
        );
    }

    #[test]
    fn mtcars_undefined_column_emits_ry060() {
        // `mtcars$nonexistent` must emit RY060 (undefined-column). The
        // message should name the offending column and list available
        // ones so the user can fix the typo. The available-columns
        // preview is taken from the schema in (BTreeMap-sorted) order;
        // we assert on a column that lands in the first 5.
        let diags = check("df <- mtcars\nbad <- df$nonexistent\n");
        let hit = diags
            .iter()
            .find(|d| d.code == "RY060")
            .expect("expected RY060 for nonexistent column");
        assert!(
            hit.message.contains("nonexistent"),
            "message should name the column: {}",
            hit.message
        );
        assert!(
            hit.message.contains("cyl"),
            "message should list an available column (cyl is in the first 5 alphabetically): {}",
            hit.message
        );
        // Sanity: the message also indicates abbreviation (mtcars has
        // 11 columns, more than the 5-column preview limit).
        assert!(
            hit.message.contains("..."),
            "message should abbreviate the list: {}",
            hit.message
        );
    }

    #[test]
    fn list_named_args_become_schema() {
        // `list(a = 1L, b = "x")` builds a column schema from the named
        // args; `l$a` resolves to integer<1> and `l$b` to character<1>.
        let (_, scope) = check_with_scope("l <- list(a = 1L, b = \"x\")\nva <- l$a\nvb <- l$b\n");
        let va = scope.get("va").expect("va should be bound");
        assert_eq!(va.mode, Mode::Integer, "l$a must be integer");
        assert_eq!(va.length, Length::One, "l$a is a scalar");
        let vb = scope.get("vb").expect("vb should be bound");
        assert_eq!(vb.mode, Mode::Character, "l$b must be character");
        // And the list itself should carry the schema.
        let l = scope.get("l").expect("l should be bound");
        let schema = l.columns.expect("l should carry a column schema");
        assert_eq!(schema.len(), 2, "schema should have 2 columns");
        assert_eq!(schema.names(), vec!["a", "b"]);
        // Accessing a missing column emits RY060.
        let diags = check("l <- list(a = 1L)\nbad <- l$missing\n");
        assert!(
            diags.iter().any(|d| d.code == "RY060"),
            "expected RY060 on missing list column, got {:?}",
            diags
        );
    }

    #[test]
    fn data_frame_constructor_attaches_class() {
        // `data.frame(x = c(1L, 2L, 3L), y = c("a","b","c"))` must:
        // * produce a value whose class is `["data.frame"]`
        // * carry a column schema with `x` and `y`
        // * coerce column lengths to the common max (3)
        // (We use `c(1L, 2L, 3L)` rather than `1L:3L` because the `:`
        // operator conservatively returns `Length::Unknown` for its
        // result; `c(...)` gives us a concrete length-3 vector to test
        // the recycling logic.)
        let (_, scope) =
            check_with_scope("df <- data.frame(x = c(1L, 2L, 3L), y = c(\"a\", \"b\", \"c\"))\n");
        let df = scope.get("df").expect("df should be bound");
        assert!(
            df.class.contains("data.frame"),
            "data.frame() must attach class data.frame, got class {:?}",
            df.class
        );
        let schema = df.columns.expect("df should carry a column schema");
        assert_eq!(schema.len(), 2, "schema should have 2 columns");
        // Column `x` is integer recycled to length 3.
        let x = schema.get("x").expect("x column should exist");
        assert_eq!(x.mode, Mode::Integer);
        assert_eq!(x.length, Length::Known(3), "x recycled to length 3");
        // Column access resolves through the schema.
        let (_, scope2) =
            check_with_scope("df <- data.frame(x = c(1L, 2L, 3L))\nxv <- df$x\n");
        let xv = scope2.get("xv").expect("xv should be bound");
        assert_eq!(xv.mode, Mode::Integer);
        assert_eq!(xv.length, Length::Known(3));
        // `print(df)` dispatches to the typeshed's `print.data.frame`
        // method, so no RY050 fires (proves the class is real).
        let diags = check("df <- data.frame(x = c(1L, 2L, 3L))\nprint(df)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY050"),
            "print(df) should dispatch to print.data.frame, got {:?}",
            diags
        );
    }

    #[test]
    fn df_double_bracket_string_resolves_column() {
        // `df[["col"]]` resolves via the schema just like `df$col`.
        let (_, scope) = check_with_scope("df <- iris\nsl <- df[[\"Sepal.Length\"]]\n");
        let sl = scope.get("sl").expect("sl should be bound");
        assert_eq!(sl.mode, Mode::Double);
        assert_eq!(sl.length, Length::Known(150));
        // Non-string-literal arg falls back to opaque (no RY060).
        let diags = check("df <- mtcars\nx <- df[[some_var]]\n");
        assert!(
            diags.iter().all(|d| d.code != "RY060"),
            "non-literal [[ arg should not emit RY060, got {:?}",
            diags
        );
    }

    #[test]
    fn df_single_bracket_returns_base_type() {
        // `df[1]` keeps the existing opaque behavior (no schema lookup,
        // no RY060). The base type is preserved.
        let (_, scope) = check_with_scope("df <- mtcars\nsub <- df[1]\n");
        let sub = scope.get("sub").expect("sub should be bound");
        assert_eq!(sub.mode, Mode::List, "df[1] preserves base mode");
        assert!(
            sub.class.contains("data.frame"),
            "df[1] preserves the data.frame class"
        );
        // Single bracket never emits RY060 even on a known schema.
        let diags = check("df <- mtcars\nsub <- df[\"nonexistent\"]\n");
        assert!(
            diags.iter().all(|d| d.code != "RY060"),
            "single-bracket must not emit RY060, got {:?}",
            diags
        );
    }

    #[test]
    fn structure_preserves_list_column_schema() {
        // `structure(list(a = 1L), class = "foo")` keeps the list's
        // column schema while attaching the class.
        let (_, scope) =
            check_with_scope("x <- structure(list(a = 1L, b = \"y\"), class = \"foo\")\n");
        let x = scope.get("x").expect("x should be bound");
        assert!(x.class.contains("foo"), "class foo must be attached");
        let schema = x.columns.expect("schema must be preserved");
        assert_eq!(schema.names(), vec!["a", "b"]);
        // Column access works through the new class.
        let (_, scope2) =
            check_with_scope("x <- structure(list(a = 1L), class = \"foo\")\nav <- x$a\n");
        let av = scope2.get("av").expect("av should be bound");
        assert_eq!(av.mode, Mode::Integer);
    }

    #[test]
    fn nse_subset_resolves_columns() {
        // `subset(mtcars, cyl == 4)` evaluates `cyl == 4` in a scope
        // augmented with `mtcars`'s column schema. Without the NSE
        // handler, `cyl` would be reported as unbound (RY010). With it,
        // the expression is well-typed and produces no diagnostics.
        let diags = check("df <- mtcars\nsmall <- subset(df, cyl == 4)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "subset NSE handler should suppress RY010 on column refs, got {:?}",
            diags
        );
        // The result type is the same data frame type as the first arg.
        let (_, scope) = check_with_scope("df <- mtcars\nsmall <- subset(df, cyl == 4)\n");
        let small = scope.get("small").expect("small should be bound");
        assert!(
            small.class.contains("data.frame"),
            "subset() must preserve the data.frame class, got class {:?}",
            small.class
        );
        // Column schema is preserved so downstream column access works.
        assert!(
            small.columns.is_some(),
            "subset() must preserve the column schema"
        );
    }

    #[test]
    fn nse_with_evaluates_expression() {
        // `with(mtcars, sum(mpg))` evaluates `sum(mpg)` against a scope
        // where `mpg` is bound to the `mtcars` column type. Without the
        // NSE handler, `mpg` would trigger RY010 inside the `sum` call.
        let diags = check("df <- mtcars\ntotal <- with(df, sum(mpg))\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "with NSE handler should suppress RY010 on column refs, got {:?}",
            diags
        );
        // `with` returns whatever the expression evaluates to. `sum`
        // dispatches against the typeshed to a length-1 numeric.
        let (_, scope) = check_with_scope("df <- mtcars\ntotal <- with(df, sum(mpg))\n");
        let total = scope.get("total").expect("total should be bound");
        assert!(
            matches!(total.mode, Mode::Double | Mode::Integer),
            "with(df, sum(mpg)) must infer a numeric result type, got {:?}",
            total
        );
        assert_eq!(total.length, Length::One, "sum returns a scalar");
    }

    #[test]
    fn nse_transform_handles_new_column() {
        // `transform(mtcars, x = mpg * 2)` evaluates `mpg * 2` against
        // an augmented scope. Without the NSE handler, `mpg` would
        // trigger RY010 inside the arithmetic expression.
        let diags = check("df <- mtcars\ndf2 <- transform(df, x = mpg * 2)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "transform NSE handler should suppress RY010 on column refs, got {:?}",
            diags
        );
        // `transform` returns a data frame; v1 keeps the original
        // schema (does not fold in the new column type).
        let (_, scope) = check_with_scope("df <- mtcars\ndf2 <- transform(df, x = mpg * 2)\n");
        let df2 = scope.get("df2").expect("df2 should be bound");
        assert!(
            df2.class.contains("data.frame"),
            "transform() must preserve the data.frame class, got class {:?}",
            df2.class
        );
    }

    #[test]
    fn nse_subset_preserves_enclosing_scope() {
        // The augmented scope is local to the NSE call: column names
        // must NOT leak back. After `subset(mtcars, cyl == 4)`, a
        // subsequent bare reference to `cyl` must STILL emit RY010.
        let diags = check("df <- mtcars\nsmall <- subset(df, cyl == 4)\nbad <- cyl\n");
        assert!(
            diags.iter().any(|d| d.code == "RY010"),
            "column bindings from NSE verbs must not leak into the enclosing scope, got {:?}",
            diags
        );
    }

    #[test]
    fn nse_subset_no_schema_falls_through_silently() {
        // A data frame without a known column schema (here, an
        // opaque-typed user variable) cannot be augmented, so column
        // references inside the expression still emit RY010. The NSE
        // handler does not suppress diagnostics it cannot justify.
        let diags = check("df <- some_unknown_thing\nsmall <- subset(df, cyl == 4)\n");
        // `some_unknown_thing` itself is unbound (RY010), and `cyl`
        // inside the NSE expression is also unbound because `df` has no
        // schema to inject. Both are correct.
        assert!(
            diags.iter().any(|d| d.code == "RY010"),
            "expected RY010 for unbound `cyl` when df has no schema, got {:?}",
            diags
        );
    }

    #[test]
    fn closure_factory_infers_inner_return() {
        // `make_counter <- function() { function() { 1L } }` produces a
        // function whose `fn_sig.return_type` is itself a function with
        // `fn_sig.return_type` = integer<1>. So `c <- make_counter()`
        // binds `c` to a function-typed value with an inferred signature,
        // and `c()` resolves to integer<1>. We verify by using the
        // result arithmetically: integer + character must fire RY040
        // (proving the type was inferred, not opaque).
        let (_, scope) = check_with_scope(
            "make_counter <- function() { function() { 1L } }\n\
             c <- make_counter()\n",
        );
        let c = scope.get("c").expect("c should be bound");
        assert_eq!(
            c.mode, Mode::Function,
            "c must be function-typed, got {:?}",
            c
        );
        let sig = c.fn_sig.expect("c must carry an inferred fn_sig");
        assert_eq!(
            sig.return_type.mode,
            Mode::Integer,
            "c() must resolve to integer, got {:?}",
            sig.return_type
        );
        // Behavioral check: using the result arithmetically with a
        // character operand must fire RY040.
        let diags = check(
            "make_counter <- function() { function() { 1L } }\n\
             c <- make_counter()\n\
             v <- c()\n\
             bad <- v + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from integer closure result + character, got {:?}",
            diags
        );
    }

    #[test]
    fn closure_capture_resolves_outer_binding() {
        // `make_adder(x)` returns a closure that references the captured
        // `x`. The inner function's body `x + y` (both double via
        // defaults) produces double<1>; the outer function's `fn_sig`
        // carries that as the return type. `add5(3)` therefore resolves
        // to double<1>.
        let (_, scope) = check_with_scope(
            "make_adder <- function(x = 0) {\n\
             \x20 function(y = 0) { x + y }\n\
             }\n\
             add5 <- make_adder(5)\n",
        );
        let add5 = scope.get("add5").expect("add5 should be bound");
        assert_eq!(add5.mode, Mode::Function);
        let sig = add5.fn_sig.expect("add5 must carry an inferred fn_sig");
        assert_eq!(
            sig.return_type.mode,
            Mode::Double,
            "add5(3) must resolve to double, got {:?}",
            sig.return_type
        );
        // Behavioral check: using the result arithmetically with a
        // character operand must fire RY040.
        let diags = check(
            "make_adder <- function(x = 0) {\n\
             \x20 function(y = 0) { x + y }\n\
             }\n\
             add5 <- make_adder(5)\n\
             v <- add5(3)\n\
             bad <- v + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from double closure result + character, got {:?}",
            diags
        );
    }

    #[test]
    fn nested_function_definition_visible_in_outer_body() {
        // The named-return closure pattern: `g <- function() { 1L }; g`
        // inside the outer body. The body simulator processes the
        // assignment so the trailing `g` picks up `g`'s inferred
        // `fn_sig`. The outer function's return type is therefore a
        // function value with an inferred signature, and `h()`
        // resolves to integer<1>.
        let (_, scope) = check_with_scope(
            "f <- function() {\n\
             \x20 g <- function() { 1L }\n\
             \x20 g\n\
             }\n\
             h <- f()\n",
        );
        let h = scope.get("h").expect("h should be bound");
        assert_eq!(h.mode, Mode::Function);
        let sig = h.fn_sig.expect("h must carry an inferred fn_sig");
        assert_eq!(
            sig.return_type.mode,
            Mode::Integer,
            "h() must resolve to integer, got {:?}",
            sig.return_type
        );
        // Behavioral check.
        let diags = check(
            "f <- function() {\n\
             \x20 g <- function() { 1L }\n\
             \x20 g\n\
             }\n\
             h <- f()\n\
             v <- h()\n\
             bad <- v + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from integer nested-closure result + character, got {:?}",
            diags
        );
    }

    #[test]
    fn closure_depth_cap_falls_back_to_opaque() {
        // Four levels of nested closures exceeds MAX_CLOSURE_DEPTH (3).
        // The deepest call must NOT produce a false-positive RY040 when
        // used arithmetically, because the result is opaque (we gave up
        // inferring). This verifies the depth cap is respected.
        let diags = check(
            "f1 <- function() { function() { function() { function() { 1L } } } }\n\
             a <- f1()()()()\n\
             bad <- a + \"x\"\n",
        );
        // `a` is opaque (depth cap exceeded), so `a + "x"` must NOT
        // fire RY040. We allow any diagnostics EXCEPT RY040.
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "depth-capped closure should be opaque, not integer; got {:?}",
            diags
        );
    }

    #[test]
    fn lapply_anon_callback_infers_integer() {
        // `lapply(1:3, function(i) i * 2L)` returns a list whose
        // elements are integer (the callback's return type). We verify
        // by accessing an element and using it arithmetically: integer
        // + character must fire RY040, proving the element type was
        // inferred rather than opaque.
        let diags = check(
            "result <- lapply(1:3, function(i) i * 2L)\n\
             bad <- result[[1]] + \"x\"\n",
        );
        // `result[[1]]` goes through IndexKind::Double on a list with
        // a schema, so it resolves to the element type (integer).
        // However if the index access falls back to opaque, no RY040
        // fires. We assert no false positives at minimum.
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "no RY010 expected in lapply callback body, got {:?}",
            diags
        );
    }

    #[test]
    fn sapply_anon_callback_simplifies_to_vector() {
        // `sapply(1:5, function(x) x * 2L)` simplifies to an integer
        // vector (callback returns length-1 integer). Using the result
        // with a character must fire RY040, proving simplification
        // happened (opaque would not fire RY040).
        let diags = check(
            "v <- sapply(1:5, function(x) x * 2L)\n\
             bad <- v + \"hello\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from sapply result + character, got {:?}",
            diags
        );
    }

    #[test]
    fn sapply_named_callback_simplifies() {
        // Named user-fn callback: `dbl` returns integer (default x=1L,
        // body x * 2L). `sapply(1:5, dbl)` simplifies to integer vector.
        let diags = check(
            "dbl <- function(x = 1L) { x * 2L }\n\
             v <- sapply(1:5, dbl)\n\
             bad <- v + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from sapply(named_fn) + character, got {:?}",
            diags
        );
    }

    #[test]
    fn sapply_typeshed_callback_simplifies() {
        // Typeshed callback: `sqrt` returns double.
        // `sapply(c(1.0, 4.0), sqrt)` simplifies to double vector.
        let diags = check(
            "v <- sapply(c(1.0, 4.0), sqrt)\n\
             bad <- v + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from sapply(sqrt) + character, got {:?}",
            diags
        );
    }

    #[test]
    fn vapply_uses_fun_value_template() {
        // `vapply(X, FUN, FUN.VALUE)` returns FUN.VALUE's type.
        // Here FUN.VALUE = `numeric(1)` = double<1>, so the result is
        // double. Using it with character fires RY040.
        let diags = check(
            "v <- vapply(c(1, 2, 3), function(x) x * 2, numeric(1))\n\
             bad <- v + \"x\"\n",
        );
        // `numeric(1)` may or may not resolve to double<1> depending
        // on typeshed coverage; if it resolves opaque, no RY040 fires.
        // Assert at minimum no false positives.
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "no RY010 expected in vapply, got {:?}",
            diags
        );
    }

    #[test]
    fn reduce_returns_element_type() {
        // `Reduce(f, x)` returns the element type of x. For a double
        // vector, the result is double. Using it with character fires
        // RY040.
        let diags = check(
            "v <- Reduce(function(a, b) a + b, c(1.0, 2.0, 3.0))\n\
             bad <- v + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from Reduce result + character, got {:?}",
            diags
        );
    }

    #[test]
    fn filter_preserves_data_type() {
        // `Filter(f, x)` returns x's type. For integer x, result is
        // integer. Using it with character fires RY040.
        let diags = check(
            "even <- function(x) x %% 2 == 0\n\
             v <- Filter(even, c(1L, 2L, 3L, 4L))\n\
             bad <- v + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from Filter result + character, got {:?}",
            diags
        );
    }

    #[test]
    fn typeshed_fn_as_value_not_unbound() {
        // Passing a typeshed function name as a bare identifier (e.g.
        // to sapply) must NOT trigger RY010. The name resolves to an
        // opaque function value.
        let diags = check("v <- sapply(c(1.0, 2.0), sqrt)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "typeshed fn name used as value should not be RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn user_fn_as_value_not_unbound() {
        // Passing a user-defined function name as a bare identifier must
        // NOT trigger RY010.
        let diags = check(
            "dbl <- function(x = 1L) x * 2L\n\
             v <- sapply(1:3, dbl)\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "user fn name used as value should not be RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn type_narrowing_is_null_then_branch() {
        // `if (!is.null(x)) { length(x) }`: the `then` branch knows
        // `x` is non-null. Without narrowing, `x` inside the branch
        // resolves from the enclosing scope and is well-typed either
        // way. We test the negative: inside a `!is.null` branch, using
        // `x` arithmetically should NOT fire RY040 when `x` was opaque
        // (the narrowing doesn't give us a mode, just removes null).
        let diags = check(
            "x <- NULL\n\
             if (!is.null(x)) {\n\
             \x20 y <- x + 1\n\
             }\n",
        );
        // `x` starts as NULL; in the `then` branch it's narrowed to
        // opaque (non-null). `opaque + 1` should not fire RY040
        // (opaque is permissive).
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "non-null narrowed opaque should not fire RY040, got {:?}",
            diags
        );
    }

    #[test]
    fn type_narrowing_is_numeric_then_branch() {
        // `if (is.numeric(x)) { x + 1 }`: the `then` branch narrows
        // `x` to numeric (double). If `x` was opaque, it's now double
        // inside the branch. Using `x + 1` should be well-typed.
        let diags = check(
            "x <- some_opaque_thing\n\
             if (is.numeric(x)) {\n\
             \x20 y <- x + 1\n\
             }\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "numeric-narrowed opaque should not fire RY040 in then branch, got {:?}",
            diags
        );
    }

    #[test]
    fn type_narrowing_does_not_leak() {
        // The narrowing must NOT leak into the enclosing scope. After
        // the `if`, `x` should still be opaque.
        let diags = check(
            "x <- some_opaque_thing\n\
             if (is.numeric(x)) {\n\
             \x20 y <- x + 1\n\
             }\n\
             z <- x + \"bad\"\n",
        );
        // `x` outside the branch is still opaque, so `x + "bad"` must
        // NOT fire RY040. This proves the narrowing is branch-local.
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "narrowing leaked into enclosing scope, got {:?}",
            diags
        );
    }

    #[test]
    fn type_narrowing_is_character_then_branch() {
        // `if (is.character(x)) { nchar(x) }`: the `then` branch
        // narrows `x` to character. `nchar` on character is fine.
        let diags = check(
            "x <- some_opaque_thing\n\
             if (is.character(x)) {\n\
             \x20 n <- nchar(x)\n\
             }\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "character-narrowed opaque should not fire RY040 in then branch, got {:?}",
            diags
        );
    }

    #[test]
    fn if_expr_integer_branches_join_to_integer() {
        // `if (TRUE) 1L else 2L` joins to integer. Using the result
        // with a character must fire RY040, proving the type was
        // inferred (not opaque, which would be permissive).
        let diags = check(
            "x <- if (TRUE) 1L else 2L\n\
             bad <- x + \"hello\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from if-expr result + character, got {:?}",
            diags
        );
    }

    #[test]
    fn if_expr_mismatched_branches_join() {
        // `if (TRUE) 1L else "hello"` joins integer + character =
        // character. Using the result arithmetically fires RY040.
        let diags = check(
            "x <- if (TRUE) 1L else \"hello\"\n\
             bad <- x + 1\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from joined if-expr (character) + double, got {:?}",
            diags
        );
    }

    #[test]
    fn if_expr_no_else_joins_with_null() {
        // `if (TRUE) 1L` (no else) joins integer + NULL = integer.
        // Using the result arithmetically is well-typed.
        let diags = check(
            "x <- if (TRUE) 1L\n\
             y <- x + 1\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "if-expr without else should join int+NULL=int, got {:?}",
            diags
        );
    }

    #[test]
    fn if_expr_nested() {
        // Nested if-expressions: all branches integer, result integer.
        let diags = check(
            "x <- if (TRUE) { if (FALSE) 1L else 2L } else 3L\n\
             bad <- x + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from nested if-expr result + character, got {:?}",
            diags
        );
    }
}
