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
| `stopifnot(is.null(x) || length(x) == 1L)` before a later use | The later expression can still receive RY032 after the assertion has excluded non-scalar inputs. |
| `length(x) > 1L || !x %in% modes` returned to `if` | ry can miss the missing-value result for an empty input. |
| `is.numeric(x) \|\| all(is.na(x))` before a stub-declared mode demand | RY110 covers this guard side of the empty-input blind spot when the accepted path and the demand are in the same function (#462). A guard returned by a helper (`is_numeric_or_na <- function(x) ...`) keeps its demand invisible: the flow is interprocedural, as with the length facts of #351; that deferred half lives in #479. A bare `all(is.na(x))` guard (skip logic, no predicate operand) stays quiet by design, as does a positive guard's continuation (the demand there also runs when the guard is FALSE). |
| A parameter copied to a local and then reassigned in a loop | ry can lose the possible vector length before a later `&&` comparison. |

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
The flow work in [#351](https://github.com/sims1253/ry/issues/351) still needs
length facts that survive assertions, aliases, and loop joins, and remains
outside this milestone's implementation scope under the milestone's rule for
work that requires broader analysis changes.
