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
| `stopifnot(is.null(x) || length(x) == 1L)` before a later use | A successful base `stopifnot` carries a scalar-or-NULL fact to the later RY032 check, within the proof boundary below. Outside it, RY032 stays as before. |
| `length(x) > 1L || !x %in% modes` returned to `if` | ry can miss the missing-value result for an empty input. |
| `is.numeric(x) \|\| all(is.na(x))` before a stub-declared mode demand | RY110 covers this guard side of the empty-input blind spot when the accepted path and the demand are in the same function (#462), including guards returned by a single-formal helper in the same file (`is_numeric_or_na <- function(x) ...`) and applied by the demanding function itself -- directly, via `stopifnot()`, or elementwise through a `map`-family call reduced with `all()` (#479). A guard validated in one function but demanded in another (a validator-summary hop), or applied from another file, keeps its demand invisible: the former flow belongs to the #351 flow-sensitivity cycle, the latter cannot attribute the helper's span to the consuming file. A bare `all(is.na(x))` guard (skip logic, no predicate operand) stays quiet by design, as does a positive guard's continuation (the demand there also runs when the guard is FALSE). |
| A value copied to a local and then reassigned in a loop | A proven unclassed vector of length two or more survives plain aliases and loop joins as a possible first-iteration or empty-loop path. Unknown length alone stays quiet. |

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

## Scalar facts from `stopifnot`

A successful base `stopifnot(is.null(x) || length(x) == 1L)`, or
`stopifnot(is.null(x) || (x > 0 && x <= n))` as in purrr's `prepend()`,
proves that `x` is NULL or scalar ([#351](https://github.com/sims1253/ry/issues/351)).
The proof needs base identity for `stopifnot`, `is.null`, `length`, the
comparison, `&&`, `||`, and `(`; no project comparison or `length` method;
a final predicate argument (not `local`, `exprs`, or `exprObject`); a known
search path and data mask; and a subject whose first read cannot rebind it (a
local, a formal without a default, or a formal with a literal default). The
rest of an `&&` assertion may use only literals, the subject, other local
values, and base comparisons.

The fact ends when `x` is reassigned, after unknown effects, and after any
call that might install a binding in the frame: `assign`, `delayedAssign`,
`makeActiveBinding`, `do.call`, `eval`, `rm`, and similar base calls; a
computed call head; a formal, local, or project binding called directly; or
a closure passed as an argument unless its body is a single literal. After
such a call, later assertions in the same frame prove nothing, and in a
loop a possible installer anywhere in the body applies to the whole body.
These rules are deliberately coarse: a harmless helper loses the fact too,
which leaves the RY032 warning ry gave before this proof existed.
