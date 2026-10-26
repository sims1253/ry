//! Unguarded network calls during package load (RY124).
//!
//! CRAN policy and basic hygiene agree: loading a package must not use
//! the network. Code at the top level of a package's `R/` files runs
//! when the namespace is built, `data-raw` scripts run as written, and
//! `.onLoad`/`.onAttach` run on `library()`. A proven network call in
//! those positions therefore fires; the same call inside an ordinary
//! function runs only when a user invokes it and stays silent.
//!
//! Connection constructors follow R's opening rules, verified against
//! Rscript: `url()` and `curl::curl()` defer connecting until opened
//! (their `open` formal defaults to `""`), while `socketConnection()`
//! connects immediately (`open` defaults to `"a+"`). Only a literal
//! `open` argument can settle the question; a computed one stays silent.
//! The direct path and the project-wrapper path share
//! `call_reaches_network`, so a computed `open` is silent in both: a
//! wrapper never warns where its inlined body would not.

use super::callee::CallHead;
use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use std::ops::ControlFlow;

/// How a recognized head reaches the network.
#[derive(Clone, Copy)]
enum NetworkHead {
    /// The call itself performs the request.
    Performs,
    /// A connection constructor: it connects only once opened. `formals`
    /// is the constructor's full formal list (so `match_arguments` can
    /// bind `open` exactly as R does), and `opens_by_default` records
    /// the `open` formal's default (`"a+"` for `socketConnection`).
    Connection {
        formals: &'static [&'static str],
        opens_by_default: bool,
    },
}

/// The package a bare member must resolve to, plus its network shape.
/// Deliberately a closed list: spelling alone never proves a request.
fn network_head(lookup_name: &str) -> Option<(&'static str, NetworkHead)> {
    Some(match lookup_name {
        "download.file" => ("utils", NetworkHead::Performs),
        "url" => (
            "base",
            NetworkHead::Connection {
                formals: &[
                    "description",
                    "open",
                    "blocking",
                    "encoding",
                    "method",
                    "headers",
                ],
                opens_by_default: false,
            },
        ),
        "socketConnection" => (
            "base",
            NetworkHead::Connection {
                formals: &[
                    "host", "port", "server", "blocking", "open", "encoding", "timeout", "options",
                ],
                opens_by_default: true,
            },
        ),
        "curl_download" | "curl_fetch_memory" => ("curl", NetworkHead::Performs),
        "curl" => (
            "curl",
            NetworkHead::Connection {
                formals: &["url", "open", "handle"],
                opens_by_default: false,
            },
        ),
        "GET" | "POST" => ("httr", NetworkHead::Performs),
        "req_perform" => ("httr2", NetworkHead::Performs),
        _ => return None,
    })
}

/// Whether this call provably opens its connection (or performs outright).
///
/// The `open` actual is found with R's own argument matching
/// (`match_arguments`: exact names, unambiguous partial names, then
/// position), so `url(description = u, 'r')` binds `'r'` to `open` and
/// `socketConnection(..., o = '')` binds nothing — `o` is an ambiguous
/// prefix of `open`/`options` in current R and the call errors before
/// connecting (verified with Rscript). A call whose actuals do not bind
/// one-to-one (duplicates, unmatched names) errors in R before any
/// connection and stays silent, as does a computed `open`.
fn call_reaches_network(head: NetworkHead, args: &[Arg]) -> bool {
    match head {
        NetworkHead::Performs => true,
        NetworkHead::Connection {
            formals,
            opens_by_default,
        } => {
            let matched = match_arguments(formals, args);
            if !matched.is_one_to_one() {
                return false;
            }
            let Some(open_index) = formals.iter().position(|formal| *formal == "open") else {
                return false;
            };
            match matched.arg_for_param(open_index) {
                // A literal mode settles it; a computed one could be
                // either way and stays silent.
                Some(arg_index) => match &args[arg_index].value {
                    Expr::String(mode, _) => !mode.is_empty(),
                    _ => false,
                },
                // No `open` actual: the formal's default applies.
                None => opens_by_default,
            }
        }
    }
}

/// Path trees whose code is not package-load execution. `data-raw` is
/// deliberately absent: those scripts run as written, like top level.
const NON_LOAD_TREES: [&str; 5] = ["tests", "demo", "vignettes", "inst", "examples"];

/// Whether a file runs its top level at package load: a package `R/`
/// file (`source_package` is set only there), a `data-raw` script, or a
/// file of unknown provenance (a bare single-file check), which ry
/// treats as load-executed like its other syntactic rules. Known
/// non-load trees stay silent. Computed once per pass (see
/// `Checker::network_load_context_file`), not per call.
pub(crate) fn load_context_file(path: &str, is_package_r: bool) -> bool {
    if is_package_r {
        return true;
    }
    let is_component = |needle: &str| {
        std::path::Path::new(path)
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .any(|component| component == needle)
    };
    // A non-load tree wins over a nested `data-raw` component
    // (`tests/data-raw/` is test code, not a load-time script).
    if NON_LOAD_TREES.iter().any(|tree| is_component(tree)) {
        return false;
    }
    // `data-raw` scripts and files of unknown provenance run as written.
    true
}

impl Checker {
    fn in_load_context_file(&self) -> bool {
        self.network_load_context_file
    }

    /// Whether the walk is currently evaluating load-time code: the
    /// file's top level, or directly inside a `.onLoad`/`.onAttach`
    /// body (not inside a closure nested in one).
    fn at_load_time(&self) -> bool {
        self.lazy_context_depth == 0
            && (self.enclosing_formals.is_empty()
                || self.load_hook_frames.last() == Some(&self.enclosing_formals.len()))
    }

    fn in_guarded_span(&self, span: Span) -> bool {
        self.network_guarded_spans
            .iter()
            .any(|guarded| span.start >= guarded.start && span.end <= guarded.end)
    }

    /// Whether a bare head may use `package` provenance without an
    /// explicit import or attachment. Base-database packages are on
    /// every search path; other packages must be named by the call or
    /// a NAMESPACE `importFrom`, or be attached on this file's search
    /// path. `bare_loaded` is the per-file attachment set the CLI
    /// installs (`Project::set_bare_loaded` is keyed by path); direct
    /// `Project` users without precise per-file paths fall back to
    /// the declared union, the deliberate coarser semantics. The
    /// project-wide `loaded` union does not prove provenance at load
    /// time: another file's `library()` call has not necessarily run
    /// while this file's namespace is built.
    fn bare_package_in_scope(&self, lookup_name: &str, package: &str) -> bool {
        crate::resolve::BASE_DATABASE_PACKAGES.contains(&package)
            || self
                .imported_from
                .get(lookup_name)
                .is_some_and(|imported| imported == package)
            || self.bare_loaded.contains(package)
    }

    /// RY124: a proven network call evaluated while package code loads.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_top_level_network_call(
        &mut self,
        original_name: &str,
        func: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        head: &CallHead,
        args: &[Arg],
        scope: &Scope,
        span: Span,
    ) {
        if self.discarding || !self.at_load_time() || !self.in_load_context_file() {
            return;
        }
        if self.in_guarded_span(span) {
            return;
        }
        // A project wrapper the existing resolution proves this call
        // reaches: its own body is ordinary function code, so the
        // load-time fault is at this call site.
        if let Some(function) = head.user_function()
            && head.bound_definition().is_some()
            && function.source_path == self.path
            && self.wrapper_performs_network_call(function)
        {
            self.emit(
                Severity::Warning,
                span,
                "RY124",
                format!(
                    "`{lookup_name}()` performs a network transfer and is evaluated \
                     while package code loads; move the download behind an explicit \
                     opt-in or into a function the user calls"
                ),
            );
            return;
        }
        let Some((package, network)) = network_head(lookup_name) else {
            return;
        };
        if !call_reaches_network(network, args) {
            return;
        }
        // Qualified heads and base-database heads use the shared
        // provenance proof, which is built around search-path masking
        // of base. A bare head from another package is instead proven
        // by the import or attachment that puts it in scope at all,
        // provided nothing local binds the name first.
        let qualified = Callee::parse(semantic_name).is_qualified();
        let proven = if qualified || crate::resolve::BASE_DATABASE_PACKAGES.contains(&package) {
            matches!(
                self.special_call_provenance(
                    original_name,
                    func,
                    semantic_name,
                    lookup_name,
                    package,
                    scope,
                ),
                crate::resolve::SpecialCallProvenance::Proven
            ) && (qualified || self.bare_package_in_scope(lookup_name, package))
        } else {
            self.bare_package_in_scope(lookup_name, package)
                && !self.call_head_bound_locally(lookup_name, scope)
                && !scope.is_parameter(lookup_name)
        };
        if proven {
            self.emit(
                Severity::Warning,
                span,
                "RY124",
                format!(
                    "`{semantic_name}()` performs a network transfer while package \
                     code loads; move it behind an explicit opt-in (an option, an \
                     environment variable, or an interactive check) or into a \
                     function the user calls"
                ),
            );
        }
    }

    /// Memoized [`body_performs_network_call`], positives only: the
    /// scan reads mutable provenance state (`bare_loaded` grows when
    /// a top-level `library()` runs), so a negative result can flip
    /// later in the same pass and must be recomputed, while a
    /// positive can only gain provenance support. The cache is
    /// cleared with the guarded spans it depends on (see `lib.rs`
    /// pass-3 setup).
    fn wrapper_performs_network_call(&mut self, function: &UserFn) -> bool {
        if self
            .network_wrapper_cache
            .contains_key(&function.definition_span)
        {
            return true;
        }
        let result = self.body_performs_network_call(function);
        if result {
            self.network_wrapper_cache
                .insert(function.definition_span, true);
        }
        result
    }

    /// Whether a project function's body directly calls a recognized
    /// network head outside any nested closure and outside a guarded
    /// span. Bare heads masked by a formal, by a name the body itself
    /// binds (a locally defined function hides under a nested table
    /// name such as `fetch$url`, invisible to `functions.binds`), or
    /// by a project definition of the same name do not count. Calls
    /// R does not evaluate when the wrapper runs — inside `quote()` /
    /// `delayedAssign()` / `substitute()` / `expression()`, and in
    /// the dead arm of a literal `if`/`while` — do not count either.
    fn body_performs_network_call(&self, function: &UserFn) -> bool {
        let local_names = assigned_names_in_body(&function.body);
        let dead_spans = dead_arm_spans(&function.body);
        let in_dead_span = |span: Span| {
            dead_spans
                .iter()
                .any(|dead| span.start >= dead.start && span.end <= dead.end)
        };
        let mut policy = Walk::ALL;
        policy.fn_bodies = false;
        let mut found = false;
        let _ = walk_stmts(&function.body, policy, |node, _| {
            let AstNode::Expr(Expr::Call { func, args, span }) = node else {
                return ControlFlow::Continue(Descend::Into);
            };
            let Expr::Ident { name, .. } = func.as_ref() else {
                return ControlFlow::Continue(Descend::Into);
            };
            let member = crate::semantic_lists::bare_name(semantic_argument_name(name));
            // Deferred execution: the arguments of these heads are
            // captured as language/promises, not evaluated by the call.
            let deferred = matches!(
                member,
                "quote" | "delayedAssign" | "substitute" | "expression"
            ) && match Callee::parse(name) {
                Callee::Bare(_) => true,
                Callee::Qualified { package, .. } => package.trim_end_matches(':') == "base",
            };
            if deferred {
                return ControlFlow::Continue(Descend::Skip);
            }
            let callee = Callee::parse(name);
            if let Some((package, network)) = network_head(member)
                && call_reaches_network(network, args)
                && !self.in_guarded_span(*span)
                && !in_dead_span(*span)
                && match callee {
                    Callee::Qualified { package: named, .. } => {
                        named.trim_end_matches(':') == package
                    }
                    Callee::Bare(_) => {
                        !function
                            .params
                            .iter()
                            .any(|parameter| parameter.name == member)
                            && !local_names
                                .iter()
                                .any(|local| semantic_argument_name(local) == member)
                            && !self.fn_table.functions.binds(member)
                            && self.bare_package_in_scope(member, package)
                    }
                }
            {
                found = true;
                return ControlFlow::Break(());
            }
            ControlFlow::Continue(Descend::Into)
        });
        found
    }
}

/// The member name of a call head that may be read as a base call:
/// a bare head or an explicit `base::` head.
fn base_call_member(func: &Expr) -> Option<&str> {
    let Expr::Ident { name, .. } = func else {
        return None;
    };
    match Callee::parse(name) {
        Callee::Bare(_) => {}
        Callee::Qualified { package, .. } if package.trim_end_matches(':') == "base" => {}
        Callee::Qualified { .. } => return None,
    }
    Some(crate::semantic_lists::bare_name(semantic_argument_name(
        name,
    )))
}

/// If `expr` is a direct opt-in probe call (`interactive()`,
/// `getOption()`, `Sys.getenv()`, bare or `base::`), its name. A
/// bare head masked by a local or file-level binding of the same
/// name (`masked`) is not base's probe and proves nothing.
fn probe_call_name(expr: &Expr, masked: &dyn Fn(&str, Span) -> bool) -> Option<&'static str> {
    let Expr::Call { func, span, .. } = expr else {
        return None;
    };
    let member = base_call_member(func)?;
    let probe = match member {
        "interactive" => "interactive",
        "getOption" => "getOption",
        "Sys.getenv" => "Sys.getenv",
        _ => return None,
    };
    let Expr::Ident { name, .. } = func.as_ref() else {
        return None;
    };
    if matches!(Callee::parse(name), Callee::Bare(_)) && masked(probe, *span) {
        return None;
    }
    Some(probe)
}

/// Whether an equality between a probe call and a literal, holding
/// exactly when `eq_holds`, establishes the opt-in. Only literals whose
/// comparison pins the probe's positive value count: `TRUE`, a
/// non-empty string for the character probes, `""` under inequality,
/// and — only for `interactive()`, whose result is a logical scalar —
/// `FALSE` under inequality. `identical(interactive(), FALSE)` under
/// equality proves the opposite of an opt-in and never counts.
fn equality_established(
    lhs: &Expr,
    rhs: &Expr,
    eq_holds: bool,
    masked: &dyn Fn(&str, Span) -> bool,
) -> bool {
    let (probe, literal) = if let Some(probe) = probe_call_name(lhs, masked) {
        (probe, rhs)
    } else if let Some(probe) = probe_call_name(rhs, masked) {
        (probe, lhs)
    } else {
        return false;
    };
    match literal {
        Expr::Logical(true, _) => eq_holds,
        Expr::Logical(false, _) => !eq_holds && probe == "interactive",
        Expr::String(value, _) if matches!(probe, "Sys.getenv" | "getOption") => {
            if value.is_empty() {
                !eq_holds
            } else {
                eq_holds
            }
        }
        _ => false,
    }
}

/// Whether `expr` evaluating to `when_true` proves an opt-in probe
/// held. This is structural, not a mention scan: only shapes whose
/// outcome pins the probe's positive value count (the probe itself,
/// `isTRUE(probe)`, `nzchar(Sys.getenv(...))`, and literal equalities),
/// so a probe buried in an unevaluated nested function or under an
/// unrelated predicate establishes nothing. A probe read through a
/// variable (`enabled <- getOption(...); if (enabled)`) is likewise
/// not recognized; that stays a known remainder rather than a guess.
fn probe_established(expr: &Expr, when_true: bool, masked: &dyn Fn(&str, Span) -> bool) -> bool {
    match expr {
        Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } => probe_established(expr, !when_true, masked),
        Expr::BinOp {
            op: BinOpKind::Eq,
            lhs,
            rhs,
            ..
        } => equality_established(lhs, rhs, when_true, masked),
        Expr::BinOp {
            op: BinOpKind::Ne,
            lhs,
            rhs,
            ..
        } => equality_established(lhs, rhs, !when_true, masked),
        Expr::Call { func, args, .. } => {
            if probe_call_name(expr, masked).is_some() {
                return when_true;
            }
            // A bare wrapper head masked by a local binding is not
            // base's predicate and establishes nothing.
            let head_masked = matches!(func.as_ref(), Expr::Ident { name, .. }
                if matches!(Callee::parse(name), Callee::Bare(_)))
                && base_call_member(func).is_some_and(|member| masked(member, span_of(func)));
            if head_masked {
                return false;
            }
            match base_call_member(func) {
                Some("isTRUE") => {
                    when_true
                        && args
                            .first()
                            .is_some_and(|arg| probe_call_name(&arg.value, masked).is_some())
                }
                Some("nzchar") => {
                    when_true
                        && args.first().is_some_and(|arg| {
                            probe_call_name(&arg.value, masked) == Some("Sys.getenv")
                        })
                }
                Some("identical") => {
                    args.len() >= 2
                        && equality_established(&args[0].value, &args[1].value, when_true, masked)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// The `TRUE`/`FALSE` value of a literal logical, through `!`.
pub(crate) fn literal_logical(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Logical(value, _) => Some(*value),
        Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } => literal_logical(expr).map(|value| !value),
        _ => None,
    }
}

/// Source ranges a literal condition proves never execute: the taken
/// arm of `if (FALSE)`, the else arm of `if (TRUE)`, and the body of
/// `while (FALSE)`, in this frame (nested closures are not entered).
fn dead_arm_spans(body: &[Stmt]) -> Vec<Span> {
    let mut dead = Vec::new();
    let mut policy = Walk::ALL;
    policy.fn_bodies = false;
    let _: ControlFlow<()> = walk_stmts(body, policy, |node, _| {
        match node {
            AstNode::Stmt(Stmt::If {
                cond, then, else_, ..
            }) => match literal_logical(cond) {
                Some(false) => dead.extend(then.iter().map(vacuous::stmt_span)),
                Some(true) => {
                    if let Some(else_stmts) = else_ {
                        dead.extend(else_stmts.iter().map(vacuous::stmt_span));
                    }
                }
                None => {}
            },
            AstNode::Stmt(Stmt::While { cond, body, .. })
                if literal_logical(cond) == Some(false) =>
            {
                dead.extend(body.iter().map(vacuous::stmt_span));
            }
            AstNode::Expr(Expr::If {
                cond, then, else_, ..
            }) => match literal_logical(cond) {
                Some(false) => dead.push(span_of(then)),
                Some(true) => {
                    if let Some(else_expr) = else_ {
                        dead.push(span_of(else_expr));
                    }
                }
                None => {}
            },
            _ => {}
        }
        ControlFlow::Continue(Descend::Into)
    });
    dead
}

/// Whether `cond` evaluating to `when_true` proves an opt-in probe
/// held, so the arm taken on that outcome is guarded. `&&`/`&` prove
/// their conjuncts when true; `||`/`|` prove theirs when false.
fn condition_implies_guard(
    cond: &Expr,
    when_true: bool,
    masked: &dyn Fn(&str, Span) -> bool,
) -> bool {
    match cond {
        Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } => condition_implies_guard(expr, !when_true, masked),
        Expr::BinOp {
            op: BinOpKind::AndAnd | BinOpKind::And,
            lhs,
            rhs,
            ..
        } => {
            // True: every conjunct held, so one establishing conjunct
            // suffices. False: only some conjunct failed, so each
            // conjunct must establish the probe by failing.
            if when_true {
                condition_implies_guard(lhs, true, masked)
                    || condition_implies_guard(rhs, true, masked)
            } else {
                condition_implies_guard(lhs, false, masked)
                    && condition_implies_guard(rhs, false, masked)
            }
        }
        Expr::BinOp {
            op: BinOpKind::OrOr | BinOpKind::Or,
            lhs,
            rhs,
            ..
        } => {
            // False: every disjunct failed, so one establishing
            // disjunct suffices. True: only some disjunct held, so
            // each disjunct must establish the probe by holding —
            // a non-probe disjunct may be the true one.
            if when_true {
                condition_implies_guard(lhs, true, masked)
                    && condition_implies_guard(rhs, true, masked)
            } else {
                condition_implies_guard(lhs, false, masked)
                    || condition_implies_guard(rhs, false, masked)
            }
        }
        _ => probe_established(cond, when_true, masked),
    }
}

/// Whether a call head leaves its function or errors: a qualified
/// `base::stop`/`base::return`, a bare `return`, or a bare `stop` —
/// but a bare `stop` only where no local binding shadows it
/// (`bare_stop_is_base`), since a locally defined `stop` returns
/// normally and execution continues to the guarded continuation.
fn call_diverges(func: &Expr, bare_stop_is_base: bool) -> bool {
    let Expr::Ident { name, .. } = func else {
        return false;
    };
    let member = crate::semantic_lists::bare_name(semantic_argument_name(name));
    match Callee::parse(name) {
        Callee::Qualified { package, .. } => {
            package.trim_end_matches(':') == "base" && matches!(member, "stop" | "return")
        }
        Callee::Bare(_) => member == "return" || (member == "stop" && bare_stop_is_base),
    }
}

/// Whether a statement list leaves its function or errors on every
/// path ry can see from its direct statements (`return`, `stop`).
/// A braced block used as a statement diverges as its statements do.
fn arm_diverges(stmts: &[Stmt], bare_stop_is_base: bool) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::Return { .. } => true,
        Stmt::Expr(Expr::Block { body, .. }) => arm_diverges(body, bare_stop_is_base),
        Stmt::Expr(Expr::Call { func, .. }) => call_diverges(func, bare_stop_is_base),
        _ => false,
    })
}

/// Index the ranges an opt-in condition guards: the taken arm of an
/// `if` whose outcome proves the probe held, the right operand of a
/// short-circuited `&&`/`||`, and the continuation after a diverging
/// `if (!probe) return()` guard. Polarity matters: the arm that runs
/// when the probe failed is exactly the unguarded one.
pub(crate) fn index_guarded_spans(
    stmts: &[Stmt],
    continuations: &std::collections::HashMap<Span, Span>,
    guarded: &mut Vec<Span>,
) {
    // Where a bare head is not base's: a file-level binding shadows
    // it everywhere (load hooks run after the file's top level), and
    // a function that binds the name — as a formal or by assignment
    // in its own frame — shadows it within its span. Applies to the
    // diverging `stop` and to the opt-in probes alike: a locally
    // defined `interactive()` is not an opt-in check.
    let shadowed_heads = crate::semantic_lists::GUARD_SHADOWED_HEADS;
    let binds = |names: &std::collections::HashSet<String>, wanted: &str| {
        names
            .iter()
            .any(|name| semantic_argument_name(name) == wanted)
    };
    // File-level shadowing respects source order for top-level code:
    // a binding masks only probes at or after its assignment, plus
    // probes inside any function body (which may run after the whole
    // top level). Assignments are collected outside function bodies.
    let mut file_assign_spans: Vec<(Span, &'static str)> = Vec::new();
    let no_fn_bodies = Walk {
        fn_bodies: false,
        ..Walk::ALL
    };
    let _: ControlFlow<()> = walk_stmts(stmts, no_fn_bodies, |node, _| {
        let (target, span) = match node {
            AstNode::Stmt(Stmt::Assign { target, span, .. }) => (target, span),
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::Assign | BinOpKind::SuperAssign,
                lhs,
                span,
                ..
            }) => (lhs.as_ref(), span),
            _ => return ControlFlow::Continue(Descend::Into),
        };
        if let Expr::Ident { name, .. } = target {
            let bound = semantic_argument_name(name);
            if let Some(wanted) = shadowed_heads.iter().copied().find(|w| *w == bound) {
                file_assign_spans.push((*span, wanted));
            }
        }
        ControlFlow::Continue(Descend::Into)
    });
    let mut shadow_spans: Vec<(Span, &'static str)> = Vec::new();
    let mut fn_spans: Vec<Span> = Vec::new();
    let _: ControlFlow<()> = walk_stmts(stmts, Walk::ALL, |node, _| {
        let (params, body, span) = match node {
            AstNode::Stmt(Stmt::FunctionDef { params, body, span }) => (params, body, span),
            AstNode::Expr(Expr::Function { params, body, span }) => (params, body, span),
            _ => return ControlFlow::Continue(Descend::Into),
        };
        fn_spans.push(*span);
        let body_assigned = assigned_names_in_body(body);
        for wanted in shadowed_heads.iter().copied() {
            if params
                .iter()
                .any(|param| semantic_argument_name(&param.name) == wanted)
                || binds(&body_assigned, wanted)
            {
                shadow_spans.push((*span, wanted));
            }
        }
        ControlFlow::Continue(Descend::Into)
    });
    let masked = |wanted: &str, span: Span| {
        let in_fn = fn_spans
            .iter()
            .any(|f| span.start >= f.start && span.end <= f.end);
        file_assign_spans
            .iter()
            .any(|(assign, name)| *name == wanted && (in_fn || span.start >= assign.start))
            || shadow_spans.iter().any(|(shadow, name)| {
                *name == wanted && span.start >= shadow.start && span.end <= shadow.end
            })
    };
    let bare_stop_is_base = |span: Span| !masked("stop", span);
    let _: ControlFlow<()> = walk_stmts(stmts, Walk::ALL, |node, _| {
        match node {
            AstNode::Stmt(Stmt::If {
                cond,
                then,
                else_,
                span,
            }) => {
                if condition_implies_guard(cond, true, &masked) {
                    guarded.extend(then.iter().map(vacuous::stmt_span));
                }
                if condition_implies_guard(cond, false, &masked) {
                    if let Some(else_stmts) = else_ {
                        guarded.extend(else_stmts.iter().map(vacuous::stmt_span));
                    }
                    // `if (!probe) return()` guards whatever follows it
                    // in the same statement list.
                    if arm_diverges(then, bare_stop_is_base(*span))
                        && let Some(continuation) = continuations.get(span)
                    {
                        guarded.push(*continuation);
                    }
                }
            }
            AstNode::Expr(Expr::If {
                cond, then, else_, ..
            }) => {
                if condition_implies_guard(cond, true, &masked) {
                    guarded.push(span_of(then));
                }
                if condition_implies_guard(cond, false, &masked)
                    && let Some(else_expr) = else_
                {
                    guarded.push(span_of(else_expr));
                }
            }
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::AndAnd,
                lhs,
                rhs,
                ..
            }) if condition_implies_guard(lhs, true, &masked) => {
                guarded.push(span_of(rhs));
            }
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::OrOr,
                lhs,
                rhs,
                ..
            }) if condition_implies_guard(lhs, false, &masked) => {
                guarded.push(span_of(rhs));
            }
            _ => {}
        }
        ControlFlow::Continue(Descend::Into)
    });
}
