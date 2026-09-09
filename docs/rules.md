# Rule reference

[Getting started](../README.md) · [Usage](usage.md) · [Configuration](configuration.md)

## Rules

Defaults can be overridden per-project; `ry explain rule RY040` prints the
explanation for one rule.

| code  | name                     | severity | summary                                                                                                                                                                                                   |
| :---- | :----------------------- | :------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RY000 | syntax-error             | error    | Unparseable input. tree-sitter could not recover this region; subsequent diagnostics may be unreliable.                                                                                                   |
| RY001 | invalid-condition        | warning  | `if` / `while` condition is not a length-1 logical or a value R coerces to one.                                                                                                                                                       |
| RY002 | condition-length         | warning  | `if` condition has more than one element; R requires a length-1 condition.                                                                                                                      |
| RY003 | numeric-condition        | info     | `if` / `while` condition is numeric; R coerces nonzero to TRUE. Legal but implicit; prefer an explicit comparison.                                                                                         |
| RY010 | unbound-variable         | warning  | Reference to a variable with no binding in scope.                                                                                                                                                         |
| RY020 | unary-minus-type         | error    | Unary `-` applied to a non-numeric type.                                                                                                                                                                  |
| RY021 | unary-not-type           | error    | Unary `!` applied to a non-coercible-to-logical type.                                                                                                                                                     |
| RY030 | invalid-comparison       | error    | Comparison between types with no defined ordering.                                                                                                                                                        |
| RY031 | invalid-logical-op       | error    | `&` / `&#124;` / `&&` / `&#124;&#124;` applied to non-coercible types.                                                                                                                                    |
| RY032 | scalar-logical-length    | warning  | `&&` and `&#124;&#124;` require length-1 operands. Use `&`/`&#124;` for vectorized operations.                          |
| RY033 | comparison-mode-mismatch | warning  | Comparing a character value with a numeric value is valid R but almost always unintended. R coerces the numeric value to character before comparing.                                                                  |
| RY034 | compare-na               | warning  | Comparing with `NA` using `==` or `!=` always produces `NA`. Use `is.na()` instead.                                                                                                                       |
| RY040 | invalid-arithmetic       | error    | Arithmetic operator between incompatible types.                                                                                                                                                           |
| RY041 | non-divisible-recycling  | warning  | Vector lengths do not divide evenly, so R recycles values with a warning and may produce unintended results.                                                                                             |
| RY042 | factor-arithmetic        | warning  | Arithmetic on factors produces missing values. Operate on levels or convert explicitly.                                                                                                                  |
| RY050 | missing-s3-method        | warning  | S3 generic called on a value with no defined method for its class.                                                                                                                                        |
| RY051 | incompatible-s3-operator-methods | warning | Both proven chooseOpsMethod results reject distinct S3 operator methods, so R warns and uses the primitive operator. |
| RY060 | undefined-column         | error    | Column access on a value whose schema does not contain that column.                                                                                                                                       |
| RY061 | dollar-on-atomic         | error    | The $ operator is invalid for atomic vectors (integer, double, character, logical). It only works on list-like types (lists, data frames, environments).                                                  |
| RY070 | call-non-function        | error    | A call head names no reachable function. For a bare symbol R uses function-mode lookup (non-function bindings are skipped in every frame), so the diagnostic fires only when no function of that name is reachable and R errors 'could not find function'; value-expression heads (a literal like `42()`, or `pkg::dataset()`) error 'attempt to apply non-function'. |
| RY080 | map-return-type-mismatch | error    | A purrr typed-map (`map_dbl`, `map_int`, ...) callback returns a value whose mode is incompatible with the target vector type. R rejects incompatible callback results.          |
| RY090 | unknown-argument         | warning  | A named call argument does not match any formal parameter after R's exact and partial argument matching.                                                                                                  |
| RY091 | missing-required-argument | warning | A required formal parameter has no supplied value, including an explicitly omitted actual.                                                                                                                                             |
| RY092 | argument-type-mismatch   | error    | A call argument has a known mode incompatible with the parameter type declared by the resolved signature.                                                                                                 |
| RY093 | comparison-inside-length | warning  | A comparison directly inside `length()` (also `nchar()`, `abs()`) is usually a parenthesization mistake.                                                                                                  |
| RY094 | printf-argument-count    | warning  | A literal printf-family format string has more conversions than supplied value arguments.                                                                                                                 |
| RY096 | hasarg-non-formal        | warning  | `hasArg()` names a parameter that is not a formal of an enclosing function without `...`.                                                                                                                 |
| RY097 | not-r-source             | info     | File does not appear to be R source (e.g. Ratfor); its diagnostics are suppressed.                                                                                                                        |
| RY098 | default-forced-before-assignment | warning | A parameter default references a body-local that may not be assigned yet on some execution path.                                                                                                    |
| RY099 | discarded-conditional-value | warning | A value-producing expression in a non-tail one-arm `if` is discarded, commonly because an assignment was omitted.                                                                                   |
| RY100 | comparison-inside-math-call | warning | A comparison directly inside a numeric math function is usually a parenthesization mistake.                                                                                                         |
| RY101 | identical-list-subset-scalar | warning | `identical()` compares a single-bracket list subset with an atomic scalar, making the result always `FALSE`; use `[[` to extract the element.                                                        |
| RY102 | named-list-element-arrow | warning | `<-` where `=` was meant inside `list()`/`c()`/`data.frame()`/`structure()`. The element is created without a name and a stray binding is assigned as a side effect.                                        |
| RY103 | class-equality           | warning | `class(x)` compared with `==`/`!=` in a length-1 logical context. `class()` returns a character vector, so a multi-class object makes `if`/`&&` error. Use `inherits()`.                                    |
| RY105 | constant-length-comparison | warning | `length()` of a value that is length-1 by construction, compared with a literal. The comparison has a constant result, so the guard is dead.                                                          |

RY003 is registered but default-off: it is omitted from output unless a
severity override or rule selection names it (for example
`warn = ["RY003"]`).
