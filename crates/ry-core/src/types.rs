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

/// A fully-described R type at the granularity v1 cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RType {
    pub mode: Mode,
    pub length: Length,
    pub na: NaFlag,
}

impl RType {
    pub const UNKNOWN: RType = RType {
        mode: Mode::Opaque,
        length: Length::Unknown,
        na: NaFlag(true),
    };

    pub const fn new(mode: Mode, length: Length, na: bool) -> Self {
        RType {
            mode,
            length,
            na: NaFlag(na),
        }
    }

    /// A scalar literal of the given mode.
    pub const fn scalar(mode: Mode, na: bool) -> Self {
        Self::new(mode, Length::One, na)
    }

    /// Result of `lhs op rhs` for an arithmetic operator, or None if the
    /// mode combination is invalid (e.g. list + numeric).
    pub fn arith(self, rhs: RType) -> Option<RType> {
        let mode = self.mode.arith_result(rhs.mode)?;
        let length = self.length.binary(rhs.length);
        let na = NaFlag(self.na.0 || rhs.na.0 || mode == Mode::Double);
        Some(RType { mode, length, na })
    }

    pub fn compare(self, rhs: RType) -> Option<RType> {
        let mode = self.mode.compare_result(rhs.mode)?;
        let length = self.length.binary(rhs.length);
        Some(RType {
            mode,
            length,
            na: NaFlag(true),
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
        RType::new(mode, length, self.na.0 || other.na.0)
    }

    /// Element type of an iterable used in `for (var in iter)`. For an
    /// atomic vector this is a length-1 value of the same mode; for a
    /// list it is a length-1 list; for opaque input it stays opaque.
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
                return write!(f, "{}<len={}>{}`", mode, n, if self.na.0 { "?NA" } else { "" });
            }
            Length::Unknown => "?",
        };
        write!(f, "{}<len={}>{}`", mode, len, if self.na.0 { "?NA" } else { "" })
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
}
