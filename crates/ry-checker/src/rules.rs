//! Rule registry. Each diagnostic code has a stable identifier, default
//! severity, a short human-readable summary, and a worked example used by
//! `ry explain rule` and by `--error` / `--warn` / `--ignore` filters.

use crate::Severity;

#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub code: &'static str,
    pub name: &'static str,
    pub default_severity: Severity,
    pub summary: &'static str,
    /// Source of the rule's worked example, in the format that
    /// [`RuleExample::parse`] reads. Use [`Rule::example`].
    pub example_source: &'static str,
}

impl Rule {
    /// The rule's worked example. ry-cli's `rule_examples` test checks that
    /// the flagged file reports only this rule and the fixed file reports
    /// nothing.
    pub fn example(&self) -> RuleExample {
        RuleExample::parse(self.example_source)
            .unwrap_or_else(|error| panic!("{}: malformed rule example: {error}", self.code))
    }
}

/// A small project that triggers one rule, and the same project fixed.
///
/// The source format is a `fix:` line followed by files, each introduced by
/// a `--- <path>` line. `flagged.<ext>` and `fixed.<ext>` are the two
/// versions of the example file; both must use the same extension. Any other
/// path is a support file, such as `ry.toml`, present in both versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleExample {
    /// One sentence on how to fix the flagged code.
    pub fix: &'static str,
    /// Extension of the example file, such as `R` or `Rmd`.
    pub extension: &'static str,
    pub flagged: &'static str,
    pub fixed: &'static str,
    /// Other project files, as `(relative path, contents)`.
    pub support: Vec<(&'static str, &'static str)>,
}

impl RuleExample {
    pub fn parse(source: &'static str) -> Result<Self, String> {
        let source = source.strip_suffix('\n').unwrap_or(source);
        let (head, body) = source
            .split_once("\n--- ")
            .ok_or("expected at least one `--- <path>` section")?;
        let fix = head
            .strip_prefix("fix: ")
            .ok_or("the first line must start with `fix: `")?
            .trim();
        if fix.is_empty() || fix.contains('\n') {
            return Err("the fix must be one non-empty line".into());
        }
        let mut flagged = None;
        let mut fixed = None;
        let mut support = Vec::new();
        for section in body.split("\n--- ") {
            let (path, contents) = section.split_once('\n').unwrap_or((section, ""));
            let path = path.trim();
            let contents = contents.trim_end_matches('\n');
            match path.split_once('.') {
                Some(("flagged", ext)) => flagged = Some((ext, contents)),
                Some(("fixed", ext)) => fixed = Some((ext, contents)),
                _ if path.is_empty() || path.starts_with('/') || path.contains("..") => {
                    return Err(format!("invalid support path `{path}`"));
                }
                _ => support.push((path, contents)),
            }
        }
        let (extension, flagged) = flagged.ok_or("missing `--- flagged.<ext>` section")?;
        let (fixed_extension, fixed) = fixed.ok_or("missing `--- fixed.<ext>` section")?;
        if extension != fixed_extension {
            return Err("flagged and fixed files must share an extension".into());
        }
        Ok(RuleExample {
            fix,
            extension,
            flagged,
            fixed,
            support,
        })
    }
}

/// Malformed or unknown-rule ry ignore directive.
pub const INVALID_IGNORE: &str = "RY112";
/// Opt-in audit for selective ignores that matched nothing.
pub const UNUSED_IGNORE: &str = "RY113";
/// Source nested deeper than ry analyzes; the file is not checked.
pub const NESTING_LIMIT: &str = "RY129";

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
        summary: "Unparseable input, or input R's parser rejects: a region tree-sitter could not recover, a file ry cannot read or parse at all (such as one nested past R's 49-bracket limit), source bytes that are not valid UTF-8, a leading UTF-8 byte order mark, or syntax base R rejects at parse time (such as an invalid native-pipe right-hand side). A file with any RY000 reports only its RY000s, because diagnostics derived from the repaired or transcoded tree are unreliable.",
        example_source: include_str!("rule_examples/RY000.txt"),
    },
    Rule {
        code: "RY001",
        name: "invalid-condition",
        default_severity: Severity::Warning,
        summary: "`if` / `while` condition is not a length-1 logical or a value R coerces to one. Includes proven scalar `NA` conditions (an `NA` literal or a closed literal shape over one), which R always rejects. Also covers `switch(EXPR, ...)` with an EXPR provably not a length-1 vector, including the NULL component of a union return (\"EXPR must be a length 1 vector\").",
        example_source: include_str!("rule_examples/RY001.txt"),
    },
    Rule {
        code: "RY002",
        name: "condition-length",
        default_severity: Severity::Warning,
        summary: "`if` condition length is known to be greater than 1: an exact length, or a proven lower bound such as a passing `length(x) > 1L` guard or a join of longer vectors. R requires a single value and errors on longer conditions.",
        example_source: include_str!("rule_examples/RY002.txt"),
    },
    Rule {
        code: "RY003",
        name: "numeric-condition",
        default_severity: Severity::Info,
        summary: "`if` / `while` condition is numeric; R coerces nonzero to TRUE. Legal but implicit — prefer an explicit comparison.",
        example_source: include_str!("rule_examples/RY003.txt"),
    },
    Rule {
        code: "RY010",
        name: "unbound-variable",
        default_severity: Severity::Warning,
        summary: "Reference to a variable with no binding in scope.",
        example_source: include_str!("rule_examples/RY010.txt"),
    },
    Rule {
        code: "RY020",
        name: "unary-minus-type",
        default_severity: Severity::Error,
        summary: "Unary `-` applied to a non-numeric type.",
        example_source: include_str!("rule_examples/RY020.txt"),
    },
    Rule {
        code: "RY021",
        name: "unary-not-type",
        default_severity: Severity::Error,
        summary: "Unary `!` applied to a non-coercible-to-logical type.",
        example_source: include_str!("rule_examples/RY021.txt"),
    },
    Rule {
        code: "RY030",
        name: "invalid-comparison",
        default_severity: Severity::Error,
        summary: "Comparison between types with no defined ordering.",
        example_source: include_str!("rule_examples/RY030.txt"),
    },
    Rule {
        code: "RY031",
        name: "invalid-logical-op",
        default_severity: Severity::Error,
        summary: "`&` / `|` / `&&` / `||` applied to non-coercible types.",
        example_source: include_str!("rule_examples/RY031.txt"),
    },
    Rule {
        code: "RY032",
        name: "scalar-logical-length",
        default_severity: Severity::Warning,
        summary: "`&&` and `||` require single values and error on operands of length greater than 1. Use `&`/`|` for vectorized operations.",
        example_source: include_str!("rule_examples/RY032.txt"),
    },
    Rule {
        code: "RY033",
        name: "comparison-mode-mismatch",
        default_severity: Severity::Warning,
        summary: "Ordering a character value against a non-character value with `<`, `>`, `<=`, or `>=` coerces the other value to character and compares lexicographically, which is almost always unintended.",
        example_source: include_str!("rule_examples/RY033.txt"),
    },
    Rule {
        code: "RY034",
        name: "compare-na",
        default_severity: Severity::Warning,
        summary: "Comparing with `NA` using `==` or `!=` always produces `NA`. Use `is.na()` instead.",
        example_source: include_str!("rule_examples/RY034.txt"),
    },
    Rule {
        code: "RY040",
        name: "invalid-arithmetic",
        default_severity: Severity::Error,
        summary: "Arithmetic operator between incompatible types.",
        example_source: include_str!("rule_examples/RY040.txt"),
    },
    Rule {
        code: "RY041",
        name: "non-divisible-recycling",
        default_severity: Severity::Warning,
        summary: "Vector lengths do not divide evenly, so R recycles values with a warning and may produce unintended results.",
        example_source: include_str!("rule_examples/RY041.txt"),
    },
    Rule {
        code: "RY042",
        name: "factor-arithmetic",
        default_severity: Severity::Warning,
        summary: "Arithmetic on factors produces missing values. Operate on levels or convert explicitly.",
        example_source: include_str!("rule_examples/RY042.txt"),
    },
    Rule {
        code: "RY050",
        name: "missing-s3-method",
        default_severity: Severity::Warning,
        summary: "S3 generic called on a value with no defined method for its class.",
        example_source: include_str!("rule_examples/RY050.txt"),
    },
    Rule {
        code: "RY051",
        name: "incompatible-s3-operator-methods",
        default_severity: Severity::Warning,
        summary: "Both proven chooseOpsMethod results reject distinct S3 operator methods, so R warns and uses the primitive operator.",
        example_source: include_str!("rule_examples/RY051.txt"),
    },
    Rule {
        code: "RY060",
        name: "undefined-column",
        default_severity: Severity::Error,
        summary: "Column access on a value whose schema does not contain that column.",
        example_source: include_str!("rule_examples/RY060.txt"),
    },
    Rule {
        code: "RY061",
        name: "dollar-on-atomic",
        default_severity: Severity::Error,
        summary: "The $ operator is invalid for atomic vectors (integer, double, character, logical). It only works on list-like types (lists, data frames, environments).",
        example_source: include_str!("rule_examples/RY061.txt"),
    },
    Rule {
        code: "RY070",
        name: "call-non-function",
        default_severity: Severity::Error,
        summary: "A call uses a non-function value after the lookup ry can resolve. Bare-name lookup skips known non-functions; outer-frame and S7 cases can still produce false positives. Value expressions such as `42()` are checked directly.",
        example_source: include_str!("rule_examples/RY070.txt"),
    },
    Rule {
        code: "RY080",
        name: "map-return-type-mismatch",
        default_severity: Severity::Error,
        summary: "A typed-map callback returns an incompatible mode or a result whose known length is not one. R rejects that callback result.",
        example_source: include_str!("rule_examples/RY080.txt"),
    },
    Rule {
        code: "RY090",
        name: "unknown-argument",
        default_severity: Severity::Warning,
        summary: "A named call argument does not match any formal parameter after R's exact and partial argument matching.",
        example_source: include_str!("rule_examples/RY090.txt"),
    },
    Rule {
        code: "RY091",
        name: "missing-required-argument",
        default_severity: Severity::Warning,
        summary: "A required formal parameter has no supplied value, including an explicitly omitted actual.",
        example_source: include_str!("rule_examples/RY091.txt"),
    },
    Rule {
        code: "RY092",
        name: "argument-type-mismatch",
        default_severity: Severity::Error,
        summary: "A call argument has a known mode incompatible with the parameter type declared by the resolved signature, or with the single base operation a project function's body applies to it.",
        example_source: include_str!("rule_examples/RY092.txt"),
    },
    Rule {
        code: "RY093",
        name: "comparison-inside-length",
        default_severity: Severity::Warning,
        summary: "A comparison directly inside `length()` is usually a parenthesization mistake.",
        example_source: include_str!("rule_examples/RY093.txt"),
    },
    Rule {
        code: "RY094",
        name: "printf-argument-count",
        default_severity: Severity::Warning,
        summary: "A literal printf-family format string has more conversions than supplied value arguments.",
        example_source: include_str!("rule_examples/RY094.txt"),
    },
    // RY095 (negation-comparison-precedence) is retired, not reusable: it
    // assumed C precedence, but R parses `!x == y` as `!(x == y)`.
    Rule {
        code: "RY096",
        name: "hasarg-non-formal",
        default_severity: Severity::Warning,
        summary: "`hasArg()` names a parameter that is not a formal of an enclosing function without `...`.",
        example_source: include_str!("rule_examples/RY096.txt"),
    },
    Rule {
        code: "RY097",
        name: "not-r-source",
        default_severity: Severity::Info,
        summary: "File does not appear to be R source; diagnostics suppressed.",
        example_source: include_str!("rule_examples/RY097.txt"),
    },
    Rule {
        code: "RY098",
        name: "default-forced-before-assignment",
        default_severity: Severity::Warning,
        summary: "A parameter default references a body-local that may not be assigned yet on some execution path, or references its own formal in a body that provably forces the promise.",
        example_source: include_str!("rule_examples/RY098.txt"),
    },
    Rule {
        code: "RY099",
        name: "discarded-conditional-value",
        default_severity: Severity::Warning,
        summary: "A value-producing expression in a non-tail one-arm `if` is discarded, commonly because an assignment was omitted.",
        example_source: include_str!("rule_examples/RY099.txt"),
    },
    Rule {
        code: "RY100",
        name: "comparison-inside-math-call",
        default_severity: Severity::Warning,
        summary: "A comparison directly inside a numeric math function is usually a parenthesization mistake (`abs(x > y)` instead of `abs(x) > y`).",
        example_source: include_str!("rule_examples/RY100.txt"),
    },
    Rule {
        code: "RY101",
        name: "identical-list-subset-scalar",
        default_severity: Severity::Warning,
        summary: "`identical()` compares a single-bracket list subset with an atomic scalar; the subset remains a list, so the result is always FALSE. Use `[[` to extract the element.",
        example_source: include_str!("rule_examples/RY101.txt"),
    },
    Rule {
        code: "RY102",
        name: "named-list-element-arrow",
        default_severity: Severity::Warning,
        summary: "`<-` where `=` was meant inside `list()`/`c()`/`data.frame()`/`structure()`. The element is created without a name and a stray binding is assigned as a side effect.",
        example_source: include_str!("rule_examples/RY102.txt"),
    },
    Rule {
        code: "RY103",
        name: "class-equality",
        default_severity: Severity::Warning,
        summary: "`class(x)` compared with `==`/`!=` in a length-1 logical context. `class()` returns a character vector, so a multi-class object makes `if`/`&&` error. Use `inherits()`.",
        example_source: include_str!("rule_examples/RY103.txt"),
    },
    Rule {
        code: "RY105",
        name: "constant-length-comparison",
        default_severity: Severity::Warning,
        summary: "`length()` of a value that is length-1 by construction, compared with a literal. The comparison has a constant result, so the guard is dead.",
        example_source: include_str!("rule_examples/RY105.txt"),
    },
    Rule {
        code: "RY106",
        name: "ifelse-mode-collapse",
        default_severity: Severity::Warning,
        summary: "`ifelse()` builds its result from `test`, so a zero-length or all-NA test yields a logical result even when `yes`/`no` agree on another mode — in particular for typed-NA selects. Use a typed alternative such as `dplyr::if_else()`.",
        example_source: include_str!("rule_examples/RY106.txt"),
    },
    Rule {
        code: "RY107",
        name: "any-all-scalar-comparison",
        default_severity: Severity::Warning,
        summary: "`any()` or `all()` compared with a numeric literal, as in `any(x) == 0`. The call returns one logical value, so the comparison negates it or has a constant result. Write `any(x == 0)`. Comparisons that keep the call's value (`== 1`, `!= 0`, `> 0`, `>= 1`) are not reported. Silent when the argument has a known class, because an `any`, `all`, or `Summary` method can return any value.",
        example_source: include_str!("rule_examples/RY107.txt"),
    },
    Rule {
        code: "RY108",
        name: "seq-defaulted-forward",
        default_severity: Severity::Warning,
        summary: "A `seq.*` method uses its defaulted `to` without a `missing(to)` check, so a call like `seq(x, length.out = n)` forwards the default as if supplied and hits `seq.default`'s argument precedence; guard the use with `missing(to)`, as `seq.Date` does.",
        example_source: include_str!("rule_examples/RY108.txt"),
    },
    Rule {
        code: "RY109",
        name: "self-referential-default",
        default_severity: Severity::Warning,
        summary: "A formal's default refers to the formal itself, as in `copy = copy` or `n = n + 1`. Evaluating that default fails in R with \"promise already under evaluation\". A call that supplies the argument is unaffected. When the body provably forces the formal, RY098 reports it instead. Silent for a default that refers to another formal (`x = y`), a default that quotes the formal (`substitute(x)`), and a body that captures the formal (`enquo(x)`, `{{ x }}`).",
        example_source: include_str!("rule_examples/RY109.txt"),
    },
    Rule {
        code: "RY110",
        name: "vacuous-all-guard",
        default_severity: Severity::Warning,
        summary: "A guard such as `is.numeric(x) || all(is.na(x))` accepts zero-length input that fails the type predicate, because `all()` of an empty vector is `TRUE`. Reported only when the same function then passes `x` to a parameter whose mode a stub declares, where the empty value fails. For example, the Math group errors, and `mean()` warns and returns `NA`. Write `is.numeric(x) || (length(x) > 0 && all(is.na(x)))`. Also reported for a single-formal helper in the same file that returns the guard, when the demanding function calls it directly, in `stopifnot()`, or through a `map`-family call reduced with `all()`. Silent when one function validates and another demands, or when the helper is in another file.",
        example_source: include_str!("rule_examples/RY110.txt"),
    },
    Rule {
        code: "RY111",
        name: "constant-argument-shadowing",
        default_severity: Severity::Warning,
        summary: "A call passes literal `TRUE` or `FALSE` to a formal that an enclosing function exposes under the same name, as in `na.rm = TRUE` inside `function(x, na.rm = FALSE)`, and that function never reads the formal. The caller's value is ignored. Forward it with `na.rm = na.rm`. Reported only when the name exactly matches a formal of the resolved callee. Silent for partial names, unknown callees, `T`, `F`, `NA`, and numeric or string constants.",
        example_source: include_str!("rule_examples/RY111.txt"),
    },
    Rule {
        code: "RY112",
        name: "invalid-ignore",
        default_severity: Severity::Warning,
        summary: "A ry-owned inline ignore has malformed brackets or names an unknown rule. It cannot suppress diagnostics; correct the spelling or remove the directive. Foreign-only noqa lists are left to their owning tool and do not suppress ry findings.",
        example_source: include_str!("rule_examples/RY112.txt"),
    },
    Rule {
        code: "RY113",
        name: "unused-ignore",
        default_severity: Severity::Warning,
        summary: "Opt-in audit: a valid ry-owned selective line or standalone ignore names an enabled rule whose local syntax was checked in diagnostic mode and produced no matching finding before baseline or confidence filtering. The initial supported codes are RY034 and RY102. Invalid, bare, noqa, and file directives are outside this audit; parser errors, anonymous function bodies, and other unavailable analysis are left unaudited. Enable with `--warn RY113` or a severity override.",
        example_source: include_str!("rule_examples/RY113.txt"),
    },
    Rule {
        code: "RY114",
        name: "declaration-mismatch",
        default_severity: Severity::Warning,
        summary: "An independently known argument, default, or return value contradicts an explicitly adopted source declaration. For typehint's simple class clauses this compares effective R class(), not storage typeof(); unknown or incomplete facts remain silent.",
        example_source: include_str!("rule_examples/RY114.txt"),
    },
    Rule {
        code: "RY115",
        name: "declaration-incomplete",
        default_severity: Severity::Info,
        summary: "An adopted declaration has an unsupported residual or a predicate that body inference cannot represent. The source expression is retained without claiming complete checking.",
        example_source: include_str!("rule_examples/RY115.txt"),
    },
    Rule {
        code: "RY116",
        name: "declaration-conflict",
        default_severity: Severity::Warning,
        summary: "Multiple adopted records for one lexical function cannot be selected as one complete contract; none is used for checking.",
        example_source: include_str!("rule_examples/RY116.txt"),
    },
    Rule {
        code: "RY117",
        name: "declaration-invalid",
        default_severity: Severity::Warning,
        summary: "An explicitly adopted source annotation is malformed or cannot be attached unambiguously to its lexical function; it is not used for checking.",
        example_source: include_str!("rule_examples/RY117.txt"),
    },
    Rule {
        code: "RY118",
        name: "missing-box-export",
        default_severity: Severity::Warning,
        summary: "A statically resolved local box module has a complete export inventory, but a selected import or exact `$` access names no export. Missing modules, dynamic export declarations, and package inventories are not proof of absence and stay silent.",
        example_source: include_str!("rule_examples/RY118.txt"),
    },
    Rule {
        code: "RY119",
        name: "integer-coercion-range-loss",
        default_severity: Severity::Warning,
        summary: "`as.integer()` converts a proven out-of-range numeric value to `NA_integer_`; handle the new NA or constrain the input before casting.",
        example_source: include_str!("rule_examples/RY119.txt"),
    },
    Rule {
        code: "RY120",
        name: "report-boundary",
        default_severity: Severity::Warning,
        summary: "An opted-in report has a malformed R fence or exceeds the bounded extraction budget; affected R chunks are not analyzed.",
        example_source: include_str!("rule_examples/RY120.txt"),
    },
    Rule {
        code: "RY121",
        name: "report-execution-unknown",
        default_severity: Severity::Warning,
        summary: "A report execution option cannot be determined statically; analysis stops at that chunk rather than guessing which later bindings exist.",
        example_source: include_str!("rule_examples/RY121.txt"),
    },
    Rule {
        code: "RY122",
        name: "testthat-self-comparison",
        default_severity: Severity::Warning,
        summary: "A resolved testthat `expect_equal()`/`expect_identical()` compares the same conservatively deterministic expression to itself, so it cannot check two different values. Silent for differing operands, RNG/time/I/O or unknown calls, masked assertion functions, and `$`/`[[` access whose receiver is not proven plain data.",
        example_source: include_str!("rule_examples/RY122.txt"),
    },
    Rule {
        code: "RY123",
        name: "failed-namespace-access",
        default_severity: Severity::Warning,
        summary: "A `pkg::name` or `pkg:::name` access runs on the branch where `requireNamespace(\"pkg\")` returned `FALSE`, so the namespace is not loaded and the access errors instead of selecting the fallback. Reported only for a literal package name whose `base::requireNamespace` result dominates the access.",
        example_source: include_str!("rule_examples/RY123.txt"),
    },
    Rule {
        code: "RY124",
        name: "top-level-network",
        default_severity: Severity::Warning,
        summary: "A proven network/download call (`utils::download.file`, an opened `base::url`/`base::socketConnection`/`curl::curl`, `curl::curl_download`, `curl::curl_fetch_memory`, `httr::GET`/`httr::POST`, `httr2::req_perform`, or a same-file project wrapper around one) is evaluated while package code loads: at the top level of package or `data-raw` code, or in `.onLoad`/`.onAttach`. Package loading must not use the network unguarded; move the call behind an explicit opt-in or into a function the user calls. Silent inside ordinary functions, in lazily forced defaults, for connections R has not opened, under an `interactive()`/`getOption()`/`Sys.getenv()` guard, and for masked or dynamic callees.",
        example_source: include_str!("rule_examples/RY124.txt"),
    },
    Rule {
        code: "RY125",
        name: "dimensional-call-on-function",
        default_severity: Severity::Warning,
        summary: "A proven base `ncol()`/`nrow()`/`dim()` call receives a definite function value (such as the search-path `utils::data` where a data variable was meant), so the result is always `NULL`. Also reported for the same shape inside a literal template argument to a proven cli message function (`cli_abort()` and siblings). Silent for data or unknown arguments, masked callees, classed function values, dynamic templates, and cli calls carrying `.envir`.",
        example_source: include_str!("rule_examples/RY125.txt"),
    },
    Rule {
        code: "RY126",
        name: "mixed-sign-sequence-index",
        default_severity: Severity::Error,
        summary: "A `:`/`seq()`/`seq.int()`/`rlang::seq2()` sequence used as a `[` subscript (or a `[[` subscript on an atomic vector) whose proven whole-number endpoints straddle zero, so the index mixes positive and negative subscripts and R errors (\"only 0's may be mixed with negative subscripts\"). Silent for descending all-positive or all-negative sequences, computed endpoints, `seq_len()`/`seq_along()`, non-index uses, masked operators, and receivers ry cannot prove use base subsetting.",
        example_source: include_str!("rule_examples/RY126.txt"),
    },
    Rule {
        code: "RY127",
        name: "nested-rep-in-concat",
        default_severity: Severity::Warning,
        summary: "`list(rep(v, n))` inside a proven base `c()`/`append()` contributes one element holding the whole repeated vector, not `n` flat elements, when every sibling element is proven to be the same classless atomic type as `v`. Write `rep(list(v), n)`. Silent for standalone `list(rep(...))`, the `rep(list(v), n)` form, `n` proven at most 1, list, classed, or unknown element types, and masked `c`/`append`/`list`/`rep` callees.",
        example_source: include_str!("rule_examples/RY127.txt"),
    },
    Rule {
        code: "RY128",
        name: "invalid-diag-dimension",
        default_severity: Severity::Error,
        summary: "A proven base `diag()` whose matrix dimension is proven invalid: a negative or `NA` `nrow`/`ncol`, or, with neither supplied, a length-one numeric `x` proven negative or `NA`, which `diag` reads as the dimension rather than a diagonal entry, so R errors (\"invalid 'nrow' value\"). Silent for vector `x` (diagonal entries), non-negative scalars, entries mode (a scalar `x` with any second argument supplied), classed `x` that may dispatch to a `diag` method, and computed values ry cannot prove.",
        example_source: include_str!("rule_examples/RY128.txt"),
    },
    Rule {
        code: "RY129",
        name: "nesting-limit",
        default_severity: Severity::Warning,
        summary: "Source nested deeper than ry analyzes: more than 128 syntax levels, or more than 1024 nested `else if` arms. R's parser allows that depth, so this is not a syntax error, but the file is not checked. Argument and parameter lists, flat `else if` arms, and left-associative operator chains do not count toward the depth. Nesting past R's own limit of 49 brackets is RY000.",
        example_source: include_str!("rule_examples/RY129.txt"),
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
    fn every_rule_example_parses() {
        for rule in RULES {
            let example = rule.example();
            assert!(!example.flagged.is_empty(), "{}", rule.code);
        }
    }

    #[test]
    fn malformed_rule_examples_are_rejected() {
        let cases = [
            "--- flagged.R\nx\n--- fixed.R\ny\n",
            "fix: f\n--- fixed.R\ny\n",
            "fix: f\n--- flagged.R\nx\n",
            "fix: f\n--- flagged.R\nx\n--- fixed.Rmd\ny\n",
            "fix: f\n--- ../ry.toml\nz\n--- flagged.R\nx\n--- fixed.R\ny\n",
        ];
        for case in cases {
            assert!(RuleExample::parse(case).is_err(), "{case}");
        }
        let example =
            RuleExample::parse("fix: f\n--- ry.toml\nz\n--- flagged.R\nx\n--- fixed.R\ny\n")
                .unwrap();
        assert_eq!(
            (
                example.fix,
                example.extension,
                example.flagged,
                example.fixed
            ),
            ("f", "R", "x", "y")
        );
        assert_eq!(example.support, vec![("ry.toml", "z")]);
    }

    #[test]
    fn rules_are_sorted_by_code() {
        let codes: Vec<&str> = RULES.iter().map(|r| r.code).collect();
        let mut sorted = codes.clone();
        sorted.sort();
        assert_eq!(codes, sorted);
    }
}
