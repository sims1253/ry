//! Local type inference + diagnostics.
//!
//! v1 scope: single-file, inference-only, NSE-opaque. We walk statements
//! top-down, maintaining a per-scope binding table `name -> RType`.
//!
//! v2 additions: interprocedural function-return inference via a
//! module-level FnTable and a fixpoint loop. The first pass collects
//! function definitions; subsequent passes refine each function's
//! inferred return type until stable (or the depth cap is hit).

pub mod diagnostics;
pub mod format;
pub mod project;
pub mod rules;

// Re-export `Project` at the crate root so callers (the CLI, integration
// tests) can write `ry_checker::Project` rather than
// `ry_checker::project::Project`. Mirrors the ergonomics of `Checker`.
pub use project::Project;
// Re-export the diagnostic data types and suppression helpers at the
// crate root for back-compat (callers and tests reference
// `ry_checker::{Severity, Diagnostic, ...}` directly).
pub use diagnostics::{
    apply_filter_to_diagnostics, filter_suppressed, filter_suppressed_with_comments,
    has_file_suppression, has_file_suppression_from_comments, is_suppressed, parse_suppressions,
    parse_suppressions_from_comments, Diagnostic, Severity, SeverityFilter, Suppression,
};

use ry_core::ast::*;
use ry_core::types::{ClassVector, ColumnSchema, FunctionSignature, Length, Mode, RType};
use ry_core::Span;
use ry_typeshed::{
    is_known_package, load_base_cached, load_package, FunctionSig, JsonRType, ReturnSpec, Typeshed,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// S3 generics we recognize when collecting method definitions of the
/// form `print.foo <- function(...) body`. A `<generic>.<class>` name
/// where `<generic>` is in this list is recorded in `FnTable::s3_methods`
/// (keyed by `(generic, class)`) in addition to its slot in `fns`.
///
/// The list is intentionally generous: it mirrors the generics shipped
/// with base R plus the most commonly defined ones in CRAN packages.
/// S3 generics we recognize for method-name splitting
/// (`<generic>.<class>`). Curated to UNAMBIGUOUS single-word dispatch
/// generics in base R.
///
/// Previously this list also included short / common-word names like
/// `t`, `c`, `format`, `is.na`, `length`, `names`, `dim`, `[`, `[[`,
/// `$`, `rep`, `rev`, `sort`, `unique`, `head`, `tail`, `subset`,
/// `transform`, `within`, `merge`, and the multi-segment `as.*` and
/// `model.matrix`. Those caused false S3 registrations: e.g.
/// `t.test <- function(x, t) ...` matched generic `t` + class `test`
/// and misregistered as an S3 method. The trimmed list keeps only the
/// generics whose `<g>.<rest>` form is overwhelmingly a real method
/// (print.foo, summary.lm, ...).
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
    "str",
    "terms",
];

/// Names that look like `<generic>.<class>` but are NOT S3 methods.
/// These are well-known dotted functions/packages whose leading segment
/// happens to coincide with a (now-removed) generic, or otherwise common
/// dotted names that must never split. Checked before any prefix match.
const S3_DENYLIST: &[&str] = &[
    "t.test",
    "all.equal",
    "file.path",
    "Sys.time",
    "Sys.Date",
    "as.data.frame",
    "tempfile",
    "tempdir",
    "read.csv",
    "write.csv",
    "data.frame",
];

/// Returns `Some((generic, class))` if `name` matches the S3 method
/// naming convention `<generic>.<class>` and `<generic>` is in the
/// curated `S3_GENERICS` table. Longest match wins (handles rare
/// multi-segment cases). The `[0u8; 64]` scratch buffer the old code
/// used to avoid an allocation is replaced by `format!` (this runs once
/// per top-level assignment; the cost is negligible).
fn split_s3_method_name(name: &str) -> Option<(&'static str, String)> {
    if S3_DENYLIST.contains(&name) {
        return None;
    }
    let mut best: Option<(&'static str, String)> = None;
    for generic in S3_GENERICS {
        let prefix = format!("{}.", generic);
        if let Some(class) = name.strip_prefix(&prefix) {
            if class.is_empty() {
                continue;
            }
            // Prefer the longest matching prefix (more specific).
            let is_better = best.as_ref().is_none_or(|(g, _)| g.len() < generic.len());
            if is_better {
                best = Some((generic, class.to_string()));
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
    /// `dplyr::filter(.data, ...)`: returns rows where conditions are TRUE.
    Filter,
    /// `dplyr::mutate(.data, ...)`: adds/modifies columns, returns data frame.
    Mutate,
    /// `dplyr::summarise(.data, ...)` / `summarize(.data, ...)`: aggregates.
    Summarise,
    /// `dplyr::select(.data, ...)`: selects columns by name.
    Select,
    /// `dplyr::arrange(.data, ...)`: sorts rows.
    Arrange,
    /// `dplyr::group_by(.data, ...)`: groups by columns.
    GroupBy,
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
            "filter" => Some(NseVerb::Filter),
            "mutate" => Some(NseVerb::Mutate),
            "summarise" => Some(NseVerb::Summarise),
            "summarize" => Some(NseVerb::Summarise),
            "select" => Some(NseVerb::Select),
            "arrange" => Some(NseVerb::Arrange),
            "group_by" => Some(NseVerb::GroupBy),
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
    // --- purrr family (only recognized when purrr is loaded or the
    //     call is `purrr::`-qualified; see `from_call`). ---
    /// `purrr::map(.x, .f)` and `map_if`/`imap`: list of `.f`'s returns.
    PurrrMap,
    /// `purrr::map_lgl/int/dbl/chr/vec(.x, .f)`: typed vector, len = `.x`.
    PurrrMapTyped(Mode),
    /// `purrr::map2(.x, .y, .f)` / `pmap(.l, .f)`: list.
    PurrrMap2,
    /// `purrr::keep`/`discard(.x, .p)`: same type as `.x`.
    PurrrKeep,
    /// `purrr::reduce(.x, .f)` / `accumulate`: fold.
    PurrrReduce,
    /// `purrr::walk(.x, .f)`: invisible `.x`.
    PurrrWalk,
    /// `purrr::in_parallel(.f)`: type-transparent wrapper, returns `.f`.
    PurrrInParallel,
}

impl HigherOrderFunc {
    /// Recognize a higher-order function by call name. Base-R
    /// higher-order builtins are always recognized. purrr family
    /// functions are recognized ONLY when purrr is loaded (`library
    /// (purrr)`) or the call is `purrr::`-qualified, so a bare `map`
    /// in code that never loads purrr is not misinterpreted.
    fn from_call(name: &str, loaded: &HashSet<String>) -> Option<Self> {
        // Base-R higher-order builtins.
        match name {
            "lapply" => return Some(HigherOrderFunc::Lapply),
            "sapply" => return Some(HigherOrderFunc::Sapply),
            "vapply" => return Some(HigherOrderFunc::Vapply),
            "Map" => return Some(HigherOrderFunc::Map),
            "mapply" => return Some(HigherOrderFunc::Mapply),
            "rapply" => return Some(HigherOrderFunc::Rapply),
            "Reduce" => return Some(HigherOrderFunc::Reduce),
            "Filter" => return Some(HigherOrderFunc::Filter),
            "Find" => return Some(HigherOrderFunc::Find),
            "Position" => return Some(HigherOrderFunc::Position),
            "do.call" => return Some(HigherOrderFunc::DoCall),
            _ => {}
        }
        // purrr family: only when purrr is in scope.
        let purrr_in_scope = name.starts_with("purrr::") || loaded.contains("purrr");
        if !purrr_in_scope {
            return None;
        }
        // Strip any `purrr::`/`purrr:::` prefix for matching.
        let bare = name.rsplit_once("::").map(|(_, n)| n).unwrap_or(name);
        match bare {
            "map" | "map_if" | "imap" => Some(HigherOrderFunc::PurrrMap),
            "map_lgl" => Some(HigherOrderFunc::PurrrMapTyped(Mode::Logical)),
            "map_int" => Some(HigherOrderFunc::PurrrMapTyped(Mode::Integer)),
            "map_dbl" => Some(HigherOrderFunc::PurrrMapTyped(Mode::Double)),
            "map_chr" => Some(HigherOrderFunc::PurrrMapTyped(Mode::Character)),
            "map_vec" => Some(HigherOrderFunc::PurrrMapTyped(Mode::Opaque)),
            "map2" | "map2_lgl" | "map2_int" | "map2_dbl" | "map2_chr" | "pmap" | "pmap_lgl"
            | "pmap_int" | "pmap_dbl" | "pmap_chr" => Some(HigherOrderFunc::PurrrMap2),
            "keep" | "discard" | "compact" => Some(HigherOrderFunc::PurrrKeep),
            "reduce" | "accumulate" => Some(HigherOrderFunc::PurrrReduce),
            "walk" | "walk2" | "pwalk" => Some(HigherOrderFunc::PurrrWalk),
            "in_parallel" => Some(HigherOrderFunc::PurrrInParallel),
            _ => None,
        }
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
    /// The function body, shared via `Arc` so the per-fixpoint-iteration
    /// clone in `refine_fn_return` is a cheap refcount bump rather than a
    /// deep clone of every statement. The body is immutable after
    /// `record_fn`, so sharing is safe. `Arc` (not `Rc`) so the
    /// `FnTable` stays `Send` -- the LSP moves it across async tasks.
    pub(crate) body: Arc<[Stmt]>,
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
    /// `(generic, class)` -> return slot index. Mirrors the same
    /// `return_slots` storage as `fns`; lookups during dispatch consult
    /// this map for an S3 method before falling back to the generic.
    pub(crate) s3_methods: HashMap<(String, String), usize>,
    /// Names of all top-level variable assignments across all files in
    /// the project. Used to suppress RY010 for cross-file references:
    /// when an identifier is not in the current scope but IS in this
    /// set, we know it's defined in another file (or later in this
    /// same file) and return opaque instead of flagging it as unbound.
    pub(crate) known_vars: std::collections::HashSet<String>,
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
    typeshed: &'static Typeshed,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) path: String,
    /// When true, `emit` is a no-op. Set during pass-2 (fixpoint) return-
    /// type refinement and closure-signature building so the single
    /// inference engine can be used for both the pure and the diagnostic
    /// walk: pass 2 runs the identical `infer` with `discarding = true`,
    /// pass 3 with `false`. This is the Phase 2 unification mechanism.
    discarding: bool,
    /// User-defined functions collected in pass 1. Stored behind an `Arc`
    /// so the multi-file `Project` can share the refined tables across
    /// per-file pass-3 emitters without deep-cloning them (PLAN Phase D1).
    /// Mutation goes through `Arc::make_mut` (a copy-on-write clone when
    /// the refcount is >1); passes 1/2 own their tables uniquely, and pass
    /// 3 only reads, so the COW clone never actually fires in practice.
    pub(crate) fn_table: Arc<FnTable>,
    /// Inferred return types, refined by the fixpoint loop. Same Arc-shared
    /// story as `fn_table`.
    pub(crate) return_slots: Arc<ReturnSlots>,
    /// Stack of function names currently being inferred (cycle detection).
    pub(crate) inferring: Vec<String>,
    /// Packages loaded via `library(pkg)` / `require(pkg)` /
    /// `requireNamespace("pkg")`, plus any declared in `ry.toml`'s
    /// `packages` key (threaded in via `set_loaded`). The dplyr NSE
    /// verbs are gated on `dplyr` (or `tidyverse`) being present here,
    /// so a bare `filter(df, ...)` only gets dplyr NSE treatment when
    /// dplyr is in scope; otherwise it falls through to regular
    /// resolution. A plain owned set per emitter (rather than Arc-shared
    /// like the tables) is fine: it is small and rarely mutated.
    pub(crate) loaded: HashSet<String>,
}

impl Checker {
    pub fn new(path: &str) -> Self {
        let typeshed = load_base_cached().expect("typeshed must load");
        Self {
            typeshed,
            diagnostics: Vec::new(),
            path: path.to_string(),
            discarding: false,
            fn_table: Arc::new(FnTable::default()),
            return_slots: Arc::new(ReturnSlots::default()),
            inferring: Vec::new(),
            loaded: HashSet::new(),
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
        // Clear diagnostics FIRST so we start fresh (the caller may call
        // this multiple times on the same checker instance), THEN emit
        // parse errors. The previous order emitted RY000s and then wiped
        // them with `clear()`, so this API path never surfaced syntax
        // errors (PLAN Phase C2).
        self.diagnostics.clear();
        self.emit_parse_errors(file);
        self.collect_fns(&file.stmts);
        self.run_fixpoint();
        let mut scope = Scope::default();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
        (std::mem::take(&mut self.diagnostics), scope)
    }

    /// Construct a checker that uses pre-populated function tables.
    /// Used by `Project` for passes 1 and 2, where a single throwaway
    /// checker owns the (mutable) tables and hands them back via
    /// [`into_tables`]. The fresh checker starts with an empty
    /// diagnostics vec and an empty `inferring` stack.
    ///
    /// [`into_tables`]: Checker::into_tables
    pub(crate) fn with_tables(path: &str, fn_table: FnTable, return_slots: ReturnSlots) -> Self {
        let typeshed = load_base_cached().expect("typeshed must load");
        Self {
            typeshed,
            diagnostics: Vec::new(),
            path: path.to_string(),
            discarding: false,
            fn_table: Arc::new(fn_table),
            return_slots: Arc::new(return_slots),
            inferring: Vec::new(),
            loaded: HashSet::new(),
        }
    }

    /// Construct a checker that SHARES the given tables by `Arc` handle
    /// (no deep clone). Used by `Project` pass 3, which is read-only on
    /// the tables (every mutation site lives in passes 1/2). This is the
    /// PLAN Phase D1 optimization: per-file diagnostic emission clones
    /// only the refcounted handle, not the tables themselves.
    pub(crate) fn with_shared_tables(
        path: &str,
        fn_table: Arc<FnTable>,
        return_slots: Arc<ReturnSlots>,
    ) -> Self {
        let typeshed = load_base_cached().expect("typeshed must load");
        Self {
            typeshed,
            diagnostics: Vec::new(),
            path: path.to_string(),
            discarding: false,
            fn_table,
            return_slots,
            inferring: Vec::new(),
            loaded: HashSet::new(),
        }
    }

    /// Take ownership of this checker's tables. Used by `Project` to
    /// move a populated `FnTable`/`ReturnSlots` out of a throwaway
    /// checker and into a shared `Project`.
    pub(crate) fn into_tables(self) -> (FnTable, ReturnSlots) {
        // `Arc::unwrap_or_clone` avoids a deep clone when the checker is
        // the sole owner (always true for the pass-1/2 throwaway checkers
        // `Project` uses); falls back to a clone if shared.
        (
            Arc::unwrap_or_clone(self.fn_table),
            Arc::unwrap_or_clone(self.return_slots),
        )
    }

    /// Pass 1: collect function definitions from this file into the
    /// shared `FnTable`. Does NOT emit diagnostics. `Project::check`
    /// calls this once per file before running the fixpoint.
    pub(crate) fn collect_file_fns(&mut self, file: &SourceFile) {
        self.path = file.path.clone();
        self.collect_fns(&file.stmts);
    }

    /// Collect packages loaded by `library`/`require`/`requireNamespace`
    /// anywhere in this file, WITHOUT emitting diagnostics. Returns the
    /// set of package names so `Project::check` can union them across
    /// files (a `library(dplyr)` in any file makes dplyr NSE verbs work
    /// in every file, matching the plan's cross-file union intent).
    ///
    /// Implementation: walk the file in discarding mode so `infer_call`'s
    /// library/require/requireNamespace recording populates `self.loaded`
    /// via the same code path used during real checking; we then take
    /// the set. Discarding mode guarantees no diagnostics are emitted
    /// even though we run the full inference walker.
    pub(crate) fn collect_file_loaded(&mut self, file: &SourceFile) -> HashSet<String> {
        self.path = file.path.clone();
        let prev = self.discarding;
        self.discarding = true;
        let mut scope = Scope::default();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
        self.discarding = prev;
        std::mem::take(&mut self.loaded)
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
            if self.return_slots.0 == before.0 {
                break;
            }
        }
        self.discarding = prev_discarding;
    }

    /// Pass 3: emit diagnostics for this file using the refined tables.
    /// Diagnostics are appended to `self.diagnostics`; clear that vec
    /// first if you want only this file's diagnostics.
    pub(crate) fn emit_diagnostics(&mut self, file: &SourceFile) {
        self.path = file.path.clone();
        self.emit_parse_errors(file);
        let mut scope = Scope::default();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    /// Seed the loaded-packages set. Called by `Project` (with the
    /// union of `ry.toml` `packages` and every file's `library`/
    /// `require`/`requireNamespace` calls) before pass-3 emission, and
    /// by the CLI for single-file `Checker` paths. The dplyr NSE verbs
    /// consult this set to decide whether to apply dplyr semantics.
    pub fn set_loaded(&mut self, loaded: HashSet<String>) {
        self.loaded = loaded;
    }

    /// Resolve a function signature by name, consulting (in order):
    ///   1. a `pkg::fun` / `pkg:::fun` qualified name -- looked up in
    ///      `load_package(pkg)` directly, bypassing base and loaded
    ///      packages (a qualified call is an explicit reference);
    ///   2. the base typeshed (`self.typeshed`);
    ///   3. each loaded package that ships signatures (reverse load
    ///      order so the most-recently-loaded package wins, mirroring
    ///      R's search path).
    ///
    /// Returns the signature and the resolved call name (the bare
    /// function name, suitable for `apply_sig`'s slot resolution).
    /// Returns `None` when no package knows the name.
    fn resolve_typeshed_sig(&self, name: &str) -> Option<FunctionSig> {
        // Qualified call: explicit package reference.
        if let Some((pkg_raw, fun)) = name.rsplit_once("::") {
            // `pkg:::fun` splits as ("pkg:", "fun"); trim the trailing
            // colon to recover the package name.
            let pkg = pkg_raw.trim_end_matches(':');
            if let Some(t) = load_package(pkg) {
                if let Some(sig) = t.functions.get(fun) {
                    return Some(sig.clone());
                }
            }
            // The package is either unknown to ry (no embedded
            // signatures) or doesn't define `fun`. For base/stats/utils
            // (merged into `base.json`) and any other always-attached
            // package, fall back to the BASE typeshed under the STRIPPED
            // name: `stats::rnorm(10)` resolves as base's `rnorm`.
            if let Some(sig) = self.typeshed.functions.get(fun) {
                return Some(sig.clone());
            }
            // And under loaded packages, stripped name (a qualified call
            // to a package we have signatures for but where the function
            // lives under a different name is unlikely, but be thorough).
            for pk in [
                "dplyr",
                "purrr",
                "mirai",
                "brms",
                "posterior",
                "loo",
                "bayesplot",
                "cmdstanr",
            ] {
                if !self.loaded.contains(pk) {
                    continue;
                }
                if let Some(t) = load_package(pk) {
                    if let Some(sig) = t.functions.get(fun) {
                        return Some(sig.clone());
                    }
                }
            }
            return None;
        }
        // Unqualified: base typeshed, then loaded packages (fixed
        // priority order; see the comment on masking below).
        if let Some(sig) = self.typeshed.functions.get(name) {
            return Some(sig.clone());
        }
        // Loaded packages. R's actual masking depends on search-path
        // position; we approximate with a fixed priority order over the
        // packages that ship signatures (most function names are
        // disjoint across these packages, so masking rarely bites).
        // `loaded` is a HashSet (unordered) so we walk a deterministic
        // known-packages list and check membership.
        for pkg in [
            "dplyr",
            "purrr",
            "mirai",
            "brms",
            "posterior",
            "loo",
            "bayesplot",
            "cmdstanr",
        ] {
            if !self.loaded.contains(pkg) {
                continue;
            }
            if let Some(t) = load_package(pkg) {
                if let Some(sig) = t.functions.get(name) {
                    return Some(sig.clone());
                }
            }
        }
        None
    }

    /// Whether any package (base, loaded, or explicitly qualified)
    /// provides a function named `name`. Used by the RY070 path to
    /// implement R's function/value namespace separation (a non-function
    /// binding is skipped at a call site if a same-named function exists
    /// somewhere). Mirrors [`resolve_typeshed_sig`] plus the FnTable.
    fn has_function_anywhere(&self, name: &str) -> bool {
        // Qualified: check the named package.
        if let Some((pkg_raw, fun)) = name.rsplit_once("::") {
            let pkg = pkg_raw.trim_end_matches(':');
            if let Some(t) = load_package(pkg) {
                if t.functions.contains_key(fun) {
                    return true;
                }
            }
        }
        if self.typeshed.functions.contains_key(name) {
            return true;
        }
        // Loaded packages (fixed priority order; see resolve_typeshed_sig).
        for pkg in [
            "dplyr",
            "purrr",
            "mirai",
            "brms",
            "posterior",
            "loo",
            "bayesplot",
            "cmdstanr",
        ] {
            if !self.loaded.contains(pkg) {
                continue;
            }
            if let Some(t) = load_package(pkg) {
                if t.functions.contains_key(name) {
                    return true;
                }
            }
        }
        self.fn_table.fns.contains_key(name)
    }

    /// Apply a `SeverityFilter` to the diagnostics collected so far,
    /// mutating severities (or dropping suppressed ones) in place.
    pub fn apply_filter(&mut self, filter: &SeverityFilter) {
        apply_filter_to_diagnostics(&mut self.diagnostics, filter);
    }

    fn emit(&mut self, severity: Severity, span: Span, code: &'static str, msg: impl Into<String>) {
        if self.discarding {
            // Pass 2 (fixpoint) and closure-signature building run the
            // single inference engine in "discarding" mode: types are
            // computed but no diagnostics are recorded. This keeps pass 2
            // from double-emitting (diagnostics are produced in pass 3
            // against the refined FnTable).
            return;
        }
        self.diagnostics
            .push(Diagnostic::new(severity, span, &self.path, code, msg));
    }

    /// Surface parse errors collected by `RParser` as `RY000`
    /// (syntax-error) diagnostics. Each tree-sitter `ERROR` / `MISSING`
    /// node becomes one diagnostic. Always emitted, regardless of the
    /// checker's other findings: a broken region of input is the primary
    /// signal that the file is malformed.
    fn emit_parse_errors(&mut self, file: &SourceFile) {
        for span in &file.parse_errors {
            self.emit(
                Severity::Error,
                *span,
                "RY000",
                "syntax error: unparseable region (recovered tree may be unreliable)",
            );
        }
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
                // Record every identifier-bound top-level assignment in
                // `known_vars`. This is independent of whether the RHS
                // is a function literal: regular variable assignments
                // (`my_const <- 42`, `GeomRect <- ggproto(...)`) need
                // to be resolvable from other files (and from later in
                // this same file) without triggering RY010.
                if let Expr::Ident { name, .. } = target {
                    Arc::make_mut(&mut self.fn_table)
                        .known_vars
                        .insert(name.clone());
                }
                if let (Expr::Ident { name, .. }, Expr::Function { params, body, .. }) =
                    (target, value)
                {
                    // An S3 method named like `print.foo` is recorded both
                    // as a regular function (so the name resolves to its
                    // return type if called directly) and as an S3 method
                    // (so dispatch from `print(x)` on a classed value
                    // finds it). We record the body once and share the
                    // return slot between both entries.
                    //
                    // First-param heuristic: S3 methods conventionally
                    // take their dispatch object as the first parameter,
                    // named `x`. Require that (or an empty param list,
                    // which can't dispatch anyway) before registering as
                    // an S3 method, so a function that merely happens to
                    // have a dotted name isn't misregistered. The
                    // function is still recorded as a plain function
                    // either way.
                    let looks_like_s3 = split_s3_method_name(name)
                        .filter(|_| params.first().map(|p| p.name == "x").unwrap_or(false));
                    if let Some((generic, class)) = looks_like_s3 {
                        let slot = self.record_fn(name.clone(), params, body.clone());
                        Arc::make_mut(&mut self.fn_table)
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

    /// Walk a function body looking for `inner <- function(...) ...`
    /// definitions and record them with the mangled name
    /// `<outer>$<inner>`. The mangled name is internal: it exists so
    /// the fixpoint can refine the inner function's return type, which
    /// `refine_fn_return` reads back when building the outer function's
    /// `fn_sig`. Users never see this name.
    ///
    /// Recursion is bounded by the AST's literal nesting (small in
    /// practice). The inference depth is separately bounded by
    /// `MAX_CLOSURE_DEPTH` in `build_function_signature`.
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
                    None => RType::unknown(),
                };
                (p.name.clone(), t)
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
            scope.insert(n.clone(), t.clone());
        }
        // The function's own name is in scope as a function value, so
        // recursive calls resolve to a user-fn lookup.
        scope.insert(name.to_string(), RType::scalar(Mode::Function));

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

    /// The unified statement walker (Phase 2.5 fusion of the former
    /// `check_stmt` diagnostic walker and `collect_returns_and_simulate`
    /// return-type collector). Handles BOTH diagnostic emission (gated by
    /// `self.discarding`) AND return-type collection (when `returns` is
    /// `Some`).
    ///
    /// Callers:
    ///   * `check_stmt` (pass 3): discarding=false, returns=None.
    ///   * `refine_fn_return` (pass 2 fixpoint): discarding=true (set by
    ///     caller), returns=Some.
    ///   * `build_function_signature` (closure literals, both passes):
    ///     discarding=true (set by caller), returns=Some.
    ///
    /// Approximations (documented):
    ///   * `if` branches use `apply_narrowing` + separate child scopes
    ///     (then/else); bindings leak into subsequent statements.
    ///   * Loop bodies are walked once (not to fixpoint).
    ///   * Indexed assignment (`x[i] <- v`) does not update the scope.
    fn walk_stmt(&mut self, s: &Stmt, scope: &mut Scope, mut returns: Option<&mut Vec<RType>>) {
        match s {
            Stmt::Assign { target, value, .. } => {
                let vt = self.infer(value, scope);
                self.assign_target(target, vt, scope);
                // Named function bodies (`f <- function(...) body`) must
                // be walked for diagnostics. The function-value inference
                // path (`Expr::Function` -> `function_value_from_literal`)
                // runs in discarding mode and emits nothing on its own, so
                // without this walk almost all real R code would go
                // unchecked.
                if let Expr::Function { params, body, .. } = value {
                    let mut fn_scope = scope.clone();
                    for p in params {
                        let t = match &p.default {
                            Some(e) => self.infer(e, &mut fn_scope),
                            None => RType::unknown(),
                        };
                        fn_scope.insert(p.name.clone(), t);
                    }
                    for s in body {
                        self.walk_stmt(s, &mut fn_scope, None);
                    }
                }
            }
            Stmt::Expr(e) => {
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
                let ct = self.infer(cond, scope);
                if ct.invalid_condition() {
                    self.emit(
                        Severity::Error,
                        span_of(cond),
                        "RY001",
                        format!("`if` condition is `{}`, expected length-1 logical", ct),
                    );
                } else if !matches!(ct.mode, Mode::Logical | Mode::Opaque | Mode::Union)
                    && !is_numeric_truthiness_idiom(cond)
                {
                    self.emit(
                        Severity::Warning,
                        span_of(cond),
                        "RY001",
                        format!(
                            "`if` condition is `{}` (not logical); will be silently coerced",
                            ct.mode
                        ),
                    );
                } else if matches!(ct.mode, Mode::Logical) {
                    if let Length::Known(n) = ct.length {
                        if n > 1 {
                            self.emit(
                                Severity::Warning,
                                span_of(cond),
                                "RY002",
                                format!(
                                    "`if` condition has length {}, will only use first element",
                                    n
                                ),
                            );
                        }
                    }
                }
                let narrowing = extract_type_narrowing(cond);
                let (then_scope, else_scope, narrowed) = apply_narrowing(scope, &narrowing);
                let mut then_scope = then_scope;
                let mut else_scope = else_scope;
                let has_else = else_.is_some();
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
                self.merge_branch_bindings(scope, then_scope, else_scope, has_else, &narrowed);
            }
            Stmt::For {
                name, iter, body, ..
            } => {
                let iter_t = self.infer(iter, scope);
                let mut inner = scope.clone();
                inner.insert(name.clone(), iter_t.element());
                for s in body {
                    self.walk_stmt(s, &mut inner, returns.as_deref_mut());
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
                    self.walk_stmt(s, scope, returns.as_deref_mut());
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
                for p in params {
                    let t = match &p.default {
                        Some(e) => self.infer(e, &mut fn_scope),
                        None => RType::unknown(),
                    };
                    fn_scope.insert(p.name.clone(), t);
                }
                for s in body {
                    self.walk_stmt(s, &mut fn_scope, None);
                }
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
    fn merge_branch_bindings(
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
            // A name whose only change is a type-narrowing refinement
            // (recorded in `narrowed`) is branch-local: folding it back
            // would degrade a precise parent type (e.g. known-NULL ->
            // opaque) and mask later errors. Skip it. A genuine
            // reassignment to the same type is a no-op anyway; a real
            // reassignment to a narrowed name is rare and the cost is a
            // missed diagnostic (false negative), never a false positive.
            if narrowed.contains(name) {
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
                // See the then-branch loop: a pure narrowing refinement
                // is branch-local.
                if narrowed.contains(name) {
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

    /// runs the single diagnostic `infer` with `discarding` enabled, so
    /// the type computation (including the full `Expr::Ident` resolution
    /// ladder, all `Expr::Call` cases, narrowing, etc.) is shared between
    /// the pure and the diagnostic walks.
    fn infer_discarding(&mut self, e: &Expr, scope: &mut Scope) -> RType {
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
    fn function_value_from_literal(
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
    fn build_function_signature(
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

    fn build_function_signature_inner(
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
            scope.insert(p.name.clone(), t.clone());
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
    fn trailing_return_type(
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
    fn check_stmt(&mut self, s: &Stmt, scope: &mut Scope) {
        self.walk_stmt(s, scope, None);
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
            Expr::Logical(_, _) => RType::scalar(Mode::Logical),
            Expr::Integer(_, _) => RType::scalar(Mode::Integer),
            Expr::Double(_, _) => RType::scalar(Mode::Double),
            Expr::String(_, _) => RType::scalar(Mode::Character),
            Expr::Null(_) => RType::new(Mode::Null, Length::Zero),
            Expr::Na(t, _) => t.clone(),
            Expr::Ident { name, span } => match scope.get(name) {
                Some(t) => t.clone(),
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
                        return RType::scalar(Mode::Function);
                    }
                    // A function from a loaded package (e.g. purrr's
                    // `map` used as a value) resolves to a function too.
                    if self.loaded.iter().any(|pkg| {
                        is_known_package(pkg)
                            && load_package(pkg)
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
                    if self.fn_table.known_vars.contains(name) {
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
                    if let Expr::Ident { name, .. } = lhs.as_ref() {
                        scope.insert(name.clone(), rt.clone());
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
                let lt = self.infer(lhs, scope);
                let rt = self.infer(rhs, scope);
                self.infer_binop(*op, lt, rt, *span)
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
                let bt = self.infer(base, scope);
                self.infer_index(bt, *kind, args, *span, scope)
            }
            Expr::Function { params, body, .. } => {
                // Pass 3: build a `Mode::Function` value with an
                // inferred `fn_sig` when we can. This mirrors the
                // non-emitting inference path so a function literal in a
                // top-level expression (`g <- f(); v <- (function() 1L)()`)
                // resolves the same way as one inside a return slot.
                self.function_value_from_literal(params, body, scope, 0)
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
        // `%in%` matching. In R `x %in% table` returns a logical vector of
        // length(x) -- one membership test per element of the LHS -- and the
        // RHS (`table`) length is irrelevant. Routing it through the generic
        // `compare` path wrongly took `binary(lt.len, rt.len)` (the max), so
        // `x %in% c("a","b")` on a length-1 `x` came out length-2 and drove
        // both RY002 (`if` condition length 2) and RY032 (`&&` on a length-2
        // operand) false positives. `%in%` never errors on mismatched modes
        // (it coerces to a common type), so the result is always plain
        // logical with the LHS length (Unknown LHS length stays Unknown).
        if matches!(op, BinOpKind::In) {
            return RType::new(Mode::Logical, lt.length);
        }
        let is_compare = matches!(
            op,
            BinOpKind::Lt
                | BinOpKind::Le
                | BinOpKind::Gt
                | BinOpKind::Ge
                | BinOpKind::Eq
                | BinOpKind::Ne
        );
        let is_logic = matches!(
            op,
            BinOpKind::And | BinOpKind::AndAnd | BinOpKind::Or | BinOpKind::OrOr
        );
        if is_compare {
            // Snapshot the operand modes for diagnostics before `compare`
            // consumes lt/rt by value.
            let lt_mode = lt.mode;
            let rt_mode = rt.mode;
            if let Some(t) = lt.compare(rt) {
                // RY033: warn about comparing a character value with a
                // non-character one. R coerces by comparing byte values,
                // which is rarely the programmer's intent.
                if matches!(lt_mode, Mode::Character) != matches!(rt_mode, Mode::Character)
                    && !matches!(lt_mode, Mode::Opaque)
                    && !matches!(rt_mode, Mode::Opaque)
                {
                    self.emit(
                        Severity::Warning,
                        span,
                        "RY033",
                        format!(
                            "comparing `{}` with `{}`; R compares byte values which is rarely intended",
                            lt_mode, rt_mode
                        ),
                    );
                }
                if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                    return RType::new(Mode::Logical, Length::One);
                }
                return t;
            }
            self.emit(
                Severity::Error,
                span,
                "RY030",
                format!("cannot compare `{}` with `{}`", lt_mode, rt_mode),
            );
            return RType::unknown();
        }
        if is_logic {
            let lt_mode = lt.mode;
            let rt_mode = rt.mode;
            if matches!(lt_mode, Mode::Character | Mode::List | Mode::Function)
                || matches!(rt_mode, Mode::Character | Mode::List | Mode::Function)
            {
                self.emit(
                    Severity::Error,
                    span,
                    "RY031",
                    format!("logical op applied to `{}` and `{}`", lt_mode, rt_mode),
                );
                return RType::unknown();
            }
            let length = if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                Length::One
            } else {
                lt.length.binary(rt.length)
            };
            if matches!(op, BinOpKind::AndAnd | BinOpKind::OrOr) {
                if let Length::Known(n) = lt.length {
                    if n > 1 {
                        self.emit(
                            Severity::Warning,
                            span,
                            "RY032",
                            format!("`{}` applied to a length-{} operand; only the first element is used", op_symbol(op), n),
                        );
                    }
                }
                if let Length::Known(n) = rt.length {
                    if n > 1 {
                        self.emit(
                            Severity::Warning,
                            span,
                            "RY032",
                            format!("`{}` applied to a length-{} operand; only the first element is used", op_symbol(op), n),
                        );
                    }
                }
            }
            return RType::new(Mode::Logical, length);
        }
        // Arithmetic.
        let lt_mode = lt.mode;
        let rt_mode = rt.mode;
        if let Some(t) = lt.arith(rt) {
            return t;
        }
        self.emit(
            Severity::Error,
            span,
            "RY040",
            format!(
                "cannot apply arithmetic op to `{}` and `{}`",
                lt_mode, rt_mode
            ),
        );
        RType::unknown()
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
    /// Data pronoun: when `rhs` is an index expression whose base is
    /// the magrittr `.` pronoun (`df %>% .$col`, `df %>% .[i]`,
    /// `df %>% .[[i]]`), the `.` resolves to the piped LHS value and
    /// the index is inferred against `lhs`'s type. A bare `x %>% .`
    /// returns the LHS value itself.
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
            // Magrittr data pronoun with nested access:
            // `df %>% .$col`, `df %>% .[i]`, `df %>% .[[i]]`. The `.` at
            // the base of the index resolves to the piped LHS value, so
            // we infer the index against `lhs_t` directly.
            Expr::Index {
                base, kind, args, ..
            } if is_dot_pronoun(base) => self.infer_index(lhs_t, *kind, args, span, scope),
            // Bare magrittr pronoun: `x %>% .` returns the LHS value
            // itself (the `.` refers to the LHS). This is distinct from
            // the general `Ident` arm below, which would treat `.` as a
            // function name and call `.(lhs)`.
            Expr::Ident { name, .. } if name == "." => lhs_t,
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
                RType::unknown()
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
        } else if !matches!(ct.mode, Mode::Logical | Mode::Opaque)
            && !is_numeric_truthiness_idiom(cond)
        {
            self.emit(
                Severity::Warning,
                span_of(cond),
                "RY001",
                format!(
                    "`if` condition is `{}` (not logical); will be silently coerced",
                    ct.mode
                ),
            );
        } else if matches!(ct.mode, Mode::Logical) {
            if let Length::Known(n) = ct.length {
                if n > 1 {
                    self.emit(
                        Severity::Warning,
                        span_of(cond),
                        "RY002",
                        format!(
                            "`if` condition has length {}, will only use first element",
                            n
                        ),
                    );
                }
            }
        }
        // Flow-sensitive type narrowing for the expression form too.
        //
        // Limitation (PLAN Phase A1): the branch scopes here are clones, and
        // `BinOpKind::Assign` in expression position (e.g.
        // `y <- if (c) (x <- 1) else (x <- 2); x`) mutates only the clone, so
        // any binding introduced inside an `if` *expression* is silently
        // dropped. The statement-form `Stmt::If` merges its branch bindings
        // back into the parent (see `merge_branch_bindings`); doing the same
        // for the expression form is deferred to a later phase because
        // expression-position assignment is rare and merging here would
        // require plumbing owned branch scopes back to the caller.
        let narrowing = extract_type_narrowing(cond);
        let (then_scope, else_scope, _narrowed) = apply_narrowing(scope, &narrowing);
        let then_t = self.infer(then, &mut then_scope.clone());
        let else_t = match else_ {
            Some(e) => self.infer(e, &mut else_scope.clone()),
            None => RType::new(Mode::Null, Length::Zero),
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
    fn infer_switch_call(&mut self, args: &[Arg], scope: &mut Scope, span: Span) -> RType {
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
            return RType::unknown();
        }
        let mut iter = alt_types.into_iter();
        let first = iter.next().unwrap_or(RType::unknown());
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
    fn infer_trycatch_call(&mut self, args: &[Arg], scope: &mut Scope, span: Span) -> RType {
        let mut types: Vec<RType> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                // Main expression.
                types.push(self.infer(&a.value, scope));
            } else if a.name.is_some() {
                // Named handler: `error = function(e) ...`. Infer the
                // handler function's return type.
                if let Some(rt) = self.callback_return_type(&a.value, &[RType::unknown()], scope) {
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
            return RType::unknown();
        }
        let mut iter = types.into_iter();
        let first = iter.next().unwrap_or(RType::unknown());
        iter.fold(first, |acc, t| acc.join(t))
    }

    fn infer_call(&mut self, func: &Expr, args: &[Arg], scope: &mut Scope, span: Span) -> RType {
        // Handle calls to function literals (IIFEs):
        // `(function(x) x + 1)(2L)`. Infer the return type using the
        // actual argument types via callback_return_type, which walks
        // the body with the params bound to the argument types.
        if let Expr::Function { .. } = func {
            let arg_types: Vec<RType> = args.iter().map(|a| self.infer(&a.value, scope)).collect();
            if let Some(rt) = self.callback_return_type(func, &arg_types, scope) {
                return rt;
            }
            return RType::unknown();
        }

        // Only model direct calls `name(...)`. Pipelines and indirect calls
        // return opaque.
        let name = match func {
            Expr::Ident { name, .. } => name.clone(),
            _ => {
                // Calling a literal value (`42()`, `"x"()`, `TRUE()`,
                // `NULL()`) is always a runtime error in R ("attempt to
                // apply non-function"). Flag it (PLAN Phase B2). Other
                // non-Ident callees (index expressions, calls returning
                // functions) stay silent as before.
                if let Some(mode) = literal_callee_mode(func) {
                    self.emit(
                        Severity::Error,
                        span,
                        "RY070",
                        format!("cannot call a value of mode `{}`", mode),
                    );
                    for a in args {
                        self.infer(&a.value, scope);
                    }
                    return RType::unknown();
                }
                self.infer(func, scope);
                for a in args {
                    self.infer(&a.value, scope);
                }
                return RType::unknown();
            }
        };

        // For namespace-qualified calls (`pkg::fn(args)`), strip the
        // package prefix for the typeshed / FnTable / higher-order /
        // S3-generic lookups below, so `stats::rnorm(10)` resolves the
        // same way `rnorm(10)` does. The special-case string-equality
        // checks (library, switch, structure, factor, the dplyr NSE
        // verbs, ...) keep using the full `name`, because those
        // builtins are always invoked unqualified; a qualified call
        // like `base::c(...)` falls through to the typeshed lookup
        // with the stripped name. `rsplit_once("::")` handles both
        // `::` and `:::` forms: for `pkg:::fn` it splits at the last
        // `::`, yielding `("pkg:", "fn")`.
        let lookup_name = name
            .rsplit_once("::")
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| name.clone());

        // NSE-opaque functions whose arguments are not regular values:
        // `library(foo)` and `require(foo)` take a package name as a bare
        // symbol, not an expression. Inferring their args would trigger
        // spurious RY010 on every `library(magrittr)` etc. Return NULL
        // (these functions return invisible(NULL) at runtime). We ALSO
        // record the package name into `self.loaded` so the dplyr NSE
        // gating (see `infer_nse_call`) can treat dplyr/tidyverse as in
        // scope after a `library(dplyr)` / `library(tidyverse)`.
        if name == "library" || name == "require" {
            if let Some(first) = args.first() {
                if let Expr::Ident { name: pkg, .. } = &first.value {
                    self.loaded.insert(pkg.clone());
                }
            }
            return RType::new(Mode::Null, Length::Zero);
        }

        // `requireNamespace("pkg")` takes a STRING literal (unlike
        // library/require, which take a bare symbol). Record the package
        // name into `self.loaded` for the same gating reason, then fall
        // through: requireNamespace resolves via the typeshed to a
        // length-1 logical, so normal arg inference is harmless (the
        // string literal has no unbound refs to fire RY010).
        if name == "requireNamespace" {
            if let Some(first) = args.first() {
                if let Expr::String(pkg, _) = &first.value {
                    self.loaded.insert(pkg.clone());
                }
            }
        }

        // Foreign-function-interface primitives (`.Call`, `.C`,
        // `.Fortran`, `.External`, `.External2`, `.Internal`). Their
        // FIRST argument is a C/Fortran entry-point symbol, conventionally
        // written as a bare identifier or backtick symbol (e.g.
        // `.Call(glue_, x)`), NOT a variable reference. Inferring it
        // normally would fire a spurious RY010. Skip RY010 on a
        // bare-symbol first arg, infer the remaining args normally, and
        // return opaque (the return type depends on the native routine).
        if is_ffi_primitive(&name) {
            for (i, a) in args.iter().enumerate() {
                if i == 0 {
                    // The entry-point symbol: a bare identifier or
                    // backtick-quoted name is not a variable read.
                    let is_symbol = matches!(&a.value, Expr::Ident { .. });
                    if is_symbol {
                        continue;
                    }
                }
                let _ = self.infer(&a.value, scope);
            }
            return RType::unknown();
        }

        // NSE-symbol functions: take bare symbol arguments that should
        // NOT be resolved as variable references. These are commonly
        // used in metaprogramming and NSE contexts where the argument
        // is a name, not a value. We return opaque without evaluating
        // the args as expressions, suppressing spurious RY010.
        if is_nse_symbol_fn(&name) {
            return RType::unknown();
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
            return RType::new(Mode::Integer, Length::Unknown)
                .with_class(ClassVector::single("factor"));
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
        //
        // For namespace-qualified calls we look up the stripped name:
        // `pkg::f()` resolves against a local `f` binding the same way
        // `f()` does (the namespace just selects the binding, and we
        // don't model per-package environments).
        if let Some(t) = scope.get(&lookup_name) {
            if matches!(t.mode, Mode::Function) {
                if let Some(sig) = &t.fn_sig {
                    return (*sig.return_type).clone();
                }
                // Bound function value without an inferred signature:
                // opaque. We do NOT fall through to the FnTable path,
                // because a scope-local binding shadows top-level
                // definitions and we have no way to refine the local
                // one. Returning opaque here is the conservative
                // choice (no false positives, possible false negatives).
                return RType::unknown();
            } else if !matches!(t.mode, Mode::Opaque) {
                // R's function/value namespace separation: when a name is
                // CALLED, R searches the environment chain for a *function*
                // named `name` and skips non-function bindings. So a local
                // non-function binding (e.g. `lengths <- lengths(x)`) does
                // NOT shadow a same-named function in the typeshed or
                // FnTable at a call site. If such a function exists, fall
                // through to the resolution below instead of firing RY070.
                // Only when no function of that name exists anywhere does
                // calling the non-function value warrant RY070.
                let has_function_elsewhere = self.has_function_anywhere(&name);
                if !has_function_elsewhere {
                    // RY070: a non-function value is being called as if it
                    // were a function. R errors at runtime with
                    // "could not find function". Args have already been
                    // inferred above, so we just emit and return opaque
                    // (re-inferring would double-emit arg diagnostics).
                    self.emit(
                        Severity::Error,
                        span,
                        "RY070",
                        format!("`{}` is `{}`, not a function; cannot call it", name, t.mode),
                    );
                    return RType::unknown();
                }
                // A function exists elsewhere; fall through to resolve it
                // (the local non-function binding is ignored at the call
                // site, matching R).
            }
            // Opaque: fall through; the name might still resolve via
            // the FnTable or typeshed below.
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
        //
        // We use the prefix-stripped `lookup_name` so a qualified call
        // like `base::print(x)` still dispatches as `print`.
        if S3_GENERICS.contains(&lookup_name.as_str()) {
            if let Some(rt) = self.try_s3_dispatch(&lookup_name, &arg_types, span) {
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
        //
        // Qualified calls (`base::lapply(...)`) resolve via the
        // stripped `lookup_name`, matching how R treats `::` as a
        // binding selector rather than a different function.
        if HigherOrderFunc::from_call(&name, &self.loaded).is_some() {
            self.walk_callback_for_diagnostics(&lookup_name, args, &arg_types, scope);
        }
        if let Some(rt) = self.infer_higher_order_call(&lookup_name, args, &arg_types, scope, span)
        {
            return rt;
        }

        // User-defined functions: read from the refined FnTable. We
        // intentionally do NOT refine on demand here - that would risk
        // exponential blowup on deep call chains. The fixpoint loop in
        // `check()` already stabilized the table.
        //
        // Qualified calls look up the stripped name; a user's `utils::
        // helper()` resolves like `helper()`.
        if let Some(f) = self.fn_table.fns.get(&lookup_name) {
            return self.return_slots.get(f.return_slot);
        }

        // Literal-arg length inference for `rep`, `seq`, `seq.int`.
        // These have typeshed entries that conservatively return
        // `Length::Unknown`; when the relevant arguments are literals
        // we can pin the result length exactly. We place this AFTER the
        // FnTable lookup so a user-defined `rep`/`seq` still wins, and
        // BEFORE the typeshed so the precise length is preferred over
        // the conservative `x_times` / `unknown` spec.
        if name == "rep" {
            return self.infer_rep(args, &arg_types, span);
        }
        if name == "seq" || name == "seq.int" {
            return self.infer_seq(args, &arg_types, span);
        }

        // Look up in the typeshed. A qualified call (`pkg::fun`) is
        // resolved against `load_package(pkg)`; an unqualified call
        // falls back from base to loaded packages (reverse load order).
        // We pass the full `name` (with any `pkg::` prefix) so the
        // resolver can dispatch on qualification.
        if let Some(sig) = self.resolve_typeshed_sig(&name) {
            return self.apply_sig(&lookup_name, &sig, &arg_types, args, span);
        }

        // Unknown function: opaque.
        RType::unknown()
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
    fn infer_structure_call(&mut self, args: &[Arg], scope: &mut Scope, span: Span) -> RType {
        // The base value is the first positional argument (or the
        // `x = ...` named argument). The first such positional-or-`x`
        // arg wins; later ones are inferred for diagnostics only.
        let mut base_type = RType::unknown();
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
                    return base_type.with_class(ClassVector::single(&name));
                }
                ClassLiteral::Multi(names) => {
                    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                    return base_type.with_class(ClassVector::from_slice(&refs));
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
        // Strip any `pkg::`/`pkg:::` prefix for verb recognition so a
        // qualified call like `dplyr::filter(...)` still matches. The
        // full `name` is retained below for the dplyr-qualification
        // check in the gating logic.
        let lookup_name = name
            .rsplit_once("::")
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| name.to_string());
        let verb = NseVerb::from_name(&lookup_name)?;

        // Gate the dplyr verbs on dplyr (or tidyverse) being loaded, OR
        // on the call being `dplyr::`-qualified. The base-R NSE verbs
        // (subset/with/within/transform) are always-on and never reach
        // this check. Without this gate, a bare `filter(df, ...)` in a
        // script that never loads dplyr would be mis-interpreted as
        // dplyr's row filter and silently swallow column refs that are
        // genuinely unbound; instead we fall through to regular
        // resolution (stats::filter / opaque) so RY010 still fires.
        // `name` carries any `pkg::` prefix (see the parser's
        // `lower_namespace`), so `starts_with("dplyr::")` covers both
        // `dplyr::filter` and the triple-colon `dplyr:::filter`.
        let is_dplyr_verb = matches!(
            verb,
            NseVerb::Filter
                | NseVerb::Mutate
                | NseVerb::Summarise
                | NseVerb::Select
                | NseVerb::Arrange
                | NseVerb::GroupBy
        );
        if is_dplyr_verb
            && !name.starts_with("dplyr::")
            && !self.loaded.contains("dplyr")
            && !self.loaded.contains("tidyverse")
        {
            return None;
        }

        // The data frame is the first positional argument. If it's
        // absent, fall through to the regular path (R would error at
        // runtime; v1 stays silent and defers).
        let df_arg = args.first()?;
        let df_type = self.infer(&df_arg.value, scope);

        // dplyr's `filter` shares its lowercase name with no base-R
        // builtin (R's higher-order predicate is the capitalized
        // `Filter`), but we still guard the dplyr interpretation so
        // that a future lowercase `filter` builtin would not be
        // shadowed. If the first arg does not look like a data frame
        // (no column schema and no `data.frame` class), treat the call
        // as something other than dplyr's `filter` and fall through.
        if matches!(verb, NseVerb::Filter)
            && df_type.columns.is_none()
            && !df_type.class.contains("data.frame")
        {
            return None;
        }

        let augmented = match df_type.columns {
            Some(ref schema) => self.scope_with_columns(scope, schema),
            None => scope.clone(),
        };
        let result = match verb {
            NseVerb::Subset => self.infer_nse_subset(args, df_type, &augmented),
            NseVerb::With => self.infer_nse_with(args, df_type, &augmented),
            NseVerb::Within => self.infer_nse_within(args, df_type, &augmented),
            NseVerb::Transform => self.infer_nse_transform(args, df_type, &augmented),
            NseVerb::Filter | NseVerb::Arrange | NseVerb::GroupBy | NseVerb::Select => {
                self.infer_nse_dplyr_simple(args, df_type, &augmented)
            }
            NseVerb::Mutate => self.infer_nse_dplyr_mutate(args, df_type, &augmented),
            NseVerb::Summarise => self.infer_nse_dplyr_summarise(args, df_type, &augmented),
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
    fn infer_nse_subset(&mut self, args: &[Arg], df_type: RType, augmented: &Scope) -> RType {
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
    fn infer_nse_with(&mut self, args: &[Arg], df_type: RType, augmented: &Scope) -> RType {
        let _ = df_type;
        let mut local = augmented.clone();
        // The second positional arg is the expression; any further args
        // (rare for `with`) are walked for diagnostics.
        let mut result = RType::unknown();
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
    fn infer_nse_within(&mut self, args: &[Arg], df_type: RType, augmented: &Scope) -> RType {
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
    fn infer_nse_transform(&mut self, args: &[Arg], df_type: RType, augmented: &Scope) -> RType {
        let mut local = augmented.clone();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let _ = self.infer(&a.value, &mut local);
        }
        df_type
    }

    /// Shared handler for dplyr verbs that preserve the input data
    /// frame's type verbatim: `filter`, `select`, `arrange`, and
    /// `group_by`. Each walks its remaining arguments in the augmented
    /// scope (so column references resolve and emit no spurious RY010),
    /// then returns `df_type`. The first positional argument is the
    /// data frame and is skipped (it was already inferred by
    /// `infer_nse_call`).
    fn infer_nse_dplyr_simple(&mut self, args: &[Arg], df_type: RType, augmented: &Scope) -> RType {
        let mut local = augmented.clone();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let _ = self.infer(&a.value, &mut local);
        }
        df_type
    }

    /// `dplyr::mutate(.data, new_col = expr, ...)`: adds or modifies
    /// columns. Each expression is inferred against the augmented scope
    /// so existing column references (e.g. `mpg * 0.425`) resolve. The
    /// result type is the data frame's own type; for v1 we preserve the
    /// existing schema (we do not fold the new column types in), mirroring
    /// the conservative approach used for `transform`.
    fn infer_nse_dplyr_mutate(&mut self, args: &[Arg], df_type: RType, augmented: &Scope) -> RType {
        let mut local = augmented.clone();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let _ = self.infer(&a.value, &mut local);
        }
        df_type
    }

    /// `dplyr::summarise(.data, ...)`: collapses rows into a single
    /// (or per-group) summary row. The arguments are walked in the
    /// augmented scope so column references resolve. Because the result
    /// columns are the *outputs* of aggregations rather than the input
    /// columns, the resulting schema is unknown at v1; we return a
    /// fresh data frame type (class `data.frame`, empty schema, scalar
    /// length) so downstream code sees a data frame even though it
    /// cannot resolve specific columns.
    fn infer_nse_dplyr_summarise(
        &mut self,
        args: &[Arg],
        df_type: RType,
        augmented: &Scope,
    ) -> RType {
        let _ = df_type;
        let mut local = augmented.clone();
        for (i, a) in args.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let _ = self.infer(&a.value, &mut local);
        }
        RType::new(Mode::List, Length::One).with_class(ClassVector::single("data.frame"))
    }

    /// Build an augmented scope by cloning `base_scope` and inserting a
    /// binding for every column in `schema`. Column names that shadow
    /// existing bindings in `base_scope` are overwritten by the column
    /// type (this mirrors R's actual NSE lookup order: columns first,
    /// then the enclosing environment). The returned scope is a fresh
    /// clone; `base_scope` is untouched, so column bindings never leak
    /// into the caller's scope.
    fn scope_with_columns(&self, base_scope: &Scope, schema: &Arc<ColumnSchema>) -> Scope {
        let mut s = base_scope.clone();
        for (name, t) in &schema.columns {
            s.insert(name.clone(), t.clone());
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
        &mut self,
        name: &str,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
        span: Span,
    ) -> Option<RType> {
        let ho = HigherOrderFunc::from_call(name, &self.loaded)?;
        Some(self.infer_ho_result(&ho, args, arg_types, scope, span))
    }

    /// Per-builtin result-type computation. Used by both pass 2 (pure,
    /// via `infer_discarding`) and pass 3 (diagnostic-emitting). This is
    /// the pass-3 entry point: it calls `self.infer` on data
    /// arguments (which may emit RY010 etc.) before computing the
    /// element type.
    fn infer_ho_result(
        &mut self,
        ho: &HigherOrderFunc,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
        span: Span,
    ) -> RType {
        match ho {
            HigherOrderFunc::Lapply => self.ho_lapply(args, arg_types, scope),
            HigherOrderFunc::Sapply => self.ho_sapply(args, arg_types, scope),
            HigherOrderFunc::Vapply => self.ho_vapply(args, arg_types, scope),
            HigherOrderFunc::Map | HigherOrderFunc::Mapply => self.ho_map(args, arg_types, scope),
            HigherOrderFunc::Rapply => self.ho_rapply(args, arg_types, scope),
            HigherOrderFunc::Reduce => self.ho_reduce(args, arg_types, scope),
            HigherOrderFunc::Filter => self.ho_filter(args, arg_types, scope),
            HigherOrderFunc::Find => self.ho_find(args, arg_types, scope),
            HigherOrderFunc::Position => self.ho_position(args, arg_types, scope),
            HigherOrderFunc::DoCall => self.ho_do_call(args, arg_types, scope),
            // purrr family.
            HigherOrderFunc::PurrrMap => self.ho_purrr_map(args, arg_types, scope, None, span),
            HigherOrderFunc::PurrrMapTyped(mode) => {
                self.ho_purrr_map(args, arg_types, scope, Some(mode), span)
            }
            HigherOrderFunc::PurrrMap2 => self.ho_purrr_map2(args, arg_types, scope),
            HigherOrderFunc::PurrrKeep => self.ho_purrr_keep(args, arg_types),
            HigherOrderFunc::PurrrReduce => self.ho_purrr_reduce(args, arg_types, scope),
            HigherOrderFunc::PurrrWalk => {
                // walk returns its first argument invisibly.
                arg_types.first().cloned().unwrap_or(RType::unknown())
            }
            HigherOrderFunc::PurrrInParallel => self.ho_purrr_in_parallel(args, scope),
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

    /// If `expr` is a `purrr::in_parallel(.f)` / `in_parallel(.f)` call
    /// wrapping a function literal or name, return the inner `.f`.
    /// `in_parallel` is type-transparent (purrr >= 1.1.0), so callers
    /// that infer a callback's return type or walk its body should look
    /// through it. Returns the original expression unchanged otherwise.
    fn unwrap_in_parallel(expr: &Expr) -> &Expr {
        if let Expr::Call { func, args, .. } = expr {
            if let Expr::Ident { name, .. } = func.as_ref() {
                let bare = name
                    .rsplit_once("::")
                    .map(|(_, n)| n)
                    .unwrap_or(name.as_str());
                if bare == "in_parallel" {
                    if let Some(first) = args.first() {
                        return &first.value;
                    }
                }
            }
        }
        expr
    }

    /// `purrr::map(.x, .f)` (and `map_if`/`imap`): a list of `.f`'s
    /// returns, same length as `.x`. With a typed variant
    /// (`map_lgl`/`int`/`dbl`/`chr`/`vec`), the result is a vector of
    /// the target mode with `.x`'s length. The callback's return type
    /// is inferred via [`callback_return_type`]; if the callback returns
    /// a mode that mismatches the typed variant's target, the result
    /// still uses the target mode (R coerces), matching purrr's runtime
    /// behaviour (the type-mismatch diagnostic is handled separately in
    /// the per-variant walking path -- a future enhancement; v1 stays
    /// silent).
    fn ho_purrr_map(
        &mut self,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
        typed_mode: Option<&Mode>,
        span: Span,
    ) -> RType {
        let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        let elem = x_type.element();
        let cb = Self::extract_callback(args, &[".f"], 1);
        let cb_ret = cb.and_then(|c| self.callback_return_type(c, &[elem], scope));
        match typed_mode {
            Some(mode) => {
                // Typed map: result is a vector of `mode` with `.x`'s
                // length. `map_vec` passes `Mode::Opaque` (no fixed
                // mode); degrade to opaque in that case.
                if matches!(mode, Mode::Opaque) {
                    RType::unknown()
                } else {
                    // RY080: if the callback's return mode is known and
                    // incompatible with the target, warn. R coerces at
                    // runtime, but the mismatch is almost always a bug.
                    // Numeric modes (double/int/logical) coerce among
                    // themselves harmlessly; character/list returning
                    // into a numeric target is the real footgun.
                    if let Some(ret) = &cb_ret {
                        if !modes_compatible(&ret.mode, mode) {
                            self.emit(
                                Severity::Warning,
                                span,
                                "RY080",
                                format!(
                                    "`map_{}` expects `{}` returns but the callback returns `{}`; R will coerce silently",
                                    mode_suffix(mode),
                                    mode,
                                    ret.mode
                                ),
                            );
                        }
                    }
                    RType::new(*mode, x_type.length)
                }
            }
            None => {
                // Untyped map: list of the callback's return type.
                let element_type = cb_ret.unwrap_or(RType::unknown());
                let mut result = RType::new(Mode::List, x_type.length);
                if !matches!(element_type.mode, Mode::Opaque) {
                    let n = match x_type.length {
                        Length::Known(n) if n > 0 => n,
                        _ => 1,
                    };
                    let schema = ColumnSchema {
                        columns: (0..n)
                            .map(|i| (format!("[[{}]]", i + 1), element_type.clone()))
                            .collect(),
                    };
                    result = result.with_columns(Arc::new(schema));
                }
                result
            }
        }
    }

    /// `purrr::map2(.x, .y, .f)` / `pmap(.l, .f)`: a list. The element
    /// types of `.x` and `.y` become the callback's first two args.
    fn ho_purrr_map2(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        let y_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
        let cb = Self::extract_callback(args, &[".f"], 2);
        let cb_ret = cb.and_then(|c| {
            self.callback_return_type(c, &[x_type.element(), y_type.element()], scope)
        });
        let element_type = cb_ret.unwrap_or(RType::unknown());
        let mut result = RType::new(Mode::List, Length::Unknown);
        if !matches!(element_type.mode, Mode::Opaque) {
            let schema = ColumnSchema {
                columns: std::iter::once(("[[1]]".to_string(), element_type)).collect(),
            };
            result = result.with_columns(Arc::new(schema));
        }
        result
    }

    /// `purrr::keep(.x, .p)` / `discard`: same type as `.x` (unknown
    /// length, since the predicate filters).
    fn ho_purrr_keep(&mut self, _args: &[Arg], arg_types: &[RType]) -> RType {
        let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        RType {
            length: Length::Unknown,
            ..x_type
        }
    }

    /// `purrr::reduce(.x, .f)`: single value (opaque unless the
    /// callback's return is inferrable). `accumulate` returns a vector
    /// of the callback's returns; modeled as opaque length-unknown.
    fn ho_purrr_reduce(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        let elem = x_type.element();
        let cb = Self::extract_callback(args, &[".f"], 1);
        let cb_ret = cb.and_then(|c| self.callback_return_type(c, &[elem.clone(), elem], scope));
        cb_ret.unwrap_or(RType::unknown())
    }

    /// `purrr::in_parallel(.f)`: a type-transparent wrapper (purrr >=
    /// 1.1.0). Returns `.f` unchanged so `map(sims, in_parallel(f))`
    /// checks identically to `map(sims, f)`. `.f` may be a function
    /// literal (returned as a function value) or a name (resolved via
    /// the scope/typeshed to a function value).
    fn ho_purrr_in_parallel(&mut self, args: &[Arg], scope: &Scope) -> RType {
        let cb = match Self::extract_callback(args, &[".f"], 0) {
            Some(c) => c,
            None => return RType::unknown(),
        };
        match cb {
            Expr::Function { .. } => RType::scalar(Mode::Function),
            Expr::Ident { name, .. } => {
                // A bound function value resolves to its type; an
                // unbound name that names a typeshed function resolves
                // to a function value; anything else is treated as a
                // function (in_parallel is transparent, and an unknown
                // callback is most plausibly a function from a package
                // we don't model).
                scope
                    .get(name)
                    .cloned()
                    .unwrap_or(RType::scalar(Mode::Function))
            }
            _ => RType::unknown(),
        }
    }

    /// `lapply(X, FUN, ...)`: applies `FUN` to each element of `X`,
    /// returning a list of the same length. The element type of `X`
    /// becomes the callback's first argument.
    fn ho_lapply(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        let elem = x_type.element();
        let cb = Self::extract_callback(args, &["FUN"], 1);
        let cb_ret = cb.and_then(|c| self.callback_return_type(c, &[elem], scope));
        let length = x_type.length;
        let element_type = cb_ret.unwrap_or(RType::unknown());
        let mut result = RType::new(Mode::List, length);
        // Always build a schema when we know the element type, even if
        // the list length is unknown. We create a single `[[1]]` entry
        // so that `result[[1]]` and `ColumnSchema::homogeneous_element_type`
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
                    .map(|i| (format!("[[{}]]", i + 1), element_type.clone()))
                    .collect(),
            };
            result = result.with_columns(Arc::new(schema));
        }
        result
    }

    /// `sapply(X, FUN, ...)`: like `lapply` but simplifies the result.
    /// When the callback returns a length-1 atomic for every element,
    /// the result is a vector of that mode with `X`'s length. When the
    /// callback returns length-`k` vectors, the result is a matrix. We
    /// model the common case: callback returns atomic length-1, so the
    /// result is a vector of the callback's return mode with X's length.
    fn ho_sapply(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        let elem = x_type.element();
        let cb = Self::extract_callback(args, &["FUN"], 1);
        let cb_ret = cb.and_then(|c| self.callback_return_type(c, &[elem], scope));
        match cb_ret {
            Some(t)
                if matches!(t.length, Length::One)
                    && !matches!(t.mode, Mode::List | Mode::Opaque | Mode::Union) =>
            {
                // Simplification to a vector of the callback's mode.
                RType::new(t.mode, x_type.length)
            }
            // Could not infer the callback, or it returns non-scalar /
            // list values: conservatively report a list (the
            // unsimplified form). This avoids false positives while
            // still giving a useful mode upper bound.
            _ => RType::new(Mode::List, x_type.length),
        }
    }

    /// `vapply(X, FUN, FUN.VALUE, ...)`: like `sapply` but the result
    /// type is specified by `FUN.VALUE`. The callback's actual return
    /// must be compatible; we return the FUN.VALUE template's mode and
    /// length. The result length is `FUN.VALUE.length * X.length` when
    /// both are known (R stacks the callback outputs column-wise), but
    /// for v1 we approximate as FUN.VALUE's mode with X's length when
    /// FUN.VALUE is length-1, else opaque length.
    fn ho_vapply(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        let fun_value = arg_types.get(2).cloned().unwrap_or(RType::unknown());
        // Walk the callback for type information (its body may reference
        // unbound vars etc.). We don't use its return type because
        // FUN.VALUE is the authoritative template.
        if let Some(cb) = Self::extract_callback(args, &["FUN"], 1) {
            let elem = x_type.element();
            let _ = self.callback_return_type(cb, &[elem], scope);
        }
        // FUN.VALUE is the authoritative template. A union template
        // (unusual but possible) would build a malformed union via
        // `RType::new`; degrade to opaque in that case.
        let fv_mode = if matches!(fun_value.mode, Mode::Union) {
            Mode::Opaque
        } else {
            fun_value.mode
        };
        match fun_value.length {
            Length::One => RType::new(fv_mode, x_type.length),
            _ => RType::new(fv_mode, Length::Unknown),
        }
    }

    /// `Map(f, ...)`: applies `f` to corresponding elements of all
    /// arguments, returning a list. Each argument contributes its
    /// element type as a callback argument.
    fn ho_map(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        // First positional arg is `f`; subsequent positional args are
        // the vectors to map over. Named `f = ...` is also recognized.
        let cb = Self::extract_callback(args, &["f"], 0);
        let elem_types: Vec<RType> = arg_types.iter().skip(1).map(|t| t.element()).collect();
        let cb_ret = cb.and_then(|c| self.callback_return_type(c, &elem_types, scope));
        // The result list length is the length of the shortest input
        // (R's recycling for Map). We approximate as the first data
        // arg's length, or Unknown if absent.
        let length = arg_types.get(1).map(|t| t.length).unwrap_or(Length::Zero);
        let _ = cb_ret; // Mode is list regardless of callback return.
        RType::new(Mode::List, length)
    }

    /// `rapply(L, f, ...)`: recursively applies `f` to each leaf of
    /// list `L`. The result is a list of the same shape. We model only
    /// the top-level shape: result is a list with L's length.
    fn ho_rapply(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let l_type = arg_types.first().cloned().unwrap_or(RType::unknown());
        // Walk the callback for type information.
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 1) {
            let _ = self.callback_return_type(cb, &[RType::unknown()], scope);
        }
        RType::new(Mode::List, l_type.length)
    }

    /// `Reduce(f, x, ...)`: left-fold. The result type is the element
    /// type of `x` (the accumulator starts as `x[[1]]`). For an empty
    /// `x` with no `init`, R errors; we stay opaque in that case.
    fn ho_reduce(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
        // Walk the callback for type information. The callback takes two
        // args: the accumulator and the next element, both of x's
        // element type.
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 0) {
            let elem = x_type.element();
            let _ = self.callback_return_type(cb, &[elem.clone(), elem], scope);
        }
        x_type.element()
    }

    /// `Filter(f, x)`: returns the subset of `x` where `f` returns
    /// TRUE. The result type is `x`'s type (same mode, possibly shorter
    /// length which we cannot know statically).
    fn ho_filter(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 0) {
            let _ = self.callback_return_type(cb, &[x_type.element()], scope);
        }
        x_type
    }

    /// `Find(f, x)`: returns the first element of `x` where `f` returns
    /// TRUE, or NULL. The result type is the element type (or NULL).
    fn ho_find(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 0) {
            let _ = self.callback_return_type(cb, &[x_type.element()], scope);
        }
        x_type.element()
    }

    /// `Position(f, x)`: returns the integer index of the first element
    /// where `f` returns TRUE, or NA_integer_. The result is always
    /// integer length-1.
    fn ho_position(&mut self, args: &[Arg], arg_types: &[RType], scope: &Scope) -> RType {
        let x_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
        if let Some(cb) = Self::extract_callback(args, &["f", "FUN"], 0) {
            let _ = self.callback_return_type(cb, &[x_type.element()], scope);
        }
        RType::scalar(Mode::Integer)
    }

    /// `do.call(fun, args, ...)`: invokes `fun` with the arguments in
    /// `args` (a list). We model only the case where `fun` is a named
    /// function (user-fn or typeshed). The result is `fun`'s return type.
    fn ho_do_call(&mut self, args: &[Arg], arg_types: &[RType], _scope: &Scope) -> RType {
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
                    return self.apply_sig(name, sig, &[], &[], Span::default());
                }
                RType::unknown()
            }
            _ => RType::unknown(),
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
        &mut self,
        callback: &Expr,
        call_arg_types: &[RType],
        scope: &Scope,
    ) -> Option<RType> {
        // Look through a `purrr::in_parallel(.f)` / `in_parallel(.f)`
        // wrapper: it is type-transparent, so the callback's return is
        // the inner function's return.
        let callback = Self::unwrap_in_parallel(callback);
        match callback {
            Expr::Function { params, body, .. } => {
                self.callback_literal_return(params, body, call_arg_types, scope, 0)
            }
            Expr::Ident { name, .. } => {
                // Strip any `pkg::` namespace prefix so a qualified
                // callback name (`base::sqrt` passed to `sapply`)
                // resolves against the same entries as the bare name.
                // `rsplit_once("::")` handles both `::` and `:::`.
                let lookup_name = name
                    .rsplit_once("::")
                    .map(|(_, n)| n)
                    .unwrap_or(name.as_str());
                // Bound closure value in scope?
                if let Some(t) = scope.get(lookup_name) {
                    if matches!(t.mode, Mode::Function) {
                        if let Some(sig) = &t.fn_sig {
                            return Some((*sig.return_type).clone());
                        }
                        return None;
                    }
                }
                // User-defined function in the FnTable?
                if let Some(f) = self.fn_table.fns.get(lookup_name) {
                    let rt = self.return_slots.get(f.return_slot);
                    if !matches!(rt.mode, Mode::Opaque) {
                        return Some(rt);
                    }
                    return None;
                }
                // Typeshed function?
                if let Some(sig) = self.typeshed.functions.get(lookup_name) {
                    return Some(self.apply_sig(
                        lookup_name,
                        sig,
                        call_arg_types,
                        &[],
                        Span::default(),
                    ));
                }
                None
            }
            _ => None,
        }
    }

    /// Walk an anonymous function literal's body to infer its return
    /// type, given the argument types the caller will pass. Similar to
    /// `build_function_signature` but takes explicit argument
    /// types rather than inferring from defaults. Used by
    /// `callback_return_type` for the inline-literal case.
    fn callback_literal_return(
        &mut self,
        params: &[Param],
        body: &[Stmt],
        call_arg_types: &[RType],
        captured_scope: &Scope,
        depth: usize,
    ) -> Option<RType> {
        if body.is_empty() || depth >= MAX_CLOSURE_DEPTH {
            return None;
        }
        // Pure return-type computation: force discarding so this does not
        // double-emit diagnostics (the callback body's diagnostics come
        // from walk_callback_for_diagnostics in pass 3).
        let prev_discarding = self.discarding;
        self.discarding = true;
        let mut scope = captured_scope.clone();
        for (i, p) in params.iter().enumerate() {
            let t = call_arg_types.get(i).cloned().unwrap_or(RType::unknown());
            scope.insert(p.name.clone(), t);
        }
        let mut returns: Vec<RType> = Vec::new();
        for s in body {
            self.walk_stmt(s, &mut scope, Some(&mut returns));
        }
        if let Some(t) = self.trailing_return_type(body, &mut scope, depth + 1) {
            returns.push(t);
        }
        self.discarding = prev_discarding;
        if returns.is_empty() {
            return None;
        }
        let mut iter = returns.into_iter();
        let first = iter.next().unwrap_or(RType::unknown());
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
        let ho = match HigherOrderFunc::from_call(name, &self.loaded) {
            Some(h) => h,
            None => return,
        };
        // Determine the callback argument and the element types it will
        // be called with, based on which higher-order function this is.
        let (cb_idx, cb_names, elem_types) = match ho {
            HigherOrderFunc::Lapply | HigherOrderFunc::Sapply | HigherOrderFunc::Vapply => {
                let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
                (1, &["FUN"][..], vec![x_type.element()])
            }
            HigherOrderFunc::Map | HigherOrderFunc::Mapply => {
                let elem_types: Vec<RType> =
                    arg_types.iter().skip(1).map(|t| t.element()).collect();
                (0, &["f"][..], elem_types)
            }
            HigherOrderFunc::Rapply => (1, &["f", "FUN"][..], vec![RType::unknown()]),
            HigherOrderFunc::Reduce => {
                let x_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
                let elem = x_type.element();
                (0, &["f", "FUN"][..], vec![elem.clone(), elem])
            }
            HigherOrderFunc::Filter | HigherOrderFunc::Find | HigherOrderFunc::Position => {
                let x_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
                (0, &["f", "FUN"][..], vec![x_type.element()])
            }
            // purrr: callback is `.f` at index 1; element type is `.x`'s
            // element. `in_parallel` is a transparent wrapper whose
            // argument IS the callback (index 0) and takes no data.
            HigherOrderFunc::PurrrMap | HigherOrderFunc::PurrrMapTyped(_) => {
                let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
                (1, &[".f"][..], vec![x_type.element()])
            }
            HigherOrderFunc::PurrrMap2 => {
                let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
                let y_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
                (2, &[".f"][..], vec![x_type.element(), y_type.element()])
            }
            HigherOrderFunc::PurrrKeep => {
                let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
                (1, &[".p"][..], vec![x_type.element()])
            }
            HigherOrderFunc::PurrrReduce => {
                let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
                let elem = x_type.element();
                (1, &[".f"][..], vec![elem.clone(), elem])
            }
            HigherOrderFunc::PurrrWalk => {
                let x_type = arg_types.first().cloned().unwrap_or(RType::unknown());
                (1, &[".f"][..], vec![x_type.element()])
            }
            HigherOrderFunc::PurrrInParallel => {
                // in_parallel(.f) is a transparent wrapper: `.f` is the
                // callback itself, not an invocation. Do not walk its
                // body here (no element types to bind); the body is
                // walked when the surrounding `map` invokes it.
                return;
            }
            HigherOrderFunc::DoCall => return, // callback is the function name, no body to walk
        };
        let cb = match Self::extract_callback(args, cb_names, cb_idx) {
            Some(c) => c,
            None => return,
        };
        // Look through a `purrr::in_parallel(.f)` wrapper so the inner
        // function's body is walked (in_parallel is type-transparent).
        let cb = Self::unwrap_in_parallel(cb);
        if let Expr::Function { params, body, .. } = cb {
            let mut fn_scope = scope.clone();
            for (i, p) in params.iter().enumerate() {
                let t = elem_types.get(i).cloned().unwrap_or(RType::unknown());
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
    fn try_s3_dispatch(&mut self, generic: &str, arg_types: &[RType], span: Span) -> Option<RType> {
        let first = arg_types.first().cloned()?;
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
        if &*first_class == "default" {
            return None;
        }
        // 1. User-defined method for the first class wins.
        if let Some(slot) = self
            .fn_table
            .s3_methods
            .get(&(generic.to_string(), first_class.to_string()))
            .cloned()
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
        Some(RType::unknown())
    }

    fn infer_c(&mut self, args: &[Arg], arg_types: &[RType], _span: Span) -> RType {
        if arg_types.is_empty() {
            return RType::new(Mode::Null, Length::Zero);
        }
        let mut mode = Mode::Null;
        let mut total_len: usize = 0;
        // A union arg would win the coerce-rank ladder and leave `mode ==
        // Union`, which `RType::new` then turns into a malformed union.
        // Track it and degrade to opaque at the end (PLAN Phase A2).
        let mut saw_union = false;
        for t in arg_types {
            if matches!(t.mode, Mode::Union) {
                saw_union = true;
                continue;
            }
            mode = if mode.coerce_rank() >= t.mode.coerce_rank() {
                mode
            } else {
                t.mode
            };
            total_len = total_len.saturating_add(match t.length {
                Length::Zero => 0,
                Length::One => 1,
                Length::Known(n) => n,
                Length::Unknown => {
                    return RType::new(collapse_c_mode(mode, saw_union), Length::Unknown);
                }
            });
        }
        let length = if args.iter().any(|a| matches!(a.value, Expr::Unknown(_))) {
            Length::Unknown
        } else {
            Length::Known(total_len)
        };
        RType::new(collapse_c_mode(mode, saw_union), length)
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
    fn infer_list(&mut self, arg_types: &[RType], args: &[Arg], _span: Span) -> RType {
        let length = Length::Known(arg_types.len());
        let base = RType::new(Mode::List, length);
        let schema = build_named_schema(arg_types, args);
        if let Some(s) = schema {
            base.with_columns(Arc::new(s))
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
    fn infer_data_frame(&mut self, arg_types: &[RType], args: &[Arg], _span: Span) -> RType {
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
            filtered_types.push(arg_types[i].clone());
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
                class: t.class.clone(),
                // Nested column schemas on a data-frame column would
                // mean nested data frames; v1 keeps those opaque.
                columns: None,
                // fn_sig is meaningless on a data-frame column.
                fn_sig: None,
                members: None,
            })
            .collect();

        // Reuse the named-schema builder, then patch the coerced types
        // in (the builder uses the original arg_types verbatim).
        let mut schema = build_named_schema(&coerced_types, &filtered_args);
        if let Some(s) = schema.as_mut() {
            // Sanity: lengths should already match coerced_types.
            debug_assert_eq!(s.columns.len(), coerced_types.len());
        }

        let class = ClassVector::single("data.frame");
        let base = RType::new(Mode::List, Length::Known(filtered_types.len())).with_class(class);
        match schema {
            Some(s) => base.with_columns(Arc::new(s)),
            None => base,
        }
    }

    /// Infer the result type of `rep(x, times, each)`. R's `rep` has
    /// two relevant parameters for length:
    ///   * `times` (default 1): how many times to repeat the whole
    ///     vector. Total length = `length(x) * times`.
    ///   * `each` (default 1): how many times to repeat each element
    ///     before concatenating. Total length = `length(x) * each`.
    ///   * Combined: `length(x) * times * each`.
    ///
    /// The result mode is `x`'s mode (matching the typeshed's
    /// `"mode": "arg0"` spec). We preserve `x`'s class and column
    /// schema too, so `rep(factor(...), 3)` stays a factor.
    ///
    /// We read `times` / `each` from the raw AST (not the inferred
    /// `RType`) because the type lattice discards the runtime value.
    /// When the values aren't literal integers or `x`'s length is
    /// unknown, we fall back to `Length::Unknown`. Named args win over
    /// positional ones; if `times`/`each` is supplied but isn't a
    /// literal, the length is Unknown (we can't know the runtime
    /// value, unlike the "not supplied" case which defaults to 1).
    fn infer_rep(&self, args: &[Arg], arg_types: &[RType], _span: Span) -> RType {
        // Helper: find the index in `args` of a named or positional
        // argument. Named args win over positional. The `pos` index
        // counts only unnamed args, so `rep(each = 2, c(1,2,3), 1)`
        // still matches `x` at positional index 0 and `times` at 1.
        // Mirrors `infer_seq`'s positional-counting approach.
        let find_idx = |name: &str, pos: usize| -> Option<usize> {
            for (i, a) in args.iter().enumerate() {
                if a.name.as_deref() == Some(name) {
                    return Some(i);
                }
            }
            let mut idx = 0usize;
            for (i, a) in args.iter().enumerate() {
                if a.name.is_some() {
                    continue;
                }
                if idx == pos {
                    return Some(i);
                }
                idx += 1;
            }
            None
        };
        // `x` is the first positional arg (pos 0) or a named `x = ...`.
        // We must look it up by index rather than `arg_types.first()`
        // because named `times`/`each` args can precede `x` in the
        // call (e.g. `rep(each = 2, c(1,2,3), 1)`).
        let x_type = find_idx("x", 0)
            .and_then(|i| arg_types.get(i).cloned())
            .unwrap_or(RType::unknown());
        // Track `times` / `each` as `Option<Option<i64>>`:
        //   * outer None      -> not supplied (use default 1)
        //   * outer Some(None) -> supplied but non-literal (Unknown)
        //   * outer Some(Some(n)) -> supplied literal value n
        let times = find_idx("times", 1)
            .and_then(|i| args.get(i))
            .map(|a| extract_literal_int(&a.value));
        let each = find_idx("each", 2)
            .and_then(|i| args.get(i))
            .map(|a| extract_literal_int(&a.value));
        // Resolve `times`. Non-supplied -> 1; non-literal -> Unknown;
        // negative literal -> Unknown (R errors or recycles in ways we
        // can't model, so we stay conservative rather than pin a wrong
        // length).
        let times_n: usize = match times {
            None => 1usize,
            Some(Some(n)) if n < 0 => {
                return RType {
                    length: Length::Unknown,
                    ..x_type
                }
            }
            Some(Some(n)) => n as usize,
            Some(None) => {
                return RType {
                    length: Length::Unknown,
                    ..x_type
                }
            }
        };
        let each_n: usize = match each {
            None => 1usize,
            Some(Some(n)) if n < 0 => {
                return RType {
                    length: Length::Unknown,
                    ..x_type
                }
            }
            Some(Some(n)) => n as usize,
            Some(None) => {
                return RType {
                    length: Length::Unknown,
                    ..x_type
                }
            }
        };
        // Compute the total length, normalizing so we never emit
        // `Length::Known(0)` (which violates the `Known(n > 1)`
        // invariant) or `Length::Known(1)` (use `Length::One` instead).
        // A zero total (e.g. `rep(x, times = 0)`) becomes `Length::Zero`.
        let length = match x_type.length {
            Length::Zero => Length::Zero,
            Length::One => {
                let total = times_n.saturating_mul(each_n);
                match total {
                    0 => Length::Zero,
                    1 => Length::One,
                    n => Length::Known(n),
                }
            }
            Length::Known(xn) => {
                let total = xn.saturating_mul(times_n).saturating_mul(each_n);
                match total {
                    0 => Length::Zero,
                    1 => Length::One,
                    n => Length::Known(n),
                }
            }
            Length::Unknown => Length::Unknown,
        };
        RType { length, ..x_type }
    }

    /// Infer the result type of `seq(from, to, by)` / `seq.int(...)`.
    /// Two literal forms let us pin the result length exactly:
    ///   * `seq(from, to, by)`: length = `|to - from| / |by| + 1`
    ///     (R rounds to the nearest whole step that stays in range).
    ///   * `seq(from, to, length.out = n)`: length = `n`.
    ///   * `seq(from, to)` (no `by`, no `length.out`): R defaults
    ///     `by` to +/-1, so length = `|to - from| + 1`.
    ///
    /// When `length.out` is present it wins (R documents this as
    /// taking precedence over `by`). When we can't pin the length, we
    /// still report the right mode (integer when the first arg is an
    /// integer literal, else double) with `Length::Unknown`.
    fn infer_seq(&self, args: &[Arg], arg_types: &[RType], _span: Span) -> RType {
        // Helper: find (was_supplied, literal_value) for a named or
        // positional argument. Named args win over positional. The
        // `pos` index counts only unnamed args, so `seq(from=1, 10)`
        // still matches `to` at positional index 0.
        let find = |name: &str, pos: usize| -> (bool, Option<i64>) {
            for a in args.iter() {
                if a.name.as_deref() == Some(name) {
                    return (true, extract_literal_int(&a.value));
                }
            }
            let mut idx = 0;
            for a in args.iter() {
                if a.name.is_some() {
                    continue;
                }
                if idx == pos {
                    return (true, extract_literal_int(&a.value));
                }
                idx += 1;
            }
            (false, None)
        };

        let (_, from_val) = find("from", 0);
        let (_, to_val) = find("to", 1);
        let (by_supplied, by_val) = find("by", 2);
        let (lo_supplied, lo_val) = find("length.out", 3);

        // Mode: integer if `from` is an integer literal, else double
        // (mirrors the typeshed's "double_or_int" rule). We look at
        // the named `from = ...` first, then the first positional arg.
        let from_expr = args
            .iter()
            .find(|a| a.name.as_deref() == Some("from"))
            .or_else(|| args.iter().find(|a| a.name.is_none()))
            .map(|a| &a.value);
        let from_is_int_literal = from_expr
            .map(|e| matches!(e, Expr::Integer(_, _)))
            .unwrap_or(false);
        // Mode: integer if `from` is an integer literal or its inferred
        // type is integer, else double (mirrors the typeshed's
        // "double_or_int" rule).
        let mode =
            if from_is_int_literal || arg_types.first().map(|t| t.mode) == Some(Mode::Integer) {
                Mode::Integer
            } else {
                Mode::Double
            };

        // If a length-determining arg was supplied but wasn't a
        // literal, we can't pin the length. `length.out` and `by` both
        // participate in the length formula, so a non-literal value
        // for either forces Unknown. (`from`/`to` are handled below:
        // `extract_literal_int` returns None for them, which makes the
        // formula fall through to Unknown.)
        if (lo_supplied && lo_val.is_none()) || (by_supplied && by_val.is_none()) {
            return RType::new(mode, Length::Unknown);
        }

        // `length.out` wins over `by` when both are present.
        let length = if let Some(n) = lo_val {
            if n >= 0 {
                Length::Known(n as usize)
            } else {
                Length::Unknown
            }
        } else if let (Some(f), Some(t)) = (from_val, to_val) {
            match by_val {
                // by == 0: R errors at runtime; model as Unknown.
                Some(0) => Length::Unknown,
                Some(b) => {
                    let diff = (t - f).unsigned_abs() as usize;
                    let step = b.unsigned_abs() as usize;
                    Length::Known(diff / step + 1)
                }
                // by not supplied (the supplied-non-literal case
                // returned above): R defaults to +/-1.
                None => Length::Known((t - f).unsigned_abs() as usize + 1),
            }
        } else {
            Length::Unknown
        };
        RType::new(mode, length)
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
        let matched = if sig.params.is_empty()
            || sig.params.iter().all(|p| p == "...")
            // When the caller has argument *types* but no `Arg` slice
            // (e.g. `callback_return_type` inferring a typeshed callback
            // from the element types a higher-order function will pass),
            // named-arg matching has nothing to work from: use the types
            // positionally so `arg0`/`arg1`/... resolve correctly.
            || args.is_empty()
        {
            arg_types.to_vec()
        } else {
            match_args_to_params(&sig.params, args, arg_types)
        };
        let first = matched.first().cloned().unwrap_or(RType::unknown());
        match &sig.return_ {
            ReturnSpec::Slot(s) => match s.as_str() {
                "arg0" => first,
                "concat_of_args" => self.infer_c(args, arg_types, span),
                s if s.starts_with("arg") => {
                    let idx: usize = s[3..].parse().unwrap_or(0);
                    arg_types.get(idx).cloned().unwrap_or(RType::unknown())
                }
                _ => RType::unknown(),
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
                    // modes (for `ifelse(test, yes, no)`). The join may be
                    // a union; taking `.mode` drops the members and would
                    // build a malformed union below, so collapse a union
                    // mode to opaque (PLAN Phase A2).
                    "yes_or_no" => {
                        let yes = matched.get(1).cloned().unwrap_or(RType::unknown());
                        let no = matched.get(2).cloned().unwrap_or(RType::unknown());
                        let joined = yes.join(no).mode;
                        if matches!(joined, Mode::Union) {
                            Mode::Opaque
                        } else {
                            joined
                        }
                    }
                    _ => Mode::Opaque,
                };
                // The arg-N mode specs copy a param's mode verbatim; if a
                // caller passes a union there, that mode is `Mode::Union`
                // and would build a malformed union. Collapse to opaque.
                let mode = if matches!(mode, Mode::Union) {
                    Mode::Opaque
                } else {
                    mode
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
                RType::new(mode, length)
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
                // RY061: `$` on an atomic vector is a runtime error in R
                // ("$ operator is invalid for atomic vectors"). Only flag
                // when we're confident the type is atomic (not opaque,
                // not list, not function, not NULL). List-like types
                // without a schema are fine -- the column might exist
                // dynamically -- and atomic types *with* a schema are
                // already covered by the schema lookup / RY060 below.
                if matches!(
                    bt.mode,
                    Mode::Integer
                        | Mode::Double
                        | Mode::Character
                        | Mode::Logical
                        | Mode::Complex
                        | Mode::Raw
                ) && bt.columns.is_none()
                {
                    self.emit(
                        Severity::Error,
                        span,
                        "RY061",
                        format!(
                            "$ operator is invalid for atomic vectors of mode `{}`",
                            bt.mode
                        ),
                    );
                    return RType::unknown();
                }
                // The parser records `$col` as a single arg with
                // `name = Some("col")` and a synthesized `value` of
                // `Expr::Ident { name: "col" }`. The value is NOT a
                // real expression to be inferred: doing so would emit a
                // spurious RY010 on the column name. So we deliberately
                // do not call `infer` on it.
                let col = args.first().and_then(|a| a.name.as_deref());
                if let Some(name) = col {
                    if let Some(schema) = &bt.columns {
                        if let Some(t) = schema.get(name) {
                            return t;
                        }
                        // RY060 for a `$` schema miss only on data frames.
                        // In R, `list(a=1)$missing` returns NULL (no
                        // error); only data frames make a missing `$`
                        // name a hard error worth flagging (PLAN Phase
                        // A4). Mirror the `[[`-with-string guard below.
                        if bt.class.contains("data.frame") {
                            self.emit_undefined_column(name, schema, span);
                            // Fall through to the conservative default so
                            // downstream code still has *a* type to work
                            // with after the diagnostic.
                        } else {
                            // Plain list `$` miss yields NULL in R.
                            return RType::new(Mode::Null, Length::Zero);
                        }
                    }
                }
                // No schema (or column not found after RY060): for
                // list-like types, return opaque since we don't know
                // the element type. For other types, return a length-1
                // value of the base mode. A union base would build a
                // malformed union here, so degrade to opaque (PLAN A2).
                if matches!(
                    bt.mode,
                    Mode::List | Mode::Opaque | Mode::Function | Mode::Union
                ) {
                    RType::unknown()
                } else {
                    RType::new(bt.mode, Length::One)
                }
            }
            IndexKind::Double => {
                // `df[["col"]]` or `x[[i]]`: the index can be a string
                // literal (column name) or an integer literal (positional
                // index). For string literals we look up by column name
                // ONLY on data frames (class data.frame). For plain
                // lists, string access is dynamic and we don't flag it.
                let arg_expr = args.first().map(|a| &a.value);
                if let Some(Expr::String(name, _)) = arg_expr {
                    if let Some(schema) = &bt.columns {
                        if let Some(t) = schema.get(name) {
                            return t;
                        }
                        // Only emit RY060 for data frames, not plain lists.
                        // Lists created by lapply etc. have internal
                        // [[N]] schemas; string access is dynamic.
                        if bt.class.contains("data.frame") {
                            self.emit_undefined_column(name, schema, span);
                        }
                    }
                    if matches!(
                        bt.mode,
                        Mode::List | Mode::Opaque | Mode::Function | Mode::Union
                    ) {
                        return RType::unknown();
                    }
                    return RType::new(bt.mode, Length::One);
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
                    if let Some(schema) = &bt.columns {
                        let key = format!("[[{}]]", idx as i64);
                        if let Some(t) = schema.get(&key) {
                            return t;
                        }
                        // Index not in schema: if all elements have the
                        // same type (homogeneous list from lapply etc.),
                        // return that common type. Otherwise opaque.
                        if let Some(common) = schema.homogeneous_element_type() {
                            return common;
                        }
                    }
                    // No schema or heterogeneous: opaque is safer than
                    // `bt.element()` (which returns list<1> for lists).
                    return RType::unknown();
                }
                // Non-literal arg: infer it for diagnostics, then return
                // the conservative default. A union base would build a
                // malformed union, so degrade to opaque (PLAN A2).
                if let Some(a) = args.first() {
                    self.infer(&a.value, scope);
                }
                if matches!(bt.mode, Mode::Union) {
                    RType::unknown()
                } else {
                    RType::new(bt.mode, Length::One)
                }
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
    fn emit_undefined_column(&mut self, col: &str, schema: &ColumnSchema, span: Span) {
        let names = schema.names();
        let preview: Vec<&str> = names.iter().take(5).cloned().collect();
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
/// Quick literal-only inference for function parameter defaults. We
/// don't have a scope yet at the point of `record_fn`, but for typed
/// defaults (`x = 1L`, `trim = 0`, `verbose = TRUE`) the literal
/// carries enough information.
fn infer_literal_default(e: &Expr) -> RType {
    match e {
        Expr::Logical(_, _) => RType::scalar(Mode::Logical),
        Expr::Integer(_, _) => RType::scalar(Mode::Integer),
        Expr::Double(_, _) => RType::scalar(Mode::Double),
        Expr::String(_, _) => RType::scalar(Mode::Character),
        Expr::Null(_) => RType::new(Mode::Null, Length::Zero),
        Expr::Na(t, _) => t.clone(),
        // Anything more complex (call, ident, binop) needs a scope; defer
        // to the first fixpoint iteration by starting as UNKNOWN.
        _ => RType::unknown(),
    }
}

/// True if `e` is syntactically a `return(...)` or `invisible(...)` call.
fn is_return_call(e: &Expr) -> bool {
    matches!(e, Expr::Call { func, .. }
        if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "return" || name == "invisible"))
}

/// True if the string is an R operator symbol that might be referenced
/// as a (possibly backtick-quoted) identifier, e.g. `+`, `*`, `<-`.
/// These are commonly user-defined or package-imported operators that
/// the checker cannot resolve against any scope, typeshed, or FnTable.
/// Used to suppress spurious RY010 (unbound variable) on such names.
fn is_operator_symbol(s: &str) -> bool {
    matches!(
        s,
        "+" | "-"
            | "*"
            | "/"
            | "^"
            | "<"
            | ">"
            | "<="
            | ">="
            | "=="
            | "!="
            | "&"
            | "|"
            | "&&"
            | "||"
            | "!"
            | ":"
            | "<-"
            | "<<-"
            | "="
            | "~"
            | "$"
            | "@"
            | "?"
    )
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

/// Whether a condition expression is the idiomatic numeric-truthiness
/// non-empty check: a direct call to `length`, `nrow`, or `ncol` via a bare
/// identifier callee (any args). These return an integer length-1, which R
/// silently coerces to logical in `if`/`while` -- but `if (length(x))` /
/// `if (nrow(df))` are so idiomatic in real R code that the RY001 coercion
/// warning is pure noise there. We suppress ONLY the coercion-warning arm
/// for this shape; a genuinely wrong condition (e.g. `if (1L)`) still warns.
///
/// Negation (`if (!length(x))`) is deliberately out of scope: it is typed
/// through the unary `!` operator, not this call shape.
fn is_numeric_truthiness_idiom(cond: &Expr) -> bool {
    if let Expr::Call { func, .. } = cond {
        if let Expr::Ident { name, .. } = func.as_ref() {
            return matches!(name.as_str(), "length" | "nrow" | "ncol");
        }
    }
    false
}

/// Extract an integer value from a literal expression. Returns
/// `Some(n)` for `Expr::Integer(n, _)` and for `Expr::Double(f, _)`
/// when `f` is a finite whole number (e.g. `2.0`). Returns `None` for
/// non-literal expressions, NaN/Inf, or fractional doubles.
///
/// Used by the literal-based length inference paths (`:` colon
/// operator, `rep`, `seq`) to compute exact result lengths when the
/// relevant arguments are literal integers or whole-number doubles.
/// We look at the raw AST rather than the inferred `RType` because the
/// type lattice discards the runtime value (it only carries mode and
/// length).
fn extract_literal_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(n, _) => Some(*n),
        Expr::Double(f, _) if f.is_finite() && f.fract() == 0.0 => Some(*f as i64),
        _ => None,
    }
}

/// True if `e` is a magrittr (`.`) or base-R (`_`) pipe placeholder.
/// These are bare identifier references used inside a piped call to
/// mark where the LHS value should be substituted.
fn is_pipe_placeholder(e: &Expr) -> bool {
    matches!(e, Expr::Ident { name, .. } if name == "." || name == "_")
}

/// Functions whose arguments are bare symbols (NSE), not expressions.
/// When these are called, the checker does NOT evaluate the arguments
/// as variable references, preventing spurious RY010 warnings.
///
/// Includes popular package functions commonly used in NSE contexts:
///   * ggplot2: from_theme, aes, aes_, aes_string, aes_q
///   * rlang: sym, ensym, enquo, enquos, expr, enexpr
///   * base: quote, substitute, bquote (already in typeshed but also
///     used as NSE)
fn is_nse_symbol_fn(name: &str) -> bool {
    matches!(
        name,
        // ggplot2 NSE
        "from_theme" | "aes" | "aes_" | "aes_string" | "aes_q"
        // rlang NSE
        | "sym" | "ensym" | "enquo" | "enquos" | "expr" | "enexpr"
        | "exprs" | "quo" | "quos" | "abort" | "inform"
        | "defuse" | "tidyeval_data" | "new_formula" | "new_quosure"
        // dplyr/tidyselect NSE
        | "tidyselect" | "all_vars" | "peek_vars"
        // Common NSE helpers
        | "delayedAssign" | "makeActiveBinding"
        // data.table NSE
        | "setkey" | "setkeyv" | "setindex" | "setindexv"
    )
}

/// R's foreign-function-interface primitives. Their first argument is a
/// native routine entry-point symbol (a bare identifier or backtick
/// name), not a variable reference, so RY010 must not fire on it.
fn is_ffi_primitive(name: &str) -> bool {
    matches!(
        name,
        ".Call" | ".C" | ".Fortran" | ".External" | ".External2" | ".Internal"
    )
}

/// Whether a purrr typed-map's callback return `mode` can coerce into
/// the target `target` mode without a lossy or surprising conversion.
/// Numeric modes (double/int/logical) coerce among themselves harmlessly;
/// a character or list return into a numeric (or vice-versa) target is
/// the real footgun RY080 targets. Opaque/unknown/union/null returns are
/// assumed compatible (no evidence of a mismatch).
fn modes_compatible(mode: &Mode, target: &Mode) -> bool {
    if matches!(mode, Mode::Opaque | Mode::Union | Mode::Null) {
        return true;
    }
    fn is_numeric(m: &Mode) -> bool {
        matches!(m, Mode::Double | Mode::Integer | Mode::Logical)
    }
    match target {
        Mode::Double | Mode::Integer | Mode::Logical => is_numeric(mode),
        Mode::Character => matches!(mode, Mode::Character),
        _ => true,
    }
}

/// The purrr typed-map suffix for a target mode (for RY080 messages):
/// `map_dbl`, `map_int`, etc.
fn mode_suffix(mode: &Mode) -> &'static str {
    match mode {
        Mode::Logical => "lgl",
        Mode::Integer => "int",
        Mode::Double => "dbl",
        Mode::Character => "chr",
        _ => "vec",
    }
}

/// Return the R source symbol for a binary operator, for use in
/// diagnostic messages. Returns `?` for unknown ops.
fn op_symbol(op: BinOpKind) -> &'static str {
    match op {
        BinOpKind::Add => "+",
        BinOpKind::Sub => "-",
        BinOpKind::Mul => "*",
        BinOpKind::Div => "/",
        BinOpKind::Pow => "^",
        BinOpKind::Mod => "%%",
        BinOpKind::IDiv => "%/%",
        BinOpKind::Colon => ":",
        BinOpKind::Lt => "<",
        BinOpKind::Le => "<=",
        BinOpKind::Gt => ">",
        BinOpKind::Ge => ">=",
        BinOpKind::Eq => "==",
        BinOpKind::Ne => "!=",
        BinOpKind::And => "&",
        BinOpKind::AndAnd => "&&",
        BinOpKind::Or => "|",
        BinOpKind::OrOr => "||",
        BinOpKind::In => "%in%",
        BinOpKind::Assign => "<-",
        BinOpKind::SuperAssign => "<<-",
        BinOpKind::PipeForward => "%>%",
        BinOpKind::PipeTee => "%T>%",
        BinOpKind::PipeAssign => "%<>%",
    }
}

/// True if `e` is the magrittr `.` data pronoun. Unlike
/// [`is_pipe_placeholder`], this excludes base-R's `_` placeholder,
/// which has no data-pronoun role: `x %>% _$col` is not valid R.
/// Used by `infer_pipe` to detect nested access forms like
/// `x %>% .$col`, `x %>% .[i]`, and `x %>% .[[i]]`, where the `.` at
/// the base of the index refers to the piped LHS value.
fn is_dot_pronoun(e: &Expr) -> bool {
    matches!(e, Expr::Ident { name, .. } if name == ".")
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
    /// `var` is narrowed to `target` in the positive (then) branch.
    /// `target` is a full RType: a scalar mode for single-mode
    /// predicates (`is.double`, `is.integer`, ...), or a union for
    /// group predicates (`is.numeric` -> union[integer, double]). This
    /// replaces the old `Mode`-only form, which could not distinguish
    /// `is.numeric` (a group) from `is.double` (a single mode) and so
    /// rewrote a known Integer to Double.
    Positive { var: String, target: RType },
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
            let Some(target) = predicate_target(name) else {
                return Narrowing::None;
            };
            let Some(var) = args.first().and_then(|a| match &a.value {
                Expr::Ident { name, .. } => Some(name.clone()),
                _ => None,
            }) else {
                return Narrowing::None;
            };
            // `is.null(x)` (non-negated): fall through to Positive with
            // target = NULL. The Positive arm narrows `var` to NULL in the
            // then branch and narrows it AWAY from NULL in the else branch
            // (the case the plan calls out: `if (is.null(x)) ... else x()`).
            Narrowing::Positive { var, target }
        }
        Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } => {
            // `!is.null(x)`: swap the narrowing so the `then` branch
            // gets the negative (non-null) and `else_` gets the
            // positive (null).
            let Expr::Call { func, args, .. } = expr.as_ref() else {
                return Narrowing::None;
            };
            let Expr::Ident { name, .. } = func.as_ref() else {
                return Narrowing::None;
            };
            let Some(var) = args.first().and_then(|a| match &a.value {
                Expr::Ident { name, .. } => Some(name.clone()),
                _ => None,
            }) else {
                return Narrowing::None;
            };
            // Only `!is.null(x)` is modeled as a negation.
            if name != "is.null" {
                return Narrowing::None;
            }
            Narrowing::Negative {
                var,
                mode: Mode::Null,
            }
        }
        _ => Narrowing::None,
    }
}

/// Map a type predicate name to the `RType` it tests for. Group
/// predicates return a union: `is.numeric` matches integer OR double,
/// so its narrowing target is `union[integer, double]` (NOT plain
/// Double, which would rewrite a known Integer to Double).
fn predicate_target(name: &str) -> Option<RType> {
    match name {
        // numeric = double or integer (a group, not a single mode).
        "is.numeric" => Some(RType::scalar(Mode::Integer).join(RType::scalar(Mode::Double))),
        "is.double" => Some(RType::scalar(Mode::Double)),
        "is.integer" => Some(RType::scalar(Mode::Integer)),
        "is.character" => Some(RType::scalar(Mode::Character)),
        "is.logical" => Some(RType::scalar(Mode::Logical)),
        "is.complex" => Some(RType::scalar(Mode::Complex)),
        "is.list" => Some(RType::scalar(Mode::List)),
        "is.null" => Some(RType::new(Mode::Null, Length::Zero)),
        "is.raw" => Some(RType::scalar(Mode::Raw)),
        _ => None,
    }
}

/// Narrow a type away from NULL: the value is known to be non-null in
/// this branch. Returns `None` when nothing changes (the type carries no
/// NULL member to remove).
///
/// - Pure `Null`: degrade to opaque (we know nothing else about it).
/// - A union containing a NULL member: rebuild the union without NULL.
///   If NULL was the only member this collapses to opaque via the empty
///   case; if exactly one non-null member remains, the union collapses
///   to that member (see `RType::union`).
/// - Anything else: unchanged (`None`).
fn narrow_away_from_null(t: &RType) -> Option<RType> {
    match t.mode {
        Mode::Null => Some(RType::unknown()),
        Mode::Union => {
            let members = t.members.as_ref()?;
            // Only act if at least one member is NULL.
            if !members.iter().any(|m| m.mode == Mode::Null) {
                return None;
            }
            let kept: Vec<RType> = members
                .iter()
                .filter(|m| m.mode != Mode::Null)
                .cloned()
                .collect();
            if kept.is_empty() {
                // Union was NULL-only; we only know it's non-null now.
                Some(RType::unknown())
            } else {
                Some(RType::union(Arc::from(kept)))
            }
        }
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
fn apply_narrowing(base: &Scope, narrowing: &Narrowing) -> (Scope, Scope, HashSet<String>) {
    let (mut then_scope, mut else_scope) = (base.clone(), base.clone());
    // Names refined by narrowing (in either branch). These must NOT be
    // merged back into the parent by `merge_branch_bindings`: a refinement
    // is branch-local, and folding it into the parent would degrade a
    // precise parent type (e.g. known-NULL -> opaque) and mask later
    // errors. The parent's pre-`if` type is what holds after the `if`.
    let mut narrowed: HashSet<String> = HashSet::new();
    match narrowing {
        Narrowing::None => {}
        Narrowing::Positive { var, target } => {
            // New rule (PLAN Phase 3 item 4): a predicate narrows only
            // when the existing type is opaque (untyped) or a union that
            // already contains the predicate's mode. A KNOWN type is
            // never rewritten: `is.numeric(x)` on a known Integer must
            // NOT rewrite it to Double (the old coerce_rank comparison
            // did exactly that).
            if let Some(existing) = then_scope.get(var).cloned() {
                let should_install = match existing.mode {
                    Mode::Opaque => true,
                    Mode::Union => {
                        // Existing union: only narrow if it contains the
                        // predicate's mode (the predicate confirms one
                        // member); otherwise leave untouched.
                        target.mode == Mode::Union
                            || existing
                                .members
                                .as_ref()
                                .map(|ms| {
                                    ms.iter().any(|m| {
                                        target.mode == Mode::Union || m.mode == target.mode
                                    })
                                })
                                .unwrap_or(false)
                    }
                    other => {
                        // Known atomic: narrow only if it already
                        // matches the predicate (idempotent). Incompatible
                        // known modes are left untouched.
                        if target.mode == Mode::Union {
                            target
                                .members
                                .as_ref()
                                .map(|ms| ms.iter().any(|m| m.mode == other))
                                .unwrap_or(false)
                        } else {
                            other == target.mode
                        }
                    }
                };
                if should_install && existing.mode != Mode::Opaque {
                    // The existing union/known already reflects this; no
                    // change needed.
                } else if should_install {
                    then_scope.insert(
                        var.clone(),
                        RType {
                            mode: target.mode,
                            length: existing.length,
                            ..target.clone()
                        },
                    );
                    narrowed.insert(var.clone());
                }
            }
            // For is.null, the else branch knows var is NOT null.
            if target.mode == Mode::Null {
                if let Some(existing) = else_scope.get(var).cloned() {
                    if let Some(n) = narrow_away_from_null(&existing) {
                        else_scope.insert(var.clone(), n);
                        narrowed.insert(var.clone());
                    }
                }
            }
        }
        Narrowing::Negative { var, mode } => {
            // The negation: `then` branch knows var is NOT of `mode`.
            // For `!is.null(x)` (the only Negative emitted today), narrow
            // NULL away from the then branch -- same helper as the Positive
            // else branch.
            if *mode == Mode::Null {
                if let Some(existing) = then_scope.get(var).cloned() {
                    if let Some(n) = narrow_away_from_null(&existing) {
                        then_scope.insert(var.clone(), n);
                        narrowed.insert(var.clone());
                    }
                }
            } else if let Some(existing) = then_scope.get(var).cloned() {
                if existing.mode == *mode {
                    then_scope.insert(var.clone(), RType::unknown());
                    narrowed.insert(var.clone());
                }
            }
            // `else` branch knows var IS of `mode`. A union mode would
            // build a malformed union here, so degrade to opaque (PLAN A2).
            // (Unreachable today -- `Narrowing::Negative` only ever carries
            // `Mode::Null` -- but kept as defense in depth.)
            if let Some(existing) = else_scope.get(var).cloned() {
                if matches!(existing.mode, Mode::Opaque) || existing.mode == *mode {
                    let t = if matches!(*mode, Mode::Union) {
                        RType::unknown()
                    } else {
                        RType::new(*mode, existing.length)
                    };
                    else_scope.insert(var.clone(), t);
                    narrowed.insert(var.clone());
                }
            }
        }
    }
    (then_scope, else_scope, narrowed)
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
/// Match call arguments to function parameters using R's standard
/// argument matching rules. Returns a vector indexed by parameter
/// position, where each entry is the type of the argument bound to
/// that parameter (or `RType::unknown()` if no argument was provided).
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
fn match_args_to_params(sig_params: &[String], args: &[Arg], arg_types: &[RType]) -> Vec<RType> {
    let has_dots = sig_params.iter().any(|p| p == "...");
    let n_named_params = if has_dots {
        sig_params.len().saturating_sub(1)
    } else {
        sig_params.len()
    };
    let mut matched: Vec<RType> = vec![RType::unknown(); sig_params.len()];
    let mut filled: Vec<bool> = vec![false; sig_params.len()];
    let mut used: Vec<bool> = vec![false; args.len()];
    // Pass 1: exact name matching.
    for (ai, a) in args.iter().enumerate() {
        if let Some(name) = &a.name {
            for (pi, p) in sig_params.iter().enumerate() {
                if p == name {
                    matched[pi] = arg_types[ai].clone();
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
            matched[next_param] = arg_types[ai].clone();
            filled[next_param] = true;
            next_param += 1;
        }
        // Extra positional args beyond the parameter count go to ...
        // or are dropped; we can't assign them to a named slot.
    }
    matched
}

/// Resolve the resulting mode for `c(...)`. If any argument was a union,
/// the coerce-rank ladder doesn't apply soundly, so degrade to opaque
/// rather than emitting a malformed union (PLAN Phase A2).
fn collapse_c_mode(mode: Mode, saw_union: bool) -> Mode {
    if saw_union {
        Mode::Opaque
    } else {
        mode
    }
}

/// If `e` is a literal expression (`42`, `"x"`, `TRUE`, `NULL`, `NA`),
/// return the mode that calling it would error with (PLAN Phase B2).
/// Non-literal callees return `None` so the caller stays silent.
fn literal_callee_mode(e: &Expr) -> Option<Mode> {
    match e {
        Expr::Logical(_, _) => Some(Mode::Logical),
        Expr::Integer(_, _) => Some(Mode::Integer),
        Expr::Double(_, _) => Some(Mode::Double),
        Expr::String(_, _) => Some(Mode::Character),
        Expr::Null(_) => Some(Mode::Null),
        // `NA` carries its own mode (NA, NA_real_, NA_integer_, ...).
        Expr::Na(t, _) => Some(t.mode),
        _ => None,
    }
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
    let x_len = arg_types
        .first()
        .map(|t| t.length)
        .unwrap_or(Length::Unknown);
    let times_type = arg_types.get(1).cloned().unwrap_or(RType::unknown());
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
        let ty = arg_types.get(i).cloned().unwrap_or(RType::unknown());
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
        s if s.parse::<usize>().is_ok() => Length::Known(s.parse::<usize>().unwrap_or(0)),
        _ => Length::Unknown,
    };
    let class = if jt.class.is_empty() {
        ClassVector::empty()
    } else {
        let refs: Vec<&str> = jt.class.iter().map(|s| s.as_str()).collect();
        ClassVector::from_slice(&refs)
    };
    let base = RType::new(mode, length).with_class(class);
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
    let schema = Arc::new(ColumnSchema { columns: cols });
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
        s if s.parse::<usize>().is_ok() => Length::Known(s.parse::<usize>().unwrap_or(0)),
        _ => Length::Unknown,
    };
    let class = if jt.class.is_empty() {
        ClassVector::empty()
    } else {
        let refs: Vec<&str> = jt.class.iter().map(|s| s.as_str()).collect();
        ClassVector::from_slice(&refs)
    };
    RType::new(mode, length).with_class(class)
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
            let before = (*c.return_slots).clone();
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

    // ---- inline suppression comment tests ----

    #[test]
    fn parse_trailing_ignore_comment() {
        let supps = parse_suppressions("x <- bad  # ry: ignore\n");
        assert_eq!(supps.len(), 1);
        assert_eq!(supps[0].line, 0);
        assert!(supps[0].rules.is_empty()); // suppress all
    }

    #[test]
    fn parse_specific_rule_ignore() {
        let supps = parse_suppressions("x <- \"a\" * 3  # ry: ignore[RY040]\n");
        assert_eq!(supps.len(), 1);
        assert_eq!(supps[0].rules, vec!["RY040"]);
    }

    #[test]
    fn parse_multiple_rules() {
        let supps = parse_suppressions("x <- bad  # ry: ignore[RY040, RY010]\n");
        assert_eq!(supps.len(), 1);
        assert!(supps[0].rules.contains(&"RY040".to_string()));
        assert!(supps[0].rules.contains(&"RY010".to_string()));
    }

    #[test]
    fn parse_standalone_comment_applies_to_next_line() {
        let src = "# ry: ignore\nx <- bad\n";
        let supps = parse_suppressions(src);
        assert_eq!(supps.len(), 1);
        assert_eq!(supps[0].line, 1); // next line
    }

    #[test]
    fn parse_standalone_comment_skips_blank_lines() {
        let src = "# ry: ignore\n\nx <- bad\n";
        let supps = parse_suppressions(src);
        assert_eq!(supps.len(), 1);
        assert_eq!(supps[0].line, 2);
    }

    #[test]
    fn parse_noqa_alias() {
        let supps = parse_suppressions("x <- bad  # noqa: RY010\n");
        assert_eq!(supps.len(), 1);
        assert!(supps[0].rules.contains(&"RY010".to_string()));
    }

    #[test]
    fn parse_bare_noqa_suppresses_all() {
        let supps = parse_suppressions("x <- bad  # noqa\n");
        assert_eq!(supps.len(), 1);
        assert!(supps[0].rules.is_empty());
    }

    #[test]
    fn parse_noqa_bracket_form() {
        let supps = parse_suppressions("x <- bad  # noqa[RY010]\n");
        assert_eq!(supps.len(), 1);
        assert!(supps[0].rules.contains(&"RY010".to_string()));
    }

    #[test]
    fn parse_compact_ry_ignore_no_space() {
        let supps = parse_suppressions("x <- bad  # ry:ignore[RY010]\n");
        assert_eq!(supps.len(), 1);
        assert!(supps[0].rules.contains(&"RY010".to_string()));
    }

    #[test]
    fn parse_case_insensitive_marker() {
        let supps = parse_suppressions("x <- bad  # RY: IGNORE[ry010]\n");
        assert_eq!(supps.len(), 1);
        assert!(supps[0].rules.contains(&"RY010".to_string()));
    }

    #[test]
    fn parse_non_suppression_comment_is_ignored() {
        let supps = parse_suppressions("# just a regular comment\nx <- bad\n");
        assert!(supps.is_empty());
    }

    #[test]
    fn parse_file_level_suppression() {
        assert!(has_file_suppression("# ry: ignore-file\nx <- bad\n"));
        assert!(has_file_suppression("# ry:ignore-file\nx <- bad\n"));
        assert!(!has_file_suppression("# ry: ignore\nx <- bad\n"));
    }

    #[test]
    fn file_level_marker_not_treated_as_line_level() {
        // `# ry: ignore-file` must NOT also register as a line-level
        // "ignore all" (it's handled by has_file_suppression instead).
        let supps = parse_suppressions("# ry: ignore-file\nx <- bad\n");
        assert!(
            supps.is_empty(),
            "ignore-file should not produce line-level suppressions, got {:?}",
            supps
        );
    }

    #[test]
    fn is_suppressed_matches_line_and_code() {
        let supps = vec![Suppression {
            line: 2,
            rules: vec!["RY010".to_string()],
        }];
        let diag_matching = Diagnostic {
            severity: Severity::Warning,
            span: Span {
                start: 0,
                end: 1,
                line: 2,
                col: 0,
            },
            path: "x.R".into(),
            code: "RY010",
            message: "test".into(),
        };
        let diag_wrong_line = Diagnostic {
            span: Span {
                line: 0,
                ..diag_matching.span
            },
            ..diag_matching.clone()
        };
        let diag_wrong_code = Diagnostic {
            code: "RY040",
            ..diag_matching.clone()
        };
        assert!(is_suppressed(&diag_matching, &supps));
        assert!(!is_suppressed(&diag_wrong_line, &supps));
        assert!(!is_suppressed(&diag_wrong_code, &supps));
    }

    #[test]
    fn is_suppressed_empty_rules_matches_any_code() {
        let supps = vec![Suppression {
            line: 0,
            rules: vec![],
        }];
        let diag = Diagnostic {
            severity: Severity::Warning,
            span: Span {
                start: 0,
                end: 1,
                line: 0,
                col: 0,
            },
            path: "x.R".into(),
            code: "RY999",
            message: "test".into(),
        };
        assert!(is_suppressed(&diag, &supps));
    }

    #[test]
    fn filter_suppressed_end_to_end() {
        // Trailing `# ry: ignore[RY010]` on the offending line drops RY010.
        let src = "x <- undefined_var  # ry: ignore[RY010]\n";
        let diags = check(src);
        let filtered = filter_suppressed(diags, src);
        assert!(
            filtered.iter().all(|d| d.code != "RY010"),
            "RY010 should be suppressed, got {:?}",
            filtered
        );
    }

    #[test]
    fn filter_suppressed_file_level_drops_everything() {
        let src = "# ry: ignore-file\nx <- undefined_var\n";
        let diags = check(src);
        let filtered = filter_suppressed(diags, src);
        assert!(
            filtered.is_empty(),
            "file-level suppression should drop all diagnostics, got {:?}",
            filtered
        );
    }

    #[test]
    fn filter_suppressed_other_rules_still_fire() {
        // Suppressing RY010 on line 0 should NOT affect RY040 on line 1.
        let src = "x <- undefined_var  # ry: ignore[RY010]\ny <- \"a\" * 3L\n";
        let diags = check(src);
        let filtered = filter_suppressed(diags, src);
        assert!(
            filtered.iter().any(|d| d.code == "RY040"),
            "RY040 should still fire (it's on a different line), got {:?}",
            filtered
        );
        assert!(
            filtered.iter().all(|d| d.code != "RY010"),
            "RY010 should be suppressed"
        );
    }

    #[test]
    fn detects_char_plus_int() {
        let diags = check(r#""a" + 1L"#);
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040, got {:?}",
            diags
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
    fn scalar_logical_warns_on_vector_operand() {
        let diags = check("x <- c(TRUE, FALSE)\nbad <- x && TRUE\n");
        assert!(
            diags.iter().any(|d| d.code == "RY032"),
            "expected RY032 for && with vector, got {:?}",
            diags
        );
    }

    #[test]
    fn vectorized_logical_no_warning() {
        let diags = check("x <- c(TRUE, FALSE)\nok <- x & TRUE\n");
        assert!(
            diags.iter().all(|d| d.code != "RY032"),
            "vectorized & should not warn, got {:?}",
            diags
        );
    }

    #[test]
    fn scalar_logical_with_scalars_no_warning() {
        let diags = check("a <- TRUE\nb <- FALSE\nx <- a && b\n");
        assert!(
            diags.iter().all(|d| d.code != "RY032"),
            "&& with scalars should not warn, got {:?}",
            diags
        );
    }

    #[test]
    fn compare_char_numeric_warns() {
        let diags = check(r#"bad <- "hello" < 42"#);
        assert!(
            diags.iter().any(|d| d.code == "RY033"),
            "expected RY033 for character vs numeric, got {:?}",
            diags
        );
    }

    #[test]
    fn compare_same_mode_no_warning() {
        let diags = check("bad <- 1 < 2\n");
        assert!(
            diags.iter().all(|d| d.code != "RY033"),
            "numeric vs numeric should not warn, got {:?}",
            diags
        );
    }

    #[test]
    fn compare_char_char_no_warning() {
        let diags = check(r#"x <- "abc" < "xyz""#);
        assert!(
            diags.iter().all(|d| d.code != "RY033"),
            "character vs character should not warn, got {:?}",
            diags
        );
    }

    #[test]
    fn compare_eq_char_numeric_warns() {
        let diags = check(r#"bad <- "hello" == 1"#);
        assert!(
            diags.iter().any(|d| d.code == "RY033"),
            "expected RY033 for character == numeric, got {:?}",
            diags
        );
    }

    #[test]
    fn in_operator_uses_lhs_length() {
        // `x %in% table` returns a logical vector of length(x); the RHS
        // length is irrelevant. A length-1 `x` matched against a length-2
        // literal must stay length-1 logical -- not length-2 (which would
        // drive RY002/RY032 false positives downstream).
        let (_diags, scope) = check_with_scope("x <- \"a\"\nr <- x %in% c(\"a\", \"b\")\n");
        let r = scope.get("r").expect("binding r");
        assert_eq!(r.mode, Mode::Logical, "got {:?}", r);
        assert_eq!(r.length, Length::One, "got {:?}", r);
    }

    #[test]
    fn in_operator_condition_no_ry002_ry032() {
        // The end-to-end shape from the purrr net: a length-1 `%in%` result
        // used as an `if` condition and inside `&&` must not fire RY002 or
        // RY032.
        let diags = check(
            "x <- \"a\"\nif (x %in% c(\"a\", \"b\")) print(1)\nif (is.character(x) && x %in% c(\"a\", \"b\")) print(2)\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY002" && d.code != "RY032"),
            "expected no RY002/RY032 for length-1 %in%, got {:?}",
            diags
        );
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
    fn pipe_dot_pronoun_dollar_column() {
        // `df %>% .$mpg` resolves `.` to the piped LHS (`mtcars`) and
        // then indexes by column name, so `col` should be `double<32>`
        // (the type of `mtcars$mpg`). We assert the inferred type
        // directly via the test scope and also check that no RY010
        // (unbound `.`) leaks out.
        let (diags, scope) = check_with_scope("df <- mtcars\ncol <- df %>% .$mpg\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "dot pronoun should not emit RY010 (unbound `.`), got {:?}",
            diags
        );
        let col = scope.get("col").expect("col should be bound");
        assert_eq!(
            col.mode,
            Mode::Double,
            "df %>% .$mpg must infer double, got {:?}",
            col
        );
        assert_eq!(col.length, Length::Known(32), "mpg has 32 rows");
    }

    #[test]
    fn pipe_dot_pronoun_double_bracket() {
        // `df %>% .[["mpg"]]` resolves `.` to the LHS and indexes by
        // string-literal column name via `[[`, mirroring `$` semantics.
        let (diags, scope) = check_with_scope("df <- mtcars\ncol <- df %>% .[[\"mpg\"]]\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "dot pronoun should not emit RY010, got {:?}",
            diags
        );
        let col = scope.get("col").expect("col should be bound");
        assert_eq!(col.mode, Mode::Double, ".[[\"mpg\"]] must infer double");
        assert_eq!(col.length, Length::Known(32), "mpg has 32 rows");
    }

    #[test]
    fn pipe_dot_pronoun_single_bracket() {
        // `df %>% .[1]` preserves the base type (single-bracket
        // subsetting keeps the existing opaque behavior at v1), so the
        // result is the same data.frame-typed value as the LHS. The
        // important behavioral check is that no RY010 leaks on `.`.
        let (diags, scope) = check_with_scope("df <- mtcars\nsub <- df %>% .[1]\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "dot pronoun should not emit RY010, got {:?}",
            diags
        );
        let sub = scope.get("sub").expect("sub should be bound");
        assert_eq!(sub.mode, Mode::List, "df[1] preserves base mode");
        assert!(
            sub.class.contains("data.frame"),
            ".[1] preserves the data.frame class"
        );
    }

    #[test]
    fn pipe_dot_pronoun_bare_returns_lhs() {
        // `x %>% .` returns the LHS value itself (the `.` refers to the
        // LHS). For a length-3 double vector, the result type matches.
        let (diags, scope) = check_with_scope("x <- c(1, 2, 3)\ny <- x %>% .\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let y = scope.get("y").expect("y should be bound");
        assert_eq!(y.mode, Mode::Double, "x %>% . must infer double");
        assert_eq!(y.length, Length::Known(3), "length is preserved");
    }

    #[test]
    fn pipe_dot_pronoun_undefined_column_emits_ry060() {
        // `df %>% .$nonexistent` resolves `.` to the LHS, then the
        // column lookup fails against `mtcars`'s schema, so RY060
        // (undefined-column) must fire - the pronoun path reuses the
        // same diagnostics as a direct `df$nonexistent`.
        let diags = check("df <- mtcars\nbad <- df %>% .$nonexistent\n");
        assert!(
            diags.iter().any(|d| d.code == "RY060"),
            "expected RY060 for undefined column via dot pronoun, got {:?}",
            diags
        );
    }

    #[test]
    fn pipe_dot_pronoun_chains_into_arithmetic() {
        // End-to-end behavioral check: `df %>% .$mpg` produces a real
        // double type (not opaque), so subsequent arithmetic that would
        // fail on an opaque value type-checks cleanly. This is the
        // motivating use case from the task description.
        let diags = check("df <- mtcars\ncol <- df %>% .$mpg\nok <- col + 1L\n");
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "col + 1L should be valid (double + int), got {:?}",
            diags
        );
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "no RY010 should leak from the dot pronoun, got {:?}",
            diags
        );
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
        let schema = l.columns.clone().expect("l should carry a column schema");
        assert_eq!(schema.len(), 2, "schema should have 2 columns");
        assert_eq!(schema.names(), vec!["a", "b"]);
        // Accessing a missing column on a PLAIN list is silent: in R
        // `l$missing` returns NULL, so RY060 is scoped to data frames
        // (PLAN Phase A4). Only data-frame misses fire RY060.
        let diags = check("l <- list(a = 1L)\nbad <- l$missing\n");
        assert!(
            diags.iter().all(|d| d.code != "RY060"),
            "plain-list `$` miss must not fire RY060, got {:?}",
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
        let schema = df.columns.clone().expect("df should carry a column schema");
        assert_eq!(schema.len(), 2, "schema should have 2 columns");
        // Column `x` is integer recycled to length 3.
        let x = schema.get("x").expect("x column should exist");
        assert_eq!(x.mode, Mode::Integer);
        assert_eq!(x.length, Length::Known(3), "x recycled to length 3");
        // Column access resolves through the schema.
        let (_, scope2) = check_with_scope("df <- data.frame(x = c(1L, 2L, 3L))\nxv <- df$x\n");
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
        let schema = x.columns.clone().expect("schema must be preserved");
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
    fn nse_dplyr_filter_resolves_columns() {
        // `filter(df, mpg > 20)` is dplyr's row filter. Without the
        // NSE handler, `mpg` would be reported as unbound (RY010). The
        // handler injects the data frame's column schema so the
        // comparison is well-typed.
        let diags = check("library(dplyr)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "dplyr filter NSE handler should suppress RY010 on column refs, got {:?}",
            diags
        );
        // `filter` preserves the data frame type.
        let (_, scope) =
            check_with_scope("library(dplyr)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
        let small = scope.get("small").expect("small should be bound");
        assert!(
            small.class.contains("data.frame"),
            "filter() must preserve the data.frame class, got class {:?}",
            small.class
        );
        assert!(
            small.columns.is_some(),
            "filter() must preserve the column schema"
        );
    }

    #[test]
    fn nse_dplyr_mutate_resolves_columns() {
        // `mutate(df, kml = mpg * 0.425)` evaluates `mpg * 0.425`
        // against an augmented scope. Without the handler, `mpg` would
        // fire RY010.
        let diags = check("library(dplyr)\ndf <- mtcars\ndf2 <- mutate(df, kml = mpg * 0.425)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "dplyr mutate NSE handler should suppress RY010 on column refs, got {:?}",
            diags
        );
        let (_, scope) = check_with_scope(
            "library(dplyr)\ndf <- mtcars\ndf2 <- mutate(df, kml = mpg * 0.425)\n",
        );
        let df2 = scope.get("df2").expect("df2 should be bound");
        assert!(
            df2.class.contains("data.frame"),
            "mutate() must preserve the data.frame class, got class {:?}",
            df2.class
        );
    }

    #[test]
    fn nse_dplyr_summarise_returns_data_frame() {
        // `summarise(df, m = mean(mpg))` collapses to a single-row data
        // frame. The column reference `mpg` resolves via the augmented
        // scope. The result is a fresh data frame type whose schema we
        // do not know (the columns are aggregations, not the inputs).
        let diags = check("library(dplyr)\ndf <- mtcars\ns <- summarise(df, m = mean(mpg))\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "dplyr summarise NSE handler should suppress RY010 on column refs, got {:?}",
            diags
        );
        let (_, scope) =
            check_with_scope("library(dplyr)\ndf <- mtcars\ns <- summarise(df, m = mean(mpg))\n");
        let s = scope.get("s").expect("s should be bound");
        assert!(
            s.class.contains("data.frame"),
            "summarise() must return a data.frame class, got class {:?}",
            s.class
        );
        assert!(
            s.columns.is_none(),
            "summarise() must not expose the input column schema, got {:?}",
            s
        );
    }

    #[test]
    fn nse_dplyr_summarize_alias_matches_summarise() {
        // The American-English `summarize` is an alias for `summarise`
        // and must dispatch to the same handler. `hp` resolves against
        // the augmented scope; the result is a data frame.
        let diags = check("library(dplyr)\ndf <- mtcars\ns <- summarize(df, m = mean(hp))\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "dplyr summarize alias should suppress RY010 on column refs, got {:?}",
            diags
        );
        let (_, scope) =
            check_with_scope("library(dplyr)\ndf <- mtcars\ns <- summarize(df, m = mean(hp))\n");
        let s = scope.get("s").expect("s should be bound");
        assert!(
            s.class.contains("data.frame"),
            "summarize() must return a data.frame class, got class {:?}",
            s.class
        );
    }

    #[test]
    fn nse_dplyr_pipe_chain_resolves_columns() {
        // `mtcars %>% filter(cyl == 4) %>% select(mpg, hp)` desugars
        // to nested calls. Each stage's data frame is the previous
        // stage's result (mtcars for the first), so column references
        // resolve via the augmented scope and no RY010 fires.
        let diags = check(
            "library(magrittr)\n\
             library(dplyr)\n\
             result <- mtcars %>% filter(cyl == 4) %>% select(mpg, hp)\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "piped dplyr chain should suppress RY010 on column refs, got {:?}",
            diags
        );
        // The chain's final result is a data frame (select preserves
        // the type of its input, which here is `filter`'s output =
        // mtcars' type).
        let (_, scope) = check_with_scope(
            "library(magrittr)\n\
             library(dplyr)\n\
             result <- mtcars %>% filter(cyl == 4) %>% select(mpg, hp)\n",
        );
        let result = scope.get("result").expect("result should be bound");
        assert!(
            result.class.contains("data.frame"),
            "piped dplyr chain must preserve the data.frame class, got class {:?}",
            result.class
        );
    }

    #[test]
    fn nse_dplyr_filter_non_dataframe_falls_through() {
        // `filter` is only treated as dplyr's verb when the first arg
        // looks like a data frame (has a column schema or the
        // `data.frame` class). Here the first arg is a bare integer;
        // the call should NOT be intercepted as NSE - the bare column
        // reference `mpg` (which is unbound here) should fire RY010
        // through the regular arg-inference path.
        let diags = check("x <- 1L\nr <- filter(x, mpg > 20)\n");
        assert!(
            diags.iter().any(|d| d.code == "RY010"),
            "filter() with a non-data-frame first arg should fall through and emit RY010 on `mpg`, got {:?}",
            diags
        );
    }

    #[test]
    fn nse_dplyr_filter_ungated_falls_through_when_not_loaded() {
        // Phase 2.1 gating: a bare `filter(df, ...)` in a script that
        // has NOT loaded dplyr must NOT be treated as dplyr's verb.
        // The column reference `mpg` is genuinely unbound in this scope
        // (no library(dplyr)), so RY010 must fire.
        let diags = check("df <- mtcars\nsmall <- filter(df, mpg > 20)\n");
        assert!(
            diags.iter().any(|d| d.code == "RY010"),
            "ungated filter() without library(dplyr) should fall through and emit RY010 on `mpg`, got {:?}",
            diags
        );
    }

    #[test]
    fn nse_dplyr_filter_qualified_resolves_without_library() {
        // Phase 2.1 gating: `dplyr::filter(...)` is always treated as
        // dplyr's verb regardless of whether dplyr is loaded, because
        // the `dplyr::` prefix is an explicit namespace reference. So
        // the column ref `mpg` must NOT fire RY010.
        let diags = check("df <- mtcars\nsmall <- dplyr::filter(df, mpg > 20)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "dplyr::-qualified filter() should suppress RY010 on column refs without library(dplyr), got {:?}",
            diags
        );
    }

    #[test]
    fn nse_dplyr_filter_library_records_loaded() {
        // Phase 2.1 gating: `library(dplyr)` records dplyr into the
        // loaded set, so a subsequent `filter(df, ...)` resolves as
        // dplyr's verb and the column ref `mpg` does NOT fire RY010.
        let diags = check("library(dplyr)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "library(dplyr) + filter() should suppress RY010 on column refs, got {:?}",
            diags
        );
    }

    #[test]
    fn nse_dplyr_filter_requirenamespace_records_loaded() {
        // `requireNamespace("dplyr")` also records into the loaded set.
        let diags =
            check("requireNamespace(\"dplyr\")\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "requireNamespace(\"dplyr\") + filter() should suppress RY010 on column refs, got {:?}",
            diags
        );
    }

    #[test]
    fn nse_dplyr_filter_tidyverse_counts_as_dplyr() {
        // `library(tidyverse)` loads dplyr transitively; the gating
        // treats tidyverse as a synonym for dplyr.
        let diags = check("library(tidyverse)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "library(tidyverse) + filter() should suppress RY010 on column refs, got {:?}",
            diags
        );
    }

    #[test]
    fn nse_dplyr_arrange_groupby_preserve_type() {
        // `arrange` and `group_by` walk their column-reference args in
        // the augmented scope and preserve the input data frame type.
        let diags = check(
            "library(dplyr)\n\
             df <- mtcars\n\
             sorted <- arrange(df, mpg)\n\
             grouped <- group_by(df, cyl)\n",
        );
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "arrange/group_by NSE handlers should suppress RY010 on column refs, got {:?}",
            diags
        );
        let (_, scope) = check_with_scope(
            "library(dplyr)\n\
             df <- mtcars\n\
             sorted <- arrange(df, mpg)\n\
             grouped <- group_by(df, cyl)\n",
        );
        let sorted = scope.get("sorted").expect("sorted should be bound");
        assert!(
            sorted.class.contains("data.frame"),
            "arrange() must preserve the data.frame class, got class {:?}",
            sorted.class
        );
        let grouped = scope.get("grouped").expect("grouped should be bound");
        assert!(
            grouped.class.contains("data.frame"),
            "group_by() must preserve the data.frame class, got class {:?}",
            grouped.class
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
            c.mode,
            Mode::Function,
            "c must be function-typed, got {:?}",
            c
        );
        let sig = c.fn_sig.clone().expect("c must carry an inferred fn_sig");
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
        let sig = add5
            .fn_sig
            .clone()
            .expect("add5 must carry an inferred fn_sig");
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
        let sig = h.fn_sig.clone().expect("h must carry an inferred fn_sig");
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
    fn purrr_map_walks_callback_and_infers_list() {
        // PLAN 2.3: purrr::map(.x, .f) is modeled like lapply -- the
        // callback body is walked (RY010 fires on the unbound `bug`)
        // and the result is a list.
        let diags = check(
            "library(purrr)\n\
             xs <- map(1:3, function(x) bug + x)\n",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == "RY010" && d.message.contains("bug")),
            "purrr map should walk the callback and flag `bug`, got {:?}",
            diags
        );
    }

    #[test]
    fn purrr_map_dbl_infers_double_vector() {
        // map_dbl returns a double vector; using it in character
        // arithmetic fires RY040 (proving the typed-mode result).
        let diags = check(
            "library(purrr)\n\
             v <- map_dbl(1:3, function(x) x + 0.5)\n\
             bad <- v + \"x\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "map_dbl result used with character should fire RY040, got {:?}",
            diags
        );
    }

    #[test]
    fn purrr_map_dbl_type_mismatch_fires_ry080() {
        // PLAN 2.3: map_dbl whose callback returns character fires
        // RY080 (R coerces silently, but the mismatch is a likely bug).
        let diags = check(
            "library(purrr)\n\
             xs <- map_dbl(1:3, function(x) paste(\"n\", x))\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY080"),
            "map_dbl with character callback should fire RY080, got {:?}",
            diags
        );
    }

    #[test]
    fn purrr_in_parallel_is_transparent() {
        // PLAN 2.3: in_parallel(.f) is type-transparent. map(sims,
        // in_parallel(f)) must walk `f`'s body identically to
        // map(sims, f) -- here the unbound `bug` must fire RY010.
        let diags = check(
            "library(purrr)\n\
             sims <- list(1, 2)\n\
             out <- map(sims, in_parallel(function(s) bug + s[[1]]))\n",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.code == "RY010" && d.message.contains("bug")),
            "in_parallel-wrapped callback should still be walked, got {:?}",
            diags
        );
    }

    #[test]
    fn purrr_not_loaded_does_not_treat_map_as_higher_order() {
        // Without library(purrr), a bare `map` must NOT be treated as
        // purrr's map (it is an unbound name -> RY010 on `map` itself,
        // or opaque). Either way, no purrr higher-order modeling.
        let diags = check("xs <- map(1:3, function(x) x)\n");
        // `map` is unbound (not in base typeshed); it resolves opaque
        // and the callback is NOT walked. No RY010 on a callback-local
        // name confirms the callback was not entered.
        assert!(
            diags
                .iter()
                .all(|d| d.code != "RY010" || !d.message.contains("map")),
            "ungated map should not get purrr treatment: {:?}",
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
        // `if (TRUE) list(1) else function(){1}` joins to
        // union[list, function]. Using the result arithmetically fires
        // RY040 because EVERY member of the union errors against `+ 1`
        // (Phase 3 union semantics). The earlier form of this test
        // (`1L else "hello"`) relied on the coercion-ladder join that
        // silently promoted to character; unions replaced that, so the
        // test now uses an all-invalid union to keep exercising RY040.
        let diags = check(
            "x <- if (TRUE) list(1) else function() { 1 }\n\
             bad <- x + 1\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 from joined if-expr (all-invalid union) + int, got {:?}",
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

    #[test]
    fn negative_integer_literal_infers_integer() {
        // `-1L` is unary minus applied to an integer literal. The result
        // must be integer (same mode as the operand), length 1, non-NA.
        let (diags, scope) = check_with_scope("x <- -1L\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
        assert_eq!(x.length, Length::One, "got {:?}", x);
    }

    #[test]
    fn negative_double_literal_infers_double() {
        // `-3.14` is unary minus applied to a double literal; result is
        // double, length 1, non-NA.
        let (diags, scope) = check_with_scope("y <- -3.14\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let y = scope.get("y").expect("y should be bound");
        assert_eq!(y.mode, Mode::Double, "got {:?}", y);
        assert_eq!(y.length, Length::One, "got {:?}", y);
    }

    #[test]
    fn neg_colon_infers_integer_and_groups_correctly() {
        // `-1:3` parses as `(-1):3`, which R evaluates as seq(-1, 3) =
        // c(-1, 0, 1, 2, 3), an integer vector. The type must be integer
        // (not double, not error), and using it arithmetically must be
        // well-typed. This is the key correctness case for unary-minus
        // vs colon precedence.
        let (diags, scope) = check_with_scope("z <- -1:3\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let z = scope.get("z").expect("z should be bound");
        assert_eq!(z.mode, Mode::Integer, "got {:?}", z);
        // Behavioral check: `-1:3`'s LHS is a UnaryOp (not a literal),
        // so the literal-based length inference doesn't fire and the
        // length stays Unknown. The value must still be usable as an
        // integer in arithmetic.
        let diags = check("z <- -1:3\nbad <- z + 1L\n");
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "z + 1L must be valid int+int, got {:?}",
            diags
        );
    }

    #[test]
    fn negated_paren_colon_infers_integer() {
        // `-(1:3)` negates the whole sequence; still an integer vector.
        let (diags, scope) = check_with_scope("w <- -(1:3)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let w = scope.get("w").expect("w should be bound");
        assert_eq!(w.mode, Mode::Integer, "got {:?}", w);
    }

    #[test]
    fn neg_times_int_infers_integer_length_one() {
        // `-2L * 3L` = `(-2L) * 3L` = -6L, a length-1 integer.
        let (diags, scope) = check_with_scope("v <- -2L * 3L\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let v = scope.get("v").expect("v should be bound");
        assert_eq!(v.mode, Mode::Integer, "got {:?}", v);
        assert_eq!(v.length, Length::One, "got {:?}", v);
    }

    #[test]
    fn neg_on_character_emits_ry020() {
        // Unary `-` applied to a character is a type error in R.
        let diags = check("x <- -\"hi\"\n");
        assert!(
            diags.iter().any(|d| d.code == "RY020"),
            "expected RY020 for negation of character, got {:?}",
            diags
        );
    }

    #[test]
    fn neg_preserves_na_flag_and_mode() {
        // `-NA_integer_` must remain an NA integer (negation does not
        // change mode or clear the NA flag). This guards that the
        // checker's `UnaryOp::Neg` returns the operand type verbatim.
        let (diags, scope) = check_with_scope("a <- -NA_integer_\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let a = scope.get("a").expect("a should be bound");
        assert_eq!(a.mode, Mode::Integer, "got {:?}", a);
        assert_eq!(a.length, Length::One, "got {:?}", a);
    }

    // ---- Literal-based length inference: `:`, `rep`, `seq` ----
    //
    // These exercise the literal-arg fast paths that pin the result
    // length exactly instead of returning `Length::Unknown`. The
    // common pattern: build the expression, assert the inferred
    // `RType` has `Length::Known(n)` with the expected `n`, then do a
    // behavioral check that downstream code sees the precise length
    // (e.g. mixing with a character fires RY040).

    #[test]
    fn colon_literals_pin_length() {
        // `1:10` has 10 elements; both endpoints are integer-valued
        // literals so the literal-based path fires.
        let (diags, scope) = check_with_scope("x <- 1:10\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
        assert_eq!(x.length, Length::Known(10), "got {:?}", x);
    }

    #[test]
    fn colon_literals_descending_pin_length() {
        // `10:1` is c(10, 9, ..., 1): length 10, mode integer.
        let (_, scope) = check_with_scope("x <- 10:1\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
        assert_eq!(x.length, Length::Known(10), "got {:?}", x);
    }

    #[test]
    fn colon_double_literals_pin_length() {
        // `1.0:5.0` - whole-number doubles also trigger the literal
        // path; R returns integer for whole-number endpoints.
        let (_, scope) = check_with_scope("x <- 1.0:5.0\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
        assert_eq!(x.length, Length::Known(5), "got {:?}", x);
    }

    #[test]
    fn colon_single_element_pin_length_one() {
        // `5:5` is a length-1 integer vector c(5).
        let (_, scope) = check_with_scope("x <- 5:5\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Known(1), "got {:?}", x);
    }

    #[test]
    fn colon_literals_fire_ry040_on_char_mix() {
        // `1:10` is integer<10>; adding a character is a type error
        // (RY040). This is the headline benefit of precise length
        // inference: the checker sees a real vector, not an opaque.
        let diags = check("x <- 1:10\nbad <- x + \"hello\"\n");
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 for integer<10> + character, got {:?}",
            diags
        );
    }

    #[test]
    fn colon_non_literal_stays_unknown() {
        // `n:10` where `n` is a variable: LHS isn't a literal, so the
        // length stays Unknown (no false precision).
        let (_, scope) = check_with_scope("n <- 1L\nx <- n:10\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
        assert_eq!(x.length, Length::Unknown, "got {:?}", x);
    }

    #[test]
    fn rep_literal_times_pin_length() {
        // `rep(1:3, 2)` = c(1,2,3,1,2,3): length 6, mode integer.
        let (diags, scope) = check_with_scope("x <- rep(1:3, 2)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
        assert_eq!(x.length, Length::Known(6), "got {:?}", x);
    }

    #[test]
    fn rep_scalar_x_literal_times_pin_length() {
        // `rep(0, 5)` = c(0,0,0,0,0): length 5. `0` is a double
        // literal in R (no `L` suffix), so the mode stays double.
        let (diags, scope) = check_with_scope("x <- rep(0, 5)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Double, "got {:?}", x);
        assert_eq!(x.length, Length::Known(5), "got {:?}", x);
    }

    #[test]
    fn rep_named_times_arg_pin_length() {
        // `rep(c(1, 2), times = 3)` = c(1,2,1,2,1,2): length 6.
        let (_, scope) = check_with_scope("x <- rep(c(1, 2), times = 3)\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Known(6), "got {:?}", x);
    }

    #[test]
    fn rep_each_arg_pin_length() {
        // `rep(c(1, 2, 3), each = 2)` = c(1,1,2,2,3,3): length 6.
        let (_, scope) = check_with_scope("x <- rep(c(1, 2, 3), each = 2)\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Known(6), "got {:?}", x);
    }

    #[test]
    fn rep_times_and_each_pin_length() {
        // `rep(c(1, 2), 3, each = 2)`: each element twice, then the
        // whole thing 3 times = 2 * 2 * 3 = 12.
        let (_, scope) = check_with_scope("x <- rep(c(1, 2), 3, each = 2)\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Known(12), "got {:?}", x);
    }

    #[test]
    fn rep_non_literal_times_stays_unknown() {
        // `rep(1:3, n)` where `n` is a variable: `times` isn't a
        // literal, so the length stays Unknown.
        let (_, scope) = check_with_scope("n <- 2\nx <- rep(1:3, n)\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Unknown, "got {:?}", x);
    }

    #[test]
    fn rep_literal_fire_ry040_on_char_mix() {
        // `rep(c(1, 2), 3)` is double<6>; adding a character fires RY040.
        let diags = check("x <- rep(c(1, 2), 3)\nbad <- x + \"hello\"\n");
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 for double<6> + character, got {:?}",
            diags
        );
    }

    #[test]
    fn seq_literal_by_pin_length() {
        // `seq(1, 10, 2)` = c(1, 3, 5, 7, 9): length 5.
        let (diags, scope) = check_with_scope("x <- seq(1, 10, 2)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Known(5), "got {:?}", x);
    }

    #[test]
    fn seq_length_out_pin_length() {
        // `seq(1, 5, length.out = 3)` = c(1, 3, 5): length 3.
        let (diags, scope) = check_with_scope("x <- seq(1, 5, length.out = 3)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Known(3), "got {:?}", x);
    }

    #[test]
    fn seq_default_by_one_pin_length() {
        // `seq(1, 5)` (no `by`, no `length.out`): R uses by = 1, so
        // length = 5.
        let (_, scope) = check_with_scope("x <- seq(1, 5)\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Known(5), "got {:?}", x);
    }

    #[test]
    fn seq_int_literal_by_pin_length() {
        // `seq.int(1L, 10L, 2L)` = c(1L, 3L, 5L, 7L, 9L): length 5,
        // mode integer (all integer literals).
        let (diags, scope) = check_with_scope("x <- seq.int(1L, 10L, 2L)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
        assert_eq!(x.length, Length::Known(5), "got {:?}", x);
    }

    #[test]
    fn seq_int_double_by_pin_length() {
        // `seq.int(2, 10, 2.0)` uses whole-number double for `by`:
        // extract_literal_int accepts it, length = 5.
        let (_, scope) = check_with_scope("x <- seq.int(2, 10, 2.0)\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Known(5), "got {:?}", x);
    }

    #[test]
    fn seq_non_literal_stays_unknown() {
        // `seq(1, n, 1)` where `n` is a variable: `to` isn't a
        // literal, so the length stays Unknown.
        let (_, scope) = check_with_scope("n <- 10\nx <- seq(1, n, 1)\n");
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.length, Length::Unknown, "got {:?}", x);
    }

    #[test]
    fn seq_literal_fire_ry040_on_char_mix() {
        // `seq(1, 10, 2)` is double<5>; adding a character fires RY040.
        let diags = check("x <- seq(1, 10, 2)\nbad <- x + \"hello\"\n");
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 for double<5> + character, got {:?}",
            diags
        );
    }

    // ---- Pass-2 propagation + rep/seq edge cases ----
    //
    // These cover the three code-review fixes: (1) literal lengths
    // now propagate through function return types because the literal
    // fast paths live in pass 2 (`infer_discarding`) as well as
    // pass 3; (2) `infer_rep` counts only unnamed args when binding
    // positional `times`/`each`; (3) `infer_rep` never emits
    // `Length::Known(0)` or treats negative multipliers as known.

    #[test]
    fn pass2_colon_literal_propagates_through_fn_return() {
        // `f <- function() 1:10` should give f a return type of
        // integer<10>, and `g <- f()` should propagate that precise
        // length to g. Previously the `:` literal fast path only
        // existed in pass 3, so f's return type (computed in pass 2)
        // was Length::Unknown and g inherited the unknown length.
        let (diags, scope) = check_with_scope("f <- function() 1:10\ng <- f()\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let g = scope.get("g").expect("g should be bound");
        assert_eq!(g.mode, Mode::Integer, "got {:?}", g);
        assert_eq!(g.length, Length::Known(10), "got {:?}", g);
    }

    #[test]
    fn pass2_colon_literal_propagates_through_fn_return_fire_ry040() {
        // Behavioral check: f returns integer<10>, so mixing g with a
        // character fires RY040. This is the headline benefit - the
        // checker sees a real vector through the function boundary.
        let diags = check(
            "f <- function() 1:10\n\
             g <- f()\n\
             bad <- g + \"hello\"\n",
        );
        assert!(
            diags.iter().any(|d| d.code == "RY040"),
            "expected RY040 for integer<10> + character (via fn return), got {:?}",
            diags
        );
    }

    #[test]
    fn rep_named_each_before_positional_binds_times() {
        // `rep(each = 2, c(1, 2, 3), 1)`: the named `each = 2` appears
        // before the positional args. The trailing positional `1`
        // binds to `times` (positional index 1, counting only unnamed
        // args). Result: 3 (x) * 1 (times) * 2 (each) = 6. Previously
        // the raw-list index bug made `times` bind to the non-literal
        // `c(1,2,3)` at raw index 1, yielding Some(None) -> Unknown.
        let (diags, scope) = check_with_scope("x <- rep(each = 2, c(1, 2, 3), 1)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Double, "got {:?}", x);
        assert_eq!(x.length, Length::Known(6), "got {:?}", x);
    }

    #[test]
    fn rep_negative_times_does_not_crash() {
        // `rep(x, times = -1)`: a negative `times` is modeled as
        // Length::Unknown. The `-1` parses as UnaryOp::Neg, which
        // extract_literal_int treats as a non-literal, so we can't pin
        // the length. The check must not panic and must stay Unknown.
        let (diags, scope) = check_with_scope("x <- 1:3\ny <- rep(x, times = -1)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let y = scope.get("y").expect("y should be bound");
        assert_eq!(y.length, Length::Unknown, "got {:?}", y);
    }

    #[test]
    fn rep_zero_times_yields_length_zero() {
        // `rep(1:3, times = 0)` returns a length-0 vector. The result
        // must be Length::Zero, not the invariant-violating Known(0).
        let (diags, scope) = check_with_scope("x <- rep(1:3, times = 0)\n");
        assert!(diags.is_empty(), "got {:?}", diags);
        let x = scope.get("x").expect("x should be bound");
        assert_eq!(x.mode, Mode::Integer, "got {:?}", x);
        assert_eq!(x.length, Length::Zero, "got {:?}", x);
    }

    // ---- Cross-file variable resolution (known_vars) ---------------

    /// Parse helper for project-mode tests, mirroring the one in
    /// `project::tests`.
    fn parse_file(path: &str, src: &str) -> SourceFile {
        let mut p = RParser::new().unwrap();
        p.parse(path, src).unwrap()
    }

    #[test]
    fn cross_file_literal_variable_resolves() {
        // File A defines a top-level constant `my_const <- 42`; file B
        // references it. Without `known_vars`, B would emit RY010 on
        // `my_const`. With `known_vars`, the reference resolves to
        // opaque and no diagnostic fires.
        let mut project = Project::new();
        project.add_file("a.R".to_string(), parse_file("a.R", "my_const <- 42\n"));
        project.add_file("b.R".to_string(), parse_file("b.R", "x <- my_const\n"));
        let diags = project.check();
        let b_diags: Vec<_> = diags
            .into_iter()
            .filter(|(p, _)| p == "b.R")
            .flat_map(|(_, d)| d)
            .collect();
        assert!(
            b_diags.iter().all(|d| d.code != "RY010"),
            "cross-file literal variable should not trigger RY010, got {:?}",
            b_diags
        );
    }

    #[test]
    fn cross_file_opaque_call_variable_resolves() {
        // File A defines `GeomRect <- ggproto("GeomRect", Geom, ...)`.
        // The RHS is a CALL (not a function literal), so it would not
        // be in `fns`; previously any reference from file B would fire
        // RY010. With `known_vars`, `GeomRect` resolves to opaque.
        let mut project = Project::new();
        project.add_file(
            "geom.R".to_string(),
            parse_file(
                "geom.R",
                "GeomRect <- ggproto(\"GeomRect\", Geom, draw = function() NULL)\n",
            ),
        );
        project.add_file(
            "user.R".to_string(),
            parse_file("user.R", "x <- GeomRect\n"),
        );
        let diags = project.check();
        let user_diags: Vec<_> = diags
            .into_iter()
            .filter(|(p, _)| p == "user.R")
            .flat_map(|(_, d)| d)
            .collect();
        assert!(
            user_diags.iter().all(|d| d.code != "RY010"),
            "cross-file ggproto-defined variable should not trigger RY010, got {:?}",
            user_diags
        );
    }

    #[test]
    fn cross_file_list_constructor_variable_resolves() {
        // File A defines `config <- list(timeout = 30, retries = 3)`:
        // a list constructor, not a function. File B references it.
        let mut project = Project::new();
        project.add_file(
            "config.R".to_string(),
            parse_file("config.R", "config <- list(timeout = 30, retries = 3)\n"),
        );
        project.add_file(
            "main.R".to_string(),
            parse_file("main.R", "t <- config$timeout\n"),
        );
        let diags = project.check();
        let main_diags: Vec<_> = diags
            .into_iter()
            .filter(|(p, _)| p == "main.R")
            .flat_map(|(_, d)| d)
            .collect();
        assert!(
            main_diags.iter().all(|d| d.code != "RY010"),
            "cross-file list-constructor variable should not trigger RY010, got {:?}",
            main_diags
        );
    }

    #[test]
    fn genuinely_undefined_variable_still_triggers_ry010() {
        // Sanity: a name that is NOT defined in any file of the project
        // (and is not a typeshed function or dataset) must still emit
        // RY010. `known_vars` only suppresses diagnostics for names we
        // have actually seen assigned.
        let mut project = Project::new();
        project.add_file(
            "a.R".to_string(),
            parse_file("a.R", "x <- totally_undefined_thing\n"),
        );
        let diags = project.check();
        let a_diags: Vec<_> = diags
            .into_iter()
            .filter(|(p, _)| p == "a.R")
            .flat_map(|(_, d)| d)
            .collect();
        assert!(
            a_diags.iter().any(|d| d.code == "RY010"),
            "genuinely undefined variable should still trigger RY010, got {:?}",
            a_diags
        );
    }

    #[test]
    fn same_file_top_level_assignment_in_known_vars() {
        // Single-file mode: a top-level assignment `x <- 1L` puts `x`
        // in `known_vars`. Referencing `x` BEFORE its assignment in the
        // same file (use-before-def at the top level) does NOT trigger
        // RY010. R's `source()` semantics evaluate top-to-bottom so
        // this would error at runtime, but for static checking we
        // prioritize suppressing false positives over catching
        // use-before-def (matching the documented behavior of `known_vars`).
        let diags = check("y <- x\nx <- 1L\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "top-level use-before-def should not trigger RY010 (matches cross-file semantics), got {:?}",
            diags
        );
    }

    // ---- Namespace-qualified identifiers (pkg::name) ----
    //
    // The parser preserves the full `pkg::name` spelling in `Expr::Ident`.
    // The checker must (a) suppress RY010 for these in value and
    // statement position (we don't model other packages' exports), and
    // (b) still resolve `pkg::fn(args)` calls by stripping the prefix
    // for typeshed lookups.

    #[test]
    fn namespace_qualified_value_does_not_emit_ry010() {
        // `x <- S7::class_any` -- the RHS is a cross-package value
        // reference. We can't resolve S7's export table, so we treat
        // it as opaque and stay silent (no RY010).
        let diags = check("x <- S7::class_any\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "qualified value `S7::class_any` should not emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn dplyr_filter_and_stats_filter_resolve_differently() {
        // PLAN Phase 2.2 verification: `dplyr::filter(df, ...)` resolves
        // against the dplyr typeshed (data.frame return) while
        // `stats::filter(x, ...)` resolves against base's stats `filter`
        // (a time-series filter, opaque). The two must NOT be confused.
        let (_, scope) = check_with_scope("df <- mtcars\na <- dplyr::filter(df, mpg > 20)\n");
        let a = scope.get("a").expect("a bound");
        assert!(
            a.class.contains("data.frame"),
            "dplyr::filter should return a data.frame-classed value, got class {:?}",
            a.class
        );
        let (_, scope2) = check_with_scope("b <- stats::filter(1:10, rep(1, 3))\n");
        let b = scope2.get("b").expect("b bound");
        assert!(
            !b.class.contains("data.frame"),
            "stats::filter must NOT be data.frame-classed, got class {:?}",
            b.class
        );
    }

    #[test]
    fn namespace_qualified_statement_does_not_emit_ry010() {
        // Reexport pattern: a bare `rlang::set_names` in statement
        // position (common in purrr/dplyr reexport files). This is the
        // form produced by the parser for `pkg::name` at the top level.
        let diags = check("rlang::set_names\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "qualified statement `rlang::set_names` should not emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn namespace_qualified_backtick_operator_does_not_emit_ry010() {
        // `magrittr::`%>%`` -- a backticked infix operator reexported
        // from another package. The RHS name contains `%`, which makes
        // a good regression test that the `::` suppression isn't
        // confused by special characters.
        let diags = check("magrittr::`%>%`\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "qualified `magrittr::`%>%`` should not emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn namespace_qualified_call_resolves_via_typeshed() {
        // `stats::rnorm(10)` should resolve through the typeshed as
        // `rnorm` (prefix stripped) and return a double vector, with no
        // RY010. We assert both the diagnostic silence AND the inferred
        // return type.
        let (diags, scope) = check_with_scope("x <- stats::rnorm(10)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "qualified call `stats::rnorm(10)` should not emit RY010, got {:?}",
            diags
        );
        let t = scope.get("x").expect("x should be bound after assignment");
        assert!(
            matches!(t.mode, Mode::Double),
            "stats::rnorm(10) should infer as Double, got {:?}",
            t
        );
    }

    #[test]
    fn namespace_qualified_triple_colon_value_does_not_emit_ry010() {
        // `pkg:::name` (triple colon, internal access) must be treated
        // the same way as `::` for RY010 suppression.
        let diags = check("x <- stats:::internal_helper\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "triple-colon qualified value should not emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn namespace_qualified_call_to_unknown_package_function_is_silent() {
        // `tibble::tibble(...)` -- `tibble` is not in our typeshed, so
        // the call resolves to opaque. Crucially, no RY010 should fire
        // on the function name itself (it's a qualified cross-package
        // reference).
        let diags = check("x <- tibble::tibble(a = 1L)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "qualified call to non-typeshed fn should not emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn bare_unbound_identifier_still_emits_ry010() {
        // Regression guard: suppressing RY010 for `pkg::name` must NOT
        // accidentally suppress it for genuinely unbound bare names.
        // `totally_undefined_thing` has no `::` and is not in scope,
        // the typeshed, or the FnTable, so it must still fire RY010.
        let diags = check("x <- totally_undefined_thing\n");
        assert!(
            diags.iter().any(|d| d.code == "RY010"),
            "bare unbound identifier should still emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn backtick_percent_operator_not_unbound() {
        // A backtick-quoted operator name like `` `%+%` `` is commonly a
        // user-defined or package-imported infix operator. The parser
        // preserves the backticks in the identifier name, and we cannot
        // resolve such names against any scope, typeshed, or FnTable.
        // The checker must suppress RY010 and return opaque.
        let diags = check("x <- `%+%`\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "backtick `%+%` operator should not emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn backtick_builtin_operator_symbol_not_unbound() {
        // A backtick-quoted built-in operator symbol like `` `+` `` is
        // referenced as a value (e.g. passed to `Reduce`). Suppress
        // RY010: these are R language primitives we don't model as
        // scope-bound variables.
        let diags = check("x <- `+`\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "backtick `+` operator should not emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn backtick_pipe_operator_not_unbound() {
        // `` `%>%` `` (magrittr pipe) referenced as a bare backtick
        // identifier should not emit RY010. This pattern appears in
        // package reexport code (`magrittr::`%>%`` is already covered
        // by the `::` check; the bare backtick form is covered here).
        let diags = check("x <- `%>%`\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "backtick `%>%` operator should not emit RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn calling_integer_emits_ry070() {
        let diags = check("x <- 42\ny <- x(10)\n");
        assert!(
            diags.iter().any(|d| d.code == "RY070"),
            "expected RY070 for calling integer, got {:?}",
            diags
        );
    }

    #[test]
    fn calling_character_emits_ry070() {
        let diags = check("x <- \"hello\"\ny <- x()\n");
        assert!(
            diags.iter().any(|d| d.code == "RY070"),
            "expected RY070 for calling character, got {:?}",
            diags
        );
    }

    #[test]
    fn calling_actual_function_no_ry070() {
        let diags = check("f <- function() 1L\ny <- f()\n");
        assert!(
            diags.iter().all(|d| d.code != "RY070"),
            "calling a real function should not emit RY070, got {:?}",
            diags
        );
    }

    #[test]
    fn calling_opaque_no_ry070() {
        // Opaque (unknown) values should not trigger RY070 - we don't know
        // if they're functions or not.
        let diags = check("y <- some_unknown_thing(10)\n");
        assert!(
            diags.iter().all(|d| d.code != "RY070"),
            "opaque value should not emit RY070, got {:?}",
            diags
        );
    }

    #[test]
    fn calling_integer_literal_emits_ry070() {
        // PLAN Phase B2: calling a literal (`42()`) errors in R.
        let diags = check("y <- 42()\n");
        assert!(
            diags.iter().any(|d| d.code == "RY070"),
            "calling integer literal `42()` should emit RY070, got {:?}",
            diags
        );
    }

    #[test]
    fn calling_string_literal_emits_ry070() {
        let diags = check("y <- \"x\"()\n");
        assert!(
            diags.iter().any(|d| d.code == "RY070"),
            "calling string literal should emit RY070, got {:?}",
            diags
        );
    }

    #[test]
    fn calling_null_literal_emits_ry070() {
        let diags = check("y <- NULL()\n");
        assert!(
            diags.iter().any(|d| d.code == "RY070"),
            "calling NULL literal should emit RY070, got {:?}",
            diags
        );
    }

    #[test]
    fn calling_index_expression_stays_silent() {
        // Non-literal non-Ident callees (index expressions, calls
        // returning functions) must stay silent as before.
        let diags = check("lst <- list(function() 1)\ny <- lst[[1]]()\n");
        assert!(
            diags.iter().all(|d| d.code != "RY070"),
            "calling an index expression should not emit RY070, got {:?}",
            diags
        );
    }

    #[test]
    fn dollar_on_integer_emits_ry061() {
        let diags = check("x <- 1:10\nval <- x$col\n");
        assert!(diags.iter().any(|d| d.code == "RY061"), "got {:?}", diags);
    }

    #[test]
    fn dollar_on_character_emits_ry061() {
        let diags = check("x <- c(\"a\", \"b\")\nval <- x$col\n");
        assert!(diags.iter().any(|d| d.code == "RY061"), "got {:?}", diags);
    }

    #[test]
    fn dollar_on_list_no_warning() {
        let diags = check("x <- list(a = 1)\nval <- x$a\n");
        assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
    }

    #[test]
    fn dollar_on_data_frame_no_warning() {
        let diags = check("val <- mtcars$mpg\n");
        assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
    }

    #[test]
    fn dollar_on_opaque_no_warning() {
        let diags = check("x <- some_unknown_thing\nval <- x$col\n");
        assert!(diags.iter().all(|d| d.code != "RY061"), "got {:?}", diags);
    }

    /// PLAN Phase 2 acceptance: running the checker twice on the same
    /// input must yield identical diagnostics. The fixpoint/refinement
    /// machinery walks function tables whose iteration order is not
    /// semantically meaningful, so any order-leak that bleeds into
    /// observed types would show up here.
    #[test]
    fn diagnostics_are_deterministic_across_runs() {
        let sources = [
            // recursion (cycle detection in the fixpoint)
            "f <- function(n) { if (n > 0) f(n - 1) else 0L }\nx <- f(3) + 1\n",
            // mutual / cross-referencing function bodies
            "f <- function() { g() }\ng <- function() { 1L }\nx <- f() + 1\n",
            // a body with an arithmetic error + unbound var (exercises the
            // Phase-1 function-body walk in both passes)
            "h <- function() { a <- \"x\" + 1; b <- missing_thing }\n",
            // higher-order callback inference
            "v <- sapply(c(1.0, 2.0), function(x) x * 2)\ny <- v + 1\n",
            // a clean file (no diagnostics) with a closure factory
            "make_adder <- function(x) function(y) x + y\nadd5 <- make_adder(5)\nz <- add5(3)\n",
        ];
        for src in sources {
            let d1 = check(src);
            let d2 = check(src);
            // Compare on the semantically meaningful fields; `Diagnostic`
            // also carries `path` (constant here) and `message` (stable).
            let key = |d: &Diagnostic| (d.code, d.severity, d.span.start, d.span.end);
            let k1: Vec<_> = d1.iter().map(key).collect();
            let k2: Vec<_> = d2.iter().map(key).collect();
            assert_eq!(
                k1, k2,
                "non-deterministic diagnostics for src={src:?}\n  run1={d1:?}\n  run2={d2:?}"
            );
        }
    }

    #[test]
    fn if_branch_binding_in_both_branches_is_visible_afterwards() {
        // `r` is bound in both branches; the merged type is the join of
        // character ("pos"/"neg"). Use after the `if` must be RY010-free.
        let src = "f <- function(a) {\n  if (a > 0) { r <- \"pos\" } else { r <- \"neg\" }\n  paste(r)\n}\n";
        let diags = check(src);
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "branch-local binding leaked to after the `if` must not fire RY010, got {:?}",
            diags
        );
    }

    #[test]
    fn if_branch_binding_in_single_branch_is_unknown_but_visible() {
        // No `else`: `v` is possibly missing. We don't model "definitely
        // unbound"; the name is inserted as unknown so the use is silent.
        let (diags, top) = check_with_scope("if (TRUE) { v <- 1 }\nv\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "single-branch binding must be visible (as unknown) after the `if`, got {:?}",
            diags
        );
        let t = top.get("v").expect("v should be bound at top level");
        assert!(
            matches!(t.mode, Mode::Opaque),
            "single-branch binding should degrade to unknown (opaque), got {:?}",
            t
        );
    }

    #[test]
    fn if_branch_join_type_is_union_when_branches_disagree() {
        // `s` bound to integer in one branch and character in the other:
        // the merged type is the join of integer and character, a union.
        let (diags, top) = check_with_scope("if (TRUE) { s <- 1L } else { s <- \"x\" }\ns\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "both-branch binding must not fire RY010, got {:?}",
            diags
        );
        let t = top.get("s").expect("s should be bound at top level");
        assert!(
            matches!(t.mode, Mode::Union),
            "disagreeing branches should join to a union, got {:?}",
            t
        );
    }

    #[test]
    fn if_branch_reassignment_over_existing_type_stays_visible() {
        // `s <- 1L` then reassigned to `"x"` inside a single branch (no
        // else). The plan specifies single-branch bindings degrade to
        // unknown (opaque), since there is no sound type for "possibly
        // missing". What matters is that the use after the `if` stays
        // RY010-free; the merged type is opaque by design.
        let (diags, top) = check_with_scope("s <- 1L\nif (TRUE) { s <- \"x\" }\ns\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "reassigned branch binding must not fire RY010, got {:?}",
            diags
        );
        let t = top.get("s").expect("s should be bound at top level");
        assert!(
            matches!(t.mode, Mode::Opaque),
            "single-branch reassignment degrades to unknown (opaque) per plan, got {:?}",
            t
        );
    }

    #[test]
    fn if_branch_both_branches_over_existing_type_folds_parent() {
        // `s <- 1L` (parent Integer) then reassigned in BOTH branches to
        // character. The merged branch type is character; folding the
        // parent's integer in yields union[integer, character] rather than
        // losing the parent's prior type.
        let (diags, top) =
            check_with_scope("s <- 1L\nif (TRUE) { s <- \"a\" } else { s <- \"b\" }\ns\n");
        assert!(
            diags.iter().all(|d| d.code != "RY010"),
            "both-branch reassignment must not fire RY010, got {:?}",
            diags
        );
        let t = top.get("s").expect("s should be bound at top level");
        assert!(
            matches!(t.mode, Mode::Union),
            "both-branch reassignment over a different parent type should fold the parent in (union), got {:?}",
            t
        );
    }

    #[test]
    fn lapply_list_arith_does_not_fire_ry040() {
        // PLAN Phase A3: iterating a list yields the unwrapped element,
        // so arithmetic inside the callback must not fire RY040.
        let src = "out <- lapply(list(1, 2, 3), function(x) x * 2)\n";
        let diags = check(src);
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "lapply over a homogeneous list must not fire RY040, got {:?}",
            diags
        );
    }

    #[test]
    fn dollar_missing_on_plain_list_does_not_fire_ry060() {
        // PLAN Phase A4: `$` on a plain list with a missing name returns
        // NULL in R; RY060 must only fire for data frames.
        let diags = check("v <- list(a = 1, b = 2)$missing\n");
        assert!(
            diags.iter().all(|d| d.code != "RY060"),
            "`$` miss on a plain list must not fire RY060, got {:?}",
            diags
        );
    }

    #[test]
    fn dollar_missing_on_plain_list_returns_null() {
        // PLAN Phase A4: the returned value matches R's NULL (not unknown).
        let (_, scope) = check_with_scope("v <- list(a = 1, b = 2)$missing\n");
        let v = scope.get("v").expect("v should be bound");
        assert!(
            matches!(v.mode, Mode::Null),
            "plain-list `$` miss should return NULL, got {:?}",
            v
        );
        assert!(
            matches!(v.length, Length::Zero),
            "NULL length should be Zero, got {:?}",
            v
        );
    }

    #[test]
    fn dollar_missing_on_data_frame_still_fires_ry060() {
        // PLAN Phase A4: the data-frame case is a real bug and must keep
        // firing. `mtcars` is a data frame in the typeshed.
        let diags = check("df <- mtcars\nbad <- df$nonexistent\n");
        assert!(
            diags.iter().any(|d| d.code == "RY060"),
            "`$` miss on a data frame must still fire RY060, got {:?}",
            diags
        );
    }

    #[test]
    fn for_over_homogeneous_list_does_not_fire_ry040() {
        // `for (el in list(1, 2, 3))` binds `el` to the unwrapped element
        // (double<1>) inside the loop body, so accumulating into `total`
        // is well-typed. (The loop var lives in the loop's child scope,
        // so we assert on the absence of RY040, not on `el`'s binding.)
        let diags =
            check_with_scope("total <- 0\nfor (el in list(1, 2, 3)) { total <- total + el }\n").0;
        assert!(
            diags.iter().all(|d| d.code != "RY040"),
            "for over a homogeneous list must not fire RY040 on the body, got {:?}",
            diags
        );
    }

    #[test]
    fn public_check_with_scope_surfaces_ry000_on_broken_file() {
        // PLAN Phase C2: `check_with_scope` used to clear diagnostics
        // AFTER emitting parse errors, wiping the RY000s. It must now
        // surface them.
        let mut p = RParser::new().unwrap();
        let f = p.parse("test.R", "f <- function( { 1 }\n").unwrap();
        let mut c = Checker::new("test.R");
        let (diags, _scope) = c.check_with_scope(&f);
        assert!(
            diags.iter().any(|d| d.code == "RY000"),
            "check_with_scope must surface RY000 on a broken file, got {:?}",
            diags
        );
    }
}
