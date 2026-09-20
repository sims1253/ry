//! Recall rules targeting known false-negative shapes (RY102, RY103,
//! RY105, RY107).
//!
//! These codes exist to catch real defects the 62-package Posit corpus audit
//! found and 0.8.0 missed. They are grouped here because they share a
//! property that the rest of the checker does not: each is decided by the
//! *shape* of an expression rather than by the inferred type of a value, so
//! their false-positive surface is bounded by the syntax they match.
//!
//! `tests/recall_rules.rs` pins both the positive and the negative
//! direction of each rule.
//!
//! One of the original sketches is deliberately **not** implemented.
//!
//! `not-before-comparison` was premised on `!x >= y` parsing as
//! `(!x) >= y`, but R's `?Syntax` places unary `!` *below* the comparison
//! operators, so it parses as `!(x >= y)` and the flagged code is correct.
//! That is the precise model error that retired `RY095` in 0.4.1.
//!
//! `constant-condition`'s `any(v) == 0` half (glue `R/utils.R:32`) is a real
//! bug — `any(lengths == 0)` was meant — but the sketch's justification for
//! flagging it, "is always FALSE", is wrong: `any()` yields a logical and
//! `FALSE == 0` is `TRUE`. The shape originally stayed open because it
//! seemed indistinguishable from diffobj's legitimate `!all(diff(x)) == 1L`,
//! pinned as must-stay-silent in `testdata/ry095_ry096_real_shapes.R`. RY107
//! separates them by *outcome* rather than by shape: a comparison that
//! preserves the scalar logical's value (`== 1`, `> 0`, ...) is the diffobj
//! idiom and stays silent, while one that negates it (`== 0`, `!= 1`, ...) or
//! is constant (`> 1`, `< 0`, ...) computes something other than what the
//! element-level reading suggests and is reported. The outcome claims are
//! honest about the base functions' own result domain: `any()`/`all()`
//! return `NA` when an `NA` element is undetermined by a deciding
//! `TRUE`/`FALSE` (and `NA` compares as `NA`), and both are S4 generics —
//! a direct method or the `Summary` group, selected even through
//! `base::any()`, can return values outside the domain entirely — so
//! constant outcomes are worded as holding "when the base result is not
//! `NA`" and the element-level reading stays a suspicion, not a proven
//! intent. The `length(sum(...)) > 0` half ships as RY105.

use super::*;

/// Containers whose arguments become *named elements* of the result, so a
/// `<-` typed where `=` was meant silently drops the name. Restricted to
/// this family on purpose: `local(x <- 1)`, `suppressWarnings(x <- f())`
/// and every user function take an ordinary assignment as an argument
/// without losing anything.
///
/// The list is maintained in [`crate::semantic_lists`] and validated by an
/// R-oracle coherence test.
use crate::semantic_lists::NAME_CARRYING_CONTAINERS;

/// The callee of a direct call, with any `pkg::` / `pkg:::` prefix removed.
/// Indirect callees (an index expression, a call returning a function) have
/// no name and are never matched by these rules.
fn bare_callee(expr: &Expr) -> Option<&str> {
    let Expr::Call { func, .. } = expr else {
        return None;
    };
    let name = match func.as_ref() {
        Expr::Ident { name, .. } | Expr::String(name, _) => name.as_str(),
        _ => return None,
    };
    Some(crate::semantic_lists::bare_name(name))
}

/// A call to `f(x)` with exactly one positional argument.
fn unary_call_to(expr: &Expr, callee: &str) -> bool {
    let Expr::Call { args, .. } = expr else {
        return false;
    };
    bare_callee(expr) == Some(callee) && args.len() == 1 && args[0].name.is_none()
}

/// The source spelling of a direct call's callee together with its single
/// positional argument, or `None` for any other shape. Indirect callees have
/// no name to resolve, and a call with several arguments is not the
/// misplaced-parenthesis reading this family matches (its `na.rm` style
/// controls would be lost by the suggested rewrite).
fn unary_call_callee(expr: &Expr) -> Option<(&str, &Expr)> {
    let Expr::Call { func, args, .. } = expr else {
        return None;
    };
    let name = match func.as_ref() {
        Expr::Ident { name, .. } | Expr::String(name, _) => name.as_str(),
        _ => return None,
    };
    if args.len() != 1 || args[0].name.is_some() {
        return None;
    }
    Some((name, &args[0].value))
}

/// The value of a numeric literal, with a leading unary minus folded so
/// `-1` reads as the value -1. Only a literal operand folds: R's unary
/// minus binds looser than `^`, so `-2^2` is `-(2^2)` and stays opaque.
/// Unary `+` needs no case here — the parser drops it entirely, so `+2`
/// lowers to the bare `Integer`/`Double` literal.
pub(crate) fn numeric_literal(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Integer(value, _) => Some(*value as f64),
        Expr::Double(value, _) => Some(*value),
        Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            expr,
            ..
        } => numeric_literal(expr).map(|value| -value),
        _ => None,
    }
}

/// Whether the operand is a numeric literal at most zero (`0`, `-1`,
/// `-0.5`, ...). [`Checker::check_constant_length_comparison`] admits
/// exactly these bounds: `length()` is never negative, so against them a
/// length-1-by-construction operand makes the comparison constant, while
/// a positive literal (`length(x) == 1`) can still be a deliberate scalar
/// assertion.
fn zero_or_negative_literal(expr: &Expr) -> bool {
    numeric_literal(expr).is_some_and(|value| value <= 0.0)
}

/// The name a `<-` inside a container argument would have produced had `=`
/// been typed instead. Only a bare identifier or a string literal qualifies:
/// `list(x[[1]] <- 2)` and `list(names(y) <- z)` are replacement functions,
/// not mistyped names.
fn mistyped_element_name(target: &Expr) -> Option<&str> {
    match target {
        Expr::Ident { name, .. } | Expr::String(name, _) => Some(name.as_str()),
        _ => None,
    }
}

/// Strip any leading unary `!` operators, returning the operand under
/// them. Also used by the tidyeval `!!`/`!!!` handling in `infer`, which
/// tree-sitter parses as nested unary `!`.
pub(crate) fn strip_negation(expr: &Expr) -> &Expr {
    let mut inner = expr;
    while let Expr::UnaryOp {
        op: UnaryOpKind::Not,
        expr: next,
        ..
    } = inner
    {
        inner = next.as_ref();
    }
    inner
}

/// Extract the first positional argument expression from a call.
fn call_argument(expr: &Expr) -> Option<&Expr> {
    let Expr::Call { args, .. } = expr else {
        return None;
    };
    args.first().map(|arg| &arg.value)
}

impl Checker {
    /// RY102: `list("a" <- 1)` where `list("a" = 1)` was meant.
    ///
    /// `names(list("a" <- 1, "b" = 2))` is `c("", "b")` — the element is
    /// created unnamed and a variable `a` is assigned as a side effect. Found
    /// in pak `R/pak-sitrep-data.R:41`.
    ///
    /// Purely syntactic: it needs no type information and cannot be
    /// suppressed or widened by inference. `<<-` is excluded because an
    /// explicit super-assignment is never a mistyped `=`.
    ///
    /// An **identifier** on the left additionally requires that some *other*
    /// argument of the same call is named. `c(out, outn <- paste(...))` is a
    /// deliberate assign-and-append idiom — measured in Hmisc, knitr, nlme,
    /// xfun, DescTools and fitdistrplus, where the container builds an
    /// unnamed vector and losing a name costs nothing. When a sibling
    /// argument *is* named, the call is demonstrably building a named
    /// structure and the odd `<-` out is a typo (AER, Hmisc's markdown
    /// helper list, markdown, mclust, psych, and pak's original report).
    /// A string literal on the left needs no such corroboration: `"a" <- 1`
    /// is not an idiom anyone writes on purpose.
    pub(crate) fn check_named_element_arrow(&mut self, func: &Expr, args: &[Arg], scope: &Scope) {
        let name = match func {
            Expr::Ident { name, .. } | Expr::String(name, _) => name.as_str(),
            _ => return,
        };
        // Only base:: versions of these containers carry the name-dropping
        // semantic. Delegate to the canonical base-call resolution operation
        // so the shadowing order lives in one place.
        let lookup_name = crate::semantic_lists::bare_name(name);
        if !self.resolves_to_base_lenient(name, scope) {
            return;
        }
        if !NAME_CARRYING_CONTAINERS.contains(&lookup_name) {
            return;
        }
        let builds_named_structure = args.iter().any(|argument| argument.name.is_some());
        for argument in args {
            if argument.name.is_some() {
                continue;
            }
            let Expr::BinOp {
                op: BinOpKind::Assign,
                lhs,
                span,
                ..
            } = &argument.value
            else {
                continue;
            };
            let Some(name) = mistyped_element_name(lhs) else {
                continue;
            };
            if matches!(lhs.as_ref(), Expr::Ident { .. }) && !builds_named_structure {
                continue;
            }
            let spelling = self.source_text(span_of(lhs)).unwrap_or(name).to_string();
            let message = format!(
                "`<-` inside `{lookup_name}()` assigns `{name}` and leaves the element unnamed; write `{spelling} = ...` to name it"
            );
            self.emit(Severity::Warning, *span, "RY102", message);
        }
    }

    /// RY103: `class(x)` compared with `==` / `!=` in a length-1 logical
    /// context.
    ///
    /// `class()` returns a character *vector*. For a multi-class object
    /// (`c("tbl_df", "tbl", "data.frame")`) the comparison is length 3, and
    /// `if` / `&&` then error with `'length = 3' in coercion to logical(1)`
    /// on R >= 4.3. Found in sparklyr `R/worker_apply.R:522`.
    ///
    /// `expr` is a single operand of such a context. The scan strips `!`
    /// (which is what a caller writes around the comparison) but deliberately
    /// does **not** descend into `&&` / `||`: their operands are passed here
    /// individually by `infer_short_circuit_binop`, so every site is reported
    /// exactly once regardless of nesting depth.
    ///
    /// Silent for `class(x)[1] == "y"` (an `Index`, not a `Call`, and
    /// explicitly length-1) and for any use outside a scalar logical context,
    /// where a vector result is the point.
    pub(crate) fn check_class_equality_operand(&mut self, expr: &Expr, scope: &Scope) {
        let Expr::BinOp {
            op: op @ (BinOpKind::Eq | BinOpKind::Ne),
            lhs,
            rhs,
            span,
        } = strip_negation(expr)
        else {
            return;
        };
        let lhs_is_class = unary_call_to(lhs, "class");
        let rhs_is_class = unary_call_to(rhs, "class");
        if !lhs_is_class && !rhs_is_class {
            return;
        }

        // The message can name the concrete `inherits()` rewrite only when
        // exactly one operand is a class() call proven to be base::class;
        // a qualified or shadowed callee with unrelated semantics would
        // make the suggestion wrong, so those cases get the generic wording.
        let suggestable_operands = match (lhs_is_class, rhs_is_class) {
            (true, false) if self.is_base_class_call(lhs, scope) => Some((lhs, rhs)),
            (false, true) if self.is_base_class_call(rhs, scope) => Some((rhs, lhs)),
            _ => None,
        };
        let prefix = if matches!(op, BinOpKind::Eq) { "" } else { "!" };
        let suggestion = suggestable_operands
            .and_then(|(class_call, other_side)| {
                call_argument(class_call).map(|argument| (argument, other_side))
            })
            .and_then(|(argument, other_side)| {
                self.source_text(span_of(argument))
                    .zip(self.source_text(span_of(other_side)))
            })
            .map(|(argument, class)| format!("{prefix}inherits({argument}, {class})"));
        let message = suggestion.as_ref().map_or_else(
            || "`class()` returns a character vector, so this comparison is not length-1 for a multi-class object and the enclosing condition errors; use `inherits()`".to_string(),
            |suggestion| format!("`class()` returns a character vector, so this comparison is not length-1 for a multi-class object and the enclosing condition errors; use `{suggestion}`"),
        );
        self.emit(Severity::Warning, *span, "RY103", message);
    }

    fn is_base_class_call(&self, expr: &Expr, scope: &Scope) -> bool {
        let Expr::Call { func, .. } = expr else {
            return false;
        };
        let Some(name) = ident_name(func) else {
            return false;
        };
        // The callee must be `class` (or `base::class`). Delegate to the
        // canonical base-call resolution operation so the shadowing order
        // lives in one place.
        crate::semantic_lists::bare_name(name) == "class" && self.resolves_to_base(name, scope)
    }

    /// RY105: `length(x) <op> 0` where `x` is length 1 by construction, as in
    /// pak `R/confirmation.R:42` — `length(sum(...)) > 0`.
    ///
    /// The guard reads as an emptiness check but its operand can never be
    /// empty, so the branch is dead (or, for `== 0`, unreachable). Only a
    /// literal at most zero is flagged: comparisons against `1`
    /// (`length(x) == 1`) are deliberate scalar assertions, which assertion
    /// helpers write on purpose, while `length()` is never negative, so a
    /// negative bound (`length(x) > -1`) is as dead as `0` is.
    ///
    /// "Length 1 by construction" means one of two things, both chosen so the
    /// claim does not rest on inference that could be over-narrow (the failure
    /// mode these rules guard against):
    ///
    /// 1. a direct call to a function whose typeshed stub declares a return
    ///    length of exactly 1 and whose arguments cannot trigger S3 dispatch;
    ///    or
    /// 2. a local binding whose inferred type is a length-1 *atomic* and which
    ///    is neither a parameter, a parameter default, nor a flow-narrowed
    ///    refinement — a parameter's type comes from one default or one call
    ///    site and is not proof of the runtime length. The binding must also
    ///    have a known empty class vector: storage length does not determine
    ///    `length(x)` for a classed object.
    pub(crate) fn check_constant_length_comparison(
        &mut self,
        op: BinOpKind,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        scope: &Scope,
    ) {
        if !is_comparison(op) {
            return;
        }
        fn length_operand(expr: &Expr) -> Option<&Expr> {
            match expr {
                Expr::Call { args, .. }
                    if bare_callee(expr) == Some("length")
                        && args.len() == 1
                        && args[0].name.is_none() =>
                {
                    Some(&args[0].value)
                }
                _ => None,
            }
        }
        let (measured, measured_on_left, length_call) =
            match (length_operand(lhs), length_operand(rhs)) {
                (Some(measured), None) if zero_or_negative_literal(rhs) => (measured, true, lhs),
                (None, Some(measured)) if zero_or_negative_literal(lhs) => (measured, false, rhs),
                _ => return,
            };
        // The outer `length()` call must resolve to base::length; a
        // shadowed or qualified `length` (e.g. `other::length(...)`) may
        // have unrelated semantics.
        let length_name = match length_call {
            Expr::Call { func, .. } => ident_name(func).unwrap_or("length"),
            _ => "length",
        };
        if !self.resolves_to_base_lenient(length_name, scope) {
            return;
        }
        let Some(reason) = self.scalar_by_construction(measured, scope) else {
            return;
        };
        // Normalize to `length(...) <op> <literal>` by mirroring the
        // operator when the literal is on the left side of the comparison.
        let effective_op = if measured_on_left {
            op
        } else {
            match op {
                BinOpKind::Lt => BinOpKind::Gt,
                BinOpKind::Le => BinOpKind::Ge,
                BinOpKind::Gt => BinOpKind::Lt,
                BinOpKind::Ge => BinOpKind::Le,
                other => other,
            }
        };
        // `length(...)` is exactly 1 and the admitted bound is at most 0,
        // so the same outcome table covers 0 and every negative bound.
        let outcome = match effective_op {
            BinOpKind::Eq | BinOpKind::Lt | BinOpKind::Le => "FALSE",
            _ => "TRUE",
        };
        self.emit(
            Severity::Warning,
            span,
            "RY105",
            format!(
                "{reason}, so `length(...)` is 1 here and this length guard is always {outcome}"
            ),
        );
    }

    /// RY107: a comparison with a direct `any(...)`/`all(...)` call on one
    /// side and a numeric literal on the other, when the comparison does not
    /// preserve the call's scalar logical value.
    ///
    /// `any()`/`all()` return a length-1 logical, which numeric comparison
    /// coerces to 0/1, so exactly three outcomes exist:
    ///
    /// * **preserving** (`== 1`, `!= 0`, `> 0`, `>= 1`): the comparison
    ///   computes what the bare call already computes. That is diffobj's
    ///   `!all(diff(x)) == 1L` idiom, pinned must-stay-silent in
    ///   `testdata/ry095_ry096_real_shapes.R`, so these stay silent.
    /// * **negating** (`== 0`, `!= 1`, `<= 0`, `< 1`): the comparison
    ///   computes `!any(x)`, which reads nothing like the source. Found in
    ///   glue `R/utils.R:32` — `any(lengths) == 0` where
    ///   `any(lengths == 0)` was meant.
    /// * **constant** (`> 1`, `>= 2`, `< 0`, `== 2`, `> -1`, ...): the
    ///   guard is always TRUE or always FALSE over the base domain's
    ///   non-`NA` results.
    ///
    /// The negating and constant outcomes are reported with the element-level
    /// rewrite. The wording is scoped to the base functions' own result
    /// domain rather than asserted unconditionally: an `NA` result — for
    /// `any()`, an `NA` element with no `TRUE` present; for `all()`, an
    /// `NA` element with no `FALSE` — compares as `NA`, so constant
    /// outcomes are stated as holding "when the base result is not `NA`".
    /// `any()`/`all()` are also S4 generics (R 4.6.1 runtime-verified: a
    /// direct `any` method and a `Summary`-group method can return any
    /// value, even a longer vector, and `base::any()` still selects the
    /// generic), so the length-1 premise is qualified and the rewrite is
    /// framed as the probable intent. An open-world argument is never a
    /// reason to go silent — the founding glue shape passes a parameter —
    /// and a scalar or unclassed parameter default never proves
    /// classlessness for the caller's values.
    ///
    /// Scoped against the neighbors: RY093 and RY100 report a comparison
    /// nested *inside* `length()`/`nchar()`/a math call on the same span this
    /// rule would report, and those checks run at the enclosing call before
    /// its arguments are inferred, so an existing same-span diagnostic means
    /// the site is already covered. RY105 requires a `length()` call as an
    /// operand — a different shape from the bare `any()`/`all()` call matched
    /// here — so the two rules cannot meet on one comparison.
    pub(crate) fn check_any_all_scalar_comparison(
        &mut self,
        op: BinOpKind,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        scope: &Scope,
    ) {
        if !is_comparison(op) {
            return;
        }
        if self.diagnostics.iter().any(|diagnostic| {
            diagnostic.span == span && matches!(diagnostic.code, "RY093" | "RY100")
        }) {
            return;
        }
        let ((callee, argument), literal_expr, aggregate_on_left) =
            match (unary_call_callee(lhs), unary_call_callee(rhs)) {
                (Some(call), None) => (call, rhs, true),
                (None, Some(call)) => (call, lhs, false),
                // Both sides calls, or neither: the scalar-vs-literal shape
                // this family matches needs a literal to compare against.
                _ => return,
            };
        let bare = crate::semantic_lists::bare_name(callee);
        if !matches!(bare, "any" | "all") {
            return;
        }
        // The length-1 logical premise holds only for the base functions; a
        // shadowed or differently-imported `any`/`all` may return anything.
        if !self.resolves_to_base_lenient(callee, scope) {
            return;
        }
        let Some(literal) = numeric_literal(literal_expr) else {
            return;
        };
        // The logical operand coerces to 0 under FALSE and 1 under TRUE, so
        // evaluating both pins the comparison's behavior on every non-NA
        // input.
        let outcome = |value: f64| -> bool {
            let (left, right) = if aggregate_on_left {
                (value, literal)
            } else {
                (literal, value)
            };
            match op {
                BinOpKind::Lt => left < right,
                BinOpKind::Le => left <= right,
                BinOpKind::Gt => left > right,
                BinOpKind::Ge => left >= right,
                BinOpKind::Eq => left == right,
                BinOpKind::Ne => left != right,
                _ => false,
            }
        };
        let when_false = outcome(0.0);
        let when_true = outcome(1.0);
        // The preserving family computes the bare call's own value, which is
        // a written-on-purpose idiom (diffobj); only negating and constant
        // outcomes mislead.
        if !when_false && when_true {
            return;
        }
        // Mirror the operator for the suggestion when the literal is on the
        // left, so `0 == any(x)` suggests `any(x == 0)`.
        let suggested_op = if aggregate_on_left {
            op
        } else {
            match op {
                BinOpKind::Lt => BinOpKind::Gt,
                BinOpKind::Le => BinOpKind::Ge,
                BinOpKind::Gt => BinOpKind::Lt,
                BinOpKind::Ge => BinOpKind::Le,
                other => other,
            }
        };
        // Outcome claims are scoped to the base result domain: an `NA`
        // result compares as `NA`, and a dispatched `any`/`all` or
        // `Summary`-group method can return any value, so the wording
        // conditions on the base computation's non-`NA` results. The
        // element-level reading is the probable intent, never a proven
        // one.
        let outcome_text = if when_false == when_true {
            if when_false {
                "always TRUE when the base result is not `NA` (an `NA` result compares as `NA`)"
            } else {
                "always FALSE when the base result is not `NA` (an `NA` result compares as `NA`)"
            }
        } else {
            "TRUE exactly when the call is FALSE (`NA` when it is `NA`)"
        };
        let message = match (
            self.source_text(span_of(argument)),
            self.source_text(span_of(literal_expr)),
        ) {
            (Some(argument_text), Some(literal_text)) => format!(
                "`{bare}()` returns a length-1 logical unless an `any`/`all` or `Summary`-group method dispatches, so this comparison is {outcome_text}; the comparison was probably meant for the elements: `{bare}({argument_text} {} {literal_text})`",
                op_symbol(suggested_op),
            ),
            _ => format!(
                "`{bare}()` returns a length-1 logical unless an `any`/`all` or `Summary`-group method dispatches, so this comparison is {outcome_text}; compare the elements inside the call instead"
            ),
        };
        self.emit(Severity::Warning, span, "RY107", message);
    }

    /// Why `expr` is length 1 for every input, or `None` when that cannot be
    /// established from construction alone. See
    /// [`Checker::check_constant_length_comparison`] for the two admitted
    /// forms.
    ///
    /// Form 1 consults the typeshed stubs: any function whose declared
    /// return length is exactly 1 is a scalar reduction. This replaces a
    /// hardcoded list — the stubs are the authoritative source and are
    /// maintained in one place.
    fn scalar_by_construction(&self, expr: &Expr, scope: &Scope) -> Option<String> {
        // Use the original callee (with pkg:: prefix preserved) for the
        // typeshed lookup, so otherpkg::sum does not resolve to base::sum.
        let Expr::Call { func, args, .. } = expr else {
            // Fall through to the local-binding check below.
            return self.scalar_by_construction_local(expr, scope);
        };
        let callee = match func.as_ref() {
            Expr::Ident { name, .. } | Expr::String(name, _) => name.as_str(),
            _ => return None,
        };
        // Use the canonical base-call resolution: if the call does not
        // resolve to base, the user's function may have entirely different
        // semantics and the typeshed claim does not apply.
        let bare = crate::semantic_lists::bare_name(callee);
        if !self.resolves_to_base_lenient(callee, scope) {
            return None;
        }
        if !args.is_empty()
            && self.is_typeshed_scalar_reduction(callee)
            && self.scalar_call_is_classless(callee, args, scope)
        {
            return Some(format!("`{bare}()` always returns a single value"));
        }
        None
    }

    /// Whether every argument to a dispatch-capable scalar call is proven to
    /// be classless. Keep this proof at the literal/local seam: inferring a
    /// class through arbitrary calls or operators would duplicate inference
    /// and could miss dispatch or rebinding.
    fn scalar_call_is_classless(&self, callee: &str, args: &[Arg], scope: &Scope) -> bool {
        let bare = crate::semantic_lists::bare_name(callee);
        let may_dispatch = bare == "length"
            || crate::higher_order::is_dispatch_capable_generic(&self.typeshed.globals, bare);
        !may_dispatch
            || args
                .iter()
                .all(|argument| self.scalar_call_argument_is_classless(&argument.value, scope))
    }

    /// Prove the narrow set of argument expressions whose classlessness is
    /// established without running an arbitrary call. Operators are admitted
    /// only while their base identity is known; otherwise a rebound operator
    /// can manufacture a classed value from scalar literals.
    fn scalar_call_argument_is_classless(&self, expr: &Expr, scope: &Scope) -> bool {
        match expr {
            Expr::Ident { name, .. } => {
                !(scope.parameter_bindings.contains(name)
                    || scope.default_parameter_bindings.contains(name)
                    || scope.narrowed_bindings.contains(name))
                    && scope
                        .get(name)
                        .is_some_and(|ty| ty.class.known && ty.class.len == 0)
            }
            Expr::Logical(..)
            | Expr::Integer(..)
            | Expr::Double(..)
            | Expr::String(..)
            | Expr::Null(..)
            | Expr::Na(..) => true,
            Expr::BinOp { op, lhs, rhs, .. }
                if op.is_arithmetic() || matches!(op, BinOpKind::Colon) =>
            {
                self.scalar_call_operator_is_base(op_symbol(*op), scope)
                    && self.scalar_call_argument_is_classless(lhs, scope)
                    && self.scalar_call_argument_is_classless(rhs, scope)
            }
            Expr::UnaryOp { op, expr, .. } => {
                self.scalar_call_unary_operator_is_base(*op, scope)
                    && self.scalar_call_argument_is_classless(expr, scope)
            }
            _ => false,
        }
    }

    fn scalar_call_operator_is_base(&self, symbol: &str, scope: &Scope) -> bool {
        !scope.ops_environment_unknown
            && !scope.data_mask_unknown
            && !scope.search_path_unknown
            && self.bare_loaded.is_empty()
            && !ops_chooser::syntax_rebound(self, scope)
            && !ops_chooser::operator_rebound(self, symbol, scope)
    }

    fn scalar_call_unary_operator_is_base(&self, op: UnaryOpKind, scope: &Scope) -> bool {
        let symbol = match op {
            UnaryOpKind::Neg => "-",
            UnaryOpKind::Not => "!",
        };
        self.scalar_call_operator_is_base(symbol, scope)
    }

    fn scalar_by_construction_local(&self, expr: &Expr, scope: &Scope) -> Option<String> {
        let Expr::Ident { name, .. } = expr else {
            return None;
        };
        if scope.parameter_bindings.contains(name)
            || scope.default_parameter_bindings.contains(name)
            || scope.narrowed_bindings.contains(name)
        {
            return None;
        }
        let bound = scope.get(name)?;
        if !matches!(bound.length, Length::One) {
            return None;
        }
        if !matches!(
            bound.mode,
            Mode::Logical | Mode::Integer | Mode::Double | Mode::Complex | Mode::Character
        ) {
            return None;
        }
        if !bound.class.known || bound.class.len != 0 {
            return None;
        }
        Some(format!("`{name}` is a length-1 {}", bound.mode))
    }

    /// Whether the typeshed stub for `callee` declares a concrete return
    /// length of exactly 1.
    fn is_typeshed_scalar_reduction(&self, callee: &str) -> bool {
        let Some(sig) = self.resolve_typeshed_sig(callee) else {
            return false;
        };
        matches!(
            &sig.return_,
            ReturnSpec::Concrete(rt) if rt.length == "1" && rt.class.is_empty()
        )
    }
}
#[cfg(test)]
mod scalar_reduction_tests {

    /// Representative functions whose typeshed stub declares return length 1.
    /// If the stubs are updated to change any of these, the test surfaces it.
    #[test]
    fn known_scalar_reductions_fire_ry105() {
        fn fires(src: &str, code: &str) -> bool {
            let file = crate::tests::parse_file("t.R", src);
            let mut checker = crate::Checker::new("t.R");
            checker.check(&file);
            checker.take_diagnostics().iter().any(|d| d.code == code)
        }
        for callee in ["sum", "any", "all", "length", "isTRUE", "identical"] {
            let args = match callee {
                "identical" => "1L, 1L",
                _ => "1L",
            };
            let src = format!("if (length({callee}({args})) > 0) 1\n");
            assert!(
                fires(&src, "RY105"),
                "`{callee}` has return length 1 in the typeshed but RY105 did not fire"
            );
        }
    }
}
