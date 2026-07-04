//! Rule registry. Each diagnostic code has a stable identifier, default
//! severity, and a short human-readable summary used by `ry explain rule`
//! and by `--error` / `--warn` / `--ignore` filters.

use crate::Severity;

#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub code: &'static str,
    pub name: &'static str,
    pub default_severity: Severity,
    pub summary: &'static str,
}

/// All rules currently emitted by the checker. Keep codes lexicographic.
pub const RULES: &[Rule] = &[
    Rule {
        code: "RY000",
        name: "syntax-error",
        default_severity: Severity::Error,
        summary: "Unparseable input. tree-sitter could not recover this region; subsequent diagnostics may be unreliable.",
    },
    Rule {
        code: "RY001",
        name: "invalid-condition",
        default_severity: Severity::Warning,
        summary: "`if` / `while` condition is not a length-1 logical.",
    },
    Rule {
        code: "RY002",
        name: "condition-length",
        default_severity: Severity::Warning,
        summary: "`if` condition has length > 1; only the first element is used.",
    },
    Rule {
        code: "RY010",
        name: "unbound-variable",
        default_severity: Severity::Warning,
        summary: "Reference to a variable with no binding in scope.",
    },
    Rule {
        code: "RY020",
        name: "unary-minus-type",
        default_severity: Severity::Error,
        summary: "Unary `-` applied to a non-numeric type.",
    },
    Rule {
        code: "RY021",
        name: "unary-not-type",
        default_severity: Severity::Error,
        summary: "Unary `!` applied to a non-coercible-to-logical type.",
    },
    Rule {
        code: "RY030",
        name: "invalid-comparison",
        default_severity: Severity::Error,
        summary: "Comparison between types with no defined ordering.",
    },
    Rule {
        code: "RY031",
        name: "invalid-logical-op",
        default_severity: Severity::Error,
        summary: "`&` / `|` / `&&` / `||` applied to non-coercible types.",
    },
    Rule {
        code: "RY032",
        name: "scalar-logical-length",
        default_severity: Severity::Warning,
        summary: "`&&` and `||` only use the first element of their operands; using them with vectors of length > 1 is almost always a bug. Use `&`/`|` for vectorized operations.",
    },
    Rule {
        code: "RY033",
        name: "comparison-mode-mismatch",
        default_severity: Severity::Warning,
        summary: "Comparing a character value with a numeric value is valid R but almost always unintended. R compares byte values, not semantic equality.",
    },
    Rule {
        code: "RY040",
        name: "invalid-arithmetic",
        default_severity: Severity::Error,
        summary: "Arithmetic operator between incompatible types.",
    },
    Rule {
        code: "RY050",
        name: "missing-s3-method",
        default_severity: Severity::Warning,
        summary: "S3 generic called on a value with no defined method for its class.",
    },
    Rule {
        code: "RY060",
        name: "undefined-column",
        default_severity: Severity::Error,
        summary: "Column access on a value whose schema does not contain that column.",
    },
    Rule {
        code: "RY061",
        name: "dollar-on-atomic",
        default_severity: Severity::Error,
        summary: "The $ operator is invalid for atomic vectors (integer, double, character, logical). It only works on list-like types (lists, data frames, environments).",
    },
    Rule {
        code: "RY070",
        name: "call-non-function",
        default_severity: Severity::Error,
        summary: "A non-function value (a variable bound to a non-function, or a literal like `42()`) is being called as a function. R will error at runtime ('attempt to apply non-function' / 'could not find function').",
    },
];

pub fn find(code: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.code == code || r.name == code)
}

/// Severity for `all` shorthand in CLI filters.
pub fn all_codes() -> Vec<&'static str> {
    RULES.iter().map(|r| r.code).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_by_code() {
        assert_eq!(find("RY040").unwrap().name, "invalid-arithmetic");
    }

    #[test]
    fn find_by_name() {
        assert_eq!(find("unbound-variable").unwrap().code, "RY010");
    }

    #[test]
    fn rules_are_sorted_by_code() {
        let codes: Vec<&str> = RULES.iter().map(|r| r.code).collect();
        let mut sorted = codes.clone();
        sorted.sort();
        assert_eq!(codes, sorted);
    }
}
