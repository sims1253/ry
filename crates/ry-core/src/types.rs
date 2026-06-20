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
}
