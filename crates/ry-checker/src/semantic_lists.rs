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
//!   typeshed (base, rlang, …). Adding an item to the list without a matching
//!   typeshed stub fails the test.
//! - [`CheckKind::SiblingEquality`] — the list must equal another
//!   representation of the same data within the codebase.
//! - [`CheckKind::ROracle`] — the list must match R's own behaviour, verified
//!   by running `Rscript --vanilla`. Adding an item R does not recognise as
//!   part of the asserted group fails the test.
//!
//! See `docs/plans/35-invariants-over-examples.md`, P35-W7.

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

/// Functions that capture an argument as a promise without forcing it.
///
/// Checked by typeshed agreement: every base member (`expression`, `quote`,
/// `substitute`, `bquote`, `alist`) exists in the embedded base typeshed;
/// every rlang member exists in the rlang vendor stub.
pub const DEFUSING_CALLS: &[&str] = &[
    "expression",
    "quote",
    "substitute",
    "bquote",
    "alist",
    "expr",
    "exprs",
    "quo",
    "quos",
    "enexpr",
    "enquo",
    "ensym",
    "enquos",
    "ensyms",
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
            name: "DEFUSING_CALLS",
            items: DEFUSING_CALLS,
            check: CheckKind::TypeshedAgreement,
            claim: "base and rlang functions that capture without forcing",
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
