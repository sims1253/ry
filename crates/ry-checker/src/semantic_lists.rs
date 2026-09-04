//! Registry of hardcoded semantic lists.
//!
//! Every hardcoded list of R names used for semantic decisions is registered
//! here. Each entry states its claim and declares the single check that
//! validates it. The registry is the single source of truth: production code
//! reads from the constants defined here, so adding a member in only one
//! representation is a drift the coherence test catches.
//!
//! Three check kinds are permitted:
//!
//! - [`CheckKind::TypeshedAgreement`] — every item must exist in the embedded
//!   typeshed (base, rlang, …): a function stub, an ambient function, or an
//!   S3-generic global. Adding an item to the list without a matching
//!   typeshed entry fails the test.
//! - [`CheckKind::SiblingEquality`] — the list must equal another
//!   representation of the same data within the codebase.
//! - [`CheckKind::ROracle`] — the list must match R's own behaviour, verified
//!   by running `Rscript --vanilla`. Adding an item R does not recognise as
//!   part of the asserted group fails the test.

/// How a registered semantic list is validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    /// Every item exists in the embedded typeshed (base, rlang, …).
    TypeshedAgreement,
    /// The list equals another representation of the same data.
    SiblingEquality,
    /// The list matches R's own behaviour, verified by running Rscript.
    ROracle,
}

/// A registered hardcoded semantic list.
#[derive(Debug)]
pub struct SemanticList {
    /// Stable identifier used in test output and the coherence test.
    pub name: &'static str,
    /// The items in the list. Production code references this slice.
    pub items: &'static [&'static str],
    /// The single check that validates this list.
    pub check: CheckKind,
    /// A human-readable statement of the list's semantic claim.
    pub claim: &'static str,
}

// ── Registered lists ──────────────────────────────────────────────────────

/// R operators that can appear as S3 generic names (e.g. `+.widget`).
///
/// Checked by R oracle: the set equals `getGroupMembers("Arith")` joined with
/// `getGroupMembers("Compare")` — the two operator groups R recognises for
/// S3 dispatch.
pub const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "^", "%%", "%/%", "==", "!=", "<", "<=", ">", ">=",
];

/// R foreign-function-interface primitives.
///
/// Re-exported from ry-core. The typeshed-agreement coherence test is
/// registered below.
pub use ry_core::FFI_PRIMITIVES;

/// Containers whose named arguments become named elements of the result, so
/// `<-` typed where `=` was meant silently drops the name.
///
/// Checked by R oracle: each function preserves argument names in its output.
pub const NAME_CARRYING_CONTAINERS: &[&str] = &["list", "c", "data.frame", "structure"];

/// `data.frame()` arguments that are metadata, not columns.
///
/// Checked by R oracle: the set equals the non-`...` parameter names of
/// `formals(data.frame)`.
pub const METADATA_ARGS: &[&str] = &[
    "row.names",
    "check.rows",
    "check.names",
    "stringsAsFactors",
    "fix.empty.names",
];

/// Functions whose ordinary calls dispatch through the S3 `Math` group
/// generic, so `abs(x)` can hit a `Math.foo` method (the `Ops` group is
/// operator syntax, handled in `infer/binop.rs`).
///
/// Checked by typeshed agreement: every member exists in the embedded base
/// typeshed -- most in the `functions` map, with `acosh`, `asinh`, and
/// `atanh` declared as ambient functions. The set is the subset of R's
/// `Math` group that ry models; R additionally routes `signif`, the
/// `cum*` family, and the `*pi`/`digamma`/`trigamma` members through the
/// group, which ry does not model.
pub const S3_MATH_GENERICS: &[&str] = &[
    "abs", "acos", "acosh", "asin", "asinh", "atan", "atanh", "ceiling", "cos", "cosh", "exp",
    "expm1", "floor", "gamma", "lgamma", "log", "log10", "log1p", "log2", "round", "sign", "sin",
    "sinh", "sqrt", "tan", "tanh", "trunc",
];

/// Functions whose ordinary calls dispatch through the S3 `Summary` group
/// generic, so `sum(x)` can hit a `Summary.foo` method. This is exactly
/// R's `Summary` group.
///
/// Checked by typeshed agreement: every member exists in the embedded base
/// typeshed's `functions` map.
pub const S3_SUMMARY_GENERICS: &[&str] = &["all", "any", "max", "min", "prod", "range", "sum"];

/// The four S3 group-generic group names, i.e. the method-name prefixes
/// (`Ops.foo`, `Math.foo`, ...) whose definitions register as group
/// methods rather than needing the first-parameter heuristic.
///
/// Checked by typeshed agreement: every name is declared in the embedded
/// base typeshed's `globals.s3_generics`.
pub const GROUP_GENERICS: &[&str] = &["Ops", "Math", "Summary", "matrixOps"];

/// Whether `name` is one of the S3 group-generic group names.
pub(crate) fn is_group_generic(name: &str) -> bool {
    GROUP_GENERICS.contains(&name)
}

/// S7 constructor entry points: calls whose result is a callable S7
/// class, generic, or S3-interop class object, so the bound variable is
/// callable even though it is not a function literal.
///
/// Checked by R oracle: all three are exported by the S7 package (the
/// check skips when S7 is not installed).
pub const S7_OBJECT_CONSTRUCTORS: &[&str] =
    &["S7::new_class", "S7::new_generic", "S7::new_S3_class"];

/// Call constructors that quote their language arguments: a formula, an
/// expression vector, or a tidyselect column specification. Names inside
/// them resolve later in a model/data environment, not at construction
/// time.
///
/// Checked by R oracle: each member accepts a bare, undefined symbol
/// without evaluating it (`~` and `expression` in vanilla R; `vars` via
/// ggplot2, skipped when ggplot2 is not installed).
pub const QUOTING_FORMS: &[&str] = &["~", "expression", "vars"];

/// Whether `name` constructs a quoted language object (formula,
/// expression vector, or tidyselect column specification).
pub(crate) fn is_quoting_form(name: &str) -> bool {
    QUOTING_FORMS.contains(&name)
}

/// Bindings injected for Shiny application server fragments.
///
/// Checked by R oracle: these are the conventional parameters of a Shiny
/// server function, as documented by the `shiny` package.
pub const BUILTIN_ENVIRONMENT_BINDINGS: &[&str] = &["input", "output", "session"];

/// The complete registry. Every hardcoded semantic list must appear here.
pub fn registry() -> Vec<SemanticList> {
    vec![
        SemanticList {
            name: "OPERATORS",
            items: OPERATORS,
            check: CheckKind::ROracle,
            claim: "R operators usable as S3 generic names (Arith + Compare groups)",
        },
        SemanticList {
            name: "FFI_PRIMITIVES",
            items: FFI_PRIMITIVES,
            check: CheckKind::TypeshedAgreement,
            claim: "R foreign-function-interface primitives",
        },
        SemanticList {
            name: "NAME_CARRYING_CONTAINERS",
            items: NAME_CARRYING_CONTAINERS,
            check: CheckKind::ROracle,
            claim: "containers whose named arguments become named elements",
        },
        SemanticList {
            name: "METADATA_ARGS",
            items: METADATA_ARGS,
            check: CheckKind::ROracle,
            claim: "data.frame parameters that are not columns",
        },
        SemanticList {
            name: "BUILTIN_ENVIRONMENT_BINDINGS",
            items: BUILTIN_ENVIRONMENT_BINDINGS,
            check: CheckKind::ROracle,
            claim: "Shiny server function parameters",
        },
        SemanticList {
            name: "S3_MATH_GENERICS",
            items: S3_MATH_GENERICS,
            check: CheckKind::TypeshedAgreement,
            claim: "functions whose calls dispatch through the S3 Math group generic",
        },
        SemanticList {
            name: "S3_SUMMARY_GENERICS",
            items: S3_SUMMARY_GENERICS,
            check: CheckKind::TypeshedAgreement,
            claim: "functions whose calls dispatch through the S3 Summary group generic",
        },
        SemanticList {
            name: "GROUP_GENERICS",
            items: GROUP_GENERICS,
            check: CheckKind::TypeshedAgreement,
            claim: "S3 group-generic group names declared by the base stub globals",
        },
        SemanticList {
            name: "S7_OBJECT_CONSTRUCTORS",
            items: S7_OBJECT_CONSTRUCTORS,
            check: CheckKind::ROracle,
            claim: "S7 package constructors returning callable class or generic objects",
        },
        SemanticList {
            name: "QUOTING_FORMS",
            items: QUOTING_FORMS,
            check: CheckKind::ROracle,
            claim: "call constructors that quote their language arguments",
        },
    ]
}

/// Extract the bare function name from a potentially qualified callee,
/// stripping any `pkg::` or `pkg:::` prefix.
pub(crate) fn bare_name(name: &str) -> &str {
    name.rsplit_once("::").map(|(_, bare)| bare).unwrap_or(name)
}

/// Whether a `pkg::` or `pkg:::` prefix on `name` denotes the `base` package.
pub(crate) fn is_base_qualified(name: &str) -> bool {
    name.rsplit_once("::")
        .is_some_and(|(pkg, _)| pkg.trim_end_matches(':') == "base")
}
