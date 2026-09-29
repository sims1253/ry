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
| `stopifnot(is.null(x) || length(x) == 1L)` before a later use | A successful base `stopifnot` now carries a scalar-or-NULL fact to the later RY032 check. A final named assertion in `...` has the same effect; `local`, `exprs`, and `exprObject` are controls and establish no predicate fact. The pinned purrr `stopifnot(is.null(before) || (before > 0 && before <= n))` has the same accepted-path result because `&&` checks the comparison's length and `before = NULL` cannot rebind itself when forced. A nonliteral default may return a scalar while replacing its own formal with a vector, so that assertion cannot establish a fact about the later binding. Reassignment kills the fact; an effectful or unknown RHS, an unforced formal or delayed binding read, shadowed operators or parentheses, and locally known S3 dispatch risks cannot establish it. |
| The same assertion after `library(stats)` or `attach(list())` | A preceding call that leaves the search path, data mask, or effects uncertain can prevent the checker from proving that the assertion used the expected base operations and kept the same binding. RY032 may therefore remain where the otherwise identical isolated guard is quiet. An unmodeled `load()` or `detach()` has the same certainty boundary. |
| A helper called before the assertion installs or might install a binding | A called helper, forced default, callable alias, or computed call may replace the subject even if its old value was a literal default. An invoked formal can receive an installing callback even when its default is harmless; an unused or proven inert callback leaves the subject stable. A callback wrapped by another expression is still uncertain if that value reaches a possible invocation; merely storing it with `base::invisible()` does not invoke it. Top-level aliases also carry callable values through qualified `base::identity()` and similar value wrappers, including simple assignments inside a returned block. A string used as a `do.call()` target can name an installer, and an invoked callback can carry that effect across a package-local helper hop. A `[[` extraction stays uncertain because that operator can be masked. The checker retains RY032 when it cannot rule out a replacement. A non-base qualified helper given a possibly caller-owned frame is uncertain without package identity; it is not equated with an unrelated local function of the same bare name. An unqualified `new.env()` or `invisible()` can be masked outside the helper, so it cannot certify a local-only installer frame or an unforced default. Qualified `base::new.env()`, `base::environment()`, and `base::invisible()` preserve those quiet controls when the remaining effect path is known. Simply backticked aliases use their ordinary binding names. An escaped alias that enters the bounded binding graph can make more project helpers uncertain because its target cannot be identified. |
| A helper invokes a computed function head such as `ops$run(NULL)` | The current caller-binding summary cannot prove a computed call harmless, even if `ops <- list(run = base::identity)` makes this example succeed in R. It conservatively retains RY032 after the helper; a direct `base::identity(NULL)` control stays quiet. |
| `length(x) > 1L || !x %in% modes` returned to `if` | ry checks `&&`/`||` outside conditions. For empty `x`, this particular expression returns `NA` rather than throwing at `||`; the later caller's `if` rejects that `NA`. Propagating this return alternative to the consumer remains part of the local-call and proven-NA work (#568, #354). |
| `is.numeric(x) \|\| all(is.na(x))` before a stub-declared mode demand | RY110 covers this guard side of the empty-input blind spot when the accepted path and the demand are in the same function (#462), including guards returned by a single-formal helper in the same file (`is_numeric_or_na <- function(x) ...`) and applied by the demanding function itself -- directly, via `stopifnot()`, or elementwise through a `map`-family call reduced with `all()` (#479). A guard validated in one function but demanded in another (a validator-summary hop), or applied from another file, keeps its demand invisible: the former flow belongs to the #351 flow-sensitivity cycle, the latter cannot attribute the helper's span to the consuming file. A bare `all(is.na(x))` guard (skip logic, no predicate operand) stays quiet by design, as does a positive guard's continuation (the demand there also runs when the guard is FALSE). |
| A value copied to a local and then reassigned in a loop | A proven unclassed length-greater-than-one input now survives the alias and loop join as a possible vector path for RY032, including another plain alias inside or after the loop. A custom `&&` or `||` operator does not have base R's scalar requirement. An unknown-length parameter such as tibble's `.rows` remains quiet without a proven vector call path; unknown length alone is not evidence of a vector error. |

For helper-local `do.call()`, ry follows a callable copied into a local at the
assignment where that copy occurs. Rebinding the old name later does not
change the copy. A repeated `for`, `while`, or `repeat` body can carry a new
callable back to an earlier `do.call()` on its next iteration; ry keeps the
effect uncertain when that path may install a caller binding. A literal
single-element `for` sequence has no next iteration. The spelling `1:1`
does not certify one iteration because R permits a masked `:` operator.
Copying an outer project helper name before its value is established locally
also stays uncertain, even when that helper is harmless at runtime. Calls
through a known `delayedAssign` alias, to a helper that uses `assign()` on
its caller's frame, or to `assign()` with an explicit current-frame target can
invalidate a successful earlier scalar assertion. A qualified fresh local
environment remains a quiet control. In a direct call,
`base::environment()` names the current frame; `base::new.env()` creates a
distinct frame. The effect check follows simple copied, wrapped, returned,
and branch-joined callable values. A merely assigned callable is harmless
until invoked; a conditional alias remains uncertain if one reachable value
can install a binding. A `do.call()` target and a function-valued formal may
invoke an installer too, while a proven pure target or a later pure overwrite
keeps the guarded binding stable.

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
