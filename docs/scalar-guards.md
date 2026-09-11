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
| `length(x) == 1L && x == 1L` in a package | A bare `length` may come from the package or an import. ry can warn even when the guard uses base R and the input is an ordinary vector. |
| `stopifnot(is.null(x) || length(x) == 1L)` before a later use | The later expression can still receive RY032 after the assertion has excluded non-scalar inputs. |
| `length(x) > 1L || !x %in% modes` returned to `if` | ry can miss the missing-value result for an empty input. |
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
Qualifying the function name alone does not establish a scalar value.

The remaining work in [#372](https://github.com/sims1253/ry/issues/372) needs
callee provenance, class and dispatch facts, and invalidation after effects.
The flow work in [#351](https://github.com/sims1253/ry/issues/351) also needs
length facts that survive assertions, aliases, and loop joins. Both remain
outside this milestone's implementation scope under the milestone's rule for work
that requires broader analysis changes. Existing guards should stay in
place while these cases remain unresolved.
