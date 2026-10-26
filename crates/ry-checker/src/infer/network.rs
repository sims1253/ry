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

use super::callee::CallHead;
use super::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_expr, walk_stmts};
use std::ops::ControlFlow;

/// How a recognized head reaches the network.
#[derive(Clone, Copy)]
enum NetworkHead {
    /// The call itself performs the request.
    Performs,
    /// A connection constructor: it connects only once opened. `open_index`
    /// is the positional index of the `open` formal, and `opens_by_default`
    /// records the formal's default (`"a+"` for `socketConnection`).
    Connection {
        open_index: usize,
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
                open_index: 1,
                opens_by_default: false,
            },
        ),
        "socketConnection" => (
            "base",
            NetworkHead::Connection {
                open_index: 4,
                opens_by_default: true,
            },
        ),
        "curl_download" | "curl_fetch_memory" => ("curl", NetworkHead::Performs),
        "curl" => (
            "curl",
            NetworkHead::Connection {
                open_index: 1,
                opens_by_default: false,
            },
        ),
        "GET" | "POST" => ("httr", NetworkHead::Performs),
        "req_perform" => ("httr2", NetworkHead::Performs),
        _ => return None,
    })
}

/// The literal `open` mode of a connection call: `Some(true)` opens,
/// `Some(false)` stays closed, `None` is not statically known.
fn literal_open_opens(args: &[Arg], open_index: usize) -> Option<bool> {
    let mut positional = 0usize;
    let mut open_arg = None;
    for arg in args {
        match arg.name.as_deref() {
            Some("open") => open_arg = Some(arg),
            Some(_) => {}
            None => {
                if positional == open_index {
                    open_arg = Some(arg);
                }
                positional += 1;
            }
        }
    }
    match open_arg?.value {
        Expr::String(ref mode, _) => Some(!mode.is_empty()),
        _ => None,
    }
}

/// Whether this call provably opens its connection (or performs outright).
fn call_reaches_network(head: NetworkHead, args: &[Arg]) -> bool {
    match head {
        NetworkHead::Performs => true,
        NetworkHead::Connection {
            open_index,
            opens_by_default,
        } => match literal_open_opens(args, open_index) {
            Some(opens) => opens,
            // No literal `open`: an absent argument takes the formal's
            // default, but a computed argument could be either way.
            None => {
                opens_by_default
                    && !args.iter().any(|arg| arg.name.as_deref() == Some("open"))
                    && args.iter().filter(|arg| arg.name.is_none()).count() <= open_index
            }
        },
    }
}

/// Path trees whose code is not package-load execution. `data-raw` is
/// deliberately absent: those scripts run as written, like top level.
const NON_LOAD_TREES: [&str; 5] = ["tests", "demo", "vignettes", "inst", "examples"];

impl Checker {
    /// Whether the file being emitted runs its top level at package
    /// load: a package `R/` file (`source_package` is set only there),
    /// a `data-raw` script, or a file of unknown provenance (a bare
    /// single-file check), which ry treats as load-executed like its
    /// other syntactic rules. Known non-load trees stay silent.
    fn in_load_context_file(&self) -> bool {
        if self.package_facts.source_package.is_some() {
            return true;
        }
        let components: Vec<&str> = std::path::Path::new(&self.path)
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .collect();
        if components.contains(&"data-raw") {
            return true;
        }
        !components
            .iter()
            .any(|component| NON_LOAD_TREES.contains(component))
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
    /// every search path; other packages must be named by the call,
    /// a NAMESPACE `importFrom`, or an attachment ry knows about.
    fn bare_package_in_scope(&self, lookup_name: &str, package: &str) -> bool {
        crate::resolve::BASE_DATABASE_PACKAGES.contains(&package)
            || self
                .imported_from
                .get(lookup_name)
                .is_some_and(|imported| imported == package)
            || self.bare_loaded.contains(package)
            || self.loaded.contains(package)
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
            && self.body_performs_network_call(function)
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
        let proven = if Callee::parse(semantic_name).is_qualified()
            || crate::resolve::BASE_DATABASE_PACKAGES.contains(&package)
        {
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
            ) && (Callee::parse(semantic_name).is_qualified()
                || self.bare_package_in_scope(lookup_name, package))
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

    /// Whether a project function's body directly calls a recognized
    /// network head outside any nested closure and outside a guarded
    /// span. Bare heads masked by a formal or by a project definition
    /// of the same name do not count.
    fn body_performs_network_call(&self, function: &UserFn) -> bool {
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
            let callee = Callee::parse(name);
            let member = crate::semantic_lists::bare_name(semantic_argument_name(name));
            if let Some((package, network)) = network_head(member)
                && call_reaches_network(network, args)
                && !self.in_guarded_span(*span)
                && match callee {
                    Callee::Qualified { package: named, .. } => {
                        named.trim_end_matches(':') == package
                    }
                    Callee::Bare(_) => {
                        !function
                            .params
                            .iter()
                            .any(|parameter| parameter.name == member)
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

/// Whether an expression mentions an opt-in probe (`interactive()`,
/// `getOption()`, `Sys.getenv()`) anywhere inside it, under a bare or
/// `base::` head. A probe guarded only through a variable
/// (`enabled <- getOption(...); if (enabled)`) is not recognized; that
/// stays a known remainder rather than a guess.
fn mentions_guard_probe(expr: &Expr) -> bool {
    let mut found = false;
    let _ = walk_expr(expr, Walk::ALL, |node, _| {
        if let AstNode::Expr(Expr::Call { func, .. }) = node
            && let Expr::Ident { name, .. } = func.as_ref()
        {
            let member = crate::semantic_lists::bare_name(semantic_argument_name(name));
            let qualified_ok = match Callee::parse(name) {
                Callee::Bare(_) => true,
                Callee::Qualified { package, .. } => package.trim_end_matches(':') == "base",
            };
            if qualified_ok && matches!(member, "interactive" | "getOption" | "Sys.getenv") {
                found = true;
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(Descend::Into)
    });
    found
}

/// Whether `cond` evaluating to `when_true` proves an opt-in probe
/// held, so the arm taken on that outcome is guarded. `&&`/`&` prove
/// their conjuncts when true; `||`/`|` prove theirs when false.
fn condition_implies_guard(cond: &Expr, when_true: bool) -> bool {
    match cond {
        Expr::UnaryOp {
            op: UnaryOpKind::Not,
            expr,
            ..
        } => condition_implies_guard(expr, !when_true),
        Expr::BinOp {
            op: BinOpKind::AndAnd | BinOpKind::And,
            lhs,
            rhs,
            ..
        } => {
            when_true && (condition_implies_guard(lhs, true) || condition_implies_guard(rhs, true))
        }
        Expr::BinOp {
            op: BinOpKind::OrOr | BinOpKind::Or,
            lhs,
            rhs,
            ..
        } => {
            !when_true
                && (condition_implies_guard(lhs, false) || condition_implies_guard(rhs, false))
        }
        _ => when_true && mentions_guard_probe(cond),
    }
}

/// Whether a statement list leaves its function or errors on every
/// path ry can see from its direct statements (`return`, `stop`).
fn arm_diverges(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::Return { .. } => true,
        Stmt::Expr(Expr::Call { func, .. }) => matches!(
            func.as_ref(),
            Expr::Ident { name, .. }
                if matches!(
                    crate::semantic_lists::bare_name(semantic_argument_name(name)),
                    "return" | "stop"
                )
        ),
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
    let _: ControlFlow<()> = walk_stmts(stmts, Walk::ALL, |node, _| {
        match node {
            AstNode::Stmt(Stmt::If {
                cond,
                then,
                else_,
                span,
            }) => {
                if condition_implies_guard(cond, true) {
                    guarded.extend(then.iter().map(vacuous::stmt_span));
                }
                if condition_implies_guard(cond, false) {
                    if let Some(else_stmts) = else_ {
                        guarded.extend(else_stmts.iter().map(vacuous::stmt_span));
                    }
                    // `if (!probe) return()` guards whatever follows it
                    // in the same statement list.
                    if arm_diverges(then)
                        && let Some(continuation) = continuations.get(span)
                    {
                        guarded.push(*continuation);
                    }
                }
            }
            AstNode::Expr(Expr::If {
                cond, then, else_, ..
            }) => {
                if condition_implies_guard(cond, true) {
                    guarded.push(span_of(then));
                }
                if condition_implies_guard(cond, false)
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
            }) if condition_implies_guard(lhs, true) => {
                guarded.push(span_of(rhs));
            }
            AstNode::Expr(Expr::BinOp {
                op: BinOpKind::OrOr,
                lhs,
                rhs,
                ..
            }) if condition_implies_guard(lhs, false) => {
                guarded.push(span_of(rhs));
            }
            _ => {}
        }
        ControlFlow::Continue(Descend::Into)
    });
}
