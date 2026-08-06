//! Recall rules from plan 31 workstream W18 (RY102, RY103, RY105).
//!
//! These codes exist to catch real defects the 62-package Posit corpus audit
//! found and 0.8.0 missed. They are grouped here because they share a
//! property that the rest of the checker does not: each is decided by the
//! *shape* of an expression rather than by the inferred type of a value, so
//! their false-positive surface is bounded by the syntax they match.
//!
//! `docs/plans/repro/31/fn.R` holds one reproduction per rule and
//! `tests/plan31_recall_rules.rs` pins both the positive and the negative
//! direction of each.
//!
//! Two of the plan's sketches are deliberately **not** implemented.
//!
//! `not-before-comparison` was premised on `!x >= y` parsing as
//! `(!x) >= y`, but R's `?Syntax` places unary `!` *below* the comparison
//! operators, so it parses as `!(x >= y)` and the flagged code is correct.
//! That is the precise model error that retired `RY095` in 0.4.1.
//!
//! `constant-condition`'s `any(v) == 0` half (glue `R/utils.R:32`) is a real
//! bug — `any(lengths == 0)` was meant — but the plan's justification for
//! flagging it, "is always FALSE", is wrong: `any()` yields a logical and
//! `FALSE == 0` is `TRUE`. The shape is also indistinguishable from
//! diffobj's legitimate `!all(diff(x)) == 1L`, pinned as must-stay-silent in
//! `testdata/ry095_ry096_real_shapes.R`. The false negative stays open
//! rather than being traded for a false-positive source. Its second half
//! (`length(sum(...)) > 0`) is decidable and ships as RY105.

use super::*;

/// Containers whose arguments become *named elements* of the result, so a
/// `<-` typed where `=` was meant silently drops the name. Restricted to
/// this family on purpose: `local(x <- 1)`, `suppressWarnings(x <- f())`
/// and every user function take an ordinary assignment as an argument
/// without losing anything.
const NAME_CARRYING_CONTAINERS: &[&str] = &["list", "c", "data.frame", "structure"];

/// Base functions whose result is length 1 for every input, so `length()`
/// of a call to one of them is the constant 1. Kept to reductions whose
/// documented value is a single number; anything vectorised (`nchar`,
/// `range`, `which`) is excluded.
pub(crate) const SCALAR_REDUCTIONS: &[&str] = &[
    "sum",
    "prod",
    "mean",
    "median",
    "length",
    "NROW",
    "NCOL",
    "nlevels",
    "any",
    "all",
    "isTRUE",
    "isFALSE",
    "identical",
];

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
    Some(name.rsplit_once("::").map(|(_, bare)| bare).unwrap_or(name))
}

/// A call to `f(x)` with exactly one positional argument.
fn unary_call_to(expr: &Expr, callee: &str) -> bool {
    let Expr::Call { args, .. } = expr else {
        return false;
    };
    bare_callee(expr) == Some(callee) && args.len() == 1 && args[0].name.is_none()
}

/// The integer value of a whole-number numeric literal.
fn numeric_literal(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Integer(value, _) => Some(*value as f64),
        Expr::Double(value, _) => Some(*value),
        _ => None,
    }
}

/// The name a `<-` inside a container argument would have produced had `=`
/// been typed instead. Only a bare identifier or a string literal qualifies:
/// `list(x[[1]] <- 2)` and `list(names(y) <- z)` are replacement functions,
/// not mistyped names.
fn mistyped_element_name(target: &Expr) -> Option<(&str, String)> {
    match target {
        Expr::Ident { name, .. } => Some((name.as_str(), name.clone())),
        // A string LHS keeps its quotes in the suggestion: `github-ref` is
        // not a syntactic name, so `"github-ref" = ...` is the only fix that
        // parses.
        Expr::String(name, _) => Some((name.as_str(), format!("\"{name}\""))),
        _ => None,
    }
}

/// Strip any leading unary `!` operators, returning the negated expression.
fn strip_negation(expr: &Expr) -> &Expr {
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
    pub(crate) fn check_named_element_arrow(&mut self, func: &Expr, args: &[Arg]) {
        let name = match func {
            Expr::Ident { name, .. } | Expr::String(name, _) => name.as_str(),
            _ => return,
        };
        let lookup_name = name.rsplit_once("::").map(|(_, bare)| bare).unwrap_or(name);
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
            let Some((name, spelling)) = mistyped_element_name(lhs) else {
                continue;
            };
            if matches!(lhs.as_ref(), Expr::Ident { .. }) && !builds_named_structure {
                continue;
            }
            self.emit(
                Severity::Warning,
                *span,
                "RY102",
                format!(
                    "`<-` inside `{lookup_name}()` assigns `{name}` and leaves the element unnamed; write `{spelling} = ...` to name it"
                ),
            );
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
    pub(crate) fn check_class_equality_operand(&mut self, expr: &Expr) {
        let Expr::BinOp {
            op: op @ (BinOpKind::Eq | BinOpKind::Ne),
            lhs,
            rhs,
            span,
        } = strip_negation(expr)
        else {
            return;
        };
        if !unary_call_to(lhs, "class") && !unary_call_to(rhs, "class") {
            return;
        }
        let suggestion = if matches!(op, BinOpKind::Eq) {
            "inherits(x, class)"
        } else {
            "!inherits(x, class)"
        };
        self.emit(
            Severity::Warning,
            *span,
            "RY103",
            format!(
                "`class()` returns a character vector, so this comparison is not length-1 for a multi-class object and the enclosing condition errors; use `{suggestion}`"
            ),
        );
    }

    /// RY105: `length(x) <op> 0` where `x` is length 1 by construction, as in
    /// pak `R/confirmation.R:42` — `length(sum(...)) > 0`.
    ///
    /// The guard reads as an emptiness check but its operand can never be
    /// empty, so the branch is dead (or, for `== 0`, unreachable). Only the
    /// literal `0` is flagged: comparisons against `1` (`length(x) == 1`) are
    /// deliberate scalar assertions, which assertion helpers write on purpose.
    ///
    /// "Length 1 by construction" means one of two things, both chosen so the
    /// claim does not rest on inference that could be over-narrow (the failure
    /// mode plan 31 files as W12):
    ///
    /// 1. a direct call to a base reduction whose documented value is a single
    ///    number ([`SCALAR_REDUCTIONS`]); or
    /// 2. a local binding whose inferred type is a length-1 *atomic* and which
    ///    is neither a parameter, a parameter default, nor a flow-narrowed
    ///    refinement — a parameter's type comes from one default or one call
    ///    site and is not proof of the runtime length.
    pub(crate) fn check_constant_length_comparison(
        &mut self,
        op: BinOpKind,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        scope: &Scope,
    ) {
        if !matches!(
            op,
            BinOpKind::Eq
                | BinOpKind::Ne
                | BinOpKind::Lt
                | BinOpKind::Le
                | BinOpKind::Gt
                | BinOpKind::Ge
        ) {
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
        let (measured, measured_on_left) = match (length_operand(lhs), length_operand(rhs)) {
            (Some(measured), None) if numeric_literal(rhs) == Some(0.0) => (measured, true),
            (None, Some(measured)) if numeric_literal(lhs) == Some(0.0) => (measured, false),
            _ => return,
        };
        let Some(reason) = self.scalar_by_construction(measured, scope) else {
            return;
        };
        // Normalize to `length(...) <op> 0` by mirroring the operator when
        // the zero literal is on the left side of the comparison.
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
        let outcome = match effective_op {
            BinOpKind::Eq | BinOpKind::Lt | BinOpKind::Le => "FALSE",
            _ => "TRUE",
        };
        self.emit(
            Severity::Warning,
            span,
            "RY105",
            format!(
                "{reason}, so `length(...)` is 1 here and this zero-length guard is always {outcome}"
            ),
        );
    }

    /// Why `expr` is length 1 for every input, or `None` when that cannot be
    /// established from construction alone. See
    /// [`Checker::check_constant_length_comparison`] for the two admitted
    /// forms.
    fn scalar_by_construction(&self, expr: &Expr, scope: &Scope) -> Option<String> {
        if let Some(callee) = bare_callee(expr)
            && SCALAR_REDUCTIONS.contains(&callee)
            && !scope.is_lexical_function(callee)
            && matches!(expr, Expr::Call { args, .. } if !args.is_empty())
        {
            return Some(format!("`{callee}()` always returns a single value"));
        }
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
        Some(format!("`{name}` is a length-1 {}", bound.mode))
    }
}

#[cfg(test)]
mod scalar_reductions_tests {
    use super::*;

    /// Every entry in [`SCALAR_REDUCTIONS`] must cause RY105 to fire when used
    /// inside `length(f(x)) > 0`. A stale entry (a function that was removed
    /// from the list or renamed) is a dead match arm: the rule silently
    /// ignores it and the author never notices.
    ///
    /// This test does NOT verify the semantic claim ("returns length-1 for all
    /// inputs") — that requires R runtime knowledge. Its value is making the
    /// list visible and ensuring its mechanism works for every member, so a
    /// careless addition or removal is at least noticed at test time.
    #[test]
    fn every_scalar_reduction_fires_ry105() {
        fn fires(src: &str, code: &str) -> bool {
            let mut parser = ry_core::RParser::new().expect("parser init");
            let file = parser.parse("t.R", src).expect("parse");
            let mut checker = crate::Checker::new("t.R");
            checker.check(&file);
            checker.take_diagnostics().iter().any(|d| d.code == code)
        }
        for &callee in SCALAR_REDUCTIONS {
            let src = format!(
                "f <- function(x) if (length({callee}(x)) > 0) 1
"
            );
            assert!(
                fires(&src, "RY105"),
                "`{callee}` is in SCALAR_REDUCTIONS but did not fire RY105"
            );
        }
    }
}
