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

/// Malformed or unknown-rule ry ignore directive.
pub const INVALID_IGNORE: &str = "RY112";
/// Opt-in audit for selective ignores that matched nothing.
pub const UNUSED_IGNORE: &str = "RY113";

/// Rules omitted from normal output unless explicitly selected with a
/// severity override. Keep this policy in the registry so every checker
/// entry point and CLI output path agrees.
pub fn enabled_by_default(code: &str) -> bool {
    !matches!(code, "RY003" | UNUSED_IGNORE)
}

/// Directive diagnostics take severity overrides only: an ignore comment
/// must not hide a finding about ignore comments.
pub fn suppressible(code: &str) -> bool {
    !matches!(code, INVALID_IGNORE | UNUSED_IGNORE)
}

/// All rules currently emitted by the checker. Keep codes lexicographic.
pub const RULES: &[Rule] = &[
    Rule {
        code: "RY000",
        name: "syntax-error",
        default_severity: Severity::Error,
        summary: "Unparseable input, or input R's parser rejects: a region tree-sitter could not recover, a file ry cannot read or parse at all (such as one past ry's syntax nesting limits), source bytes that are not valid UTF-8, a leading UTF-8 byte order mark, or syntax base R rejects at parse time (such as an invalid native-pipe right-hand side). A file with any RY000 reports only its RY000s, because diagnostics derived from the repaired or transcoded tree are unreliable.",
    },
    Rule {
        code: "RY001",
        name: "invalid-condition",
        default_severity: Severity::Warning,
        summary: "`if` / `while` condition is not a length-1 logical or a value R coerces to one. Also covers a `switch(EXPR, ...)` whose EXPR is provably not a length-1 vector, the crash a NULL component of a union return produces (\"EXPR must be a length 1 vector\").",
    },
    Rule {
        code: "RY002",
        name: "condition-length",
        default_severity: Severity::Warning,
        summary: "`if` condition length is known to be greater than 1. R requires a single value and errors on longer conditions.",
    },
    Rule {
        code: "RY003",
        name: "numeric-condition",
        default_severity: Severity::Info,
        summary: "`if` / `while` condition is numeric; R coerces nonzero to TRUE. Legal but implicit — prefer an explicit comparison.",
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
        summary: "`&&` and `||` require single values and error on operands of length greater than 1. Use `&`/`|` for vectorized operations.",
    },
    Rule {
        code: "RY033",
        name: "comparison-mode-mismatch",
        default_severity: Severity::Warning,
        summary: "Ordering a character value against a non-character value with `<`, `>`, `<=`, or `>=` coerces the other value to character and compares lexicographically, which is almost always unintended.",
    },
    Rule {
        code: "RY034",
        name: "compare-na",
        default_severity: Severity::Warning,
        summary: "Comparing with `NA` using `==` or `!=` always produces `NA`. Use `is.na()` instead.",
    },
    Rule {
        code: "RY040",
        name: "invalid-arithmetic",
        default_severity: Severity::Error,
        summary: "Arithmetic operator between incompatible types.",
    },
    Rule {
        code: "RY041",
        name: "non-divisible-recycling",
        default_severity: Severity::Warning,
        summary: "Vector lengths do not divide evenly, so R recycles values with a warning and may produce unintended results.",
    },
    Rule {
        code: "RY042",
        name: "factor-arithmetic",
        default_severity: Severity::Warning,
        summary: "Arithmetic on factors produces missing values. Operate on levels or convert explicitly.",
    },
    Rule {
        code: "RY050",
        name: "missing-s3-method",
        default_severity: Severity::Warning,
        summary: "S3 generic called on a value with no defined method for its class.",
    },
    Rule {
        code: "RY051",
        name: "incompatible-s3-operator-methods",
        default_severity: Severity::Warning,
        summary: "Both proven chooseOpsMethod results reject distinct S3 operator methods, so R warns and uses the primitive operator.",
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
        summary: "A call uses a non-function value after the lookup ry can resolve. Bare-name lookup skips known non-functions; outer-frame and S7 cases can still produce false positives. Value expressions such as `42()` are checked directly.",
    },
    Rule {
        code: "RY080",
        name: "map-return-type-mismatch",
        default_severity: Severity::Error,
        summary: "A typed-map callback returns an incompatible mode or a result whose known length is not one. R rejects that callback result.",
    },
    Rule {
        code: "RY090",
        name: "unknown-argument",
        default_severity: Severity::Warning,
        summary: "A named call argument does not match any formal parameter after R's exact and partial argument matching.",
    },
    Rule {
        code: "RY091",
        name: "missing-required-argument",
        default_severity: Severity::Warning,
        summary: "A required formal parameter has no supplied value, including an explicitly omitted actual.",
    },
    Rule {
        code: "RY092",
        name: "argument-type-mismatch",
        default_severity: Severity::Error,
        summary: "A call argument has a known mode incompatible with the parameter type declared by the resolved signature, or with the single base operation a project function's body applies to it.",
    },
    Rule {
        code: "RY093",
        name: "comparison-inside-length",
        default_severity: Severity::Warning,
        summary: "A comparison directly inside `length()` is usually a parenthesization mistake.",
    },
    Rule {
        code: "RY094",
        name: "printf-argument-count",
        default_severity: Severity::Warning,
        summary: "A literal printf-family format string has more conversions than supplied value arguments.",
    },
    // RY095 (negation-comparison-precedence) is retired, not reusable: it
    // assumed C precedence, but R parses `!x == y` as `!(x == y)`.
    Rule {
        code: "RY096",
        name: "hasarg-non-formal",
        default_severity: Severity::Warning,
        summary: "`hasArg()` names a parameter that is not a formal of an enclosing function without `...`.",
    },
    Rule {
        code: "RY097",
        name: "not-r-source",
        default_severity: Severity::Info,
        summary: "File does not appear to be R source; diagnostics suppressed.",
    },
    Rule {
        code: "RY098",
        name: "default-forced-before-assignment",
        default_severity: Severity::Warning,
        summary: "A parameter default references a body-local that may not be assigned yet on some execution path, or references its own formal in a body that provably forces the promise.",
    },
    Rule {
        code: "RY099",
        name: "discarded-conditional-value",
        default_severity: Severity::Warning,
        summary: "A value-producing expression in a non-tail one-arm `if` is discarded, commonly because an assignment was omitted.",
    },
    Rule {
        code: "RY100",
        name: "comparison-inside-math-call",
        default_severity: Severity::Warning,
        summary: "A comparison directly inside a numeric math function is usually a parenthesization mistake (`abs(x > y)` instead of `abs(x) > y`).",
    },
    Rule {
        code: "RY101",
        name: "identical-list-subset-scalar",
        default_severity: Severity::Warning,
        summary: "`identical()` compares a single-bracket list subset with an atomic scalar; the subset remains a list, so the result is always FALSE. Use `[[` to extract the element.",
    },
    Rule {
        code: "RY102",
        name: "named-list-element-arrow",
        default_severity: Severity::Warning,
        summary: "`<-` where `=` was meant inside `list()`/`c()`/`data.frame()`/`structure()`. The element is created without a name and a stray binding is assigned as a side effect.",
    },
    Rule {
        code: "RY103",
        name: "class-equality",
        default_severity: Severity::Warning,
        summary: "`class(x)` compared with `==`/`!=` in a length-1 logical context. `class()` returns a character vector, so a multi-class object makes `if`/`&&` error. Use `inherits()`.",
    },
    Rule {
        code: "RY105",
        name: "constant-length-comparison",
        default_severity: Severity::Warning,
        summary: "`length()` of a value that is length-1 by construction, compared with a literal. The comparison has a constant result, so the guard is dead.",
    },
    Rule {
        code: "RY106",
        name: "ifelse-mode-collapse",
        default_severity: Severity::Warning,
        summary: "`ifelse()` builds its result from `test`, so a zero-length or all-NA test yields a logical result even when `yes`/`no` agree on another mode — in particular for typed-NA selects. Use a typed alternative such as `dplyr::if_else()`.",
    },
    Rule {
        code: "RY107",
        name: "any-all-scalar-comparison",
        default_severity: Severity::Warning,
        summary: "`any()` or `all()` compared with a numeric literal, as in `any(x) == 0`. The call returns one logical value, so the comparison negates it or has a constant result. Write `any(x == 0)`. Comparisons that keep the call's value (`== 1`, `!= 0`, `> 0`, `>= 1`) are not reported. Silent when the argument has a known class, because an `any`, `all`, or `Summary` method can return any value.",
    },
    Rule {
        code: "RY108",
        name: "seq-defaulted-forward",
        default_severity: Severity::Warning,
        summary: "A `seq.*` method uses its defaulted `to` without a `missing(to)` check, so a call like `seq(x, length.out = n)` forwards the default as if supplied and hits `seq.default`'s argument precedence; guard the use with `missing(to)`, as `seq.Date` does.",
    },
    Rule {
        code: "RY109",
        name: "self-referential-default",
        default_severity: Severity::Warning,
        summary: "A formal's default refers to the formal itself, as in `copy = copy` or `n = n + 1`. Evaluating that default fails in R with \"promise already under evaluation\". A call that supplies the argument is unaffected. When the body provably forces the formal, RY098 reports it instead. Silent for a default that refers to another formal (`x = y`), a default that quotes the formal (`substitute(x)`), and a body that captures the formal (`enquo(x)`, `{{ x }}`).",
    },
    Rule {
        code: "RY110",
        name: "vacuous-all-guard",
        default_severity: Severity::Warning,
        summary: "A guard such as `is.numeric(x) || all(is.na(x))` accepts zero-length input that fails the type predicate, because `all()` of an empty vector is `TRUE`. Reported only when the same function then passes `x` to a parameter whose mode a stub declares, where the empty value fails. For example, the Math group errors, and `mean()` warns and returns `NA`. Write `is.numeric(x) || (length(x) > 0 && all(is.na(x)))`. Also reported for a single-formal helper in the same file that returns the guard, when the demanding function calls it directly, in `stopifnot()`, or through a `map`-family call reduced with `all()`. Silent when one function validates and another demands, or when the helper is in another file.",
    },
    Rule {
        code: "RY111",
        name: "constant-argument-shadowing",
        default_severity: Severity::Warning,
        summary: "A call passes literal `TRUE` or `FALSE` to a formal that an enclosing function exposes under the same name, as in `na.rm = TRUE` inside `function(x, na.rm = FALSE)`, and that function never reads the formal. The caller's value is ignored. Forward it with `na.rm = na.rm`. Reported only when the name exactly matches a formal of the resolved callee. Silent for partial names, unknown callees, `T`, `F`, `NA`, and numeric or string constants.",
    },
    Rule {
        code: "RY112",
        name: "invalid-ignore",
        default_severity: Severity::Warning,
        summary: "A ry-owned inline ignore has malformed brackets or names an unknown rule. It cannot suppress diagnostics; correct the spelling or remove the directive. Foreign-only noqa lists are left to their owning tool and do not suppress ry findings.",
    },
    Rule {
        code: "RY113",
        name: "unused-ignore",
        default_severity: Severity::Warning,
        summary: "Opt-in audit: a valid ry-owned selective line or standalone ignore names an enabled rule whose local syntax was checked in diagnostic mode and produced no matching finding before baseline or confidence filtering. The initial supported codes are RY034 and RY102. Invalid, bare, noqa, and file directives are outside this audit; parser errors, anonymous function bodies, and other unavailable analysis are left unaudited. Enable with `--warn RY113` or a severity override.",
    },
    Rule {
        code: "RY114",
        name: "declaration-mismatch",
        default_severity: Severity::Warning,
        summary: "An independently known argument, default, or return value contradicts an explicitly adopted source declaration. For typehint's simple class clauses this compares effective R class(), not storage typeof(); unknown or incomplete facts remain silent.",
    },
    Rule {
        code: "RY115",
        name: "declaration-incomplete",
        default_severity: Severity::Info,
        summary: "An adopted declaration has an unsupported residual or a predicate that body inference cannot represent. The source expression is retained without claiming complete checking.",
    },
    Rule {
        code: "RY116",
        name: "declaration-conflict",
        default_severity: Severity::Warning,
        summary: "Multiple adopted records for one lexical function cannot be selected as one complete contract; none is used for checking.",
    },
    Rule {
        code: "RY117",
        name: "declaration-invalid",
        default_severity: Severity::Warning,
        summary: "An explicitly adopted source annotation is malformed or cannot be attached unambiguously to its lexical function; it is not used for checking.",
    },
    Rule {
        code: "RY118",
        name: "missing-box-export",
        default_severity: Severity::Warning,
        summary: "A statically resolved local box module has a complete export inventory, but a selected import or exact `$` access names no export. Missing modules, dynamic export declarations, and package inventories are not proof of absence and stay silent.",
    },
    Rule {
        code: "RY119",
        name: "integer-coercion-range-loss",
        default_severity: Severity::Warning,
        summary: "`as.integer()` converts a proven out-of-range numeric value to `NA_integer_`; handle the new NA or constrain the input before casting.",
    },
    Rule {
        code: "RY120",
        name: "report-boundary",
        default_severity: Severity::Warning,
        summary: "An opted-in report has a malformed R fence or exceeds the bounded extraction budget; affected R chunks are not analyzed.",
    },
    Rule {
        code: "RY121",
        name: "report-execution-unknown",
        default_severity: Severity::Warning,
        summary: "A report execution option cannot be determined statically; analysis stops at that chunk rather than guessing which later bindings exist.",
    },
    Rule {
        code: "RY122",
        name: "testthat-self-comparison",
        default_severity: Severity::Warning,
        summary: "A resolved testthat `expect_equal()`/`expect_identical()` compares the same conservatively deterministic expression to itself, so it cannot check two different values. Silent for differing operands, RNG/time/I/O or unknown calls, masked assertion functions, and `$`/`[[` access whose receiver is not proven plain data.",
    },
    Rule {
        code: "RY123",
        name: "failed-namespace-access",
        default_severity: Severity::Warning,
        summary: "A `pkg::name` or `pkg:::name` access runs on the branch where `requireNamespace(\"pkg\")` returned `FALSE`, so the namespace is not loaded and the access errors instead of selecting the fallback. Reported only for a literal package name whose `base::requireNamespace` result dominates the access.",
    },
];

pub fn find(code: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.code == code || r.name == code)
}

/// All rule codes, expanded for the `all` shorthand in CLI severity filters.
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
