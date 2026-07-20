//! Local type inference + diagnostics.
//!
//! v1 scope: single-file, inference-only, NSE-opaque. We walk statements
//! top-down, maintaining a per-scope binding table `name -> RType`.
//!
//! v2 additions: interprocedural function-return inference via a
//! module-level FnTable and a fixpoint loop. The first pass collects
//! function definitions; subsequent passes refine each function's
//! inferred return type until stable (or the depth cap is hit).

#![allow(clippy::collapsible_if)]

mod collect;
pub mod diagnostics;
pub mod format;
mod higher_order;
mod infer;
mod nse;
pub mod packages;
pub mod project;
pub mod rules;
mod suppress;

// Re-export `Project` at the crate root so callers (the CLI, integration
// tests) can write `ry_checker::Project` rather than
// `ry_checker::project::Project`. Mirrors the ergonomics of `Checker`.
pub use project::Project;
// Re-export the diagnostic data types and suppression helpers at the
// crate root for back-compat (callers and tests reference
// `ry_checker::{Severity, Diagnostic, ...}` directly).
pub use diagnostics::{
    Confidence, Diagnostic, Severity, SeverityFilter, Suppression, apply_filter_to_diagnostics,
    filter_suppressed, filter_suppressed_with_comments, has_file_suppression,
    has_file_suppression_from_comments, is_suppressed, parse_suppressions,
    parse_suppressions_from_comments,
};

use crate::infer::semantic_argument_name;
use ry_core::Span;
use ry_core::ast::*;
use ry_core::types::{ClassVector, ColumnSchema, FunctionSignature, Length, Mode, RType};
use ry_typeshed::{
    CallbackArg, EvalMode, FunctionSig, Globals, HigherOrderResultKind, HigherOrderSpec,
    JsonLength, JsonMode, JsonRType, ParamSpec, ReturnSlot, ReturnSpec, SchemaEffect, ScopeEffect,
    Typeshed, is_known_package, known_packages, load_base_cached, load_package,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// Metadata marker for a serialized workspace too large to enumerate safely.
/// This is deliberately not an R identifier; package metadata passes it through
/// the ordinary external-bindings channel so pass 3 can open the file scope.
pub const SERIALIZED_BINDINGS_UNENUMERABLE: &str = "\0serialized:unenumerable";

fn string_literals(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::String(value, _) => vec![value.clone()],
        Expr::Call { func, args, .. } => {
            let Some(name) = ident_name(func) else {
                return Vec::new();
            };
            let bare = name.rsplit_once("::").map(|(_, n)| n).unwrap_or(name);
            if bare != "c" {
                return Vec::new();
            }
            args.iter()
                .flat_map(|arg| string_literals(&arg.value))
                .collect()
        }
        _ => Vec::new(),
    }
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident { name, .. } => Some(name),
        _ => None,
    }
}

fn binding_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident { name, .. } | Expr::String(name, _) => Some(name),
        _ => None,
    }
}

fn is_na_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Na(_, _))
}

fn non_divisible_recycling(lhs: Length, rhs: Length) -> Option<(usize, usize)> {
    let known = |length| match length {
        Length::One => Some(1),
        Length::Known(n) => Some(n),
        Length::Zero | Length::Unknown => None,
    };
    let (a, b) = (known(lhs)?, known(rhs)?);
    if a > 1 && b > 1 && a.max(b) % a.min(b) != 0 {
        Some((a, b))
    } else {
        None
    }
}

fn assigned_column_name(kind: IndexKind, args: &[Arg]) -> Option<&str> {
    match kind {
        IndexKind::Dollar => args.first().and_then(|arg| arg.name.as_deref()),
        IndexKind::Double => match args.first().map(|arg| &arg.value) {
            Some(Expr::String(name, _)) => Some(name.as_str()),
            _ => None,
        },
        IndexKind::Single => None,
    }
}

fn type_with_assigned_column(mut base: RType, name: &str, value: RType) -> RType {
    let mut schema = base
        .columns
        .as_ref()
        .map(|schema| (**schema).clone())
        .unwrap_or_default();
    if let Some((_, existing)) = schema.columns.iter_mut().find(|(col, _)| col == name) {
        *existing = value;
    } else {
        schema.columns.push((name.to_string(), value));
    }
    if matches!(base.mode, Mode::Null) {
        base.mode = Mode::List;
    }
    base.with_columns(Arc::new(schema))
}

/// Returns `Some((generic, class))` if `name` matches the S3 method
/// naming convention `<generic>.<class>` and `<generic>` is in the
/// curated stub-data generic table. Longest match wins (handles rare
/// multi-segment cases).
fn split_s3_method_name(name: &str, globals: &Globals) -> Option<(String, String)> {
    if globals
        .s3_split_denylist
        .iter()
        .any(|denied| denied == name)
    {
        return None;
    }
    let mut best: Option<(String, String)> = None;
    for generic in &globals.s3_generics {
        if let Some(class) = name
            .strip_prefix(generic)
            .and_then(|rest| rest.strip_prefix('.'))
        {
            if class.is_empty() {
                continue;
            }
            // Prefer the longest matching prefix (more specific).
            let is_better = best.as_ref().is_none_or(|(g, _)| g.len() < generic.len());
            if is_better {
                best = Some((generic.clone(), class.to_string()));
            }
        }
    }
    best
}

/// Return the dispatch name for the deliberately small S3-generic shape we
/// can reason about without executing arbitrary setup code.
fn usemethod_generic_name(body: &[Stmt]) -> Option<String> {
    let [Stmt::Expr(Expr::Call { func, args, .. })] = body else {
        return None;
    };
    let Expr::Ident { name, .. } = func.as_ref() else {
        return None;
    };
    if name != "UseMethod" {
        return None;
    }
    match args.first().map(|argument| &argument.value) {
        Some(Expr::String(generic, _)) => Some(generic.clone()),
        _ => None,
    }
}

/// Split operator-specific S3 methods such as `+.widget`. These cannot use
/// the dotted-generic helper because the generic itself is punctuation.
fn split_s3_operator_method_name(name: &str) -> Option<(&'static str, String)> {
    const OPERATORS: &[&str] = &[
        "+", "-", "*", "/", "^", "%%", "%/%", "==", "!=", "<", "<=", ">", ">=",
    ];
    OPERATORS.iter().find_map(|operator| {
        name.strip_prefix(operator)
            .and_then(|rest| rest.strip_prefix('.'))
            .filter(|class| !class.is_empty())
            .map(|class| (*operator, class.to_string()))
    })
}

struct EnvironmentProfile {
    bindings: &'static [&'static str],
    path_trigger: fn(&str) -> bool,
}

// One built-in profile: Shiny application fragments. User-defined profiles
// (named, path-glob-triggered) come from `ry.toml` `[[environments]]` and are
// threaded through the CLI config instead.
const BUILTIN_ENVIRONMENTS: &[EnvironmentProfile] = &[EnvironmentProfile {
    bindings: &["input", "output", "session"],
    path_trigger: is_shiny_app_fragment_path,
}];

/// Whether a file is plausibly sourced into a Shiny application server.
fn is_shiny_app_fragment_path(path: &str) -> bool {
    use std::path::Path;

    let path = Path::new(path);
    if path.components().any(|component| {
        component.as_os_str().to_str().is_some_and(|name| {
            name.eq_ignore_ascii_case("shiny") || name.eq_ignore_ascii_case("shinyapp")
        })
    }) {
        return true;
    }

    path.parent().is_some_and(|parent| {
        parent.ancestors().any(|directory| {
            ["app.R", "server.R", "ui.R"]
                .iter()
                .any(|entry| directory.join(entry).is_file())
        })
    })
}

/// A single scope's binding table.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub bindings: HashMap<String, RType>,
    /// Names whose current binding was installed by flow narrowing rather
    /// than an R assignment. `insert` clears this marker, so branch merging
    /// can distinguish a temporary refinement from a rebinding.
    pub(crate) narrowed_bindings: HashSet<String>,
    /// Bindings whose current type came from a function parameter default.
    /// A default is one call shape, not a complete declaration of the
    /// parameter's runtime type, so an explicit `is.*()` guard may replace
    /// an otherwise incompatible default-derived type in its true branch.
    pub default_parameter_bindings: HashSet<String>,
    /// Bare-identifier function aliases, keyed by the local binding name.
    /// The value is the ultimate semantic callee name used by call inference.
    pub function_aliases: HashMap<String, String>,
    pub data_mask_unknown: bool,
    pub search_path_unknown: bool,
    /// Execution cannot continue in this block because a preceding operation
    /// is known to throw. Cloned scopes keep this fact local to that path.
    pub(crate) unreachable: bool,
}

impl Scope {
    pub fn get(&self, name: &str) -> Option<&RType> {
        self.bindings.get(name)
    }

    pub fn insert(&mut self, name: impl Into<String>, t: RType) {
        let name = name.into();
        self.function_aliases.remove(&name);
        self.default_parameter_bindings.remove(&name);
        self.narrowed_bindings.remove(&name);
        self.bindings.insert(name, t);
    }

    pub(crate) fn insert_narrowed(&mut self, name: impl Into<String>, t: RType) {
        let name = name.into();
        self.insert(name.clone(), t);
        self.narrowed_bindings.insert(name);
    }

    pub(crate) fn insert_parameter_default(&mut self, name: impl Into<String>, t: RType) {
        let name = name.into();
        self.function_aliases.remove(&name);
        self.default_parameter_bindings.insert(name.clone());
        self.narrowed_bindings.remove(&name);
        self.bindings.insert(name, t);
    }

    pub(crate) fn is_default_parameter(&self, name: &str) -> bool {
        self.default_parameter_bindings.contains(name)
    }

    pub(crate) fn set_function_alias(&mut self, name: impl Into<String>, target: String) {
        self.function_aliases.insert(name.into(), target);
    }

    pub(crate) fn function_alias(&self, name: &str) -> Option<&str> {
        self.function_aliases.get(name).map(String::as_str)
    }

    pub fn with_unknown_data_mask(mut self) -> Self {
        self.data_mask_unknown = true;
        self
    }

    pub fn mark_search_path_unknown(&mut self) {
        self.search_path_unknown = true;
    }
}

/// A user-defined function recorded for interprocedural inference.
/// We store the AST nodes by index into a side-table the checker owns,
/// avoiding lifetime entanglement with the SourceFile.
#[derive(Debug, Clone)]
pub(crate) struct UserFn {
    pub(crate) params: Vec<UserParam>,
    // The function body, shared via `Arc` so the per-fixpoint-iteration
    // clone in `refine_fn_return` is a cheap refcount bump rather than a
    // deep clone of every statement. The body is immutable after
    // `record_fn`, so sharing is safe. `Arc` (not `Rc`) so the
    // `FnTable` stays `Send` -- the LSP moves it across async tasks.
    pub(crate) body: Arc<[Stmt]>,
    // Currently-inferred return type. Starts as UNKNOWN, refined by
    // each fixpoint iteration. Stored as a slot index so all calls
    // observe the latest refinement without rebuilding the table.
    pub(crate) return_slot: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct UserParam {
    pub(crate) name: String,
    pub(crate) type_: RType,
    pub(crate) required: bool,
    pub(crate) defused: bool,
    /// Whether the function captures this argument as an unevaluated
    /// expression (for example through `substitute(x)`).
    pub(crate) quoting: bool,
}

/// Side-table of inferred return types, indexed by `UserFn::return_slot`.
/// Stored separately so we can clone the table cheaply when entering a
/// nested inference pass without deep-cloning the function bodies.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReturnSlots(pub(crate) Vec<RType>);

impl ReturnSlots {
    fn get(&self, i: usize) -> RType {
        self.0.get(i).cloned().unwrap_or(RType::unknown())
    }
    fn set(&mut self, i: usize, t: RType) {
        if i >= self.0.len() {
            self.0.resize(i + 1, RType::unknown());
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
    // `(generic, class)` -> return slot index. Mirrors the same
    // `return_slots` storage as `fns`; lookups during dispatch consult
    // this map for an S3 method before falling back to the generic.
    pub(crate) s3_methods: HashMap<(String, String), usize>,
    pub(crate) s4_methods: HashMap<(String, String), usize>,
    pub(crate) s4_classes: HashMap<String, HashMap<String, String>>,
    // Names of all top-level variable assignments across all files in
    // the project. Used to suppress RY010 for cross-file references:
    // when an identifier is not in the current scope but IS in this
    // set, we know it's defined in another file (or later in this
    // same file) and return opaque instead of flagging it as unbound.
    pub(crate) known_vars: std::collections::HashSet<String>,
    // Syntactic call sites used only for conservative internal-helper
    // default selection. Each argument records its optional exact name.
    pub(crate) call_sites: HashMap<String, Vec<Vec<Option<String>>>>,
    // Calls that forward an enclosing formal directly into another
    // function. Used to propagate evidence that a caller's default can reach
    // a callee parameter without treating every callee default as exhaustive.
    forwarded_calls: Vec<ForwardedCall>,
}

impl FnTable {
    fn append_collected(
        &mut self,
        mut collected: FnTable,
        return_slots: &mut ReturnSlots,
        collected_slots: ReturnSlots,
    ) {
        let slot_offset = return_slots.0.len();
        for function in collected.fns.values_mut() {
            function.return_slot += slot_offset;
        }
        for slot in collected.s3_methods.values_mut() {
            *slot += slot_offset;
        }
        for slot in collected.s4_methods.values_mut() {
            *slot += slot_offset;
        }
        return_slots.0.extend(collected_slots.0);

        self.fns.extend(collected.fns);
        self.s3_methods.extend(collected.s3_methods);
        self.s4_methods.extend(collected.s4_methods);
        self.s4_classes.extend(collected.s4_classes);
        self.known_vars.extend(collected.known_vars);
        for (name, sites) in collected.call_sites {
            self.call_sites.entry(name).or_default().extend(sites);
        }
        self.forwarded_calls.extend(collected.forwarded_calls);
    }
}

#[derive(Debug, Clone)]
struct ForwardedCall {
    caller: String,
    callee: String,
    /// Original syntactic callee name, retaining a package qualifier for
    /// typeshed resolution (`dbplyr::translate_sql`, for example).
    stub_callee: String,
    caller_params: Vec<Param>,
    arguments: Vec<(Option<String>, Option<String>)>,
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

#[derive(Clone)]
pub(crate) struct EnclosingFormals {
    pub(crate) names: HashSet<String>,
    pub(crate) has_dots: bool,
}

pub struct Checker {
    typeshed: Arc<Typeshed>,
    user_stubs: Arc<BTreeMap<String, Typeshed>>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) path: String,
    // When true, `emit` is a no-op. Set during pass-2 (fixpoint) return-
    // type refinement and closure-signature building so the single
    // inference engine can be used for both the pure and the diagnostic
    // walk: pass 2 runs the identical `infer` with `discarding = true`,
    // pass 3 with `false`.
    discarding: bool,
    validate_user_call_arguments: bool,
    // User-defined functions collected in pass 1. Stored behind an `Arc`
    // so the multi-file `Project` can share the refined tables across
    // per-file pass-3 emitters without deep-cloning them.
    // Mutation goes through `Arc::make_mut` (a copy-on-write clone when
    // the refcount is >1); passes 1/2 own their tables uniquely, and pass
    // 3 only reads, so the COW clone never actually fires in practice.
    pub(crate) fn_table: Arc<FnTable>,
    /// Top-level bindings that may suppress RY010 for the file being
    /// emitted. Project checking installs either a package R/ pool or the
    /// current script's own bindings.
    known_vars: Arc<HashSet<String>>,
    // Inferred return types, refined by the fixpoint loop. Same Arc-shared
    // story as `fn_table`.
    pub(crate) return_slots: Arc<ReturnSlots>,
    // Stack of function names currently being inferred (cycle detection).
    pub(crate) inferring: Vec<String>,
    // Packages attached via `library(pkg)` / `require(pkg)`, plus any
    // declared in `ry.toml`'s
    // `packages` key (threaded in via `set_loaded`). The dplyr NSE
    // verbs are gated on `dplyr` (or `tidyverse`) being present here,
    // so a bare `filter(df, ...)` only gets dplyr NSE treatment when
    // dplyr is in scope; otherwise it falls through to regular
    // resolution. Pass-3 emitters share the project-wide set by Arc; the
    // single-file library/require path uses copy-on-write mutation.
    pub(crate) loaded: Arc<HashSet<String>>,
    /// Packages that may supply ordinary bare names in this file.  This is
    /// deliberately narrower than `loaded`: Project keeps the latter as a
    /// union for dplyr NSE gating, while R's search path is file-local.
    pub(crate) bare_loaded: Arc<HashSet<String>>,
    // Opaque names proven to exist by metadata for the current source file.
    // Kept separate from the project-wide FnTable so imports from one R
    // package cannot suppress RY010 in an unrelated package checked in the
    // same invocation.
    external_bindings: HashSet<String>,
    imported_from: HashMap<String, String>,
    external_s3_methods: HashSet<(String, String)>,
    load_bindings: HashMap<usize, HashSet<String>>,
    // Names assigned anywhere in enclosing function bodies. They are added
    // only when checking a nested closure, matching R's deferred lexical
    // capture without making a direct read-before-assignment valid. The
    // current body's set also models expressions deferred by `on.exit()`.
    deferred_captures: Vec<HashSet<String>>,
    // Lexical function context used by call-site rules such as RY096.
    // A stack is required because nested functions replace, rather than
    // inherit, the set of formals relevant to `hasArg()`.
    enclosing_formals: Vec<EnclosingFormals>,
    // Values already inferred before a pipe is desugared into a call. This
    // cache is populated only for the duration of that rewritten call, so it
    // never crosses a scope-changing inference boundary.
    pipe_argument_types: HashMap<Span, RType>,
}

impl Checker {
    pub fn new(path: &str) -> Self {
        let typeshed = embedded_base();
        Self {
            typeshed,
            user_stubs: Arc::new(BTreeMap::new()),
            diagnostics: Vec::new(),
            path: path.to_string(),
            discarding: false,
            validate_user_call_arguments: true,
            fn_table: Arc::new(FnTable::default()),
            known_vars: Arc::new(HashSet::new()),
            return_slots: Arc::new(ReturnSlots::default()),
            inferring: Vec::new(),
            loaded: Arc::new(HashSet::new()),
            bare_loaded: Arc::new(HashSet::new()),
            external_bindings: HashSet::new(),
            imported_from: HashMap::new(),
            external_s3_methods: HashSet::new(),
            load_bindings: HashMap::new(),
            deferred_captures: Vec::new(),
            enclosing_formals: Vec::new(),
            pipe_argument_types: HashMap::new(),
        }
    }

    pub fn check(&mut self, file: &SourceFile) -> &[Diagnostic] {
        self.path = file.path.clone();

        // Parse errors first: a syntax error means the recovered tree is
        // unreliable, so RY000 is the primary signal for broken input. We
        // still run the checker on the recovered tree (downstream
        // diagnostics may be noise, but ty takes the same approach).
        self.emit_parse_errors(file);

        // Pass 1: collect function definitions into the FnTable. We don't
        // emit diagnostics yet - the body's `return` types depend on the
        // table being fully populated.
        self.collect_fns(&file.stmts);

        // Pass 2 (fixpoint): refine each function's inferred return type
        // until the table stabilizes or we hit MAX_FIXPOINT_DEPTH.
        self.run_fixpoint();
        self.known_vars = Arc::new(self.fn_table.known_vars.clone());

        // Pass 3: final walk, emitting all diagnostics. Function calls
        // now resolve against the refined FnTable.
        self.emit_diagnostics(file);
        &self.diagnostics
    }

    // Check a file and return both diagnostics and the final top-level
    // scope. Used by the LSP server for hover support: the scope maps
    // variable names to their inferred types, so hovering over a
    // variable shows its type.
    pub fn check_with_scope(&mut self, file: &SourceFile) -> (Vec<Diagnostic>, Scope) {
        self.path = file.path.clone();
        // Clear diagnostics FIRST so we start fresh (the caller may call
        // this multiple times on the same checker instance), THEN emit
        // parse errors. The previous order emitted RY000s and then wiped
        // them with `clear()`, so this API path never surfaced syntax
        // errors.
        self.diagnostics.clear();
        self.emit_parse_errors(file);
        self.collect_fns(&file.stmts);
        self.run_fixpoint();
        self.known_vars = Arc::new(self.fn_table.known_vars.clone());
        let mut scope = self.top_level_scope();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
        (std::mem::take(&mut self.diagnostics), scope)
    }

    // Construct a checker that uses pre-populated function tables.
    // Used by `Project` for passes 1 and 2, where a single throwaway
    // checker owns the (mutable) tables and hands them back via
    // [`into_tables`]. The fresh checker starts with an empty
    // diagnostics vec and an empty `inferring` stack.
    //
    // [`into_tables`]: Checker::into_tables
    pub(crate) fn with_tables(path: &str, fn_table: FnTable, return_slots: ReturnSlots) -> Self {
        let typeshed = embedded_base();
        Self {
            typeshed,
            user_stubs: Arc::new(BTreeMap::new()),
            diagnostics: Vec::new(),
            path: path.to_string(),
            discarding: false,
            validate_user_call_arguments: true,
            fn_table: Arc::new(fn_table),
            known_vars: Arc::new(HashSet::new()),
            return_slots: Arc::new(return_slots),
            inferring: Vec::new(),
            loaded: Arc::new(HashSet::new()),
            bare_loaded: Arc::new(HashSet::new()),
            external_bindings: HashSet::new(),
            imported_from: HashMap::new(),
            external_s3_methods: HashSet::new(),
            load_bindings: HashMap::new(),
            deferred_captures: Vec::new(),
            enclosing_formals: Vec::new(),
            pipe_argument_types: HashMap::new(),
        }
    }

    // Construct a checker that SHARES the given tables by `Arc` handle
    // (no deep clone). Used by `Project` pass 3, which is read-only on
    // the tables (every mutation site lives in passes 1/2). This is the
    // Sharing optimization: per-file diagnostic emission clones
    // only the refcounted handle, not the tables themselves.
    pub(crate) fn with_shared_tables(
        path: &str,
        fn_table: Arc<FnTable>,
        return_slots: Arc<ReturnSlots>,
    ) -> Self {
        let typeshed = embedded_base();
        Self {
            typeshed,
            user_stubs: Arc::new(BTreeMap::new()),
            diagnostics: Vec::new(),
            path: path.to_string(),
            discarding: false,
            validate_user_call_arguments: true,
            fn_table,
            known_vars: Arc::new(HashSet::new()),
            return_slots,
            inferring: Vec::new(),
            loaded: Arc::new(HashSet::new()),
            bare_loaded: Arc::new(HashSet::new()),
            external_bindings: HashSet::new(),
            imported_from: HashMap::new(),
            external_s3_methods: HashSet::new(),
            load_bindings: HashMap::new(),
            deferred_captures: Vec::new(),
            enclosing_formals: Vec::new(),
            pipe_argument_types: HashMap::new(),
        }
    }

    // Take ownership of this checker's tables. Used by `Project` to
    // move a populated `FnTable`/`ReturnSlots` out of a throwaway
    // checker and into a shared `Project`.
    pub(crate) fn into_tables(self) -> (FnTable, ReturnSlots) {
        // `Arc::unwrap_or_clone` avoids a deep clone when the checker is
        // the sole owner (always true for the pass-1/2 throwaway checkers
        // `Project` uses); falls back to a clone if shared.
        (
            Arc::unwrap_or_clone(self.fn_table),
            Arc::unwrap_or_clone(self.return_slots),
        )
    }

    pub(crate) fn disable_user_call_argument_validation(&mut self) {
        self.validate_user_call_arguments = false;
    }

    // Pass 1: collect function definitions from this file into the
    // shared `FnTable`. Does NOT emit diagnostics. `Project::check`
    // calls this once per file before running the fixpoint.
    pub(crate) fn collect_file_fns(&mut self, file: &SourceFile) {
        self.path = file.path.clone();
        self.collect_fns(&file.stmts);
    }

    // Collect packages attached by `library`/`require`
    // anywhere in this file, WITHOUT emitting diagnostics. Returns the
    // set of package names so `Project::check` can union them across
    // files (a `library(dplyr)` in any file makes dplyr NSE verbs work
    // in every file, matching the plan's cross-file union intent).
    //
    // Implementation: walk the file in discarding mode so `infer_call`'s
    // library/require recording populates `self.loaded`
    // via the same code path used during real checking; we then take
    // the set. Discarding mode guarantees no diagnostics are emitted
    // even though we run the full inference walker.
    pub(crate) fn collect_file_loaded(&mut self, file: &SourceFile) -> HashSet<String> {
        self.path = file.path.clone();
        let prev = self.discarding;
        self.discarding = true;
        let mut scope = self.top_level_scope();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
        self.discarding = prev;
        Arc::unwrap_or_clone(std::mem::take(&mut self.loaded))
    }

    // Pass 2: refine all function return types until convergence.
    // Iterates the shared `FnTable`; safe to call once after all files
    // have been collected.
    //
    // S3 methods (`print.foo`, etc.) are inserted into `fns` under
    // their full name during pass 1, with `s3_methods` pointing at
    // the same return slot. Iterating `fns.keys()` therefore refines
    // S3 method bodies alongside regular functions; dispatch reads
    // the refined slot via the `s3_methods` map.
    pub(crate) fn run_fixpoint(&mut self) {
        // Pass 2 runs the unified walker in discarding mode: types are
        // computed (for return-slot refinement) but no diagnostics are
        // recorded. Diagnostics are produced in pass 3 (emit_diagnostics)
        // against the fully refined FnTable. Save/restore so a panic
        // (unreachable in practice) does not leak the flag.
        let prev_discarding = self.discarding;
        self.discarding = true;
        for _ in 0..MAX_FIXPOINT_DEPTH {
            // Snapshot the *contents* (not the Arc handle) for the
            // convergence check -- cloning the Arc would alias the same
            // data and the comparison would always be equal.
            let before = (*self.return_slots).clone();
            let names: Vec<String> = self.fn_table.fns.keys().cloned().collect();
            for name in names {
                self.refine_fn_return(&name);
            }
            // A formal inherits quoting only when it is passed directly to a
            // quoting formal of another user function.  Do this in the same
            // bounded loop as return refinement so chains across files (and
            // mutual recursion) converge before diagnostics are emitted.
            let generic_quoting_changed = self.propagate_s3_generic_quoting();
            let quoting_changed = self.propagate_forwarded_quoting();
            if self.return_slots.0 == before.0 && !generic_quoting_changed && !quoting_changed {
                break;
            }
        }
        self.discarding = prev_discarding;
    }

    /// A `UseMethod()` generic is evaluated before its selected method, but
    /// its callers must still supply promises compatible with that method's
    /// NSE behavior.  Derive the generic's quoting formals from every known
    /// `generic.class` implementation.  This is intentionally a union: one
    /// quoting method is enough to make the corresponding generic argument
    /// opaque at a call site.
    fn propagate_s3_generic_quoting(&mut self) -> bool {
        let mut inherited = Vec::new();

        for (name, generic) in &self.fn_table.fns {
            let Some(dispatch_name) = usemethod_generic_name(&generic.body) else {
                continue;
            };
            if semantic_argument_name(name) != dispatch_name {
                continue;
            }

            let mut method_slots = std::collections::HashSet::new();
            let prefix = format!("{dispatch_name}.");
            for (method_name, method) in &self.fn_table.fns {
                if method_name
                    .strip_prefix(&prefix)
                    .is_some_and(|class| !class.is_empty())
                {
                    method_slots.insert(method.return_slot);
                }
            }
            // Registered methods can have an internal name (for example a
            // dynamically collected definition), so include their shared
            // return slots as well as conventionally named methods.
            for ((registered_generic, _), slot) in &self.fn_table.s3_methods {
                if registered_generic == &dispatch_name {
                    method_slots.insert(*slot);
                }
            }

            let dots = generic
                .params
                .iter()
                .position(|parameter| parameter.name == "...");
            for slot in method_slots {
                let Some(method) = self
                    .fn_table
                    .fns
                    .values()
                    .find(|function| function.return_slot == slot)
                else {
                    continue;
                };
                for parameter in &method.params {
                    if !parameter.quoting {
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
                        inherited.push((name.clone(), target));
                    }
                }
            }
        }

        let table = Arc::make_mut(&mut self.fn_table);
        let mut changed = false;
        for (generic, position) in inherited {
            if let Some(parameter) = table
                .fns
                .get_mut(&generic)
                .and_then(|function| function.params.get_mut(position))
                && !parameter.quoting
            {
                parameter.quoting = true;
                changed = true;
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
    fn propagate_forwarded_quoting(&mut self) -> bool {
        let mut inherited = Vec::new();

        for call in &self.fn_table.forwarded_calls {
            let Some(caller) = self.fn_table.fns.get(&call.caller) else {
                continue;
            };

            // An explicit namespace call bypasses any same-named user
            // binding, just as normal call resolution does.
            let user_callee = (!call.stub_callee.contains("::"))
                .then(|| self.fn_table.fns.get(&call.callee))
                .flatten();
            let stub_callee = self.resolve_typeshed_sig(&call.stub_callee);
            if user_callee.is_none() && stub_callee.is_none() {
                continue;
            }

            let mut claimed = std::collections::HashSet::new();
            let mut next_positional = 0;
            for (argument_name, source) in &call.arguments {
                let Some(source) = source else {
                    continue;
                };
                let target = if source == "..." {
                    // `callee(...)` forwards the caller's dots only to the
                    // callee's dots promise, never to an arbitrary formal.
                    user_callee
                        .and_then(|callee| {
                            callee.params.iter().position(|param| param.name == "...")
                        })
                        .or_else(|| {
                            stub_callee.as_ref().and_then(|sig| {
                                sig.params.iter().position(|param| param.name == "...")
                            })
                        })
                } else if let Some(argument_name) = argument_name {
                    user_callee
                        .and_then(|callee| {
                            callee
                                .params
                                .iter()
                                .position(|param| param.name == *argument_name)
                                .or_else(|| {
                                    callee.params.iter().position(|param| param.name == "...")
                                })
                        })
                        .or_else(|| {
                            stub_callee.as_ref().and_then(|sig| {
                                sig.params
                                    .iter()
                                    .position(|param| param.name == *argument_name)
                                    .or_else(|| {
                                        sig.params.iter().position(|param| param.name == "...")
                                    })
                            })
                        })
                } else {
                    let params: Vec<&str> = if let Some(callee) = user_callee {
                        callee
                            .params
                            .iter()
                            .map(|param| param.name.as_str())
                            .collect()
                    } else {
                        stub_callee
                            .as_ref()
                            .map(|sig| sig.params.iter().map(|param| param.name.as_str()).collect())
                            .unwrap_or_default()
                    };
                    while next_positional < params.len()
                        && (params[next_positional] == "..." || claimed.contains(&next_positional))
                    {
                        next_positional += 1;
                    }
                    let target = (next_positional < params.len()).then_some(next_positional);
                    next_positional += usize::from(target.is_some());
                    target
                };
                let Some(target) = target else {
                    continue;
                };
                claimed.insert(target);
                // `target` was computed against whichever params list was
                // selected above; the other source's list may be shorter, so
                // every index below must stay bounds-checked.
                let inherits_quoting = user_callee
                    .is_some_and(|callee| callee.params.get(target).is_some_and(|p| p.quoting))
                    || stub_callee.as_ref().is_some_and(|sig| {
                        sig.params.get(target).is_some_and(|param| {
                            sig.eval.get(&param.name).is_some_and(|mode| {
                                matches!(mode, EvalMode::QuotedExpression | EvalMode::QuotedSymbol)
                            })
                        })
                    });
                // Dots capture is already modeled as defusing (rather than
                // quoting) so its direct arguments remain opaque.  Preserve
                // that stronger behavior while forwarding `...` to another
                // dots-capturing user function.
                let inherits_defusing = source == "..."
                    && user_callee
                        .is_some_and(|callee| callee.params.get(target).is_some_and(|p| p.defused));
                if (inherits_quoting || inherits_defusing)
                    && caller.params.iter().any(|param| param.name == *source)
                {
                    inherited.push((call.caller.clone(), source.clone(), inherits_quoting));
                }
            }
        }

        let table = Arc::make_mut(&mut self.fn_table);
        let mut changed = false;
        for (caller, parameter, quoting) in inherited {
            if let Some(parameter) = table
                .fns
                .get_mut(&caller)
                .and_then(|function| function.params.iter_mut().find(|p| p.name == parameter))
            {
                if quoting && !parameter.quoting {
                    parameter.quoting = true;
                    changed = true;
                } else if !quoting && !parameter.defused {
                    parameter.defused = true;
                    changed = true;
                }
            }
        }
        changed
    }

    // Pass 3: emit diagnostics for this file using the refined tables.
    // Diagnostics are appended to `self.diagnostics`; clear that vec
    // first if you want only this file's diagnostics.
    pub(crate) fn emit_diagnostics(&mut self, file: &SourceFile) {
        self.path = file.path.clone();
        self.emit_parse_errors(file);
        let mut scope = self.top_level_scope();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Build the outermost scope for a checked file. Shiny app fragments are
    /// sourced inside a server function, where these names are supplied by
    /// Shiny rather than assigned in the fragment itself.
    fn top_level_scope(&self) -> Scope {
        let mut scope = Scope::default();
        if self
            .external_bindings
            .contains(SERIALIZED_BINDINGS_UNENUMERABLE)
        {
            scope.mark_search_path_unknown();
        }
        for profile in BUILTIN_ENVIRONMENTS {
            if !(profile.path_trigger)(&self.path) {
                continue;
            }
            for name in profile.bindings {
                scope.insert(*name, RType::unknown());
            }
        }
        scope
    }

    // Seed the loaded-packages set. Called by `Project` (with the
    // union of `ry.toml` `packages` and every file's `library`/
    // `require` calls) before pass-3 emission, and
    // by the CLI for single-file `Checker` paths. The dplyr NSE verbs
    // consult this set to decide whether to apply dplyr semantics.
    pub fn set_loaded(&mut self, loaded: HashSet<String>) {
        self.bare_loaded = Arc::new(loaded.clone());
        self.loaded = Arc::new(loaded);
    }

    pub(crate) fn set_shared_loaded(&mut self, loaded: Arc<HashSet<String>>) {
        self.loaded = loaded;
    }

    pub(crate) fn set_bare_loaded(&mut self, loaded: HashSet<String>) {
        self.bare_loaded = Arc::new(loaded);
    }

    pub(crate) fn set_shared_known_vars(&mut self, known_vars: Arc<HashSet<String>>) {
        self.known_vars = known_vars;
    }

    /// Install runtime stubs for this checker. A matching package replaces
    /// the embedded package wholesale; `base` replaces the embedded base
    /// database for every lookup made by this checker.
    pub fn set_user_stubs(&mut self, stubs: Arc<BTreeMap<String, Typeshed>>) {
        self.typeshed = stubs
            .get("base")
            .cloned()
            .map(Arc::new)
            .unwrap_or_else(embedded_base);
        self.user_stubs = stubs;
    }

    pub(crate) fn package_typeshed(&self, package: &str) -> Option<&Typeshed> {
        self.user_stubs
            .get(package)
            .or_else(|| load_package(package))
    }

    pub(crate) fn package_is_known(&self, package: &str) -> bool {
        self.user_stubs.contains_key(package) || is_known_package(package)
    }

    pub(crate) fn available_package_names(&self) -> Vec<&str> {
        let mut packages: Vec<&str> = known_packages().collect();
        packages.extend(
            self.user_stubs
                .keys()
                .map(String::as_str)
                .filter(|package| *package != "base" && !is_known_package(package)),
        );
        packages
    }

    // Seed opaque bindings established by metadata for this source file.
    pub fn set_external_bindings(&mut self, bindings: HashSet<String>) {
        self.external_bindings = bindings;
    }

    pub fn set_imported_from(&mut self, imports: HashMap<String, String>) {
        self.imported_from = imports;
    }

    pub fn set_external_s3_methods(&mut self, methods: HashSet<(String, String)>) {
        self.external_s3_methods = methods;
    }

    pub fn set_load_bindings(&mut self, bindings: HashMap<usize, HashSet<String>>) {
        self.load_bindings = bindings;
    }

    // Resolve a function signature by name, consulting (in order):
    //   1. a `pkg::fun` / `pkg:::fun` qualified name -- looked up in
    //      `load_package(pkg)` directly, bypassing base and loaded
    //      packages (a qualified call is an explicit reference);
    //   2. the base typeshed (`self.typeshed`);
    //   3. each loaded package that ships signatures (reverse load
    //      order so the most-recently-loaded package wins, mirroring
    //      R's search path).
    //
    // Returns the signature and the resolved call name (the bare
    // function name, suitable for `apply_sig`'s slot resolution).
    // Returns `None` when no package knows the name.
}

fn embedded_base() -> Arc<Typeshed> {
    static BASE: std::sync::OnceLock<Arc<Typeshed>> = std::sync::OnceLock::new();
    Arc::clone(
        BASE.get_or_init(|| Arc::new(load_base_cached().expect("typeshed must load").clone())),
    )
}

#[cfg(test)]
mod tests;
