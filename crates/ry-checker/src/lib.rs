//! Local type inference + diagnostics.
//!
//! v1 scope: single-file, inference-only, NSE-opaque. We walk statements
//! top-down, maintaining a per-scope binding table `name -> RType`.
//!
//! v2 additions: interprocedural function-return inference via a
//! module-level FnTable and a fixpoint loop. The first pass collects
//! function definitions; subsequent passes refine each function's inferred
//! return type until stable (or the depth cap is hit).

// Not vestigial: collapsible-if sites remain in infer/,
// higher_order.rs, and collect.rs.
#![allow(clippy::collapsible_if)]

mod collect;
pub mod diagnostics;
pub mod format;
mod higher_order;
mod infer;
mod nse;
pub mod project;
mod resolve;
pub mod rules;
pub mod semantic_lists;

pub use project::Project;
// Re-export the diagnostic data types and suppression helpers at the
// crate root for back-compat (callers and tests reference
// `ry_checker::{Severity, Diagnostic, ...}` directly).
pub use diagnostics::{
    Confidence, Diagnostic, Severity, SeverityFilter, Suppression, apply_filter_to_diagnostics,
    filter_suppressed_with_comments, has_file_suppression_from_comments, is_suppressed,
    parse_suppressions_from_comments,
};

// These builders live here, not in ry-config, because ry-checker depends
// on ry-config and the reverse direction would be a cycle.

/// Build a [`SeverityFilter`] from the `error`, `warn`, and `ignore`
/// rule lists in a config.
pub fn build_filter(error: &[String], warn: &[String], ignore: &[String]) -> SeverityFilter {
    let mut f = SeverityFilter::default();
    for e in error {
        f.add_error(e);
    }
    for w in warn {
        f.add_warn(w);
    }
    for i in ignore {
        f.add_ignore(i);
    }
    f
}

/// Convenience: build a [`SeverityFilter`] directly from a
/// config's `error`, `warn`, `ignore`, `select`, and `extend_select` fields.
pub fn filter_from_config(cfg: &ry_config::Config) -> SeverityFilter {
    let mut filter = build_filter(&cfg.error, &cfg.warn, &cfg.ignore);
    if let Some(select) = &cfg.select {
        filter.begin_selection();
        for rule in select {
            filter.add_select(rule);
        }
    }
    for rule in &cfg.extend_select {
        filter.add_extend_select(rule);
    }
    filter
}

use crate::infer::semantic_argument_name;
use ry_core::Span;
use ry_core::ast::*;
use ry_core::types::{ClassVector, ColumnSchema, FunctionSignature, Length, Mode, RType};
use ry_typeshed::{
    AssertionProvenanceKind, AssertionSpec, CallbackArg, ConditionalScopeEffect,
    DefaultCurrentScope, EvalMode, FunctionSig, Globals, HigherOrderResultKind, HigherOrderSpec,
    JsonLength, JsonMode, JsonRType, ParamSpec, ReturnLengthSpec, ReturnSlot, ReturnSpec,
    SchemaEffect, ScopeEffect, Typeshed, is_known_package, known_packages, load_base_cached,
    load_package,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

fn string_literals(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::String(value, _) => vec![value.clone()],
        Expr::Call { func, args, .. } => {
            let Some(name) = ident_name(func) else {
                return Vec::new();
            };
            let bare = crate::semantic_lists::bare_name(name);
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

/// Whether `library()` / `require()` was passed `character.only = TRUE`.
fn character_only(args: &[Arg]) -> bool {
    args.iter().any(|argument| {
        argument.name.as_deref() == Some("character.only")
            && matches!(argument.value, Expr::Logical(true, _))
    })
}

/// The package a `library()` / `require()` call attaches, given its
/// argument list: the first argument names the package as a bare symbol
/// — unless `character.only = TRUE` restricts it to a string literal.
/// `None` when no literal name is available.
fn attached_package_name(args: &[Arg]) -> Option<&str> {
    match &args.first()?.value {
        Expr::Ident { name, .. } if !character_only(args) => Some(name),
        Expr::String(name, _) => Some(name),
        _ => None,
    }
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
    use crate::semantic_lists::OPERATORS;
    OPERATORS.iter().find_map(|operator| {
        name.strip_prefix(operator)
            .and_then(|rest| rest.strip_prefix('.'))
            .filter(|class| !class.is_empty())
            .map(|class| (*operator, class.to_string()))
    })
}

/// Whether a file is plausibly sourced into a Shiny application server.
/// This is ry's one built-in ambient-environment extension; user-defined
/// profiles (named, path-glob-triggered) come from `ry.toml`
/// `[[environments]]` and are threaded through the CLI config instead.
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
    /// Bindings that still refer directly to function parameters. Assigning
    /// to the name clears this marker; flow narrowing preserves it.
    pub parameter_bindings: HashSet<String>,
    /// Bindings derived from list-valued expressions even when later subset
    /// inference loses the concrete mode. Used by container-shape rules.
    pub list_origin_bindings: HashSet<String>,
    /// Bindings whose current type came from a function parameter default.
    /// A default is one call shape, not a complete declaration of the
    /// parameter's runtime type, so an explicit `is.*()` guard may replace
    /// an otherwise incompatible default-derived type in its true branch.
    pub default_parameter_bindings: HashSet<String>,
    /// Bare-identifier function aliases, keyed by the local binding name.
    /// The value is the ultimate semantic callee name used by call inference.
    pub function_aliases: HashMap<String, String>,
    /// Function literals defined in a nested lexical environment. These must
    /// not be resolved through the project-wide, name-only function table.
    pub(crate) lexical_functions: HashSet<String>,
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
        self.lexical_functions.remove(&name);
        self.list_origin_bindings.remove(&name);
        self.parameter_bindings.remove(&name);
        self.default_parameter_bindings.remove(&name);
        self.narrowed_bindings.remove(&name);
        self.bindings.insert(name, t);
    }

    pub(crate) fn insert_narrowed(&mut self, name: impl Into<String>, t: RType) {
        // Narrowing refines the value without rebinding the name, so the
        // parameter markers survive `insert`'s clearing.
        let name = name.into();
        let was_parameter = self.parameter_bindings.contains(&name);
        let was_default_parameter = self.default_parameter_bindings.contains(&name);
        self.insert(name.clone(), t);
        if was_parameter {
            self.parameter_bindings.insert(name.clone());
        }
        if was_default_parameter {
            self.default_parameter_bindings.insert(name.clone());
        }
        self.narrowed_bindings.insert(name);
    }

    pub(crate) fn insert_parameter(&mut self, name: impl Into<String>, t: RType) {
        let name = name.into();
        self.insert(name.clone(), t);
        self.parameter_bindings.insert(name);
    }

    pub(crate) fn insert_parameter_default(&mut self, name: impl Into<String>, t: RType) {
        // Unlike a plain rebinding, a defaulted parameter shadows its
        // captured-scope namesake without disturbing the lexical-function
        // and list-origin markers (`insert` would clear both).
        let name = name.into();
        let was_lexical_function = self.lexical_functions.contains(&name);
        let was_list_origin = self.list_origin_bindings.contains(&name);
        self.insert(name.clone(), t);
        if was_lexical_function {
            self.lexical_functions.insert(name.clone());
        }
        if was_list_origin {
            self.list_origin_bindings.insert(name.clone());
        }
        self.parameter_bindings.insert(name.clone());
        self.default_parameter_bindings.insert(name);
    }

    pub(crate) fn mark_list_origin(&mut self, name: impl Into<String>) {
        self.list_origin_bindings.insert(name.into());
    }

    pub(crate) fn has_list_origin(&self, name: &str) -> bool {
        self.list_origin_bindings.contains(name)
    }

    pub(crate) fn is_parameter(&self, name: &str) -> bool {
        self.parameter_bindings.contains(name)
    }

    pub(crate) fn is_default_parameter(&self, name: &str) -> bool {
        self.default_parameter_bindings.contains(name)
    }

    pub(crate) fn set_function_alias(&mut self, name: impl Into<String>, target: String) {
        self.function_aliases.insert(name.into(), target);
    }

    pub(crate) fn mark_lexical_function(&mut self, name: impl Into<String>) {
        self.lexical_functions.insert(name.into());
    }

    pub(crate) fn is_lexical_function(&self, name: &str) -> bool {
        self.lexical_functions.contains(name)
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

/// Which lexical scope a [`ScopeRecord`] snapshot came from.
///
/// R has exactly two lexical scope kinds: the top level of a source file
/// and function bodies. Braced blocks, `if` branches, and loop bodies all
/// assign into the enclosing function's environment, so no other kinds
/// exist to record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeRecordKind {
    Top,
    Function,
}

/// One lexical scope's binding table, snapshotted when the checker's
/// diagnostic walk finishes that scope's body.
///
/// The snapshot is the FINAL state of the table (after the body's last
/// statement), matching what a reader at the closing brace would observe;
/// consumers that need "in scope at line N" semantics can use each
/// binding's definition position, which the dump layer derives from the
/// AST. Only the pass-3 (diagnostic-emitting) walk records scopes: the
/// fixpoint and signature-building walks reuse the same walker but run
/// in discarding mode, so no scope is captured twice.
#[derive(Debug, Clone)]
pub struct ScopeRecord {
    pub kind: ScopeRecordKind,
    /// The bound name for `f <- function(...)` definitions; `None` for
    /// anonymous statement-position literals and the top level.
    pub name: Option<String>,
    /// Span of the function literal; the whole file for the top level.
    pub span: Span,
    /// `(name, span)` for every formal parameter. Parameters are also
    /// present in `scope.bindings` (marked in `parameter_bindings`), but
    /// the source spans live only here because `Scope` is a plain
    /// name-to-type table.
    pub params: Vec<(String, Span)>,
    /// The final binding table of the scope.
    pub scope: Scope,
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UserParam {
    pub(crate) name: String,
    pub(crate) type_: RType,
    pub(crate) required: bool,
    pub(crate) defused: bool,
    /// Whether the function captures this argument as an unevaluated
    /// expression (for example through `substitute(x)`).
    pub(crate) quoting: bool,
}

/// The complete portion of a user-defined function signature that can affect
/// how a caller is analyzed. Keep this snapshot separate from return slots:
/// callers depend on argument matching and evaluation semantics even when a
/// function's inferred return type is unchanged.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CallerVisibleSignature {
    parameters: Vec<UserParam>,
}

impl UserFn {
    pub(crate) fn caller_visible_signature(&self) -> CallerVisibleSignature {
        CallerVisibleSignature {
            parameters: self.params.clone(),
        }
    }

    /// Returns whether the seed wrote any parameter metadata.
    fn seed_caller_visible_signature(&mut self, signature: &CallerVisibleSignature) -> bool {
        // A function outside the incremental fixpoint scope has an unchanged
        // definition. Still guard the identity shape so a bad scope can never
        // copy metadata onto different formals.
        if self.params.len() != signature.parameters.len()
            || self
                .params
                .iter()
                .zip(&signature.parameters)
                .any(|(current, previous)| current.name != previous.name)
        {
            return false;
        }
        for (current, previous) in self.params.iter_mut().zip(&signature.parameters) {
            *current = previous.clone();
        }
        true
    }
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
    // Names bound to callable objects that are not ordinary R functions.
    // S7 class and generic objects implement `()` themselves, so call-position
    // lookup must treat them as candidates even though their inferred value is
    // otherwise opaque.
    pub(crate) callable_vars: std::collections::HashSet<String>,
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
        collected: &FnTable,
        return_slots: &mut ReturnSlots,
        collected_slots: &ReturnSlots,
    ) {
        let slot_offset = return_slots.0.len();
        return_slots.0.extend_from_slice(&collected_slots.0);

        self.fns.extend(collected.fns.iter().map(|(name, f)| {
            let mut f = f.clone();
            f.return_slot += slot_offset;
            (name.clone(), f)
        }));
        self.s3_methods.extend(
            collected
                .s3_methods
                .iter()
                .map(|(k, &slot)| (k.clone(), slot + slot_offset)),
        );
        self.s4_methods.extend(
            collected
                .s4_methods
                .iter()
                .map(|(k, &slot)| (k.clone(), slot + slot_offset)),
        );
        self.s4_classes.extend(collected.s4_classes.clone());
        self.known_vars.extend(collected.known_vars.iter().cloned());
        self.callable_vars
            .extend(collected.callable_vars.iter().cloned());
        for (name, sites) in &collected.call_sites {
            self.call_sites
                .entry(name.clone())
                .or_default()
                .extend(sites.iter().cloned());
        }
        self.forwarded_calls
            .extend(collected.forwarded_calls.iter().cloned());
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
    /// Source text corresponding to `path`, set at every production check seam.
    /// Messages that quote source spelling slice this exact text by parser
    /// spans.
    pub(crate) source: String,
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
    // Whether this file's package declares `useDynLib(..., .registration =
    // TRUE)`. Derived from `external_bindings` in `set_external_bindings`.
    pub(crate) native_registration: bool,
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
    // When true, the pass-3 walk snapshots every completed lexical scope
    // into `scope_records`. Off by default so ordinary checks (and the
    // LSP) pay nothing; `dump-types` opts in. Recording is additionally
    // suppressed while `discarding`, which keeps the fixpoint and
    // signature-building walks (the same walker in discarding mode) from
    // double-capturing a body.
    capture_scopes: bool,
    scope_records: Vec<ScopeRecord>,
}

impl Checker {
    pub fn new(path: &str) -> Self {
        Self::with_tables_impl(
            path,
            Arc::new(FnTable::default()),
            Arc::new(ReturnSlots::default()),
        )
    }

    pub fn check(&mut self, file: &SourceFile) -> &[Diagnostic] {
        // Passes 1-2 (collect + fixpoint) reset every derived table first,
        // so a second `check` on the same instance starts fresh rather
        // than accumulating the previous run's functions, known-vars, and
        // diagnostics.
        self.run_passes(file);

        // Pass 3: final walk, emitting all diagnostics. Function calls
        // now resolve against the refined FnTable.
        self.emit_diagnostics(file);
        &self.diagnostics
    }

    // Check a file and return both diagnostics and the final top-level
    // scope. Used by the LSP server's scope cache: the scope maps variable
    // names to their inferred types, feeding inlay hint lookups.
    pub fn check_with_scope(&mut self, file: &SourceFile) -> (Vec<Diagnostic>, Scope) {
        self.run_passes(file);
        // Emit parse errors after the collection/refinement passes (both
        // run with emission suppressed), so RY000s lead the diagnostic
        // vector rather than being wiped or buried by it.
        let scope = self.emit_diagnostics(file);
        (std::mem::take(&mut self.diagnostics), scope)
    }

    /// The shared prologue of [`check`](Self::check) and
    /// [`check_with_scope`](Self::check_with_scope): set the source seams,
    /// clear the previous run's diagnostics and derived tables, run pass 1
    /// (collection) and pass 2 (the return-type fixpoint), and refresh
    /// `known_vars` from the refined table. Emits nothing: collection is
    /// silent by design and the fixpoint forces discarding mode.
    fn run_passes(&mut self, file: &SourceFile) {
        self.path = file.path.clone();
        self.source.clone_from(&file.source);
        self.diagnostics.clear();
        self.fn_table = Arc::new(FnTable::default());
        self.return_slots = Arc::new(ReturnSlots::default());

        // Pass 1: collect function definitions into the FnTable. We don't
        // emit diagnostics yet - the body's `return` types depend on the
        // table being fully populated.
        self.collect_fns(&file.stmts);

        // Pass 2 (fixpoint): refine each function's inferred return type
        // until the table stabilizes or we hit MAX_FIXPOINT_DEPTH.
        self.run_fixpoint();
        self.known_vars = Arc::new(self.fn_table.known_vars.clone());
    }

    // Construct a checker that uses pre-populated function tables.
    // Used by `Project` for passes 1 and 2, where a single throwaway
    // checker owns the (mutable) tables and hands them back via
    // [`into_tables`]. The fresh checker starts with an empty
    // diagnostics vec and an empty `inferring` stack.
    //
    // [`into_tables`]: Checker::into_tables
    pub(crate) fn with_tables(path: &str, fn_table: FnTable, return_slots: ReturnSlots) -> Self {
        Self::with_tables_impl(path, Arc::new(fn_table), Arc::new(return_slots))
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
        Self::with_tables_impl(path, fn_table, return_slots)
    }

    /// Shared private constructor: builds a checker with the given
    /// (already-shared) tables and the standard default field list.
    /// The three public/crate constructors differ only in table
    /// ownership, so they delegate here rather than re-listing every
    /// field (keeps the field list in one place).
    fn with_tables_impl(
        path: &str,
        fn_table: Arc<FnTable>,
        return_slots: Arc<ReturnSlots>,
    ) -> Self {
        Self {
            typeshed: embedded_base(),
            user_stubs: Arc::new(BTreeMap::new()),
            diagnostics: Vec::new(),
            path: path.to_string(),
            source: String::new(),
            discarding: false,
            validate_user_call_arguments: true,
            fn_table,
            known_vars: Arc::new(HashSet::new()),
            return_slots,
            inferring: Vec::new(),
            loaded: Arc::new(HashSet::new()),
            bare_loaded: Arc::new(HashSet::new()),
            external_bindings: HashSet::new(),
            native_registration: false,
            imported_from: HashMap::new(),
            external_s3_methods: HashSet::new(),
            load_bindings: HashMap::new(),
            deferred_captures: Vec::new(),
            enclosing_formals: Vec::new(),
            pipe_argument_types: HashMap::new(),
            capture_scopes: false,
            scope_records: Vec::new(),
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

    // Pass 1: collect this file's function definitions into the shared
    // `FnTable` and harvest its `library()`/`require()` attachments in
    // the same walk — one collection pass instead of a fn-collection
    // walk plus a discarding inference walk (issue #178). Does NOT emit
    // diagnostics; returns the attachments for `Project::check` to
    // union across files.
    pub(crate) fn collect_file_fns(&mut self, file: &SourceFile) -> HashSet<String> {
        self.path = file.path.clone();
        self.collect_fns(&file.stmts);
        self.harvest_attached_packages(&file.stmts)
    }

    /// Packages attached anywhere in `stmts` by `library(pkg)` /
    /// `require(pkg)`, collected on the shared walker (pure syntax, no
    /// inference; the callee must be the bare name — a string call head
    /// is R-legal and treated the same). Not a superset of the inference
    /// walk it replaced: direct calls after code the walker proves
    /// unreachable (past a `stop()`) are now included — the safe
    /// direction for a project-wide union — while the rare alias
    /// indirection `lib <- library; lib(dplyr)` is not.
    fn harvest_attached_packages(&self, stmts: &[Stmt]) -> HashSet<String> {
        use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
        use std::ops::ControlFlow;

        let mut attached = HashSet::new();
        let _ = walk_stmts(
            stmts,
            Walk::ALL,
            |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
                if let AstNode::Expr(Expr::Call { func, args, .. }) = node
                    && matches!(binding_name(func), Some("library" | "require"))
                    && let Some(package) = attached_package_name(args)
                {
                    attached.insert(package.to_string());
                }
                ControlFlow::Continue(Descend::Into)
            },
        );
        attached
    }

    /// Overlay previously-refined return types onto the current return slots.
    ///
    /// Called before [`run_fixpoint`](Self::run_fixpoint) to seed the fixpoint
    /// with the previous solution. Functions whose definition
    /// has not changed and whose callees' return types are unchanged will
    /// keep their seeded value, reducing the number of fixpoint iterations
    /// needed to re-stabilise.
    ///
    /// Only functions that exist in both the current table and the seed map
    /// are updated. Functions new to the table (or absent from the seed)
    /// keep their pass-1 collection value.
    pub(crate) fn seed_return_types(&mut self, seed: &HashMap<String, ry_core::RType>) {
        for (name, uf) in &self.fn_table.fns {
            if let Some(t) = seed.get(name) {
                Arc::make_mut(&mut self.return_slots).set(uf.return_slot, t.clone());
            }
        }
    }

    /// Restore propagated caller-visible metadata for functions outside the
    /// current fixpoint scope. Pass-1 caches contain definition-local flags;
    /// without this seed, an unrelated incremental edit would silently drop
    /// quoting/defusing propagated from another function.
    pub(crate) fn seed_caller_visible_signatures(
        &mut self,
        seed: &HashMap<String, CallerVisibleSignature>,
        scope: Option<&HashSet<String>>,
    ) {
        let table = Arc::make_mut(&mut self.fn_table);
        for (name, function) in &mut table.fns {
            if scope.is_some_and(|scope| scope.contains(name)) {
                continue;
            }
            if let Some(signature) = seed.get(name) {
                function.seed_caller_visible_signature(signature);
            }
        }
    }

    // Pass 2: refine all function return types until convergence.
    // Safe to call once, after all files have been collected.
    //
    // S3 methods (`print.foo`, etc.) sit in `fns` under their full
    // name, with `s3_methods` pointing at the same return slot, so
    // iterating `fns` refines their bodies alongside regular
    // functions; dispatch reads the refined slot via `s3_methods`.
    pub(crate) fn run_fixpoint(&mut self) {
        self.run_fixpoint_inner(None);
    }

    /// Run the fixpoint, but only refine functions in `scope`. Functions
    /// outside the scope keep their current (seeded) return type. Used by
    /// `Project` for incremental checks where only a subset of functions
    /// can have changed.
    ///
    /// The set must include every function whose definition or callees
    /// changed; functions outside the set are assumed stable. The fixpoint
    /// still iterates until convergence *within the scope* — a scoped
    /// function whose return type changes can still affect other scoped
    /// functions that call it.
    pub(crate) fn run_fixpoint_scoped(&mut self, scope: &HashSet<String>) {
        self.run_fixpoint_inner(Some(scope));
    }

    /// Shared fixpoint loop. When `scope` is `None`, refines all functions;
    /// when `Some`, only functions in the scope set.
    fn run_fixpoint_inner(&mut self, scope: Option<&HashSet<String>>) {
        if scope.is_some_and(|s| s.is_empty()) {
            return;
        }
        let prev_discarding = self.discarding;
        self.discarding = true;
        for _ in 0..MAX_FIXPOINT_DEPTH {
            let before = (*self.return_slots).clone();
            let names: Vec<String> = match scope {
                Some(s) => self
                    .fn_table
                    .fns
                    .keys()
                    .filter(|name| s.contains(*name))
                    .cloned()
                    .collect(),
                None => self.fn_table.fns.keys().cloned().collect(),
            };
            for name in names {
                self.refine_fn_return(&name);
            }
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
                if semantic_argument_name(method_name)
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
    // first if you want only this file's diagnostics. Returns the final
    // top-level scope (also what `check_with_scope` hands to the LSP).
    pub(crate) fn emit_diagnostics(&mut self, file: &SourceFile) -> Scope {
        self.path = file.path.clone();
        self.source.clone_from(&file.source);
        self.emit_parse_errors(file);
        let mut scope = self.top_level_scope();
        for s in &file.stmts {
            self.check_stmt(s, &mut scope);
        }
        // The top level is itself a lexical scope in R; record it after
        // the walk so the snapshot reflects every top-level assignment.
        if self.capture_scopes {
            let record = ScopeRecord {
                kind: ScopeRecordKind::Top,
                name: None,
                span: whole_file_span(&self.source),
                params: Vec::new(),
                scope: scope.clone(),
            };
            self.scope_records.push(record);
        }
        scope
    }

    /// Opt this checker into snapshotting every completed lexical scope
    /// during the diagnostic walk. See [`ScopeRecord`].
    pub fn enable_scope_capture(&mut self) {
        self.capture_scopes = true;
    }

    /// Take the scopes recorded since the last call. Empty unless
    /// [`enable_scope_capture`](Self::enable_scope_capture) was called.
    pub fn take_scope_records(&mut self) -> Vec<ScopeRecord> {
        std::mem::take(&mut self.scope_records)
    }

    // Snapshot one completed function-body scope. Called at the end of
    // `walk_stmt`'s two function-definition arms; `discarding` guards the
    // fixpoint / signature-building re-walks of the same body.
    pub(crate) fn record_scope(
        &mut self,
        name: Option<&str>,
        span: Span,
        params: &[Param],
        scope: &Scope,
    ) {
        if !self.capture_scopes || self.discarding {
            return;
        }
        self.scope_records.push(ScopeRecord {
            kind: ScopeRecordKind::Function,
            name: name.map(str::to_string),
            span,
            params: params.iter().map(|p| (p.name.clone(), p.span)).collect(),
            scope: scope.clone(),
        });
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
            .contains(ry_core::SERIALIZED_BINDINGS_UNENUMERABLE)
        {
            scope.mark_search_path_unknown();
        }
        if is_shiny_app_fragment_path(&self.path) {
            for name in crate::semantic_lists::BUILTIN_ENVIRONMENT_BINDINGS {
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

    /// All package names with typeshed signatures, in resolution
    /// order: the embedded known packages, then user-stub packages
    /// that do not shadow a known one. Each name appears once. The
    /// iterator chains over static data and the stub map directly, so
    /// no per-call `Vec` is allocated (callers iterate inside nested
    /// loops over loaded packages).
    pub(crate) fn available_package_names<'a>(&'a self) -> impl Iterator<Item = &'a str> + 'a {
        known_packages().map(|package| package as &'a str).chain(
            self.user_stubs
                .keys()
                .map(String::as_str)
                .filter(|package| *package != "base" && !is_known_package(package)),
        )
    }

    // Seed opaque bindings established by metadata for this source file.
    pub fn set_external_bindings(&mut self, bindings: HashSet<String>) {
        self.native_registration =
            bindings.contains(ry_workspace::packages::NATIVE_REGISTRATION_SENTINEL);
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
}

fn embedded_base() -> Arc<Typeshed> {
    static BASE: std::sync::OnceLock<Arc<Typeshed>> = std::sync::OnceLock::new();
    Arc::clone(
        BASE.get_or_init(|| Arc::new(load_base_cached().expect("typeshed must load").clone())),
    )
}

/// Span covering the entire file, used as the top-level scope record's
/// extent so position queries anywhere in the file resolve to it.
fn whole_file_span(source: &str) -> Span {
    let line = source.matches('\n').count();
    let col = source
        .rsplit_once('\n')
        .map(|(_, last)| last.len())
        .unwrap_or(source.len());
    Span::new(0, source.len(), line, col)
}

#[cfg(test)]
mod tests;
