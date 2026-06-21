//! R type lattice.
//!
//! Models what R's `typeof()` would return, plus a length dimension and a
//! flag for whether NA can occur. This is intentionally coarser than R's
//! actual semantics: S4 and RC are stubbed, environments are opaque, and
//! "unknown" is used whenever inference gives up (e.g. function call whose
//! signature is not in the typeshed).

use std::fmt;

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
            (Null, x) | (x, Null) => Some(x),
            (Opaque, _) | (_, Opaque) => Some(Opaque),
            (Character, _) | (_, Character) => None,
            (List, _) | (_, List) => None,
            (Function, _) | (_, Function) => None,
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
            (Known(a), Known(b)) => {
                if a % b == 0 || b % a == 0 {
                    Known(a.max(b))
                } else {
                    // R would warn but produce max(a, b); model as Known.
                    Known(a.max(b))
                }
            }
            (Known(_), Unknown) | (Unknown, Known(_)) | (Unknown, Unknown) => Unknown,
        }
    }
}

/// Whether a value may contain `NA`. R's NA has type-specific forms
/// (`NA_real_`, `NA_integer_`, `NA_character_`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NaFlag(pub bool);

/// S3 class attribute. Up to 4 class names. Uses `&'static str` so the
/// type stays `Copy` (and so does `RType`). Class names that aren't
/// known at compile time (e.g. user-defined via `structure(class = my_var)`)
/// are dropped to `ClassVector::unknown()`.
///
/// The `known` flag distinguishes three states:
///   * `known == true`, `len == 0`: we know there is no class attribute.
///   * `known == true`, `len > 0`: we know the class vector.
///   * `known == false`: we couldn't determine the class (do not warn).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClassVector {
    pub names: [Option<&'static str>; 4],
    pub len: u8,
    pub known: bool,
}

impl ClassVector {
    /// A value with no class attribute set. The class is *known* to be
    /// empty (e.g. a plain atomic literal); callers may rely on this to
    /// suppress RY050 since there's nothing to dispatch on.
    pub const fn empty() -> Self {
        ClassVector {
            names: [None; 4],
            len: 0,
            known: true,
        }
    }

    /// A single-class value, e.g. `structure(x, class = "foo")`.
    pub const fn single(name: &'static str) -> Self {
        ClassVector {
            names: [Some(name), None, None, None],
            len: 1,
            known: true,
        }
    }

    /// We couldn't determine the class at all (dynamic input). Used for
    /// `RType::UNKNOWN` and any value whose class depends on unknown
    /// state. Distinguished from `empty()` so callers avoid emitting
    /// RY050 on values they can't reason about.
    pub const fn unknown() -> Self {
        ClassVector {
            names: [None; 4],
            len: 0,
            known: false,
        }
    }

    /// First class name, if any. R's S3 dispatch walks the class vector
    /// in order; for v1 we model only the first-element rule.
    pub fn first(&self) -> Option<&'static str> {
        if self.len == 0 {
            None
        } else {
            self.names[0]
        }
    }

    /// True if `name` is in the class vector. Mirrors R's `inherits()`.
    pub fn contains(&self, name: &str) -> bool {
        if self.len == 0 {
            return false;
        }
        for i in 0..(self.len as usize).min(4) {
            if let Some(n) = self.names[i] {
                if n == name {
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

    /// Build a class vector from a slice of static strings, truncating
    /// to the first 4 entries (R's S3 rarely uses more; the truncation
    /// is logged at debug level by the caller if needed).
    pub fn from_static_slice(names: &[&'static str]) -> Self {
        if names.is_empty() {
            return ClassVector::empty();
        }
        let mut out = ClassVector::empty();
        for (i, n) in names.iter().take(4).enumerate() {
            out.names[i] = Some(*n);
        }
        out.len = names.len().min(4) as u8;
        out.known = true;
        out
    }

    /// Set the class on an existing `RType`, returning a new `RType`.
    /// Used by the checker when it sees `structure(x, class = "foo")`.
    pub fn with_class(mut self, names: &[&'static str]) -> Self {
        self.names = [None; 4];
        if names.is_empty() {
            self.len = 0;
            self.known = true;
            return self;
        }
        for (i, n) in names.iter().take(4).enumerate() {
            self.names[i] = Some(*n);
        }
        self.len = names.len().min(4) as u8;
        self.known = true;
        self
    }
}

/// Class literals we recognize at type-inference time. Class names that
/// aren't in this table (e.g. user-defined `"myclass"`) are still
/// interned via `intern_class_name` so we can keep `ClassVector: Copy`
/// while supporting user-defined classes from `structure(...)`.
pub const KNOWN_CLASSES: &[(&str, &str)] = &[
    ("data.frame", "data.frame"),
    ("lm", "lm"),
    ("factor", "factor"),
    ("ts", "ts"),
    ("Date", "Date"),
    ("POSIXct", "POSIXct"),
    ("POSIXlt", "POSIXlt"),
    ("table", "table"),
    ("matrix", "matrix"),
];

/// Intern a runtime `&str` into `&'static str` so it can be stored in a
/// `Copy` `ClassVector`. We cache by string content using a `OnceLock`
/// HashMap; the underlying strings are leaked (acceptable for v1: the
/// number of distinct class names in a program is small and bounded by
/// the source's literal string constants).
pub fn intern_class_name(s: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static TABLE: OnceLock<Mutex<std::collections::HashMap<String, &'static str>>> =
        OnceLock::new();
    let lock = TABLE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut guard = match lock.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(existing) = guard.get(s) {
        return existing;
    }
    // First check the well-known table so common class names don't
    // accumulate duplicate leaks.
    if let Some((_, canonical)) = KNOWN_CLASSES.iter().find(|(k, _)| *k == s) {
        guard.insert(s.to_string(), canonical);
        return canonical;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    guard.insert(s.to_string(), leaked);
    leaked
}

/// Schema for a record-like value (data frame, list with known shape).
/// Stores an ordered `(name, RType)` list so we can both look up by
/// name (column access) and iterate in source order (display, audit).
///
/// Interned via `intern_column_schema` so `RType` stays `Copy`: the
/// schema lives for the lifetime of the program (acceptable for v1: the
/// number of distinct schemas in a run is bounded by the source's
/// literal `list(...)` / `data.frame(...)` constructors plus the
/// typeshed's built-in datasets).
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
            .map(|(_, t)| *t)
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
}

/// Intern a runtime-built `ColumnSchema` into `&'static ColumnSchema`
/// so it can be stored in a `Copy` `RType`. Cached by content using a
/// `OnceLock`-protected `Vec`; identical schemas collapse to the same
/// static reference. The schemas are leaked (see `ColumnSchema` docs for
/// the rationale).
pub fn intern_column_schema(schema: ColumnSchema) -> &'static ColumnSchema {
    use std::sync::{Mutex, OnceLock};
    static TABLE: OnceLock<Mutex<Vec<&'static ColumnSchema>>> = OnceLock::new();
    let lock = TABLE.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard = match lock.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    for existing in guard.iter() {
        if **existing == schema {
            return existing;
        }
    }
    let leaked: &'static ColumnSchema = Box::leak(Box::new(schema));
    guard.push(leaked);
    leaked
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
/// Interned via `intern_function_signature` so `RType` stays `Copy`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSignature {
    /// Positional parameter types the checker could infer from
    /// defaults or call sites. May be shorter than the real arity;
    /// missing entries are treated as opaque by callers.
    pub params: Vec<RType>,
    /// Joined return type of the function body. `RType::UNKNOWN` if the
    /// body gave us nothing to work with.
    pub return_type: Box<RType>,
}

/// Intern a runtime-built `FunctionSignature` into
/// `&'static FunctionSignature` so it can be stored in a `Copy`
/// `RType`. Same caching/leaking pattern as `intern_column_schema`:
/// identical signatures collapse to the same static reference, and the
/// storage is leaked for the program's lifetime (acceptable for v1:
/// the number of distinct closure signatures in a run is bounded by
/// the source's literal `function(...) ...` expressions).
pub fn intern_function_signature(sig: FunctionSignature) -> &'static FunctionSignature {
    use std::sync::{Mutex, OnceLock};
    static TABLE: OnceLock<Mutex<Vec<&'static FunctionSignature>>> = OnceLock::new();
    let lock = TABLE.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard = match lock.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    for existing in guard.iter() {
        if **existing == sig {
            return existing;
        }
    }
    let leaked: &'static FunctionSignature = Box::leak(Box::new(sig));
    guard.push(leaked);
    leaked
}

/// A fully-described R type at the granularity v1 cares about. Includes
/// an optional S3 class vector and an optional record schema (data
/// frame columns, named list shape); arithmetic / comparison strip the
/// class and schema (matching R), and control-flow `join` keeps both
/// only when both sides agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RType {
    pub mode: Mode,
    pub length: Length,
    pub na: NaFlag,
    pub class: ClassVector,
    /// Optional record schema for data frames and named lists. Interned
    /// via `intern_column_schema` so this field stays `Copy`. `None`
    /// means "we don't know the shape" (the conservative default for
    /// values whose construction we can't see).
    pub columns: Option<&'static ColumnSchema>,
    /// Optional inferred signature for `Mode::Function` values. Carries
    /// the joined return type (and any positional param types we could
    /// infer) for closures returned from function factories like
    /// `make_counter <- function() { function() { 1L } }`.
    ///
    /// `None` for opaque functions (built-ins without a typeshed entry,
    /// user functions whose body we couldn't walk, or closures deeper
    /// than the 3-level nesting cap). Interned via
    /// `intern_function_signature` so this field stays `Copy`.
    ///
    /// For non-`Function` modes this is always `None`; arithmetic and
    /// comparison ops clear it (you cannot meaningfully add two
    /// closures).
    pub fn_sig: Option<&'static FunctionSignature>,
}

impl RType {
    pub const UNKNOWN: RType = RType {
        mode: Mode::Opaque,
        length: Length::Unknown,
        na: NaFlag(true),
        class: ClassVector::unknown(),
        columns: None,
        fn_sig: None,
    };

    pub const fn new(mode: Mode, length: Length, na: bool) -> Self {
        RType {
            mode,
            length,
            na: NaFlag(na),
            class: ClassVector::empty(),
            columns: None,
            fn_sig: None,
        }
    }

    /// A scalar literal of the given mode.
    pub const fn scalar(mode: Mode, na: bool) -> Self {
        Self::new(mode, Length::One, na)
    }

    /// Return a copy of `self` with the S3 class vector replaced. The
    /// caller is responsible for providing interned static strings; use
    /// `intern_class_name` for runtime-derived names.
    pub const fn with_class(self, class: ClassVector) -> Self {
        RType { class, ..self }
    }

    /// Return a copy of `self` with the column schema replaced. The
    /// caller is responsible for interning via `intern_column_schema`.
    pub const fn with_columns(self, schema: &'static ColumnSchema) -> Self {
        RType {
            columns: Some(schema),
            ..self
        }
    }

    /// Return a copy of `self` with the function signature replaced.
    /// The caller is responsible for interning via
    /// `intern_function_signature`. Only meaningful when `mode` is
    /// `Mode::Function`; on other modes the signature is silently
    /// dropped (arithmetic / comparison clear it).
    pub const fn with_fn_sig(self, sig: &'static FunctionSignature) -> Self {
        RType {
            fn_sig: Some(sig),
            ..self
        }
    }

    /// Result of `lhs op rhs` for an arithmetic operator, or None if the
    /// mode combination is invalid (e.g. list + numeric). Arithmetic
    /// strips any S3 class attribute and the column schema, matching
    /// R's actual semantics (arithmetic on data frames is either a loop
    /// over columns producing a new frame or a runtime error depending
    /// on the operator; we conservatively report the bare atomic mode).
    pub fn arith(self, rhs: RType) -> Option<RType> {
        let mode = self.mode.arith_result(rhs.mode)?;
        let length = self.length.binary(rhs.length);
        let na = NaFlag(self.na.0 || rhs.na.0 || mode == Mode::Double);
        Some(RType {
            mode,
            length,
            na,
            // Arithmetic on S3 objects strips the class in R, and the
            // column schema is meaningless on the atomic result. The
            // function signature is likewise dropped: you cannot add
            // two closures and get a meaningful signature back.
            class: ClassVector::empty(),
            columns: None,
            fn_sig: None,
        })
    }

    /// Comparison operators return plain logical values (no class, no
    /// schema).
    pub fn compare(self, rhs: RType) -> Option<RType> {
        let mode = self.mode.compare_result(rhs.mode)?;
        let length = self.length.binary(rhs.length);
        Some(RType {
            mode,
            length,
            na: NaFlag(true),
            class: ClassVector::empty(),
            columns: None,
            fn_sig: None,
        })
    }

    /// Whether this type is definitely an error to use as a condition
    /// (`if (cond)`, `while (cond)`). R requires a length-1 logical, but
    /// will accept any length-1 atomic and silently coerce.
    pub fn invalid_condition(&self) -> bool {
        matches!(
            self.mode,
            Mode::List | Mode::Function | Mode::Opaque
        ) || matches!(self.length, Length::Zero)
    }

    /// Join (least upper bound) of two types, used when control flow
    /// merges: e.g. the result of `if (cond) x else y`. The mode follows
    /// R's coercion ladder (the higher-ranked operand wins). The length
    /// is Unknown if the two differ, otherwise the common value. NA
    /// propagates (true if either side can be NA).
    ///
    /// Opaque is absorbing: joining anything with unknown yields unknown,
    /// which is the correct conservative choice.
    ///
    /// The S3 class vector and column schema are preserved only when
    /// both sides agree exactly (including the `known` flag for class).
    /// When either side lacks information, the joined value drops that
    /// information rather than guessing.
    pub fn join(self, other: RType) -> RType {
        if matches!(self.mode, Mode::Opaque) || matches!(other.mode, Mode::Opaque) {
            return RType::UNKNOWN;
        }
        if self == other {
            return self;
        }
        let mode = if self.mode.coerce_rank() >= other.mode.coerce_rank() {
            self.mode
        } else {
            other.mode
        };
        let length = if self.length == other.length {
            self.length
        } else {
            Length::Unknown
        };
        let class = if !self.class.known || !other.class.known {
            // One side has undetermined class; we can't say.
            ClassVector::unknown()
        } else if self.class == other.class {
            self.class
        } else {
            // Both classes known but differ: result has no class.
            ClassVector::empty()
        };
        // Column schemas are preserved only on exact agreement; anything
        // else drops the schema so we don't fabricate columns the
        // runtime value might not have.
        let columns = if self.columns.is_some() && self.columns == other.columns {
            self.columns
        } else {
            None
        };
        // Function signatures are preserved only on exact agreement;
        // two branches returning closures with different signatures
        // collapse to an opaque function (signature dropped) rather
        // than silently picking one branch's signature.
        let fn_sig = if self.fn_sig.is_some() && self.fn_sig == other.fn_sig {
            self.fn_sig
        } else {
            None
        };
        RType {
            mode,
            length,
            na: NaFlag(self.na.0 || other.na.0),
            class,
            columns,
            fn_sig,
        }
    }

    /// Element type of an iterable used in `for (var in iter)`. For an
    /// atomic vector this is a length-1 value of the same mode; for a
    /// list it is a length-1 list; for opaque input it stays opaque.
    /// The class and column schema are dropped: iterating over a classed
    /// vector yields the bare elements in R.
    pub fn element(self) -> RType {
        match self.mode {
            Mode::Null => RType::new(Mode::Null, Length::Zero, false),
            Mode::Opaque => RType::UNKNOWN,
            _ => RType::new(self.mode, Length::One, self.na.0),
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
        } else if matches!(self.mode, Mode::Character | Mode::List | Mode::Function | Mode::Null)
            || matches!(other.mode, Mode::Character | Mode::List | Mode::Function | Mode::Null)
        {
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
        RType::new(mode, Length::Unknown, false)
    }
}

impl fmt::Display for RType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = self.mode;
        let len = match self.length {
            Length::Zero => "0",
            Length::One => "1",
            Length::Known(n) => {
                let core = write!(
                    f,
                    "{}<len={}>{}`",
                    mode,
                    n,
                    if self.na.0 { "?NA" } else { "" }
                );
                return self.fmt_class(f, core);
            }
            Length::Unknown => "?",
        };
        let core = write!(
            f,
            "{}<len={}>{}`",
            mode,
            len,
            if self.na.0 { "?NA" } else { "" }
        );
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
                if let Some(n) = self.class.names[i] {
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
        if let Some(schema) = self.columns {
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
        if let Some(sig) = self.fn_sig {
            write!(f, " -> {}", sig.return_type)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arith_integer_double_promotes() {
        let i = RType::scalar(Mode::Integer, false);
        let d = RType::scalar(Mode::Double, false);
        let r = i.arith(d).unwrap();
        assert_eq!(r.mode, Mode::Double);
        assert_eq!(r.length, Length::One);
    }

    #[test]
    fn arith_character_fails() {
        let s = RType::scalar(Mode::Character, false);
        let i = RType::scalar(Mode::Integer, false);
        assert!(s.arith(i).is_none());
    }

    #[test]
    fn length_recycling() {
        assert_eq!(Length::One.binary(Length::Known(5)), Length::Known(5));
        assert_eq!(
            Length::Known(4).binary(Length::Known(2)),
            Length::Known(4)
        );
        assert_eq!(Length::Zero.binary(Length::Known(5)), Length::Zero);
    }

    #[test]
    fn condition_rejects_lists() {
        let l = RType::new(Mode::List, Length::One, false);
        assert!(l.invalid_condition());
    }

    #[test]
    fn join_promotes_via_coercion_ladder() {
        let i = RType::scalar(Mode::Integer, false);
        let d = RType::scalar(Mode::Double, false);
        assert_eq!(i.join(d).mode, Mode::Double);
        assert_eq!(d.join(i).mode, Mode::Double);
    }

    #[test]
    fn join_with_opaque_is_unknown() {
        let i = RType::scalar(Mode::Integer, false);
        assert_eq!(i.join(RType::UNKNOWN), RType::UNKNOWN);
    }

    #[test]
    fn join_equal_returns_self() {
        let i = RType::scalar(Mode::Integer, false);
        assert_eq!(i.join(i), i);
    }

    #[test]
    fn join_different_lengths_unknown() {
        let a = RType::new(Mode::Integer, Length::Known(3), false);
        let b = RType::new(Mode::Integer, Length::Known(5), false);
        assert_eq!(a.join(b).length, Length::Unknown);
    }

    #[test]
    fn element_of_vector_is_scalar() {
        let v = RType::new(Mode::Integer, Length::Known(10), false);
        let e = v.element();
        assert_eq!(e.mode, Mode::Integer);
        assert_eq!(e.length, Length::One);
    }

    #[test]
    fn seq_int_int_is_integer() {
        let a = RType::scalar(Mode::Integer, false);
        let b = RType::scalar(Mode::Integer, false);
        assert_eq!(a.seq(b).mode, Mode::Integer);
    }

    #[test]
    fn seq_double_double_is_integer() {
        // R's `:` returns integer for whole-number double endpoints.
        // `1:3` produces c(1L, 2L, 3L) even though 1 and 3 are doubles.
        let a = RType::scalar(Mode::Double, false);
        let b = RType::scalar(Mode::Double, false);
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
        assert_eq!(cv.first(), Some("foo"));
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
    fn class_vector_from_static_slice_truncates_to_four() {
        let cv = ClassVector::from_static_slice(&["a", "b", "c", "d", "e"]);
        assert_eq!(cv.len, 4);
        assert_eq!(cv.first(), Some("a"));
        assert!(cv.contains("d"));
        assert!(!cv.contains("e"));
    }

    #[test]
    fn arith_strips_class() {
        let lhs = RType::scalar(Mode::Double, false).with_class(ClassVector::single("lm"));
        let rhs = RType::scalar(Mode::Double, false);
        let r = lhs.arith(rhs).unwrap();
        assert!(!r.class.is_unknown());
        assert!(!r.class.has_known_class());
        assert_eq!(r.class, ClassVector::empty());
    }

    #[test]
    fn compare_strips_class() {
        let lhs = RType::scalar(Mode::Double, false).with_class(ClassVector::single("ts"));
        let rhs = RType::scalar(Mode::Double, false);
        let r = lhs.compare(rhs).unwrap();
        assert_eq!(r.mode, Mode::Logical);
        assert!(!r.class.has_known_class());
    }

    #[test]
    fn join_preserves_class_when_both_sides_agree() {
        let class = ClassVector::single("lm");
        let lhs = RType::scalar(Mode::List, false).with_class(class);
        let rhs = RType::scalar(Mode::List, false).with_class(class);
        let joined = lhs.join(rhs);
        assert_eq!(joined.class, class);
    }

    #[test]
    fn join_drops_class_when_sides_differ() {
        let lhs = RType::scalar(Mode::List, false).with_class(ClassVector::single("lm"));
        let rhs = RType::scalar(Mode::List, false).with_class(ClassVector::single("ts"));
        let joined = lhs.join(rhs);
        assert!(!joined.class.has_known_class());
        assert_eq!(joined.class, ClassVector::empty());
    }

    #[test]
    fn join_class_unknown_when_one_side_unknown() {
        let lhs = RType::scalar(Mode::List, false)
            .with_class(ClassVector::single("lm"));
        let rhs = RType::UNKNOWN; // unknown class
        let joined = lhs.join(rhs);
        assert_eq!(joined, RType::UNKNOWN);
        assert!(joined.class.is_unknown());
    }

    #[test]
    fn rtype_display_includes_class_suffix() {
        let t = RType::scalar(Mode::List, false).with_class(ClassVector::single("lm"));
        let s = format!("{}", t);
        assert!(
            s.contains(":lm"),
            "expected `:lm` in display, got {}",
            s
        );
    }

    #[test]
    fn rtype_display_no_class_suffix_for_empty() {
        let t = RType::scalar(Mode::Integer, false);
        let s = format!("{}", t);
        assert!(
            !s.contains(':'),
            "expected no `:` suffix for classless type, got {}",
            s
        );
    }

    #[test]
    fn intern_class_name_is_idempotent() {
        let a = intern_class_name("data.frame");
        let b = intern_class_name("data.frame");
        assert_eq!(a.as_ptr(), b.as_ptr(), "same content must intern to same static");
        assert_eq!(a, "data.frame");
    }

    #[test]
    fn intern_class_name_user_defined() {
        let a = intern_class_name("zzz_user_class_123");
        assert_eq!(a, "zzz_user_class_123");
    }

    #[test]
    fn column_schema_lookups_by_name() {
        let schema = ColumnSchema {
            columns: vec![
                ("a".to_string(), RType::scalar(Mode::Integer, false)),
                ("b".to_string(), RType::scalar(Mode::Character, false)),
            ],
        };
        assert_eq!(schema.get("a").unwrap().mode, Mode::Integer);
        assert_eq!(schema.get("b").unwrap().mode, Mode::Character);
        assert!(schema.get("missing").is_none());
        assert_eq!(schema.names(), vec!["a", "b"]);
        assert_eq!(schema.len(), 2);
    }

    #[test]
    fn intern_column_schema_collapses_identical() {
        let s1 = intern_column_schema(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double, false))],
        });
        let s2 = intern_column_schema(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double, false))],
        });
        // Identical content must intern to the same static reference.
        assert!(
            std::ptr::eq(s1, s2),
            "identical schemas should intern to the same static"
        );
    }

    #[test]
    fn intern_column_schema_keeps_distinct() {
        let s1 = intern_column_schema(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double, false))],
        });
        let s2 = intern_column_schema(ColumnSchema {
            columns: vec![("y".to_string(), RType::scalar(Mode::Double, false))],
        });
        assert!(
            !std::ptr::eq(s1, s2),
            "distinct schemas should get distinct statics"
        );
    }

    #[test]
    fn rtype_with_columns_roundtrips() {
        let schema = intern_column_schema(ColumnSchema {
            columns: vec![("mpg".to_string(), RType::new(Mode::Double, Length::Known(32), false))],
        });
        let t = RType::new(Mode::List, Length::Known(1), false).with_columns(schema);
        assert_eq!(t.columns, Some(schema));
        // `with_columns` must not disturb other fields.
        assert_eq!(t.mode, Mode::List);
        assert_eq!(t.length, Length::Known(1));
    }

    #[test]
    fn arith_strips_columns() {
        let schema = intern_column_schema(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double, false))],
        });
        let lhs = RType::scalar(Mode::Double, false).with_columns(schema);
        let rhs = RType::scalar(Mode::Double, false);
        let r = lhs.arith(rhs).unwrap();
        assert_eq!(r.mode, Mode::Double);
        assert!(r.columns.is_none(), "arith must strip column schema");
    }

    #[test]
    fn compare_strips_columns() {
        let schema = intern_column_schema(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double, false))],
        });
        let lhs = RType::scalar(Mode::Double, false).with_columns(schema);
        let rhs = RType::scalar(Mode::Double, false);
        let r = lhs.compare(rhs).unwrap();
        assert_eq!(r.mode, Mode::Logical);
        assert!(r.columns.is_none(), "compare must strip column schema");
    }

    #[test]
    fn join_preserves_columns_when_both_sides_agree() {
        let schema = intern_column_schema(ColumnSchema {
            columns: vec![("x".to_string(), RType::scalar(Mode::Double, false))],
        });
        let lhs = RType::scalar(Mode::List, false).with_columns(schema);
        let rhs = RType::scalar(Mode::List, false).with_columns(schema);
        let joined = lhs.join(rhs);
        assert_eq!(joined.columns, Some(schema));
    }

    #[test]
    fn join_drops_columns_when_sides_differ() {
        let s1 = intern_column_schema(ColumnSchema {
            columns: vec![("a".to_string(), RType::scalar(Mode::Double, false))],
        });
        let s2 = intern_column_schema(ColumnSchema {
            columns: vec![("b".to_string(), RType::scalar(Mode::Double, false))],
        });
        let lhs = RType::scalar(Mode::List, false).with_columns(s1);
        let rhs = RType::scalar(Mode::List, false).with_columns(s2);
        let joined = lhs.join(rhs);
        assert!(joined.columns.is_none(), "differing schemas must drop");
    }

    #[test]
    fn rtype_display_includes_columns_abbreviated() {
        // Build a 5-column schema; display should show 3 then `...`.
        let cols: Vec<(String, RType)> = (0..5)
            .map(|i| (format!("c{}", i), RType::scalar(Mode::Double, false)))
            .collect();
        let schema = intern_column_schema(ColumnSchema { columns: cols });
        let t = RType::new(Mode::List, Length::Known(5), false).with_columns(schema);
        let s = format!("{}", t);
        assert!(s.contains("c0:"), "missing c0: {}", s);
        assert!(s.contains("c1:"), "missing c1: {}", s);
        assert!(s.contains("c2:"), "missing c2: {}", s);
        assert!(!s.contains("c3:"), "c3 should be abbreviated: {}", s);
        assert!(s.contains("..."), "missing ellipsis: {}", s);
    }

    #[test]
    fn intern_function_signature_collapses_identical() {
        let s1 = intern_function_signature(FunctionSignature {
            params: vec![RType::scalar(Mode::Double, false)],
            return_type: Box::new(RType::scalar(Mode::Integer, false)),
        });
        let s2 = intern_function_signature(FunctionSignature {
            params: vec![RType::scalar(Mode::Double, false)],
            return_type: Box::new(RType::scalar(Mode::Integer, false)),
        });
        assert!(
            std::ptr::eq(s1, s2),
            "identical signatures should intern to the same static"
        );
    }

    #[test]
    fn intern_function_signature_keeps_distinct() {
        let s1 = intern_function_signature(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer, false)),
        });
        let s2 = intern_function_signature(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Double, false)),
        });
        assert!(
            !std::ptr::eq(s1, s2),
            "distinct signatures should get distinct statics"
        );
    }

    #[test]
    fn rtype_with_fn_sig_roundtrips() {
        let sig = intern_function_signature(FunctionSignature {
            params: vec![RType::scalar(Mode::Double, false)],
            return_type: Box::new(RType::scalar(Mode::Integer, false)),
        });
        let t = RType::scalar(Mode::Function, false).with_fn_sig(sig);
        assert_eq!(t.fn_sig, Some(sig));
        assert_eq!(t.mode, Mode::Function);
    }

    #[test]
    fn rtype_default_fn_sig_is_none() {
        // All standard constructors must produce fn_sig = None so the
        // signature is opt-in only.
        assert!(RType::UNKNOWN.fn_sig.is_none());
        assert!(RType::scalar(Mode::Function, false).fn_sig.is_none());
        assert!(RType::new(Mode::Integer, Length::One, false).fn_sig.is_none());
    }

    #[test]
    fn arith_strips_fn_sig() {
        // Arithmetic on a function value is an error in R; even when
        // arith_result permits it (it doesn't for Function), the
        // signature must not survive. We exercise the strip via a
        // classed list whose schema we know survives only via join.
        let sig = intern_function_signature(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer, false)),
        });
        let lhs = RType::scalar(Mode::Integer, false).with_fn_sig(sig);
        let rhs = RType::scalar(Mode::Integer, false);
        let r = lhs.arith(rhs).unwrap();
        assert!(r.fn_sig.is_none(), "arith must strip fn_sig");
    }

    #[test]
    fn join_preserves_fn_sig_when_both_sides_agree() {
        let sig = intern_function_signature(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer, false)),
        });
        let lhs = RType::scalar(Mode::Function, false).with_fn_sig(sig);
        let rhs = RType::scalar(Mode::Function, false).with_fn_sig(sig);
        let joined = lhs.join(rhs);
        assert_eq!(joined.fn_sig, Some(sig));
    }

    #[test]
    fn join_drops_fn_sig_when_sides_differ() {
        let s1 = intern_function_signature(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer, false)),
        });
        let s2 = intern_function_signature(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Double, false)),
        });
        let lhs = RType::scalar(Mode::Function, false).with_fn_sig(s1);
        let rhs = RType::scalar(Mode::Function, false).with_fn_sig(s2);
        let joined = lhs.join(rhs);
        assert!(joined.fn_sig.is_none(), "differing sigs must drop");
    }

    #[test]
    fn rtype_display_includes_fn_sig() {
        let sig = intern_function_signature(FunctionSignature {
            params: vec![],
            return_type: Box::new(RType::scalar(Mode::Integer, false)),
        });
        let t = RType::scalar(Mode::Function, false).with_fn_sig(sig);
        let s = format!("{}", t);
        assert!(s.contains("->"), "missing -> in display: {}", s);
        assert!(s.contains("integer"), "missing return type: {}", s);
    }
}
