//! Test-template result modes and RY106 (ifelse-mode-collapse).
//!
//! `base::ifelse` builds its result from the `test` vector itself and only
//! overwrites the positions the test selects:
//!
//! ```r
//! ans <- test
//! ans[test] <- yes[test]
//! ans[!test & !is.na(test)] <- no[...]
//! ```
//!
//! Two shapes of test therefore leave the result in `logical` mode even
//! when both branches agree on another mode (runtime-verified from the
//! hms audit, tidyverse/hms#231):
//!
//! * a zero-length test overwrites nothing, so
//!   `ifelse(logical(0), NA_character_, "a")` is `logical(0)`; and
//! * an all-`NA` test selects nothing, so `ifelse(NA, "a", "b")` is the
//!   logical `NA`.
//!
//! A *mixed* test (`c(TRUE, NA)`) does not collapse: the subset
//! assignments coerce the whole vector up to the branch mode. Both
//! definite halves therefore rest on the *expression's shape*, never on
//! the type lattice: the all-`NA` half is claimed only for a literal
//! `NA` (or a comparison with one), and the empty half only for a
//! literal empty construction (`logical(0)`, `NULL`, ...). Inferred
//! facts are not definite proofs — a merely maybe-`NA` test (`x > 0`
//! for an NA-capable `x`) is usually NA-free, and inferred
//! `len = 0` types are pre-existing join artifacts on values that are
//! not empty at runtime (googledrive `R/drive_mime_type.R:56`,
//! testthat `R/parallel-taskq.R:194`). The *possible* half is sound
//! where the lattice says maybe-empty ([`Length::may_be_empty`], with
//! the open-world widening of [`Checker::test_may_be_empty`]).
//!
//! The warning needs more than a possible collapse: either the collapse
//! is definite (one of the literal forms above), or one branch is a
//! typed `NA` constant (`NA_character_`, `NA_real_`, ...) of the shared
//! branch mode. The typed NA is the author's written-down mode intent,
//! which is what the collapse silently betrays; without that restriction
//! every open-world `ifelse(x, 1, 0)` in a mutate pipeline would warn.
//!
//! The stub-driven half lives in the typeshed: the `ifelse` entry declares
//! a `return_mode: test_template` rule (the mode-dimension analog of
//! `seq_len`'s `return_length: param_value`), which this module applies
//! in place of the plain `yes_or_no` join when the test may collapse.

use super::recall::{numeric_literal, strip_negation};
use super::*;

/// Everything RY106 needs about one test-template call site, computed
/// once by [`Checker::infer_test_template_call`].
struct TestTemplateSite<'a> {
    lookup_name: &'a str,
    /// The test is an empty vector or all-`NA` *by construction* (a
    /// literal form): the result is the (logical) test vector itself.
    definite_collapse: bool,
    definitely_na: bool,
    /// The test argument may be empty at runtime (open-world widening
    /// applied; see [`Checker::test_may_be_empty`]).
    may_be_empty: bool,
    /// A branch is a typed `NA` constant of the shared branch mode: the
    /// author's written-down mode intent.
    typed_na_branch: bool,
    /// The non-logical atomic mode every branch agrees on, if any.
    branch_mode: Option<Mode>,
}

/// The single non-logical atomic mode shared by every value branch, when
/// the branches agree on one. Logical branches lose nothing, `NULL`,
/// opaque, and disagreeing branches have no one mode to report.
fn shared_branch_mode(value_types: &[&RType]) -> Option<Mode> {
    let branch_mode = value_types.first()?.mode;
    if is_atomic_branch_mode(branch_mode) && value_types.iter().all(|ty| ty.mode == branch_mode) {
        Some(branch_mode)
    } else {
        None
    }
}

impl Checker {
    /// The test-template stage: applies a signature's `return_mode`
    /// `test_template` rule to the call's result type and reports RY106
    /// when both value branches agree on a non-logical atomic mode that
    /// the result can silently lose.
    ///
    /// Returns `None` when the rule cannot decide (unmatched arguments,
    /// or a signature without the spec), leaving the generic typeshed
    /// stage to apply the declared `yes_or_no` join unchanged.
    pub(crate) fn infer_test_template_call(
        &mut self,
        lookup_name: &str,
        signature: &FunctionSig,
        args: &[Arg],
        arg_types: &[RType],
        scope: &Scope,
        span: Span,
    ) -> Option<RType> {
        let Some(ry_typeshed::ReturnModeSpec::TestTemplate { test, values }) =
            &signature.return_mode
        else {
            return None;
        };
        let bindings = match_params(&signature.params, args);
        let bound = |param: &str| -> Option<usize> {
            bound_argument_index_matched(&signature.params, &bindings, param)
        };
        let test_index = bound(test)?;
        let value_indices: Vec<usize> = values.iter().map(|v| bound(v)).collect::<Option<_>>()?;
        let test_expr = &args[test_index].value;
        let test_ty = arg_types.get(test_index)?;
        let value_types: Vec<&RType> = value_indices
            .iter()
            .map(|index| arg_types.get(*index))
            .collect::<Option<_>>()?;
        let value_exprs: Vec<&Expr> = value_indices
            .iter()
            .map(|index| &args[*index].value)
            .collect();

        let may_be_empty = self.test_may_be_empty(test_expr, test_ty, scope);
        let definitely_na = test_definitely_na(test_expr);
        let definitely_empty = self.test_definitely_empty(test_expr, scope);
        let definite_collapse = definitely_na || definitely_empty;
        let branch_mode = shared_branch_mode(&value_types);
        let result = self.apply_sig(signature, arg_types, args);

        let site = TestTemplateSite {
            lookup_name,
            definite_collapse,
            definitely_na,
            may_be_empty,
            typed_na_branch: value_exprs
                .iter()
                .any(|expr| matches!(expr, Expr::Na(ty, _) if Some(ty.mode) == branch_mode)),
            branch_mode,
        };
        self.check_test_template_mode_collapse(&site, span);

        if !may_be_empty && !definite_collapse {
            return Some(result);
        }
        if definite_collapse {
            // Nothing is overwritten at all: the result IS the (logical)
            // test vector.
            return Some(RType {
                mode: Mode::Logical,
                ..result
            });
        }
        // A maybe-empty test yields the branch join when nonempty and a
        // logical vector when empty — but only when the emptiness is
        // open-world knowledge (an unknown length, or a pinned length on
        // an open-world binding). A pinned `Zero` length on a computed
        // expression keeps the plain branch join: inferred len=0 facts
        // are join artifacts as often as real emptiness, and rewriting
        // them to logical poisons downstream comparisons (testthat
        // `R/parallel-taskq.R:194`).
        if is_atomic_branch_mode(result.mode)
            && !(matches!(test_ty.length, Length::Zero)
                && !self.test_binding_is_open_world(test_expr, scope))
        {
            let collapsed = RType::new(Mode::Logical, result.length);
            return Some(RType::union(std::sync::Arc::from([collapsed, result])));
        }
        Some(result)
    }

    /// RY106: warn when the result can collapse to `logical` although
    /// `yes`/`no` agree on a non-logical atomic mode — the typed-NA select
    /// `ifelse(is.na(x), NA_character_, <character>)` that returns
    /// `logical(0)` for empty `x`, which is why `dplyr::if_else()` and
    /// `vctrs::vec_if_else()` (vctrs >= 0.7.0) exist. There is no
    /// `vctrs::if_else()`.
    ///
    /// Two premises are admitted:
    ///
    /// * the collapse is *definite*: the test is an empty vector by
    ///   construction (`logical(0)`, `NULL`) or definitely all-`NA` (a
    ///   literal form); or
    /// * the collapse is *possible* (the test may be empty) and a branch
    ///   is a typed `NA` constant of the shared mode. The typed NA is the
    ///   author writing the expected mode down; without it, `ifelse(x, 1,
    ///   0)`-style calls would warn at every open-world test — the
    ///   raw-site volume the rule must not add to (the mutate pipelines in
    ///   `ok_dplyr_unknown_schema_data_mask.R` are the pinned shape).
    fn check_test_template_mode_collapse(&mut self, site: &TestTemplateSite<'_>, span: Span) {
        let Some(branch_mode) = site.branch_mode else {
            return;
        };
        if !site.definite_collapse && !(site.may_be_empty && site.typed_na_branch) {
            return;
        }
        let reason = if site.definitely_na {
            "an all-`NA` test"
        } else {
            "an empty test"
        };
        self.emit(
            Severity::Warning,
            span,
            "RY106",
            format!(
                "`{}()` builds its result from `test`, so {reason} leaves a logical result even though the branches are both {branch_mode}; use a typed alternative such as `dplyr::if_else()` when the result must be {branch_mode}",
                site.lookup_name
            ),
        );
    }

    /// Whether the test argument of a test-template call may be empty at
    /// runtime. `Length::may_be_empty` is the length-lattice answer, with
    /// one open-world widening: a pinned nonempty length on a parameter,
    /// parameter default, or narrowed binding describes one observed
    /// shape, not every caller's — the same rule scalar-guards.md applies
    /// to scalar defaults (a scalar default does not prove a scalar
    /// input). Locals keep their pinned lengths.
    pub(crate) fn test_may_be_empty(&self, expr: &Expr, ty: &RType, scope: &Scope) -> bool {
        ty.length.may_be_empty() || self.test_binding_is_open_world(expr, scope)
    }

    /// Whether `expr` names a binding whose pinned type must not serve as
    /// an emptiness proof: a parameter, a parameter default, or a
    /// flow-narrowed refinement. Parameters and defaults describe one
    /// observed call shape, not every caller's input. A narrowed binding
    /// is distrusted for the reason scalar-guards.md gives: its pinned
    /// length usually comes from a `length()` guard, and `length` is a
    /// generic whose dispatch to a `length.*` method the checker does not
    /// track (#372) — the same exclusions RY105 applies to
    /// scalar-by-construction proofs.
    pub(crate) fn test_binding_is_open_world(&self, expr: &Expr, scope: &Scope) -> bool {
        matches!(
            expr,
            Expr::Ident { name, .. }
                if scope.parameter_bindings.contains(name)
                    || scope.default_parameter_bindings.contains(name)
                    || scope.narrowed_bindings.contains(name)
        )
    }

    /// Whether the test expression is an empty vector *by construction*:
    /// a literal `NULL`, or a direct call to a base atomic-vector
    /// constructor whose only (or absent) length argument is a literal
    /// zero — `logical(0)`, `character()`, `integer(length = 0)`.
    ///
    /// An inferred `Length::Zero` on any other expression does NOT
    /// qualify: parameter refinement and index/subset joins already
    /// produce zero-length artifacts for values that are not empty at
    /// runtime (googledrive `R/drive_mime_type.R:56` refines `type` to
    /// `character<len=0>`; testthat `R/parallel-taskq.R:194` infers
    /// `logical<len=0>` for an indexed subset), so the definite premise
    /// rests on the expression's shape, exactly the discipline
    /// [`test_definitely_na`] applies to the NA half.
    pub(crate) fn test_definitely_empty(&self, expr: &Expr, scope: &Scope) -> bool {
        let Expr::Call { func, args, .. } = expr else {
            return matches!(expr, Expr::Null(..));
        };
        let Some(name) = ident_name(func) else {
            return false;
        };
        let bare = crate::semantic_lists::bare_name(name);
        if !matches!(
            bare,
            "logical" | "integer" | "double" | "numeric" | "complex" | "character" | "raw"
        ) {
            return false;
        }
        // A shadowed constructor may have unrelated semantics.
        if !self.resolves_to_base_lenient(name, scope) {
            return false;
        }
        // `logical()` defaults to length 0; a single positional or
        // `length`/`length.out` named argument must be a literal zero.
        match args.as_slice() {
            [] => true,
            [argument] => {
                let unnamed_or_length = argument
                    .name
                    .as_deref()
                    .is_none_or(|name| matches!(name, "length" | "length.out"));
                unnamed_or_length && numeric_literal(&argument.value) == Some(0.0)
            }
            _ => false,
        }
    }
}

/// Whether a test expression is definitely all-`NA` for every input, the
/// second shape that collapses a test-template result to logical.
/// Deliberately narrow: only a literal `NA` (under any number of `!`) and
/// comparisons against one qualify. A test that merely *may* contain an
/// `NA` usually does not, and a mixed test does not collapse at all, so
/// anything less certain stays silent.
pub(crate) fn test_definitely_na(expr: &Expr) -> bool {
    match strip_negation(expr) {
        Expr::Na(..) => true,
        Expr::BinOp { op, lhs, rhs, .. } if is_comparison(*op) => {
            matches!(lhs.as_ref(), Expr::Na(..)) || matches!(rhs.as_ref(), Expr::Na(..))
        }
        _ => false,
    }
}

/// The branch modes whose loss to a logical collapse is observable and
/// expressible: every atomic mode except logical itself. `NULL` branches
/// carry no mode to protect, and opaque/union branches are not precise
/// enough to claim either way.
fn is_atomic_branch_mode(mode: Mode) -> bool {
    matches!(
        mode,
        Mode::Integer | Mode::Double | Mode::Complex | Mode::Character | Mode::Raw
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(src: &str) -> Vec<Diagnostic> {
        let file = crate::tests::parse_file("ifelse.R", src);
        let mut checker = Checker::new("ifelse.R");
        checker.check(&file);
        checker.take_diagnostics()
    }

    fn fires(src: &str) -> bool {
        check(src).iter().any(|d| d.code == "RY106")
    }

    fn binding(src: &str, name: &str) -> RType {
        let file = crate::tests::parse_file("ifelse.R", src);
        let mut checker = Checker::new("ifelse.R");
        let (_, scope) = checker.check_with_scope(&file);
        scope.get(name).expect("binding exists").clone()
    }

    #[test]
    fn fires_on_the_hms_shape_with_an_open_world_test() {
        // tidyverse/hms#231 pre-fix: `as.character(hms())` returned
        // `logical(0)` because the test is empty for empty `x`.
        assert!(fires(
            "format_hms <- function(x) {\n  ifelse(is.na(x), NA_character_, paste0('00:', x))\n}\n"
        ));
    }

    #[test]
    fn fires_on_literal_empty_and_na_tests() {
        assert!(fires("a <- ifelse(logical(0), NA_character_, 'a')\n"));
        assert!(fires("b <- ifelse(NA, 'a', 'b')\n"));
    }

    #[test]
    fn fires_on_other_literal_empty_constructions() {
        assert!(fires("a <- ifelse(NULL, 'a', 'b')\n"));
        assert!(fires("a <- ifelse(character(), 'a', 'b')\n"));
        assert!(fires("a <- ifelse(integer(length = 0), 1L, 2L)\n"));
    }

    #[test]
    fn inferred_zero_length_is_not_a_definite_empty_proof() {
        // googledrive R/drive_mime_type.R:56 / testthat
        // R/parallel-taskq.R:194: call-site joins and index inference
        // produce len=0 artifacts for values that are not empty at
        // runtime, so an inferred Zero neither warns nor rewrites the
        // result mode.
        let src = "v <- c()[1]\na <- ifelse(v > 0, 'pos', 'neg')\n";
        assert!(!fires(src));
        let ty = binding(src, "a");
        assert_eq!(
            ty.mode,
            Mode::Character,
            "inferred-Zero keeps the branch join"
        );
        // A typed-NA branch still fires on a maybe-empty test: the Zero
        // length does say the value may be empty.
        assert!(fires(
            "v <- c()[1]\na <- ifelse(v > 0, 'pos', NA_character_)\n"
        ));
    }

    #[test]
    fn fires_on_a_parameter_test_with_a_typed_na_branch() {
        assert!(fires(
            "f <- function(x) ifelse(x > 0, 'pos', NA_character_)\n"
        ));
        assert!(fires("f <- function(x) ifelse(is.na(x), NA_real_, 1)\n"));
    }

    #[test]
    fn fires_for_named_and_qualified_calls() {
        assert!(fires(
            "f <- function(x) base::ifelse(test = x, yes = NA_character_, no = 'b')\n"
        ));
    }

    #[test]
    fn fires_when_only_the_default_pins_a_nonempty_test() {
        // The open-world rule scalar-guards.md applies to scalar defaults:
        // a default that happens to be nonempty does not bind callers.
        assert!(fires(
            "f <- function(flag = c(TRUE, FALSE)) ifelse(flag, NA_character_, 'b')\n"
        ));
    }

    #[test]
    fn stays_silent_for_open_world_tests_without_mode_intent() {
        // No typed NA branch means no written-down mode to lose; warning
        // here would cover every mutate pipeline (pinned by
        // ok_dplyr_unknown_schema_data_mask.R).
        assert!(!fires("f <- function(x) ifelse(x > 0, 'pos', 'neg')\n"));
        assert!(!fires("f <- function(x) ifelse(x, 1L, 2L)\n"));
        assert!(!fires("f <- function(x) ifelse(!is.na(x), 1, 0)\n"));
    }

    #[test]
    fn stays_silent_for_local_nafree_tests_of_known_length() {
        // `v` is a local of known length 3, so the test cannot be empty,
        // and a merely maybe-NA test is not an all-NA proof.
        assert!(!fires(
            "v <- c(-1, 0, 1)\na <- ifelse(v > 0, 'pos', 'neg')\nb <- ifelse(is.na(v), NA_character_, 'x')\n"
        ));
        assert!(!fires("a <- ifelse(TRUE, 'a', 'b')\n"));
        // A mixed test does not collapse: subset assignment coerces the
        // whole vector to the branch mode.
        assert!(!fires("a <- ifelse(c(TRUE, NA), 'a', 'b')\n"));
    }

    #[test]
    fn stays_silent_for_logical_or_disagreeing_branches() {
        assert!(!fires("f <- function(x) ifelse(x, TRUE, FALSE)\n"));
        assert!(!fires("f <- function(x) ifelse(x, 1L, 'a')\n"));
    }

    #[test]
    fn stays_silent_when_the_project_defines_ifelse() {
        assert!(!fires(
            "ifelse <- function(test, yes, no) test\na <- ifelse(logical(0), 'a', 'b')\n"
        ));
    }

    #[test]
    fn empty_test_infers_a_definite_logical_result() {
        let ty = binding("a <- ifelse(logical(0), NA_character_, 'a')\n", "a");
        assert_eq!(ty.mode, Mode::Logical);
        assert_eq!(ty.length, Length::Zero);
    }

    #[test]
    fn na_test_infers_a_definite_logical_result() {
        let ty = binding("b <- ifelse(NA, 'a', 'b')\n", "b");
        assert_eq!(ty.mode, Mode::Logical);
        assert_eq!(ty.length, Length::One);
    }

    #[test]
    fn maybe_empty_test_infers_the_logical_union() {
        // Inside the function `x` is open-world (maybe empty), so the
        // result is character for nonempty input and logical(0) for
        // empty input; the propagated return keeps both members.
        let ty = binding(
            "g <- function(x) ifelse(x > 0, 'pos', 'neg')\na <- g(c(1, 2))\n",
            "a",
        );
        assert_eq!(ty.mode, Mode::Union);
        let members = ty.members.expect("union carries members");
        assert_eq!(members.len(), 2);
        assert!(members.iter().any(|m| m.mode == Mode::Logical));
        assert!(members.iter().any(|m| m.mode == Mode::Character));
    }

    #[test]
    fn provably_nonempty_test_keeps_the_branch_join() {
        let ty = binding("v <- c(-1, 0, 1)\na <- ifelse(v > 0, 'pos', 'neg')\n", "a");
        assert_eq!(ty.mode, Mode::Character);
        assert_eq!(ty.length, Length::Known(3));
    }
}
