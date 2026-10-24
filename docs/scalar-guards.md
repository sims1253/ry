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
| `length(x) == 1L && x == 1L` in a package | The guard is honored when `length` resolves to base modulo the package search path, unless the parameter is provably classed or the project registers, defines, or imports a `length.*` method other than a storage-length one (below). The warning then names the method that made ry distrust the guard. A `length` method on the search path for a class ry cannot name can still make the guard lie. |
| `stopifnot(is.null(x) \|\| length(x) == 1L)` before a later use | A successful base `stopifnot` carries a scalar-or-NULL fact to the later RY032 check, within the proof boundary below. Outside it, RY032 stays as before. |
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

One kind of method cannot lie: a project definition with one formal whose
whole body is `length(unclass(x))`, with `length` and `unclass` resolving to
base and no other definition of that name. It reports the storage length,
which is what base `length` would report without the method, so it leaves the
guard honored ([#701](https://github.com/sims1253/ry/issues/701)). Any other
`length.*` method, a registered or imported one ry cannot read, and a
`NextMethod()` body all still disable the guard, and the RY032 message names
the first such method with a note at its definition. `NAMESPACE`'s
three-argument `S3method(length, cls, fun)` is read as `length.cls`, so a
storage-length `length.cls` next to a lying `fun` registered that way is
missed. A storage-length method still disables the stricter `stopifnot` and
`if` scalar facts below.

## Scalar facts from `stopifnot`

A successful base `stopifnot(is.null(x) || length(x) == 1L)`, or
`stopifnot(is.null(x) || (x > 0 && x <= n))`, proves that `x` is NULL or
scalar ([#351](https://github.com/sims1253/ry/issues/351)). The proof needs
base identity for `stopifnot`, `is.null`, `length`, the comparison, `&&`,
`||`, and `(`; a final predicate argument (not `local`, `exprs`, or
`exprObject`); a known search path and data mask; enclosing formals that
have no default or a literal one; and no project or imported S3/S4 method
for `length`, `[`, `[[`, `$`, an operator, or a safe call below. An
installed package's `length` method (such as grid's `length.arrow`) can
still fake the proof for a value of unknown class. The rest
of an `&&` assertion may use only literals, the subject, other local values,
and base comparisons.

A passed `if` condition records the same fact
([#702](https://github.com/sims1253/ry/issues/702)). R errors unless the
condition is TRUE or FALSE, so `if (x == "")`, `if (!is.null(x) && x == "")`,
and `if (is.null(x) || x == "none")` prove `x` NULL or scalar after the
statement, under the boundary above. Only a base comparison of the subject
with a scalar literal counts, possibly negated, as the condition itself or as
the tail of `||` behind `is.null(x)` or of `&&` behind `!is.null(x)`. An earlier operand
proves nothing: an empty value is NA there, and `character(0) == "" && FALSE`
is FALSE. Nor does `if (any(x == ""))`. A call in the condition, such as the
`c()` in `!x %in% c("a", "b")`, ends the fact like any other call.

Scalar facts and adopted declaration identities
([declarations](declarations.md#shared-checking-boundary)) share one call
rule. Only a few base calls keep the fact: `stopifnot`, `length`, `is.null`,
`is.na`, `nchar`, `identical`, `inherits`, `missing`, `stop`, the
`is.character`-style type predicates, and `message` with only character
literal arguments. Each must resolve to base, and the project must not
define or import a method for a generic the call dispatches internally:
`length`, `is.na`, `nchar`, and `is.numeric` for themselves; `anyNA`,
`is.na`, `all`, or the `Summary` group for a `stopifnot` predicate that is
not a plain logical expression (a comparison, `&&`, `||`, `!`, an `is.*`
call, or a logical literal) or a literal or local value known to carry no
class attribute; and `conditionMessage` for `message`. For
`message`, only arguments named exactly `domain` or `appendLF` are controls
and must be literals; every other argument, named or not, is payload and
must be a character string, and a forwarded `...` is refused.
`stopifnot` with `exprs` or `exprObject` evaluates quoted code and keeps
nothing. A qualified head such as `base::length` counts only while `::`
itself is not masked. These base calls keep scalar facts only; adopted
declaration identities do not survive them (see the exclusions below).

A project function keeps the fact when the call head names exactly one
current definition (or is a function literal), every argument is a scalar
literal, and its body and defaults read only formals and literals and run no
call, operator, or subscript. Its plain `<<-` targets then drop only their
own facts. This summary trusts the body's syntax: a project mask of `if` or
`{`, an active binding whose setter runs on `<<-`, or a later `body<-`
replacement can still change the caller's binding. A kept fact only
suppresses RY032, so these are false negatives, pinned by
`known_gap_helper_masked_syntax.R`, `known_gap_helper_superassign_setter.R`,
and `known_gap_replaced_helper_body.R`.

Any other call, including a generic, formal, box call, callback-taking
function, or user-defined function outside that summary, ends the fact, and
so does a replacement or subassignment. Such a call before the assertion also keeps
it from proving anything in that frame, and in a loop it taints the whole
repeated body. Before the assertion only, base `I()` and a `pkg::fn` call
into another package are exempt: an external package deliberately
installing an active binding for its caller's variable is not modelled. A
harmless call therefore leaves the RY032 warning ry gave before this proof
existed.

Code that runs while an allowed call executes, but is established outside
the current frame, is not modelled: a caller's calling handlers or restarts
(`message()` signals a condition, `is.na(function() NULL)` warns, and a
partial argument match warns under `options(warnPartialMatchArgs = TRUE)`),
hooks, finalizers, external active bindings, and promise code a caller
supplied for a formal that an allowed call forces. Such code reaches this
frame's bindings only through frame reflection (`sys.frames()`,
`sys.frame()`, `parent.frame(n)`), so a kept scalar fact can at worst miss
an RY032. Such code can also replace a name the frame resolves outward, so
declaration identities are dropped after every one of these base calls. The oracle fixtures
`known_gap_outer_calling_handler.R` and `known_gap_promise_reaches_frame.R`
pin this boundary.
