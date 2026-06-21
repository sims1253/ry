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

/// A fully-described R type at the granularity v1 cares about. Includes
/// an optional S3 class vector; arithmetic / comparison strip the class
/// (matching R), and control-flow `join` keeps it only when both sides
/// agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RType {
    pub mode: Mode,
    pub length: Length,
    pub na: NaFlag,
    pub class: ClassVector,
}

impl RType {
    pub const UNKNOWN: RType = RType {
        mode: Mode::Opaque,
        length: Length::Unknown,
        na: NaFlag(true),
        class: ClassVector::unknown(),
    };

    pub const fn new(mode: Mode, length: Length, na: bool) -> Self {
        RType {
            mode,
            length,
            na: NaFlag(na),
            class: ClassVector::empty(),
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

    /// Result of `lhs op rhs` for an arithmetic operator, or None if the
    /// mode combination is invalid (e.g. list + numeric). Arithmetic
    /// strips any S3 class attribute, matching R's actual semantics.
    pub fn arith(self, rhs: RType) -> Option<RType> {
        let mode = self.mode.arith_result(rhs.mode)?;
        let length = self.length.binary(rhs.length);
        let na = NaFlag(self.na.0 || rhs.na.0 || mode == Mode::Double);
        Some(RType {
            mode,
            length,
            na,
            // Arithmetic on S3 objects usually strips the class in R.
            class: ClassVector::empty(),
        })
    }

    /// Comparison operators return plain logical values (no class).
    pub fn compare(self, rhs: RType) -> Option<RType> {
        let mode = self.mode.compare_result(rhs.mode)?;
        let length = self.length.binary(rhs.length);
        Some(RType {
            mode,
            length,
            na: NaFlag(true),
            class: ClassVector::empty(),
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
    /// The S3 class vector is preserved only when both sides agree
    /// (including the `known` flag). When either side is `unknown`, the
    /// joined class is also `unknown` (we can't say anything definitive).
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
        RType {
            mode,
            length,
            na: NaFlag(self.na.0 || other.na.0),
            class,
        }
    }

    /// Element type of an iterable used in `for (var in iter)`. For an
    /// atomic vector this is a length-1 value of the same mode; for a
    /// list it is a length-1 list; for opaque input it stays opaque.
    /// The class is dropped: iterating over a classed vector yields the
    /// bare elements in R.
    pub fn element(self) -> RType {
        match self.mode {
            Mode::Null => RType::new(Mode::Null, Length::Zero, false),
            Mode::Opaque => RType::UNKNOWN,
            _ => RType::new(self.mode, Length::One, self.na.0),
        }
    }

    /// Result of the `:` sequence operator `from:to`. Always integer in
    /// R when both operands are integer-like (incl. logical), otherwise
    /// the result is still integer-ish: R actually returns integer for
    /// `1:3` but double for `1.5:3.5`. We follow R here.
    pub fn seq(self, other: RType) -> RType {
        let mode = if matches!(self.mode, Mode::Integer | Mode::Logical)
            && matches!(other.mode, Mode::Integer | Mode::Logical)
        {
            Mode::Integer
        } else if matches!(self.mode, Mode::Opaque) || matches!(other.mode, Mode::Opaque) {
            Mode::Opaque
        } else {
            Mode::Double
        };
        // Length depends on the runtime values; can't be known statically
        // except for literal operands, which the checker special-cases.
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
    /// Append the `:class` suffix to a partially-written `RType`. Used
    /// by `Display` so the class annotation lands after the mode/length.
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
    fn seq_double_double_is_double() {
        let a = RType::scalar(Mode::Double, false);
        let b = RType::scalar(Mode::Double, false);
        assert_eq!(a.seq(b).mode, Mode::Double);
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
}
