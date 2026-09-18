//! R argument matching, evaluation modes, and call diagnostics.

use super::*;

/// The callee resolution the argument rules share: a collected user
/// definition shadows the typeshed stub, and a lexical callable
/// resolves to neither (its formals are unknown at the call site). In
/// the non-validating mode a same-named `FnTable` entry shadows the
/// stub as well, mirroring the precedence `check_call_arguments` has
/// always applied -- one copy, so RY090/RY091/RY092 and RY111's
/// identical-name gate cannot drift apart.
pub(crate) enum ResolvedCallee<'a> {
    User(&'a UserFn),
    Stub(&'a FunctionSig),
    Unknown,
}

impl Checker {
    pub(crate) fn resolve_callee<'a>(
        &self,
        function_name: &str,
        user_function: Option<&'a UserFn>,
        resolved_sig: Option<&'a FunctionSig>,
        lexical_callable: bool,
    ) -> ResolvedCallee<'a> {
        if let Some(user_function) = user_function {
            ResolvedCallee::User(user_function)
        } else if !lexical_callable
            && (self.validate_user_call_arguments || !self.fn_table.fns.contains_key(function_name))
            && let Some(signature) = resolved_sig
        {
            ResolvedCallee::Stub(signature)
        } else {
            ResolvedCallee::Unknown
        }
    }
}

/// RY111's dead-formal index: for every function the file defines, the
/// set of that function's own formals its body reads. Keyed by the
/// function's span and computed once per pass-3 run in the
/// `emit_diagnostics` prologue, the same shape RY110's
/// continuation/shadow indexes use. `EnclosingFormals::function_span`
/// pushes exactly these spans for named assignments
/// (`g <- function(...)`, `infer/mod.rs`'s assign arm) and statement
/// definitions; an expression-position literal's entry exists too, but
/// nothing inside such a literal emits today (its body is only inferred
/// in discarding mode), so the entry simply waits for the day it is
/// walked. A missing entry means no read was found: the dead-formal
/// gate treats that as dead, the firing direction.
///
/// The scan is a hand-rolled recursion rather than the shared
/// `ry_core` walker (like RY109's force analysis) because its rules
/// select individual children and carry scope: each identifier
/// attributes to the *innermost* frame whose formals contain the name,
/// so a nested closure redeclaring `na.rm` shadows the outer formal
/// for its subtree exactly like R's scoping; a plain-identifier
/// assignment target is a *write* of the promise, not a read of the
/// caller's value; and formal default expressions (`force = na.rm`)
/// evaluate in the function's own scope and count as reads of the
/// sibling formals they name.
pub(crate) fn index_formal_reads(stmts: &[Stmt], map: &mut FxMap<Span, HashSet<String>>) {
    let mut frames: Vec<ReadFrame> = Vec::new();
    scan_stmts(stmts, &mut frames, map);
}

/// One function on the scan stack: its span (the entry key), its own
/// formal names, and the reads attributed to it so far.
struct ReadFrame {
    span: Span,
    formals: HashSet<String>,
    reads: HashSet<String>,
}

/// Enter one function: push its frame, scan its defaults and body, and
/// record the entry. Nested functions encountered on the way push their
/// own frames, so the whole file is indexed in a single pass.
fn scan_function(
    params: &[Param],
    body: &[Stmt],
    span: Span,
    frames: &mut Vec<ReadFrame>,
    map: &mut FxMap<Span, HashSet<String>>,
) {
    frames.push(ReadFrame {
        span,
        formals: params
            .iter()
            .filter(|parameter| parameter.name != "...")
            .map(|parameter| semantic_argument_name(&parameter.name).to_owned())
            .collect(),
        reads: HashSet::new(),
    });
    // Formal defaults evaluate in the function's own scope: a default
    // naming a sibling formal (`force = na.rm`) consumes the caller's
    // value and is a read.
    for parameter in params {
        if let Some(default) = &parameter.default {
            scan_expr(default, frames, map);
        }
    }
    scan_stmts(body, frames, map);
    let frame = frames.pop().expect("scan_function pushed a frame");
    map.insert(frame.span, frame.reads);
}

fn scan_stmts(stmts: &[Stmt], frames: &mut Vec<ReadFrame>, map: &mut FxMap<Span, HashSet<String>>) {
    for statement in stmts {
        scan_stmt(statement, frames, map);
    }
}

fn scan_stmt(stmt: &Stmt, frames: &mut Vec<ReadFrame>, map: &mut FxMap<Span, HashSet<String>>) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            scan_target(target, frames, map);
            scan_expr(value, frames, map);
        }
        Stmt::Expr(expression) => scan_expr(expression, frames, map),
        Stmt::If {
            cond, then, else_, ..
        } => {
            scan_expr(cond, frames, map);
            scan_stmts(then, frames, map);
            if let Some(else_) = else_ {
                scan_stmts(else_, frames, map);
            }
        }
        Stmt::For { iter, body, .. } => {
            // The loop variable is a write; the iterated value is a read.
            scan_expr(iter, frames, map);
            scan_stmts(body, frames, map);
        }
        Stmt::While { cond, body, .. } => {
            scan_expr(cond, frames, map);
            scan_stmts(body, frames, map);
        }
        Stmt::FunctionDef { params, body, span } => {
            scan_function(params, body, *span, frames, map);
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                scan_expr(value, frames, map);
            }
        }
    }
}

/// An assignment target: a plain identifier root is a *write* of the
/// promise (the caller's value is replaced without being read), so it
/// is not a read. Complex targets keep their evaluated parts: `x[p] <- v`
/// reads `p`, and the root identifier of each index level stays a write.
fn scan_target(expr: &Expr, frames: &mut Vec<ReadFrame>, map: &mut FxMap<Span, HashSet<String>>) {
    match expr {
        Expr::Ident { .. } => {}
        Expr::Index { base, args, .. } => {
            scan_target(base, frames, map);
            for argument in args {
                scan_expr(&argument.value, frames, map);
            }
        }
        other => scan_expr(other, frames, map),
    }
}

fn scan_expr(expr: &Expr, frames: &mut Vec<ReadFrame>, map: &mut FxMap<Span, HashSet<String>>) {
    match expr {
        Expr::Function {
            params, body, span, ..
        } => scan_function(params, body, *span, frames, map),
        Expr::Block { body, .. } => scan_stmts(body, frames, map),
        Expr::Call { func, args, .. } => {
            scan_expr(func, frames, map);
            // Argument tags are names, not identifier reads; the values
            // are (`missing(p)` arrives as an identifier argument).
            for argument in args {
                scan_expr(&argument.value, frames, map);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            scan_expr(lhs, frames, map);
            scan_expr(rhs, frames, map);
        }
        Expr::UnaryOp { expr, .. } => scan_expr(expr, frames, map),
        Expr::Index { base, args, .. } => {
            scan_expr(base, frames, map);
            for argument in args {
                scan_expr(&argument.value, frames, map);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            scan_expr(cond, frames, map);
            scan_expr(then, frames, map);
            if let Some(else_) = else_ {
                scan_expr(else_, frames, map);
            }
        }
        // Attribute to the innermost frame owning the name: a nested
        // function's same-named formal shadows the outer one for its
        // subtree, and a captured read (no inner formal) still handles
        // the owning function's caller value -- the conservative,
        // quiet direction.
        Expr::Ident { name, .. } => {
            let tag = semantic_argument_name(name);
            if let Some(frame) = frames
                .iter_mut()
                .rev()
                .find(|frame| frame.formals.contains(tag))
            {
                frame.reads.insert(tag.to_owned());
            }
        }
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_)
        | Expr::Missing(_) => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArgumentMatch {
    /// Formal parameter index for each actual argument. `None` means the
    /// argument was unmatched (or was absorbed by `...`).
    pub(crate) param_for_arg: Vec<Option<usize>>,
    pub(crate) bound_params: Vec<bool>,
    pub(crate) unmatched_named: Vec<usize>,
    pub(crate) dots: Option<usize>,
}

impl ArgumentMatch {
    /// The actual argument bound to formal parameter `formal_index`,
    /// if any: the reverse of `param_for_arg`. Repeated names
    /// (`f(x = 1, x = 2)`, a runtime error in R) bind the first actual.
    pub(crate) fn arg_for_param(&self, formal_index: usize) -> Option<usize> {
        self.param_for_arg
            .iter()
            .position(|bound| *bound == Some(formal_index))
    }
}

/// Match R call arguments in the same three passes as `match.call`: exact
/// names, unambiguous partial names, then unnamed arguments positionally.
/// Partial and positional matching stop at `...`; exact names may still bind
/// formals declared after it.
pub(crate) fn match_arguments(param_names: &[&str], args: &[Arg]) -> ArgumentMatch {
    match_argument_names(
        param_names,
        args.iter().map(|argument| argument.name.as_deref()),
    )
}

pub(crate) fn match_argument_names<'a>(
    param_names: &[&str],
    names: impl Iterator<Item = Option<&'a str>> + Clone,
) -> ArgumentMatch {
    let dots = param_names.iter().position(|name| *name == "...");
    let partial_end = dots.unwrap_or(param_names.len());
    let mut result = ArgumentMatch {
        param_for_arg: vec![None; names.clone().count()],
        bound_params: vec![false; param_names.len()],
        unmatched_named: Vec::new(),
        dots,
    };

    // Pass 1: exact names match every formal, including formals after `...`.
    for (argument_index, name) in names.clone().enumerate() {
        let Some(name) = name else {
            continue;
        };
        if let Some(parameter_index) = param_names
            .iter()
            .position(|parameter| *parameter != "..." && *parameter == name)
        {
            result.param_for_arg[argument_index] = Some(parameter_index);
            result.bound_params[parameter_index] = true;
        }
    }

    // Pass 2: only a unique prefix of a pre-dots formal is a partial match.
    for (argument_index, name) in names.clone().enumerate() {
        if result.param_for_arg[argument_index].is_some() {
            continue;
        }
        let Some(name) = name else {
            continue;
        };
        let mut candidates =
            param_names[..partial_end]
                .iter()
                .enumerate()
                .filter(|(index, parameter)| {
                    !result.bound_params[*index] && parameter.starts_with(name)
                });
        let first = candidates.next().map(|(index, _)| index);
        if let Some(parameter_index) = first
            && candidates.next().is_none()
        {
            result.param_for_arg[argument_index] = Some(parameter_index);
            result.bound_params[parameter_index] = true;
        }
    }

    // Pass 3: unnamed actuals fill the remaining pre-dots formals in order.
    let mut next_parameter = 0;
    for (argument_index, name) in names.clone().enumerate() {
        if name.is_some() {
            if result.param_for_arg[argument_index].is_none() {
                result.unmatched_named.push(argument_index);
            }
            continue;
        }
        while next_parameter < partial_end && result.bound_params[next_parameter] {
            next_parameter += 1;
        }
        if next_parameter < partial_end {
            result.param_for_arg[argument_index] = Some(next_parameter);
            result.bound_params[next_parameter] = true;
            next_parameter += 1;
        }
    }
    result
}

/// The formal-parameter view RY090/RY091 reporting needs, shared by
/// typeshed `ParamSpec`s and collected `UserParam`s so one code path
/// serves stub and user calls.
trait CallFormal {
    fn name(&self) -> &str;
    fn required(&self) -> bool;
}

impl CallFormal for ParamSpec {
    fn name(&self) -> &str {
        &self.name
    }
    fn required(&self) -> bool {
        self.required
    }
}

impl CallFormal for UserParam {
    fn name(&self) -> &str {
        &self.name
    }
    fn required(&self) -> bool {
        self.required
    }
}

/// `match_arguments` over a signature's formal specs: collect the formal
/// names once and run R's three-pass matching. Callers that need the
/// names themselves (message text, eval-mode lookup) keep their own
/// `param_names()` vector.
pub(crate) fn match_params(params: &[ParamSpec], args: &[Arg]) -> ArgumentMatch {
    let names: Vec<&str> = params.iter().map(|param| param.name.as_str()).collect();
    match_arguments(&names, args)
}

/// Return the actual argument bound to a formal under ordinary R matching.
/// Semantic metadata must use this rather than raw call positions.
pub(crate) fn bound_argument_index(
    params: &[ParamSpec],
    args: &[Arg],
    formal: &str,
) -> Option<usize> {
    bound_argument_index_matched(params, &match_params(params, args), formal)
}

/// `bound_argument_index` over an argument match already computed for
/// this call, so a site that consults several formals matches once.
pub(crate) fn bound_argument_index_matched(
    params: &[ParamSpec],
    bindings: &ArgumentMatch,
    formal: &str,
) -> Option<usize> {
    bindings.arg_for_param(params.iter().position(|param| param.name == formal)?)
}

pub(crate) fn match_args_to_params(
    sig_params: &[ParamSpec],
    args: &[Arg],
    arg_types: &[RType],
) -> Vec<RType> {
    let bindings = match_params(sig_params, args);
    let mut matched = vec![RType::unknown(); sig_params.len()];
    for (formal_index, slot) in matched.iter_mut().enumerate() {
        if let Some(argument_type) = bindings
            .arg_for_param(formal_index)
            .and_then(|argument_index| arg_types.get(argument_index))
        {
            *slot = argument_type.clone();
        }
    }
    matched
}

impl Checker {
    pub(crate) fn is_forwarded_dots(&self, argument: &Arg) -> bool {
        let Expr::Ident { name, span } = &argument.value else {
            return false;
        };
        // Lowering removes parentheses. Only the original direct symbol is
        // expanded by R; `f((...))` passes an ordinary expression instead.
        semantic_argument_name(name) == "..."
            && argument.span.end == span.end
            && self.source.get(span.start..span.end) == Some(name.as_str())
    }

    /// Non-firing policy for schema calls:
    /// - RY090 stays silent for `...`, successful exact/partial matches, and
    ///   legacy inference-only signatures without completeness metadata.
    /// - RY091 stays silent for every non-required or successfully bound
    ///   parameter.
    /// - RY092 stays silent without a declared type, for opaque/unknown
    ///   actuals, whenever a union has any compatible overlap, for R's
    ///   logical/integer/double coercion family, and for `demand_only`
    ///   parameters (relational demands such as `vec_cast(x, to)`, whose
    ///   type arms RY110 without asserting the argument's own type).
    pub(crate) fn check_typeshed_call_arguments(
        &mut self,
        function_name: &str,
        signature: &FunctionSig,
        args: &[Arg],
        arg_types: &[RType],
        call_span: Span,
    ) {
        let bindings = match_params(&signature.params, args);
        // `...` accepts every otherwise-unmatched actual argument. Without
        // it, report only named arguments; excess positionals are outside
        // this rule's deliberately narrow scope.
        let supports_unknown_argument_check = signature
            .params
            .iter()
            .any(|param| param.required || param.default.is_some() || param.type_.is_some());
        self.check_call_arity(
            function_name,
            &signature.params,
            args,
            &bindings,
            supports_unknown_argument_check,
            call_span,
        );

        for (argument_index, parameter_index) in bindings.param_for_arg.iter().enumerate() {
            let Some(parameter_index) = parameter_index else {
                continue;
            };
            let parameter = &signature.params[*parameter_index];
            let Some(expected_json) = parameter.type_.as_ref() else {
                continue;
            };
            let expected = json_rtype_to_rtype(expected_json);
            let Some(actual) = arg_types.get(argument_index) else {
                continue;
            };
            if generic_argument_may_dispatch(&self.typeshed.globals, function_name, actual) {
                continue;
            }
            // A demand-only parameter types a downstream mode demand
            // (RY110) without asserting the argument's own type: the
            // declared type is relationally incomplete (e.g.
            // `vec_cast(x, to)` accepts any `x` castable to `to`), so an
            // RY092 provable-incompatibility verdict would false-positive
            // on legal calls. The demand gate below still reads it.
            if !parameter.demand_only && types_provably_incompatible(actual, &expected) {
                self.emit(
                    Severity::Error,
                    args[argument_index].span,
                    "RY092",
                    format!(
                        "argument `{}` to `{function_name}` is `{}`, expected {}",
                        parameter.name,
                        actual.mode,
                        expected_type_label(&expected)
                    ),
                );
            }
            // RY110: a stub-declared parameter type is also the
            // downstream mode demand a vacuous-all guard needs.
            self.check_vacuous_guard_demand(function_name, &expected, &args[argument_index]);
        }
    }

    /// RY111: a call argument passes the reserved-word literal `TRUE` or
    /// `FALSE` for a formal an enclosing function also exposes under the
    /// identical name -- silently hardcoding instead of forwarding the
    /// caller's value. The founding fixture is haven @ f067fb2,
    /// `R/labelled.R:111`: `median.haven_labelled <- function(x,
    /// na.rm = TRUE, ...) { ... median(vec_data(x), na.rm = TRUE, ...) }`
    /// -- a caller's explicit `na.rm = FALSE` is silently ignored
    /// (runtime-verified in #361: returns 2.5 where base
    /// `median(c(1:4, NA), na.rm = FALSE)` returns NA).
    ///
    /// Three precision gates keep the rule narrow, all three required by
    /// issue #361, plus the dead-formal gate the tidyverse corpus
    /// forced:
    ///
    /// * *identical name, twice*: the written tag must exactly name a
    ///   formal of an enclosing function (innermost frame outward -- any
    ///   caller-facing binding of that name is dead at this site) AND a
    ///   formal of the callee under the resolution `check_call_arguments`
    ///   itself applies (a collected user signature, else the typeshed
    ///   stub; a lexical callable's formals are unknown and stay silent).
    ///   Partial tags (`na.r = TRUE`) never fire: only the exact spelling
    ///   distinguishes the mechanical mistake from a deliberate
    ///   renaming-and-defaulting idiom, where the enclosing formal has a
    ///   different name and the constant is the documented behavior.
    /// * *dead formal*: the owning function must never read the formal
    ///   anywhere in its body (an `if (p)` guard, a `f(p)` validation, a
    ///   `k = p` forward at another call, `missing(p)`, a read inside a
    ///   `return(...)` value, a sibling formal's default (`force = p`),
    ///   or a capturing closure's read -- [`index_formal_reads`]). Reads
    ///   attribute to the innermost function owning the name, so a nested
    ///   closure redeclaring it does not discharge the outer formal, and
    ///   a plain assignment target is a *write* of the promise, not a
    ///   read of the caller's value. A read demonstrates the author
    ///   handles the caller's value, so per-site constants are chosen
    ///   child semantics; without one, the formal exists only in the
    ///   signature, and the constant silently replaces the caller's
    ///   entire control over it -- haven's exact shape.
    /// * *literal value*: `TRUE`/`FALSE` are reserved words, so the
    ///   constant cannot be a rebinding (`T`/`F` are ordinary identifiers
    ///   and stay silent). `NA` is excluded -- a typed hole, not a
    ///   hardcoded policy. Numeric and string constants stay silent too:
    ///   divergent defaults for them (`sep = ","` reformatted to
    ///   `sep = "\t"` internally) are an ordinary idiom in a way logical
    ///   flags are not. Forwarding the formal (`na.rm = na.rm`) and every
    ///   other non-literal expression stay silent by the same gate.
    /// * *known callee formal*: without a resolvable signature the literal
    ///   may land in `...` and travel anywhere, so the destination -- and
    ///   with it the claim -- is unknown; RY090's dots humility applies.
    ///
    /// The check is O(args) over a cheap syntactic filter (named `TRUE`/
    /// `FALSE` actuals inside a function body) and only then consults the
    /// callee's formal list, so the `scaling_project_size` perf budget is
    /// untouched.
    pub(crate) fn check_constant_shadowed_arguments(
        &mut self,
        function_name: &str,
        user_function: Option<&UserFn>,
        resolved_sig: Option<&FunctionSig>,
        lexical_callable: bool,
        args: &[Arg],
    ) {
        if self.discarding || self.enclosing_formals.is_empty() {
            return;
        }
        // Gate 1 (cheap, syntactic): a named TRUE/FALSE actual whose tag
        // names an enclosing formal, and -- the dead-formal half -- whose
        // owning function never reads that formal anywhere in its body.
        // `...` binds no name a call site could resolve to, and backticked
        // spellings normalize like RY090's. A body that reads the formal
        // (an `if (p)` guard, a `f(p)` validation, a `k = p` forward,
        // `missing(p)`) demonstrably handles the caller's value, so a
        // per-site constant there is chosen child semantics, not the
        // shadowing mistake: the corpus's deliberate idioms (dbplyr's
        // sql_render methods forwarding `subquery` at their own wrapper,
        // stringr's `if (ignore_case)` early return before a fixed
        // `regex(...)`, tibble's `quiet = quiet` at the user-facing
        // call) all read the formal and stay quiet. haven's founding
        // fixture reads `na.rm` nowhere: the formal exists only in the
        // signature, which is exactly the dead binding the constant
        // then silently replaces.
        let owning_frame = |tag: &str| {
            self.enclosing_formals
                .iter()
                .rev()
                .find(|frame| {
                    frame
                        .names
                        .iter()
                        .any(|formal| semantic_argument_name(formal) == tag)
                })
                .filter(|frame| {
                    !self
                        .formal_reads
                        .get(&frame.function_span)
                        .is_some_and(|reads| reads.contains(tag))
                })
        };
        let candidates: Vec<(&str, bool, Span)> = args
            .iter()
            .filter_map(|argument| {
                let tag = semantic_argument_name(argument.name.as_deref()?);
                if tag.is_empty() || tag == "..." || owning_frame(tag).is_none() {
                    return None;
                }
                match &argument.value {
                    Expr::Logical(value, span) => Some((tag, *value, *span)),
                    _ => None,
                }
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        // Gate 2: the callee must actually have that formal, under the
        // shared `resolve_callee` precedence (a user definition shadows
        // the stub; a lexical callable resolves to neither).
        let callee_formals: Option<Vec<&str>> =
            match self.resolve_callee(function_name, user_function, resolved_sig, lexical_callable)
            {
                ResolvedCallee::User(user_function) => Some(
                    user_function
                        .params
                        .iter()
                        .map(|parameter| parameter.name.as_str())
                        .collect(),
                ),
                ResolvedCallee::Stub(signature) => Some(
                    signature
                        .params
                        .iter()
                        .map(|parameter| parameter.name.as_str())
                        .collect(),
                ),
                ResolvedCallee::Unknown => None,
            };
        let Some(callee_formals) = callee_formals else {
            return;
        };
        for (tag, value, span) in candidates {
            // Exact-name binding only: the tag must equal a callee formal's
            // own name, so a partial match (`na.r` for `na.rm`) -- whose
            // runtime effect is R's matching rules, not the shadowing
            // mistake -- stays silent.
            if !callee_formals
                .iter()
                .any(|formal| semantic_argument_name(formal) == tag)
            {
                continue;
            }
            let constant = if value { "TRUE" } else { "FALSE" };
            self.emit(
                Severity::Warning,
                span,
                "RY111",
                format!(
                    "argument `{tag} = {constant}` to `{function_name}` is a constant shadowing \
                     the identically-named formal of an enclosing function, so the caller's \
                     `{tag}` value is silently ignored; forward it as `{tag} = {tag}` instead"
                ),
            );
        }
    }

    pub(crate) fn check_user_call_arguments(
        &mut self,
        function_name: &str,
        function: &UserFn,
        args: &[Arg],
        call_span: Span,
    ) {
        let names: Vec<&str> = function
            .params
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        let bindings = match_arguments(&names, args);
        self.check_call_arity(
            function_name,
            &function.params,
            args,
            &bindings,
            true,
            call_span,
        );
    }

    /// Shared arity reporting over one argument match: RY090 for named
    /// actuals no formal matched, RY091 for required formals without a
    /// supplied value. The typeshed and user-function checks differ only in their
    /// formals source and unknown-argument gating.
    fn check_call_arity<P: CallFormal>(
        &mut self,
        function_name: &str,
        params: &[P],
        args: &[Arg],
        bindings: &ArgumentMatch,
        report_unknown: bool,
        call_span: Span,
    ) {
        let names: Vec<&str> = params.iter().map(|param| param.name()).collect();
        let required: Vec<bool> = params.iter().map(|param| param.required()).collect();
        self.emit_unknown_arguments(function_name, &names, args, bindings, report_unknown);
        self.emit_missing_required(function_name, &names, &required, args, bindings, call_span);
    }

    fn emit_unknown_arguments(
        &mut self,
        function_name: &str,
        names: &[&str],
        args: &[Arg],
        bindings: &ArgumentMatch,
        enabled: bool,
    ) {
        if !enabled || bindings.dots.is_some() {
            return;
        }
        let forwards_dots = args.iter().any(|argument| self.is_forwarded_dots(argument));
        for argument_index in &bindings.unmatched_named {
            let argument = &args[*argument_index];
            // R expands the dots actuals and ignores the tag on the dots expression.
            if self.is_forwarded_dots(argument) {
                continue;
            }
            let argument_name = argument.name.as_deref().unwrap_or_default();
            // Expanded exact names can change which partial matches are valid.
            if forwards_dots
                && names.iter().any(|name| {
                    semantic_argument_name(name).starts_with(semantic_argument_name(argument_name))
                })
            {
                continue;
            }
            let suggestion = closest_parameter(argument_name, names);
            let hint = suggestion
                .map(|name| format!("; did you mean `{name}`?"))
                .unwrap_or_default();
            let message = format!("unknown argument `{argument_name}` to `{function_name}`{hint}");
            self.emit(Severity::Warning, argument.span, "RY090", message);
        }
    }

    fn emit_missing_required(
        &mut self,
        function_name: &str,
        names: &[&str],
        required: &[bool],
        args: &[Arg],
        bindings: &ArgumentMatch,
        call_span: Span,
    ) {
        let forwards_dots = args.iter().any(|argument| self.is_forwarded_dots(argument));
        for (parameter_index, required) in required.iter().enumerate() {
            // Forwarded actuals can fill unmatched slots or change positional and
            // partial matches. Only an exact named hole pins a missing formal.
            let missing = if forwards_dots {
                args.iter().any(|argument| {
                    argument.name.as_deref().is_some_and(|name| {
                        semantic_argument_name(name)
                            == semantic_argument_name(names[parameter_index])
                    }) && matches!(argument.value, Expr::Missing(_))
                })
            } else {
                !bindings.bound_params[parameter_index]
                    || bindings
                        .arg_for_param(parameter_index)
                        .is_some_and(|index| matches!(args[index].value, Expr::Missing(_)))
            };
            if *required && missing {
                self.emit(
                    Severity::Warning,
                    call_span,
                    "RY091",
                    format!(
                        "missing required argument `{}` in call to `{function_name}`",
                        names[parameter_index]
                    ),
                );
            }
        }
    }
}

fn closest_parameter<'a>(argument: &str, parameters: &'a [&str]) -> Option<&'a str> {
    let mut closest = None;
    let mut minimum_distance = usize::MAX;
    let mut minimum_is_tied = false;

    for parameter in parameters.iter().copied().filter(|name| *name != "...") {
        let distance = edit_distance(argument, parameter);
        if distance > 2 {
            continue;
        }
        match distance.cmp(&minimum_distance) {
            std::cmp::Ordering::Less => {
                closest = Some(parameter);
                minimum_distance = distance;
                minimum_is_tied = false;
            }
            std::cmp::Ordering::Equal => minimum_is_tied = true,
            std::cmp::Ordering::Greater => {}
        }
    }

    if minimum_is_tied { None } else { closest }
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = Vec::with_capacity(right_chars.len() + 1);
        current.push(left_index + 1);
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_char != *right_char);
            current.push(
                (current[right_index] + 1)
                    .min(previous[right_index + 1] + 1)
                    .min(substitution),
            );
        }
        previous = current;
    }
    previous[right_chars.len()]
}

/// Whether RY092 should defer its argument mode check: a classed or NULL
/// actual may dispatch to a method that accepts it.
pub(crate) fn generic_argument_may_dispatch(
    globals: &ry_typeshed::Globals,
    function_name: &str,
    actual: &RType,
) -> bool {
    let generic = crate::higher_order::is_dispatch_capable_generic(globals, function_name);
    generic && (actual.class.has_known_class() || actual.mode == Mode::Null)
}

/// Eval mode declared for the argument at `index`.
///
/// `bindings` comes from one `match_params` call shared by the whole call
/// site, so a loop over arguments does not re-match per argument.
pub(crate) fn eval_mode_for_arg(
    sig: &FunctionSig,
    bindings: &ArgumentMatch,
    index: usize,
) -> Option<EvalMode> {
    let parameter = bindings
        .param_for_arg
        .get(index)?
        .and_then(|parameter_index| sig.params.get(parameter_index))
        .map(|param| param.name.as_str())
        .unwrap_or("...");
    sig.eval
        .get(parameter)
        .copied()
        .or_else(|| sig.eval.get("...").copied())
}

pub(crate) fn argument_eval_mode(
    sig: &FunctionSig,
    args: &[Arg],
    index: usize,
) -> Option<EvalMode> {
    eval_mode_for_arg(sig, &match_params(&sig.params, args), index)
}

/// Find the data argument by formal binding, including named arguments.
/// Without an explicit source, only an unmasked first formal supplies data.
/// Quoting helpers such as `vars(...)` have no data argument.
pub(crate) fn data_mask_source_arg(sig: &FunctionSig, args: &[Arg]) -> Option<usize> {
    let source = if let Some(source) = sig.data_mask_source.as_deref() {
        source
    } else {
        if sig.schema_effect != Some(SchemaEffect::Join)
            && !sig
                .eval
                .values()
                .any(|mode| matches!(mode, EvalMode::DataMask | EvalMode::TidySelect))
        {
            return None;
        }
        let first = sig.params.first()?;
        if first.name == "..."
            || sig
                .eval
                .get(&first.name)
                .is_some_and(|mode| *mode != EvalMode::Normal)
        {
            return None;
        }
        &first.name
    };
    let bindings = match_params(&sig.params, args);
    bound_argument_index_matched(&sig.params, &bindings, source)
}

/// Whether the matched formal processes tidy-evaluation injection syntax.
pub(crate) fn argument_supports_injection(
    sig: &FunctionSig,
    args: &[Arg],
    index: usize,
) -> Option<InjectionMode> {
    let bindings = match_params(&sig.params, args);
    bindings.param_for_arg[index]
        .or(bindings.dots)
        .and_then(|formal| sig.params.get(formal))
        .and_then(|param| sig.injection.get(&param.name).copied())
}

impl Checker {
    pub(crate) fn infer_with_injection(
        &mut self,
        expr: &Expr,
        scope: &mut Scope,
        enabled: Option<InjectionMode>,
    ) -> RType {
        let previous = scope.tidy_injection;
        scope.tidy_injection = enabled.max(previous);
        let result = self.infer(expr, scope);
        scope.tidy_injection = previous;
        result
    }
}

#[cfg(test)]
mod argument_matching_tests {
    use super::*;

    fn argument(name: Option<&str>) -> Arg {
        Arg {
            name: name.map(str::to_string),
            value: Expr::Null(Span::default()),
            span: Span::default(),
        }
    }

    #[test]
    fn exact_names_are_matched_before_positionals() {
        let args = [argument(Some("second")), argument(None)];
        let matched = match_arguments(&["first", "second"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(1), Some(0)]);
        assert_eq!(matched.bound_params, vec![true, true]);
    }

    #[test]
    fn exact_match_is_removed_before_partial_matching() {
        let args = [argument(Some("alpha")), argument(Some("al"))];
        let matched = match_arguments(&["alpha", "alpine"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0), Some(1)]);
        assert!(matched.unmatched_named.is_empty());
    }

    #[test]
    fn unique_partial_name_matches() {
        let args = [argument(Some("alp"))];
        let matched = match_arguments(&["alpha", "beta"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0)]);
        assert!(matched.unmatched_named.is_empty());
    }

    #[test]
    fn ambiguous_partial_name_stays_unmatched() {
        let args = [argument(Some("al"))];
        let matched = match_arguments(&["alpha", "alpine"], &args);
        assert_eq!(matched.param_for_arg, vec![None]);
        assert_eq!(matched.unmatched_named, vec![0]);
    }

    #[test]
    fn dots_absorb_remaining_arguments_and_stop_positionals() {
        let args = [argument(None), argument(None), argument(Some("extra"))];
        let matched = match_arguments(&["x", "...", "after"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0), None, None]);
        assert_eq!(matched.unmatched_named, vec![2]);
        assert_eq!(matched.dots, Some(1));
    }

    #[test]
    fn formal_lookup_supports_positional_exact_and_partial_matching() {
        let params = ["file", "local", "..."]
            .into_iter()
            .map(|name| ParamSpec {
                name: name.to_string(),
                type_: None,
                required: false,
                default: None,
                demand_only: false,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            bound_argument_index(&params, &[argument(None), argument(None)], "local"),
            Some(1)
        );
        assert_eq!(
            bound_argument_index(
                &params,
                &[argument(Some("local")), argument(Some("file"))],
                "local"
            ),
            Some(0)
        );
        assert_eq!(
            bound_argument_index(&params, &[argument(Some("lo"))], "local"),
            Some(0)
        );
    }

    #[test]
    fn exact_name_after_dots_still_matches_but_partial_does_not() {
        let args = [argument(Some("after")), argument(Some("aft"))];
        let matched = match_arguments(&["x", "...", "after"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(2), None]);
    }

    #[test]
    fn opaque_union_member_keeps_type_check_silent() {
        let actual = RType::union(Arc::from(vec![
            RType::unknown(),
            RType::scalar(Mode::Character),
        ]));
        let expected = RType::scalar(Mode::Double);
        assert!(!types_provably_incompatible(&actual, &expected));
    }

    #[test]
    fn closest_parameter_is_limited_to_edit_distance_two() {
        assert_eq!(
            closest_parameter("lenght", &["length", "x"]),
            Some("length")
        );
        assert_eq!(closest_parameter("unrelated", &["length", "x"]), None);
    }
}

#[cfg(test)]
mod constant_shadowing_tests {
    use super::*;

    fn check(src: &str) -> Vec<Diagnostic> {
        let file = crate::tests::parse_file("shadowing.R", src);
        let mut checker = Checker::new("shadowing.R");
        checker.check(&file);
        checker.take_diagnostics()
    }

    fn fires(src: &str) -> bool {
        check(src).iter().any(|d| d.code == "RY111")
    }

    /// One site per shape: the haven founding fixture (dots coexist with
    /// the hardcoded constant), a required enclosing formal, a collected
    /// user callee, a base stub callee, FALSE against a TRUE-leaning
    /// default, and both pipe spellings plus qualification.
    #[test]
    fn fires_on_the_haven_and_adjacent_shapes() {
        assert!(fires(
            "vec_data <- function(x) x\nmedian.labelled <- function(x, na.rm = FALSE, ...) {\n  median(vec_data(x), na.rm = TRUE, ...)\n}\n"
        ));
        assert!(fires("f <- function(x, na.rm) median(x, na.rm = FALSE)\n"));
        assert!(fires(
            "my_sum <- function(x, na.rm = FALSE) sum(x)\nf <- function(x, na.rm = FALSE) my_sum(x, na.rm = TRUE)\n"
        ));
        assert!(fires(
            "f <- function(x, na.rm = FALSE) sum(x, na.rm = TRUE)\n"
        ));
        assert!(fires(
            "f <- function(x, na.rm = TRUE) median(x, na.rm = FALSE)\n"
        ));
        assert!(fires(
            "f <- function(x, na.rm = FALSE) x |> median(na.rm = TRUE)\n"
        ));
        assert!(fires(
            "f <- function(x, na.rm = FALSE) stats::median(x, na.rm = TRUE)\n"
        ));
    }

    /// The enclosing frame may be an outer one: a closure without its own
    /// `na.rm` formal captures the enclosing binding, so the constant
    /// still orphans that function's callers. A closure with its own
    /// identically-named formal is judged by its own frame instead.
    #[test]
    fn resolves_enclosing_frames_innermost_first() {
        assert!(fires(
            "f <- function(x, na.rm = FALSE) {\n  sapply(x, function(y) median(y, na.rm = TRUE))\n}\n"
        ));
        assert!(fires(
            "f <- function(x, na.rm = FALSE) {\n  g <- function(y, na.rm = TRUE) median(y, na.rm = TRUE)\n  g(x)\n}\n"
        ));
        // The inner formal is forwarded, not shadowed: silent.
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) {\n  g <- function(y, na.rm = TRUE) median(y, na.rm = na.rm)\n  g(x)\n}\n"
        ));
    }

    /// Gate 1 (identical name) and gate 2 (enclosing formal exists):
    /// renaming idioms, partial tags, and formal-less scopes stay quiet.
    #[test]
    fn stays_silent_without_the_identical_enclosing_formal() {
        // The renaming-and-defaulting idiom: the enclosing formal has a
        // different name, so the constant is the documented behavior.
        assert!(!fires(
            "f <- function(x, remove_na = FALSE) median(x, na.rm = TRUE)\n"
        ));
        // A partial tag is not the exact spelling.
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) median(x, na.r = TRUE)\n"
        ));
        // No enclosing function at all, or none with the formal.
        assert!(!fires("median(1:3, na.rm = TRUE)\n"));
        assert!(!fires("f <- function(x) median(x, na.rm = TRUE)\n"));
        // A dots-only enclosing frame binds no such name.
        assert!(!fires(
            "f <- function(...) median(list(...)[[1]], na.rm = TRUE)\n"
        ));
    }

    /// Gate 3 (reserved-word literal): forwarding, computed values, the
    /// rebindable T/F spellings, NA, and non-logical constants stay
    /// quiet.
    #[test]
    fn stays_silent_for_non_literal_values() {
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) median(x, na.rm = na.rm)\n"
        ));
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) median(x, na.rm = as.logical(na.rm))\n"
        ));
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) median(x, na.rm = T)\n"
        ));
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) median(x, na.rm = NA)\n"
        ));
        // Numeric and string constants: the divergent-default idiom.
        assert!(!fires("f <- function(x, times = 3) rep(x, times = 3)\n"));
        assert!(!fires(
            "f <- function(x, sep = \",\") paste0(x, sep = \"\")\n"
        ));
    }

    /// Gate 4 (callee formal known): a `...`-only callee forwards the
    /// literal into dots, and an unknown callee has no formals to match.
    #[test]
    fn stays_silent_without_a_resolvable_callee_formal() {
        assert!(!fires(
            "variadic <- function(x, ...) list(x, ...)\nf <- function(x, na.rm = FALSE) variadic(x, na.rm = TRUE)\n"
        ));
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) not_collected_anywhere(x, na.rm = TRUE)\n"
        ));
        // A lexical callable shadows the tables; its formals are unknown.
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) {\n  inner <- function(y, na.rm) NULL\n  inner(x, na.rm = TRUE)\n}\n"
        ));
    }

    /// The dead-formal gate: a body that reads the formal anywhere (an
    /// `if (p)` guard, a validation call, a by-name forward at another
    /// site, a `missing(p)` test, or a nested closure's captured read)
    /// demonstrably handles the caller's value, so the per-site constant
    /// is chosen child semantics. These are the corpus's deliberate
    /// idioms: stringr's guarded `ignore_case`, dbplyr's forwarded
    /// `subquery`, tibble's forwarded `quiet`, dplyr's guarded
    /// `recursive`, rvest's `env_has(env, nm, inherit = inherit)`.
    #[test]
    fn stays_silent_when_the_body_reads_the_formal() {
        // stringr detect: the early-return guard consumes the flag
        // before the fixed child call.
        assert!(!fires(
            "f <- function(x, ignore_case = FALSE) {\n  if (ignore_case) return(x)\n  median(x, ignore_case = FALSE)\n}\n"
        ));
        // tibble set_tidy_names: forwarded by name at the user-facing
        // call, pinned at the internal one.
        assert!(!fires(
            "final <- function(x, quiet = FALSE) x\nf <- function(x, quiet = FALSE) {\n  a <- median(x, quiet = TRUE)\n  final(a, quiet = quiet)\n}\n"
        ));
        // A validation read counts: check_bool(na.rm) handles the value.
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) {\n  check_bool(na.rm)\n  median(x, na.rm = TRUE)\n}\ncheck_bool <- function(x) TRUE\n"
        ));
        // dplyr's group_split: the documented-ignored warning reads the
        // formal through missing().
        assert!(!fires(
            "f <- function(x, keep = FALSE) {\n  if (!missing(keep)) warn(\"ignored\")\n  median(x, keep = TRUE)\n}\nwarn <- function(...) NULL\n"
        ));
        // A nested closure's captured read is a read in the owning
        // body's nested AST.
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) {\n  g <- function(y) if (na.rm) y else y\n  g(x)\n  median(x, na.rm = TRUE)\n}\n"
        ));
        // The read may sit after the constant in source order.
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) {\n  a <- median(x, na.rm = TRUE)\n  if (na.rm) x else a\n}\n"
        ));
    }

    /// The dead-formal index's scoping rules (review-driven, PR #543):
    /// a nested closure redeclaring the name shadows the outer formal
    /// for its subtree (innermost-owner attribution); a plain-identifier
    /// assignment target is a write, not a read; a read inside a
    /// `return(...)` value counts; and a formal default naming a
    /// sibling formal (`force = na.rm`) consumes the caller's value.
    #[test]
    fn dead_formal_index_respects_scoping_writes_and_defaults() {
        // The nested closure reads ITS OWN na.rm; the outer formal stays
        // dead and the later hardcode fires (innermost attribution).
        assert!(fires(
            "f <- function(x, na.rm = FALSE) {\n  g <- function(y, na.rm) median(y, na.rm = na.rm)\n  g(x)\n  median(x, na.rm = TRUE)\n}\n"
        ));
        // Replacing the promise is a write, not a read: the caller's
        // value is never consumed, plain and subassigned alike.
        assert!(fires(
            "f <- function(x, na.rm = FALSE) {\n  na.rm <- FALSE\n  median(x, na.rm = TRUE)\n}\n"
        ));
        assert!(fires(
            "f <- function(x, na.rm = FALSE) {\n  na.rm[1] <- TRUE\n  median(x, na.rm = TRUE)\n}\n"
        ));
        // A read inside a return value is a read.
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) {\n  return(if (na.rm) x else x)\n}\n"
        ));
        // A default naming a sibling formal consumes the caller's value,
        // in the function's own signature and in a nested closure's.
        assert!(!fires(
            "f <- function(x, na.rm = FALSE, force = na.rm) {\n  median(x, na.rm = TRUE)\n}\n"
        ));
        assert!(!fires(
            "f <- function(x, na.rm = FALSE, k = 2 * na.rm) {\n  median(x, na.rm = TRUE)\n}\n"
        ));
        assert!(!fires(
            "f <- function(x, na.rm = FALSE) {\n  g <- function(y, keep = na.rm) NULL\n  median(x, na.rm = TRUE)\n}\n"
        ));
    }

    /// The diagnostic names the callee, the constant, and the fix, and
    /// points at the constant's own span.
    #[test]
    fn message_names_the_constant_and_the_fix() {
        let diagnostics = check("f <- function(x, na.rm = FALSE) median(x, na.rm = TRUE)\n");
        let hits: Vec<_> = diagnostics.iter().filter(|d| d.code == "RY111").collect();
        assert_eq!(hits.len(), 1, "diagnostics: {diagnostics:?}");
        assert_eq!(hits[0].severity, Severity::Warning);
        assert!(
            hits[0].message.contains("`median`"),
            "message must name the callee: {}",
            hits[0].message
        );
        assert!(
            hits[0].message.contains("`na.rm = TRUE`"),
            "message must name the constant: {}",
            hits[0].message
        );
        assert!(
            hits[0].message.contains("`na.rm = na.rm`"),
            "message must name the fix: {}",
            hits[0].message
        );
    }

    /// One finding per constant argument, not per enclosing frame or per
    /// pass.
    #[test]
    fn fires_once_per_site() {
        let diagnostics = check(
            "f <- function(x, na.rm = FALSE, drop = TRUE) {\n  a <- median(x, na.rm = TRUE)\n  b <- median(x, na.rm = TRUE)\n}\n",
        );
        assert_eq!(
            diagnostics.iter().filter(|d| d.code == "RY111").count(),
            2,
            "diagnostics: {diagnostics:?}"
        );
    }
}
