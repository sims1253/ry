//! R type lattice.
//!
//! Models what R's `typeof()` would return, plus a length dimension and a
//! flag for whether NA can occur. This is intentionally coarser than R's
//! actual semantics: S4 and RC are stubbed, environments are opaque, and
//! "unknown" is used whenever inference gives up (e.g. function call whose
//! signature is not in the typeshed).

use std::fmt;
use std::sync::Arc;

/// Atomic mode of an R vector, mirrors `typeof()` for vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Logical,
    Integer,
    Double,
    Complex,
    Character,
    Raw,
    /// `list(...)`
    List,
    /// `NULL`
    Null,
    /// Closure or builtin. Opaque payload for v1.
    Function,
    /// Environment, S4, external pointer, etc.
    Opaque,
    /// A union of two or more modes (control-flow merge of branches with
    /// incompatible types, e.g. `if (p) 1L else "a"`). The member list
    /// lives on `RType::members`, NOT on this variant (Mode is Copy and
    /// used as a HashMap key, so it carries no payload). `Mode::Union`
    /// is bypassed by the coercion ladder: `join` builds unions instead
    /// of promoting via `coerce_rank`.
    Union,
}

impl Mode {
    /// Coercion rank per R's implicit ladder. Higher wins.
    /// `NULL < logical < integer < double < complex < character`
    /// (`raw` does not coerce implicitly; `list` is its own world.)
    pub fn coerce_rank(self) -> i32 {
        match self {
            Mode::Null => -1,
            Mode::Logical => 0,
            Mode::Integer => 1,
            Mode::Double => 2,
            Mode::Complex => 3,
            Mode::Character => 4,
            Mode::Raw => 5,
            Mode::List => 6,
            Mode::Function => 7,
            Mode::Opaque => 8,
            // Unions deliberately bypass the coercion ladder; this rank is
            // only a sentinel so the match stays exhaustive. `join` handles
            // unions directly rather than via coerce_rank.
            Mode::Union => 9,
        }
    }

    /// Result of a binary arithmetic op between two atomic modes,
    /// following R's coercion rules. Returns None if the combination is
    /// not arithmetic-valid (e.g. list + double). Opaque (unknown) is
    /// permissive: an unknown operand cannot prove an error, so the
    /// result is also opaque rather than None.
    pub fn arith_result(self, other: Mode) -> Option<Mode> {
        use Mode::*;
        match (self, other) {
            // Opaque absorbs FIRST: opaque means "we don't know", so
            // `opaque + "x"` stays quiet (the opaque could be numeric).
            // Handling opaque before the rejections prevents a false RY040
            // on depth-capped / unknown operands.
            (Opaque, _) | (_, Opaque) => Some(Opaque),
            // Unions are distributed at the RType::arith layer; reaching
            // here with a Union operand is a bug. Treat conservatively as
            // opaque so a stray call doesn't panic.
            (Union, _) | (_, Union) => Some(Opaque),
            // Rejections: NULL paired with a non-arithmetic mode errors
            // (R: `NULL + "a"` -> "non-numeric argument"). Before this
            // reorder, the (Null, x) arm ran first and turned
            // `NULL + "a"` into Some(Character).
            (Character, _) | (_, Character) => None,
            (List, _) | (_, List) => None,
            (Function, _) | (_, Function) => None,
            // NULL paired with an arithmetic-valid mode yields that mode
            // (at length zero -- handled at the RType layer, which knows
            // about length; Mode has no length dimension).
            (Null, x) | (x, Null) => Some(x),
            (Raw, Raw) => Some(Raw),
            _ => {
                let r = self.coerce_rank().max(other.coerce_rank());
                Some(match r {
                    0 => Logical,
                    1 => Integer,
                    2 => Double,
                    3 => Complex,
                    _ => Double,
                })
            }
        }
    }

    /// Result of comparison op (`<`, `>`, `==`, ...). Always logical in R
    /// unless one side is a list / function, in which case it's an error.
    /// Opaque (unknown) is permissive: result is also opaque, not None.
    pub fn compare_result(self, other: Mode) -> Option<Mode> {
        use Mode::*;
        match (self, other) {
            (Opaque, _) | (_, Opaque) => Some(Opaque),
            (Union, _) | (_, Union) => Some(Opaque),
            (List, _) | (_, List) => None,
            (Function, _) | (_, Function) => None,
            _ => Some(Logical),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Mode::Logical => "logical",
            Mode::Integer => "integer",
            Mode::Double => "double",
            Mode::Complex => "complex",
            Mode::Character => "character",
            Mode::Raw => "raw",
            Mode::List => "list",
            Mode::Null => "NULL",
            Mode::Function => "function",
            Mode::Opaque => "opaque",
            Mode::Union => "union",
        };
        f.write_str(s)
    }
}

/// Length of a value, as far as inference can tell. R has no scalar type;
/// "scalars" are length-1 vectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    Zero,
    One,
    /// A specific known length greater than 1.
    Known(usize),
    /// Length unknown at compile time (function arg, dynamic computation).
    Unknown,
}

impl Length {
    /// Length of a binary op's result given the operands' lengths,
    /// following R's vector-recycling rule.
    pub fn binary(self, other: Length) -> Length {
        use Length::*;
        match (self, other) {
            (Zero, _) | (_, Zero) => Zero,
            (One, x) | (x, One) => x,
            // R recycles to max(a, b) (warning if neither divides the
            // other); we model both cases as Known(max). The previous
            // code had two identical branches here for the divides/does-
            // not-divide cases -- collapsed to one.
            (Known(a), Known(b)) => Known(a.max(b)),
            (Known(_), Unknown) | (Unknown, Known(_)) | (Unknown, Unknown) => Unknown,
        }
    }
}

/// S3 class attribute. Up to 4 class names. Class names are held as
/// `Arc<str>` so a runtime-derived name (e.g. from `structure(class =
/// my_var)`) does not require a global intern table; the `Arc` is cheap
/// to clone and is released when the owning `RType` is dropped.
///
/// The `known` flag distinguishes three states:
///   * `known == true`, `len == 0`: we know there is no class attribute.
///   * `known == true`, `len > 0`: we know the class vector.
///   * `known == false`: we couldn't determine the class (do not warn).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassVector {
    pub names: [Option<Arc<str>>; 4],
    pub len: u8,
    pub known: bool,
}

impl ClassVector {
    /// A value with no class attribute set. The class is *known* to be
    /// empty (e.g. a plain atomic literal); callers may rely on this to
    /// suppress RY050 since there's nothing to dispatch on.
    pub fn empty() -> Self {
        ClassVector {
            names: [None, None, None, None],
            len: 0,
            known: true,
        }
    }

    /// A single-class value, e.g. `structure(x, class = "foo")`.
    pub fn single(name: &str) -> Self {
        ClassVector {
            names: [Some(Arc::from(name)), None, None, None],
            len: 1,
            known: true,
        }
    }

    /// We couldn't determine the class at all (dynamic input). Used for
    /// `RType::unknown()` and any value whose class depends on unknown
    /// state. Distinguished from `empty()` so callers avoid emitting
    /// RY050 on values they can't reason about.
    pub fn unknown() -> Self {
        ClassVector {
            names: [None, None, None, None],
            len: 0,
            known: false,
        }
    }

    /// First class name, if any. R's S3 dispatch walks the class vector
    /// in order; for v1 we model only the first-element rule.
    pub fn first(&self) -> Option<Arc<str>> {
        if self.len == 0 {
            None
        } else {
            self.names[0].clone()
        }
    }

    /// True if `name` is in the class vector. Mirrors R's `inherits()`.
    pub fn contains(&self, name: &str) -> bool {
        if self.len == 0 {
            return false;
        }
        for i in 0..(self.len as usize).min(4) {
            if let Some(n) = &self.names[i] {
                if &**n == name {
                    return true;
                }
            }
        }
        false
    }

    /// True if we could not determine the class at all.
    pub fn is_unknown(&self) -> bool {
        !self.known
    }

    /// True if the value carries a known class vector with at least
    /// one entry (i.e. S3 dispatch could plausibly target a method).
    pub fn has_known_class(&self) -> bool {
        self.known && self.len > 0
    }

    /// Build a class vector from a slice of strings, truncating
    /// to the first 4 entries (R's S3 rarely uses more; the truncation
    /// is logged at debug level by the caller if needed).
    pub fn from_slice(names: &[&str]) -> Self {
        if names.is_empty() {
            return ClassVector::empty();
        }
        let mut out = ClassVector::empty();
        for (i, n) in names.iter().take(4).enumerate() {
            out.names[i] = Some(Arc::from(*n));
        }
        out.len = names.len().min(4) as u8;
        out.known = true;
        out
    }

    /// Set the class on an existing `RType`, returning a new `RType`.
    /// Used by the checker when it sees `structure(x, class = "foo")`.
    pub fn with_class(mut self, names: &[&str]) -> Self {
        self.names = [None, None, None, None];
        if names.is_empty() {
            self.len = 0;
            self.known = true;
            return self;
        }
        for (i, n) in names.iter().take(4).enumerate() {
            self.names[i] = Some(Arc::from(*n));
        }
        self.len = names.len().min(4) as u8;
        self.known = true;
        self
    }
}

/// Schema for a record-like value (data frame, list with known shape).
/// Stores an ordered `(name, RType)` list so we can both look up by
/// name (column access) and iterate in source order (display, audit).
///
/// Stored behind an `Arc` on `RType` (see `RType::columns`); the Arc is
/// released when the owning `RType` is dropped, so column schemas no
/// longer leak for the lifetime of the process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnSchema {
    pub columns: Vec<(String, RType)>,
}

impl ColumnSchema {
    /// Look up a column by name. Linear scan is fine; the column count
    /// is bounded by what appears literally in the source.
    pub fn get(&self, name: &str) -> Option<RType> {
        self.columns
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.clone())
    }

    /// All column names in declared order. Used for diagnostic messages.
    pub fn names(&self) -> Vec<&str> {
        self.columns.iter().map(|(n, _)| n.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    /// If every column of this schema carries the SAME type, return that
    /// common element type. Used to type `[[N]]` extraction (and list
    /// `element()`) for homogeneous lists such as `list(1, 2, 3)` -- where
    /// `lapply` / `for` should see the unwrapped `double<1>` rather than
    /// `list<1>`. Heterogeneous or empty schemas return `None`.
    pub fn homogeneous_element_type(&self) -> Option<RType> {
        let first = self.columns.first().map(|(_, t)| t.clone())?;
        if self.columns.iter().all(|(_, t)| t == &first) {
            Some(first)
        } else {
            None
        }
    }
}

/// Inferred signature for a function value (closures returned from
/// function factories, curried helpers, etc.).
///
/// This is v1's answer to R closures without a full dependent type
/// system. The checker walks a function literal's body, collects its
/// return types, and stores the joined result here. The signature is
/// attached to a `Mode::Function` `RType` via the `fn_sig` field so
/// callers can resolve indirect calls like `f <- make_counter();
/// f()`.
///
/// Scope limits (see `ry-checker/src/lib.rs` for the implementation):
///   * Maximum nesting depth: 3 levels of closures. Deeper nests get
///     `fn_sig = None` (opaque).
///   * Captured bindings are snapshotted at the point where the inner
///     function is inferred. Closures that close over mutable state
///     (reassigned in the body) get opaque for the captured binding.
///   * `params` carries positional inferred-or-default types and may be
///     shorter than the real arity (we only fill entries we can infer).
///
/// Stored behind an `Arc` on `RType` (see `RType::fn_sig`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSignature {
    /// Positional parameter types the checker could infer from
    /// defaults or call sites. May be shorter than the real arity;
    /// missing entries are treated as opaque by callers.
    pub params: Vec<RType>,
    /// Joined return type of the function body. `RType::unknown()` if the
    /// body gave us nothing to work with.
    pub return_type: Box<RType>,
}

/// A fully-described R type at the granularity v1 cares about. Includes
/// an optional S3 class vector and an optional record schema (data
/// frame columns, named list shape); arithmetic / comparison strip the
/// class and schema (matching R), and control-flow `join` keeps both
/// only when both sides agree.
///
/// `RType` is `Clone` (not `Copy`): the `columns` and `fn_sig` fields
/// hold `Arc`-shared heap data, and the `class` names are `Arc<str>`.
/// Cloning bumps refcounts and is cheap. The previous design held these
/// as `&'static` references into globally-leaked intern tables; the Arcs
/// are released when the owning value is dropped, so long-running LSP
/// sessions no longer accumulate schemas and class names unboundedly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RType {
    pub mode: Mode,
    pub length: Length,
    pub class: ClassVector,
    /// Optional record schema for data frames and named lists. Shared
    /// via `Arc` so cloning an `RType` is cheap. `None` means "we don't
    /// know the shape" (the conservative default for values whose
    /// construction we can't see).
    pub columns: Option<Arc<ColumnSchema>>,
    /// Optional inferred signature for `Mode::Function` values. Carries
    /// the joined return type (and any positional param types we could
    /// infer) for closures returned from function factories like
    /// `make_counter <- function() { function() { 1L } }`.
    ///
    /// `None` for opaque functions (built-ins without a typeshed entry,
    /// user functions whose body we couldn't walk, or closures deeper
    /// than the 3-level nesting cap).
    ///
    /// For non-`Function` modes this is always `None`; arithmetic and
    /// comparison ops clear it (you cannot meaningfully add two
    /// closures).
    pub fn_sig: Option<Arc<FunctionSignature>>,
    /// Members of a union type (`mode == Mode::Union`). `None` for all
    /// non-union types. Members are bare atomic shapes (class/columns/
    /// fn_sig cleared); the union owns those dimensions. Built by `join`
    /// when two incompatible branches merge (e.g. `if (p) 1L else "a"`),
    /// capped at `MAX_UNION_MEMBERS` (beyond the cap, join collapses to
    /// `RType::unknown()`).
    pub members: Option<Arc<[RType]>>,
}

/// Maximum number of distinct members in a union. Beyond this, `join`
/// gives up and collapses to `RType::unknown()` (matching ty's approach
/// to large literal unions). 4 is enough for the common control-flow
/// merges without letting pathological joins explode.
pub const MAX_UNION_MEMBERS: usize = 4;

impl RType {
    /// The opaque "unknown" type: opaque mode, unknown length, no class
    /// or schema. Used whenever inference gives up.
    ///
    /// This is a function (not a `const`) because `RType`'s `Arc` fields
    /// cannot be constructed in a const context. Callers that previously
    /// wrote `RType::UNKNOWN` should call `RType::unknown()`.
    pub fn unknown() -> RType {
        RType {
            mode: Mode::Opaque,
            length: Length::Unknown,
            class: ClassVector::unknown(),
            columns: None,
            fn_sig: None,
            members: None,
        }
    }

    pub fn new(mode: Mode, length: Length) -> Self {
        RType {
            mode,
            length,
            class: ClassVector::empty(),
            columns: None,
            fn_sig: None,
            members: None,
        }
    }

    /// A scalar literal of the given mode.
    pub fn scalar(mode: Mode) -> Self {
        Self::new(mode, Length::One)
    }

    /// Return a copy of `self` with the S3 class vector replaced.
    pub fn with_class(self, class: ClassVector) -> Self {
        RType { class, ..self }
    }

    /// Return a copy of `self` with the column schema replaced.
    pub fn with_columns(self, schema: Arc<ColumnSchema>) -> Self {
        RType {
            columns: Some(schema),
            ..self
        }
    }

    /// Return a copy of `self` with the function signature replaced.
    /// Only meaningful when `mode` is `Mode::Function`; on other modes
    /// the signature is silently dropped (arithmetic / comparison clear
    /// it).
    pub fn with_fn_sig(self, sig: Arc<FunctionSignature>) -> Self {
        RType {
            fn_sig: Some(sig),
            ..self
        }
    }

    /// Return a copy of `self` with the union members replaced.
    pub fn with_members(self, members: Arc<[RType]>) -> Self {
        RType {
            members: Some(members),
            ..self
        }
    }

    /// Checked constructor for a `Mode::Union` type. All union construction
    /// must go through here so a malformed union (`mode == Union`,
    /// `members == None`) can never be built by accident -- the audit in
    /// PLAN Phase A2 traced several false positives (e.g. the `if` condition
    /// `union[]` RY001) to ad-hoc `RType::new(Mode::Union, ...)` /
    /// `RType::new(bt.mode, ...)` sites that left `members` null.
    ///
    /// Panics (debug) / silently degrades (release) on an empty member list,
    /// since a union of nothing has no sound rendering. A single-member input
    /// collapses to that member (so "all *multi-member* union construction
    /// goes through here"; a 1-member call is an identity).
    pub fn union(members: Arc<[RType]>) -> Self {
        debug_assert!(
            !members.is_empty(),
            "RType::union requires at least one member"
        );
        if members.is_empty() {
            return RType::unknown();
        }
        if members.len() == 1 {
            // A single-member "union" is just that member.
            return (*members.first().unwrap()).clone();
        }
        // Length: common across members if they all agree, else Unknown.
        let length = members
            .iter()
            .map(|m| m.length)
            .reduce(|a, b| if a == b { a } else { Length::Unknown })
            .unwrap_or(Length::Unknown);
        RType {
            mode: Mode::Union,
            length,
            class: ClassVector::empty(),
            columns: None,
            fn_sig: None,
            members: Some(members),
        }
    }

    /// Result of `lhs op rhs` for an arithmetic operator, or None if the
    /// mode combination is invalid (e.g. list + numeric). Arithmetic
    /// strips any S3 class attribute and the column schema, matching
    /// R's actual semantics (arithmetic on data frames is either a loop
    /// over columns producing a new frame or a runtime error depending
    /// on the operator; we conservatively report the bare atomic mode).
    ///
    /// Union semantics (PLAN Phase 3 item 2): distribute over members;
    /// the op errors ONLY if every member-pair errors. A union of
    /// integer and character `+ 1` yields integer (the character member
    /// errors but the integer one is fine) -> stay quiet in v1.
    pub fn arith(self, rhs: RType) -> Option<RType> {
        distribute(self, rhs, |a, b| a.arith_atomic(b))
    }

    /// Atomic (non-union) arithmetic. The original `arith` body before
    /// Phase 3 union support; called per-member by `arith`'s distributor.
    fn arith_atomic(self, rhs: RType) -> Option<RType> {
        let mode = self.mode.arith_result(rhs.mode)?;
        // R returns a zero-length vector when one operand is NULL (e.g.
        // `NULL + 1` -> `numeric(0)`); model the length as Zero in that
        // case, overriding the normal recycling rule.
        let length = match (self.mode, rhs.mode) {
            (Mode::Null, _) | (_, Mode::Null) => Length::Zero,
            _ => self.length.binary(rhs.length),
        };
        Some(RType {
            mode,
            length,
            // Arithmetic on S3 objects strips the class in R, and the
            // column schema is meaningless on the atomic result. The
            // function signature is likewise dropped: you cannot add
            // two closures and get a meaningful signature back.
            class: ClassVector::empty(),
            columns: None,
            fn_sig: None,
            members: None,
        })
    }

    /// Comparison operators return plain logical values (no class, no
    /// schema). Like `arith`, distributes over union members.
    pub fn compare(self, rhs: RType) -> Option<RType> {
        distribute(self, rhs, |a, b| a.compare_atomic(b))
    }

    fn compare_atomic(self, rhs: RType) -> Option<RType> {
        let mode = self.mode.compare_result(rhs.mode)?;
        let length = self.length.binary(rhs.length);
        Some(RType {
            mode,
            length,
            class: ClassVector::empty(),
            columns: None,
            fn_sig: None,
            members: None,
        })
    }

    /// Whether this type is definitely an error to use as a condition
    /// (`if (cond)`, `while (cond)`). R requires a length-1 logical, but
    /// will accept any length-1 atomic and silently coerce.
    pub fn invalid_condition(&self) -> bool {
        match self.mode {
            Mode::Union => {
                // Invalid only if EVERY member is invalid (one valid
                // branch means the runtime value could be a valid
                // condition, so stay quiet -- PLAN Phase 3 item 2).
                //
                // A malformed union (`members == None`, which only a bug
                // can now produce) must NOT be treated as invalid -- that
                // is exactly the RY001 `union[]` false positive PLAN Phase
                // A2 fixed. Treat it as opaque (never invalid).
                match self.members.as_ref() {
                    Some(ms) => ms.iter().all(|t| t.invalid_condition()),
                    None => false,
                }
            }
            Mode::List | Mode::Function => true,
            _ => matches!(self.length, Length::Zero),
        }
    }

    /// Join (least upper bound) of two types, used when control flow
    /// merges: e.g. the result of `if (cond) x else y`.
    ///
    /// Phase 3 semantics: R NEVER coerces at a control-flow merge. If
    /// the two branches have the same type, that type wins. Otherwise
    /// we build an honest **union** of the two (deduplicated, capped at
    /// `MAX_UNION_MEMBERS`, collapsing to `RType::unknown()` beyond the
    /// cap). This replaces the old coercion-ladder join, which silently
    /// promoted `if (p) 1L else "a"` to `character`.
    ///
    /// Opaque is absorbing: joining anything with unknown yields unknown.
    ///
    /// The S3 class vector, column schema, and function signature are
    /// preserved only when both sides agree exactly; otherwise dropped.
    pub fn join(self, other: RType) -> RType {
        if matches!(self.mode, Mode::Opaque) || matches!(other.mode, Mode::Opaque) {
            return RType::unknown();
        }
        if self == other {
            return self;
        }
        union_of(self, other)
    }

    /// Element type of an iterable used in `for (var in iter)`. For an
    /// atomic vector this is a length-1 value of the same mode; for a
    /// list it is a length-1 list; for opaque input it stays opaque.
    /// The class and column schema are dropped: iterating over a classed
    /// vector yields the bare elements in R.
    ///
    /// For a `Mode::Union`, the element type is the union of each member's
    /// element type (PLAN Phase A2). Distributing -- rather than passing
    /// `Mode::Union` through the `_` arm and building a malformed union
    /// with `members: None` -- is what stops `for (x in if(...) TRUE else
    /// 1L) { if (x) ... }` from reporting `union[]` as the condition.
    pub fn element(&self) -> RType {
        match self.mode {
            Mode::Null => RType::new(Mode::Null, Length::Zero),
            Mode::Opaque => RType::unknown(),
            Mode::List => {
                // `[[`-style element extraction from a list yields the
                // UNWRAPPED element, not `list<1>` (PLAN Phase A3). For a
                // homogeneous list (`list(1, 2, 3)`) the unwrapped element
                // type is the common column type; heterogeneous or
                // schema-less lists degrade to unknown. Atomic vectors
                // keep the length-1 same-mode behavior below.
                match self.columns.as_ref() {
                    Some(schema) => schema
                        .homogeneous_element_type()
                        .unwrap_or_else(RType::unknown),
                    None => RType::unknown(),
                }
            }
            Mode::Union => match self.members.as_ref() {
                Some(ms) => {
                    let elems: Vec<RType> = ms.iter().map(|m| m.element()).collect();
                    let mut iter = elems.into_iter();
                    let first = iter.next().unwrap_or_else(RType::unknown);
                    iter.fold(first, |acc, t| acc.join(t))
                }
                None => RType::unknown(),
            },
            _ => RType::new(self.mode, Length::One),
        }
    }

    /// Result of the `:` sequence operator `from:to`. In R, `:` returns
    /// integer whenever both endpoints are whole numbers, regardless of
    /// whether they're typed as double or integer. Since `1:3` (with
    /// double literals) is far more common than `1.5:3.5`, we default
    /// to integer for all numeric (non-opaque) operands. The only case
    /// this gets wrong is genuinely fractional endpoints, which are
    /// rare and produce only a minor false negative (we'd report
    /// integer where R reports double).
    pub fn seq(self, other: RType) -> RType {
        let mode = if matches!(self.mode, Mode::Opaque) || matches!(other.mode, Mode::Opaque) {
            Mode::Opaque
        } else if matches!(
            self.mode,
            Mode::Character | Mode::List | Mode::Function | Mode::Null
        ) || matches!(
            other.mode,
            Mode::Character | Mode::List | Mode::Function | Mode::Null
        ) {
            // Non-numeric operands: R would coerce or error. We stay
            // opaque to avoid false positives.
            Mode::Opaque
        } else {
            // All numeric (integer, double, logical) operands: R's `:`
            // returns integer when both endpoints are whole numbers.
            // We can't check values statically, so we default to
            // integer (matching the overwhelmingly common case).
            Mode::Integer
        };
        RType::new(mode, Length::Unknown)
    }
}

/// Build a union of two (non-equal, non-opaque) types.
///
/// Flattens members of existing unions, deduplicates by `==`, caps at
/// `MAX_UNION_MEMBERS` (collapsing to `RType::unknown()` beyond the cap).
///
/// Member payloads (class/columns/fn_sig) are deliberately KEPT on each
/// member: distribute-based column access in a later phase needs the
/// per-member shape. The union itself carries no class/columns/fn_sig.
/// The length is the common member length if all members agree, else
/// `Length::Unknown`.
fn union_of(a: RType, b: RType) -> RType {
    let mut members: Vec<RType> = Vec::new();
    let push_dedup = |v: &mut Vec<RType>, t: RType| {
        if !v.iter().any(|m| m == &t) {
            v.push(t);
        }
    };
    match a.mode {
        Mode::Union => {
            if let Some(ms) = &a.members {
                for m in ms.iter() {
                    push_dedup(&mut members, m.clone());
                }
            }
        }
        _ => push_dedup(&mut members, a),
    }
    match b.mode {
        Mode::Union => {
            if let Some(ms) = &b.members {
                for m in ms.iter() {
                    push_dedup(&mut members, m.clone());
                }
            }
        }
        _ => push_dedup(&mut members, b),
    }
    if members.len() > MAX_UNION_MEMBERS {
        return RType::unknown();
    }
    if members.is_empty() {
        // Two malformed unions with no members: degrade honestly.
        return RType::unknown();
    }
    RType::union(Arc::from(members))
}

/// Distribute a binary type operation over union members. The op errors
/// (returns None) ONLY if every member-pair errors; otherwise the
/// successful results are joined (which may itself be a union).
fn distribute<F>(lhs: RType, rhs: RType, mut op: F) -> Option<RType>
where
    F: FnMut(RType, RType) -> Option<RType>,
{
    let lhs_members: Vec<RType> = match &lhs.members {
        Some(ms) if lhs.mode == Mode::Union => ms.iter().cloned().collect(),
        _ => vec![lhs],
    };
    let rhs_members: Vec<RType> = match &rhs.members {
        Some(ms) if rhs.mode == Mode::Union => ms.iter().cloned().collect(),
        _ => vec![rhs],
    };
    let mut oks: Vec<RType> = Vec::new();
    for l in &lhs_members {
        for r in &rhs_members {
            if let Some(out) = op(l.clone(), r.clone()) {
                oks.push(out);
            }
        }
    }
    if oks.is_empty() {
        return None;
    }
    let mut iter = oks.into_iter();
    let first = iter.next().unwrap();
    Some(iter.fold(first, |acc, t| acc.join(t)))
}

impl fmt::Display for RType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mode == Mode::Union {
            // A malformed union (members == None) should never exist now
            // that all construction goes through `RType::union`, but
            // render it as `opaque` rather than the misleading `union[]`
            // (PLAN Phase A2 defense-in-depth).
            return match self.members.as_ref() {
                Some(ms) => {
                    f.write_str("union[")?;
                    let mut first = true;
                    for m in ms.iter() {
                        if !first {
                            f.write_str(", ")?;
                        }
                        write!(f, "{}", m)?;
                        first = false;
                    }
                    f.write_str("]")?;
                    Ok(())
                }
                None => write!(f, "opaque"),
            };
        }
        let mode = self.mode;
        let len = match self.length {
            Length::Zero => "0",
            Length::One => "1",
            Length::Known(n) => {
                let core = write!(f, "{}<len={}>", mode, n);
                return self.fmt_class(f, core);
            }
            Length::Unknown => "?",
        };
        let core = write!(f, "{}<len={}>", mode, len);
        self.fmt_class(f, core)
    }
}

impl RType {
    /// Append the `:class` suffix (and any column schema) to a
    /// partially-written `RType`. Used by `Display` so the annotations
    /// land after the mode/length.
    fn fmt_class(&self, f: &mut fmt::Formatter<'_>, core: fmt::Result) -> fmt::Result {
        core?;
        if self.class.has_known_class() {
            let mut first = true;
            f.write_str(":")?;
            for i in 0..(self.class.len as usize).min(4) {
                if let Some(n) = &self.class.names[i] {
                    if !first {
                        f.write_str(",")?;
                    }
                    f.write_str(n)?;
                    first = false;
                }
            }
        } else if self.class.is_unknown() {
            f.write_str(":?")?;
        }
        // Column schema annotation. Abbreviated to the first 3 columns
        // plus `...` when the schema has more entries, to keep the
        // display readable for wide data frames.
        if let Some(schema) = &self.columns {
            f.write_str("{")?;
            let cols = &schema.columns;
            let limit = 3;
            for (i, (name, ty)) in cols.iter().take(limit).enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{}:{}", name, ty)?;
            }
            if cols.len() > limit {
                f.write_str(", ...")?;
            }
            f.write_str("}")?;
        }
        // Function signature annotation for closures whose return type
        // we could infer. Shown as `-> <return_type>` so it visually
        // resembles R's own `function() {}` shape.
        if let Some(sig) = &self.fn_sig {
            write!(f, " -> {}", sig.return_type)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arith_null_plus_character_errors() {
        // R: `NULL + "a"` errors with "non-numeric argument to binary
        // operator". The Character rejection must run before the Null
        // arm (this is the bug PLAN finding 7 calls out).
        let n = RType::new(Mode::Null, Length::Zero);
        let c = RType::scalar(Mode::Character);
        assert!(
            n.clone().arith(c.clone()).is_none(),
            "NULL + \"a\" must error"
        );
        assert!(c.arith(n).is_none(), "\"a\" + NULL must error");
    }

    #[test]
    fn arith_null_plus_int_is_zero_length() {
        // R: `NULL + 1` returns numeric(0). The mode is Double (R coerces
        // the NULL+int pair up) and the length is Zero.
        let n = RType::new(Mode::Null, Length::Zero);
        let i = RType::scalar(Mode::Integer);
        let r = n.arith(i).unwrap();
        assert_eq!(r.length, Length::Zero, "NULL + 1 must be length 0");
    }

    #[test]
    fn arith_integer_double_promotes() {
        let i = RType::scalar(Mode::Integer);
        let d = RType::scalar(Mode::Double);
        let r = i.arith(d).unwrap();
        assert_eq!(r.mode, Mode::Double);
        assert_eq!(r.length, Length::One);
    }

    #[test]
    fn arith_character_fails() {
        let s = RType::scalar(Mode::Character);
        let i = RType::scalar(Mode::Integer);
        assert!(s.arith(i).is_none());
    }

    #[test]
    fn length_recycling() {
        assert_eq!(Length::One.binary(Length::Known(5)), Length::Known(5));
        assert_eq!(Length::Known(4).binary(Length::Known(2)), Length::Known(4));
        assert_eq!(Length::Zero.binary(Length::Known(5)), Length::Zero);
    }

    #[test]
    fn condition_rejects_lists() {
        let l = RType::new(Mode::List, Length::One);
        assert!(l.invalid_condition());
    }

    #[test]
    fn join_of_incompatible_modes_is_a_union() {
        // Phase 3: R never coerces at a control-flow merge. `if (p) 1L
        // else 2.0` joins integer and double to an HONEST union, not to
        // Double (the old coercion-ladder behavior).
        let i = RType::scalar(Mode::Integer);
        let d = RType::scalar(Mode::Double);
        let joined = i.clone().join(d.clone());
        assert_eq!(joined.mode, Mode::Union);
        assert_eq!(joined.mode, d.join(i).mode, "join should be symmetric");
        let members = joined.members.expect("union has members");
        assert_eq!(members.len(), 2);
        assert!(members.iter().any(|m| m.mode == Mode::Integer));
        assert!(members.iter().any(|m| m.mode == Mode::Double));
    }

    #[test]
    fn join_equal_modes_returns_self_not_union() {
        // Same mode on both sides must NOT build a union.
        let i = RType::scalar(Mode::Integer);
        let joined = i.join(RType::scalar(Mode::Integer));
        assert_eq!(joined.mode, Mode::Integer);
        assert!(joined.members.is_none());
    }

    #[test]
    fn arith_distributes_over_union_quiet_when_some_member_ok() {
        // union[integer, character] + 1 -> integer ok, character errors.
        // Some member succeeds -> stay quiet (no None), result integer.
        let u = RType::scalar(Mode::Integer).join(RType::scalar(Mode::Character));
        assert_eq!(u.mode, Mode::Union);
        let r = u.arith(RType::scalar(Mode::Integer));
        assert!(r.is_some(), "some member ok -> not an error");
        assert_eq!(r.unwrap().mode, Mode::Integer);
    }

    #[test]
    fn arith_errors_when_all_union_members_invalid() {
        // union[list, function] + 1 -> both error -> None.
        let u = RType::scalar(Mode::List).join(RType::scalar(Mode::Function));
        let r = u.arith(RType::scalar(Mode::Integer));
        assert!(r.is_none(), "all members invalid -> error");
    }

    #[test]
    fn join_with_opaque_is_unknown() {
        let i = RType::scalar(Mode::Integer);
        assert_eq!(i.join(RType::unknown()), RType::unknown());
    }

    #[test]
    fn join_equal_returns_self() {
        let i = RType::scalar(Mode::Integer);
        assert_eq!(i.clone().join(i.clone()), i);
    }

    #[test]
    fn join_different_lengths_unknown() {
        let a = RType::new(Mode::Integer, Length::Known(3));
        let b = RType::new(Mode::Integer, Length::Known(5));
        assert_eq!(a.join(b).length, Length::Unknown);
    }

    #[test]
    fn element_of_vector_is_scalar() {
        let v = RType::new(Mode::Integer, Length::Known(10));
        let e = v.element();
        assert_eq!(e.mode, Mode::Integer);
        assert_eq!(e.length, Length::One);
    }

    #[test]
    fn element_of_union_distributes_over_members() {
        // element() of a union is the join of each member's element type.
        // PLAN Phase A2: previously the `_` arm built a malformed union
        // (`Mode::Union`, `members: None`).
        let u = RType::scalar(Mode::Integer).join(RType::scalar(Mode::Logical));
        assert_eq!(u.mode, Mode::Union);
        let e = u.element();
        // Integer | Logical element types are scalars of each mode; the
        // join of integer-scalar and logical-scalar is a union.
        assert_eq!(e.mode, Mode::Union, "got {e}");
        let members = e
            .members
            .expect("distributed element union must carry members");
        assert_eq!(members.len(), 2);
        assert!(members.iter().any(|m| m.mode == Mode::Integer));
        assert!(members.iter().any(|m| m.mode == Mode::Logical));
    }

    #[test]
    fn element_of_union_with_one_member_is_that_member_element() {
        // A union of two equal modes collapses to that mode; its element
        // is then that mode's scalar (no malformed union).
        let u = RType::scalar(Mode::Double).join(RType::scalar(Mode::Double));
        assert_eq!(u.mode, Mode::Double);
        assert_eq!(u.element().mode, Mode::Double);
    }

    #[test]
    fn element_of_homogeneous_list_unwraps() {
        // PLAN Phase A3: iterating a list yields the UNWRAPPED element.
        // list(1, 2, 3) has schema [[1]]=double, [[2]]=double, [[3]]=double;
        // element() returns double<1>, NOT list<1>.
        let schema = ColumnSchema {
            columns: vec![
                ("[[1]]".to_string(), RType::scalar(Mode::Double)),
                ("[[2]]".to_string(), RType::scalar(Mode::Double)),
            ],
        };
        let list = RType::new(Mode::List, Length::Known(2)).with_columns(Arc::new(schema));
        let e = list.element();
        assert_eq!(e.mode, Mode::Double);
        assert_eq!(e.length, Length::One);
    }

    #[test]
    fn element_of_heterogeneous_list_is_unknown() {
        // list(1, "a") -> double | character: element is unknown, NOT
        // list<1> (which would cause arithmetic false positives).
        let schema = ColumnSchema {
            columns: vec![
                ("[[1]]".to_string(), RType::scalar(Mode::Double)),
                ("[[2]]".to_string(), RType::scalar(Mode::Character)),
            ],
        };
        let list = RType::new(Mode::List, Length::Known(2)).with_columns(Arc::new(schema));
        let e = list.element();
        assert_eq!(e, RType::unknown());
    }

    #[test]
    fn element_of_schema_less_list_is_unknown() {
        // A bare list with no column schema degrades to unknown.
        let list = RType::new(Mode::List, Length::Unknown);
        assert_eq!(list.element(), RType::unknown());
    }

    #[test]
    fn homogeneous_element_type_returns_common_for_homogeneous_schema() {
        let schema = ColumnSchema {
            columns: vec![
                ("[[1]]".to_string(), RType::scalar(Mode::Integer)),
                ("[[2]]".to_string(), RType::scalar(Mode::Integer)),
            ],
        };
        assert_eq!(
            schema.homogeneous_element_type(),
            Some(RType::scalar(Mode::Integer))
        );
    }

    #[test]
    fn homogeneous_element_type_none_for_heterogeneous_or_empty() {
        let het = ColumnSchema {
            columns: vec![
                ("[[1]]".to_string(), RType::scalar(Mode::Integer)),
                ("[[2]]".to_string(), RType::scalar(Mode::Double)),
            ],
        };
        assert_eq!(het.homogeneous_element_type(), None);
        assert_eq!(ColumnSchema::default().homogeneous_element_type(), None);
    }

    #[test]
    fn checked_union_constructor_builds_real_union() {
        let members: Arc<[RType]> =
            Arc::from([RType::scalar(Mode::Integer), RType::scalar(Mode::Double)]);
        let u = RType::union(members);
        assert_eq!(u.mode, Mode::Union);
        assert!(u.members.is_some(), "union must carry members");
        assert_eq!(u.members.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn checked_union_constructor_collapses_single_member() {
        let members: Arc<[RType]> = Arc::from([RType::scalar(Mode::Integer)]);
        let u = RType::union(members);
        assert_eq!(u.mode, Mode::Integer, "single-member union collapses");
    }

    #[test]
    #[should_panic(expected = "RType::union requires at least one member")]
    fn checked_union_constructor_panics_on_empty() {
        // All union construction goes through `RType::union`; an empty
        // member list is a programmer error caught by the debug assert.
        let members: Arc<[RType]> = Arc::from([]);
        let _ = RType::union(members);
    }

    #[test]
    fn malformed_union_is_never_an_invalid_condition() {
        // Defense-in-depth: even a (bug-only) union with members == None
        // must not be reported as an invalid condition (that was the
        // RY001 `union[]` false positive PLAN Phase A2 fixed).
        let malformed = RType {
            mode: Mode::Union,
            length: Length::One,
            class: ClassVector::empty(),
            columns: None,
            fn_sig: None,
            members: None,
        };
        assert!(!malformed.invalid_condition());
        // And it renders as `opaque`, not the misleading `union[]`.
        assert_eq!(format!("{malformed}"), "opaque");
    }

    #[test]
    fn seq_int_int_is_integer() {
        let a = RType::scalar(Mode::Integer);
        let b = RType::scalar(Mode::Integer);
        assert_eq!(a.seq(b).mode, Mode::Integer);
    }

    #[test]
    fn seq_double_double_is_integer() {
        // R's `:` returns integer for whole-number double endpoints.
        // `1:3` produces c(1L, 2L, 3L) even though 1 and 3 are doubles.
        let a = RType::scalar(Mode::Double);
        let b = RType::scalar(Mode::Double);
        assert_eq!(a.seq(b).mode, Mode::Integer);
    }

    #[test]
    fn class_vector_empty_has_no_first() {
        let cv = ClassVector::empty();
        assert_eq!(cv.first(), None);
        assert!(!cv.has_known_class());
        assert!(!cv.is_unknown());
        assert!(!cv.contains("anything"));
    }

    #[test]
    fn class_vector_single_exposes_first() {
        let cv = ClassVector::single("foo");
        assert_eq!(cv.first().as_deref(), Some("foo"));
        assert!(cv.has_known_class());
        assert!(cv.contains("foo"));
        assert!(!cv.contains("bar"));
    }

    #[test]
    fn class_vector_unknown_is_distinguished_from_empty() {
        let cv = ClassVector::unknown();
        assert!(cv.is_unknown());
        assert!(!cv.has_known_class());
        assert_eq!(cv.first(), None);
        // Critical: empty() and unknown() must differ so RY050 can be
        // suppressed on the latter (we can't say the class is missing
        // if we don't know what the class is).
        assert_ne!(ClassVector::empty(), ClassVector::unknown());
    }

    #[test]
    fn class_vector_from_slice_truncates_to_four() {
        let cv = ClassVector::from_slice(&["a", "b", "c", "d", "e"]);
        assert_eq!(cv.len, 4);
        assert_eq!(cv.first().as_deref(), Some("a"));
        assert!(cv.contains("d"));
        assert!(!cv.contains("e"));
    }

    #[test]
    fn arith_strips_class() {
        let lhs = RType::scalar(Mode::Double).with_class(ClassVector::single("lm"));
        let rhs = RType::scalar(Mode::Double);
        let r = lhs.arith(rhs).unwrap();
        assert!(!r.class.is_unknown());
        assert!(!r.class.has_known_class());
        assert_eq!(r.class, ClassVector::empty());
    }

    #[test]
    fn compare_strips_class() {
        let lhs = RType::scalar(Mode::Double).with_class(ClassVector::single("ts"));
        let rhs = RType::scalar(Mode::Double);
        let r = lhs.compare(rhs).unwrap();
        assert_eq!(r.mode, Mode::Logical);
        assert!(!r.class.has_known_class());
    }

    #[test]
    fn join_preserves_class_when_both_sides_agree() {
        let class = ClassVector::single("lm");
        let lhs = RType::scalar(Mode::List).with_class(class.clone());
        let rhs = RType::scalar(Mode::List).with_class(class.clone());
        let joined = lhs.join(rhs);
        assert_eq!(joined.class, class);
    }

    #[test]
    fn join_drops_class_when_sides_differ() {
        let lhs = RType::scalar(Mode::List).with_class(ClassVector::single("lm"));
        let rhs = RType::scalar(Mode::List).with_class(ClassVector::single("ts"));
        let joined = lhs.join(rhs);
        assert!(!joined.class.has_known_class());
        assert_eq!(joined.class, ClassVector::empty());
    }

    #[test]
    fn join_class_unknown_when_one_side_unknown() {
        let lhs = RType::scalar(Mode::List).with_class(ClassVector::single("lm"));
        let rhs = RType::unknown(); // unknown class
        let joined = lhs.join(rhs);
        assert_eq!(joined, RType::unknown());
        assert!(joined.class.is_unknown());
    }

    #[test]
    fn rtype_display_includes_class_suffix() {
        let t = RType::scalar(Mode::List).with_class(ClassVector::single("lm"));
        let s = format!("{}", t);
        assert!(s.contains(":lm"), "expected `:lm` in display, got {}", s);
    }

    #[test]
    fn rtype_display_no_class_suffix_for_empty() {
        let t = RType::scalar(Mode::Integer);
        let s = format!("{}", t);
        assert!(
            !s.contains(':'),
            "expected no `:` suffix for classless type, got {}",
            s
        );
    }

    #[test]
    fn column_schema_lookups_by_name() {
        let schema = ColumnSchema {
            columns: vec![
                ("a".to_string(), RType::scalar(Mode::Integer)),
                ("b".to_string(), RType::scalar(Mode::Character)),
            ],
        };
        assert_eq!(schema.get("a").unwrap().mode, Mode::Integer);
        assert_eq!(schema.get("b").unwrap().mode, Mode::Character);
        assert!(schema.get("missing").is_none());
        assert_eq!(schema.names(), vec!["a", "b"]);
        assert_eq!(schema.len(), 2);
    }

    #[test]
    fn rtype_with_columns_roundtrips() {
        let schema = Arc::new(ColumnSchema {
            columns: vec![(
                "mpg".to_string(),
                RType::new(Mode::Double, Length::Known(32)),
            )],
        });
        let t = RType::new(Mode::List, Length::Known(1)).with_columns(schema.clone());
        assert_eq!(t.columns, Some(schema));
        // `with_columns` must not disturb other fields.
        assert_eq!(t.mode, Mode::List);
        assert_eq!(t.length, Length::Known(1));
    }

    #[test]
    fn arith_strips_columns() {
        let schema = Arc::new(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double))],
        });
        let lhs = RType::scalar(Mode::Double).with_columns(schema);
        let rhs = RType::scalar(Mode::Double);
        let r = lhs.arith(rhs).unwrap();
        assert_eq!(r.mode, Mode::Double);
        assert!(r.columns.is_none(), "arith must strip column schema");
    }

    #[test]
    fn compare_strips_columns() {
        let schema = Arc::new(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double))],
        });
        let lhs = RType::scalar(Mode::Double).with_columns(schema);
        let rhs = RType::scalar(Mode::Double);
        let r = lhs.compare(rhs).unwrap();
        assert_eq!(r.mode, Mode::Logical);
        assert!(r.columns.is_none(), "compare must strip column schema");
    }

    #[test]
    fn join_preserves_columns_when_both_sides_agree() {
        let schema = Arc::new(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double))],
        });
        let lhs = RType::scalar(Mode::List).with_columns(schema.clone());
        let rhs = RType::scalar(Mode::List).with_columns(schema.clone());
        let joined = lhs.join(rhs);
        assert_eq!(joined.columns, Some(schema));
    }

    #[test]
    fn join_drops_columns_when_sides_differ() {
        let s1 = Arc::new(ColumnSchema {
            columns: vec![("a".to_string(), RType::scalar(Mode::Double))],
        });
        let s2 = Arc::new(ColumnSchema {
            columns: vec![("b".to_string(), RType::scalar(Mode::Double))],
        });
        let lhs = RType::scalar(Mode::List).with_columns(s1);
        let rhs = RType::scalar(Mode::List).with_columns(s2);
        let joined = lhs.join(rhs);
        assert!(joined.columns.is_none(), "differing schemas must drop");
    }

    #[test]
    fn rtype_display_includes_columns_abbreviated() {
        // Build a 5-column schema; display should show 3 then `...`.
        let cols: Vec<(String, RType)> = (0..5)
            .map(|i| (format!("c{}", i), RType::scalar(Mode::Double)))
            .collect();
        let schema = Arc::new(ColumnSchema { columns: cols });
        let t = RType::new(Mode::List, Length::Known(5)).with_columns(schema);
        let s = format!("{}", t);
        assert!(s.contains("c0:"), "missing c0: {}", s);
        assert!(s.contains("c1:"), "missing c1: {}", s);
        assert!(s.contains("c2:"), "missing c2: {}", s);
        assert!(!s.contains("c3:"), "c3 should be abbreviated: {}", s);
        assert!(s.contains("..."), "missing ellipsis: {}", s);
    }

    #[test]
    fn rtype_with_fn_sig_roundtrips() {
        let sig = Arc::new(FunctionSignature {
            params: vec![RType::scalar(Mode::Double)],
            return_type: Box::new(RType::scalar(Mode::Integer)),
        });
        let t = RType::scalar(Mode::Function).with_fn_sig(sig.clone());
        assert_eq!(t.fn_sig, Some(sig));
        assert_eq!(t.mode, Mode::Function);
    }

    #[test]
    fn rtype_default_fn_sig_is_none() {
        // All standard constructors must produce fn_sig = None so the
        // signature is opt-in only.
        assert!(RType::unknown().fn_sig.is_none());
        assert!(RType::scalar(Mode::Function).fn_sig.is_none());
        assert!(RType::new(Mode::Integer, Length::One).fn_sig.is_none());
    }

    #[test]
    fn arith_strips_fn_sig() {
        // Arithmetic on a function value is an error in R; even when
        // arith_result permits it (it doesn't for Function), the
        // signature must not survive. We exercise the strip via a
        // classed list whose schema we know survives only via join.
        let sig = Arc::new(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer)),
        });
        let lhs = RType::scalar(Mode::Integer).with_fn_sig(sig);
        let rhs = RType::scalar(Mode::Integer);
        let r = lhs.arith(rhs).unwrap();
        assert!(r.fn_sig.is_none(), "arith must strip fn_sig");
    }

    #[test]
    fn join_preserves_fn_sig_when_both_sides_agree() {
        let sig = Arc::new(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer)),
        });
        let lhs = RType::scalar(Mode::Function).with_fn_sig(sig.clone());
        let rhs = RType::scalar(Mode::Function).with_fn_sig(sig.clone());
        let joined = lhs.join(rhs);
        assert_eq!(joined.fn_sig, Some(sig));
    }

    #[test]
    fn join_drops_fn_sig_when_sides_differ() {
        let s1 = Arc::new(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer)),
        });
        let s2 = Arc::new(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Double)),
        });
        let lhs = RType::scalar(Mode::Function).with_fn_sig(s1);
        let rhs = RType::scalar(Mode::Function).with_fn_sig(s2);
        let joined = lhs.join(rhs);
        assert!(joined.fn_sig.is_none(), "differing sigs must drop");
    }

    #[test]
    fn rtype_display_includes_fn_sig() {
        let sig = Arc::new(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer)),
        });
        let t = RType::scalar(Mode::Function).with_fn_sig(sig);
        let s = format!("{}", t);
        assert!(s.contains("->"), "missing -> in display: {}", s);
        assert!(s.contains("integer"), "missing return type: {}", s);
    }
}
