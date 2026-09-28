# Scalar guards and RY032

RY032 reports known vector operands of `&&` and `||`. It also warns on some
parameter guard patterns whose operands may have more than one element.
Those parameter warnings describe a possible input; they do not prove that a
function fails for its documented scalar inputs.

```r
f <- function(x) is.null(x) || is.na(x)
```

This function accepts `NULL` and scalar values. A vector with two elements
reaches `is.na(x)` and causes a length error. A scalar default does not prove
that every caller, or a later assignment from an option, supplies a scalar.

## Current boundaries

| Pattern | What ry can miss or overstate |
| --- | --- |
| `length(x) == 1L && x == 1L` in a package | The guard is honored when `length` resolves to base modulo the package search path, unless the parameter is provably classed or the project registers, defines, or imports any `length.*` method. A `length` method on the search path for a class ry cannot name can still make the guard lie. |
| `stopifnot(is.null(x) || length(x) == 1L)` before a later use | A successful base `stopifnot` now carries a scalar-or-NULL fact to the later RY032 check. The pinned purrr `stopifnot(is.null(before) || (before > 0 && before <= n))` has the same accepted-path result because `&&` checks the comparison's length. Reassignment kills the fact; shadowed operators and locally known S3 dispatch risks cannot establish it. |
| `length(x) > 1L || !x %in% modes` returned to `if` | ry checks `&&`/`||` outside conditions. For empty `x`, this particular expression returns `NA` rather than throwing at `||`; the later caller's `if` rejects that `NA`. Propagating this return alternative to the consumer remains part of the local-call and proven-NA work (#568, #354). |
| `is.numeric(x) \|\| all(is.na(x))` before a stub-declared mode demand | RY110 covers this guard side of the empty-input blind spot when the accepted path and the demand are in the same function (#462), including guards returned by a single-formal helper in the same file (`is_numeric_or_na <- function(x) ...`) and applied by the demanding function itself -- directly, via `stopifnot()`, or elementwise through a `map`-family call reduced with `all()` (#479). A guard validated in one function but demanded in another (a validator-summary hop), or applied from another file, keeps its demand invisible: the former flow belongs to the #351 flow-sensitivity cycle, the latter cannot attribute the helper's span to the consuming file. A bare `all(is.na(x))` guard (skip logic, no predicate operand) stays quiet by design, as does a positive guard's continuation (the demand there also runs when the guard is FALSE). |
| A value copied to a local and then reassigned in a loop | A proven unclassed length-greater-than-one input now survives the alias and loop join as a possible vector path for RY032. An unknown-length parameter such as tibble's `.rows` remains quiet without a proven vector call path; unknown length alone is not evidence of a vector error. |

Other return expressions are covered. For example, the first function above
receives RY032 even though the expression is outside an `if` condition.

## Why a length check needs more than a name

`length(x) > 0` proves only that a value is nonempty. It does not prove that
its length is one. Even an exact equality needs care for classed objects:

```r
length.disguised <- function(x) 1L
x <- structure(c(1L, 2L), class = "disguised")
base::length(x) == 1L && x == 1L
```

Base R's `length` dispatches to the method, which reports one, while the
comparison still produces two values. R rejects the right operand of `&&`.
Qualifying the function name alone does not establish a scalar value, so the
equality guard is honored only when the guarded parameter is provably
unclassed, or of unknown class while no `length.*` method is registered,
defined, or imported by the project — and the operand is not reassigned
inside the guarded expression ([#372](https://github.com/sims1253/ry/issues/372)).
The same dispatch boundary applies to assertions that rely on `length()`.
Loop-carried vector evidence is retained only for a known unclassed vector
alternative. A later exact-length guard removes that alternative from its
true path; changing the binding also invalidates the prior assertion.
