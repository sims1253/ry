# R parses `!x >= y` as `!(x >= y)` because unary `!` binds looser than
# comparison. Negation before a comparison is correct code, so no rule
# reports it and RY095 stays retired. `any(v) == 0` is not always FALSE,
# because `FALSE == 0` is TRUE.

## case: RY102: string arrow inside list() loses the element name
# expect: RY102
# `names(list("a" <- 1, "b" = 2))` is `c("", "b")`. The element loses its
# name and the call creates a stray top-level binding `a`.
l <- list("a" <- 1, "b" = 2)

## case: RY102: string arrow inside list() with a named sibling
# expect: RY102
v <- list(x = 1, "nm" <- 2)

## case: RY102: string arrow inside c()
# expect: RY102
v <- c(x = 1, "nm" <- 2)

## case: RY102: string arrow inside data.frame()
# expect: RY102
v <- data.frame(x = 1, "nm" <- 2)

## case: RY102: string arrow inside structure()
# expect: RY102
v <- structure(x = 1, "nm" <- 2)

## case: RY102: string arrow inside base::list()
# expect: RY102
v <- base::list("nm" <- 2)

## case: RY102: right assignment beside a named sibling
# expect: RY102
# `2 -> nm` assigns to an identifier, so it needs the same evidence as
# `nm <- 2`: a named sibling argument that shows the call builds a named
# structure.
v <- list(a = 1, 2 -> nm)

## case: RY102: lone right assignment in list() is silent
# no-diag
# With no named sibling there is no evidence the call builds a named
# structure.
v <- list(2 -> nm)

## case: RY102: lone left assignment in list() is silent
# no-diag
v <- list(nm <- 2)

## case: RY102: assign-and-append inside c() is silent
# expect: RY010
# `c(out, outn <- paste(...))` is a deliberate idiom in Hmisc, knitr,
# nlme, xfun, DescTools, and fitdistrplus.
v <- c(out, outn <- paste(1))

## case: RY102: correct element naming is silent
# no-diag
l <- list(a = 1, b = 2)

## case: RY102: equality test argument is silent
# no-diag
# `==` is a comparison, not an assignment. Nothing loses a name.
x <- 1
l <- list(x == 1)

## case: RY102: assignment inside local() is silent
# no-diag
# Only containers whose arguments become named elements are affected.
# `local(x <- 1)` is an ordinary assignment.
v <- local(x <- 1)

## case: RY102: assignment argument to a user function is silent
# no-diag
f <- function(a) a
v <- f(x <- 1)

## case: RY102: super assignment inside list() is silent
# no-diag
# `<<-` inside a container is an explicit side effect, not a mistyped `=`.
x <- 1
v <- list(x <<- 2)

## case: RY102: replacement assignment inside list() is silent
# no-diag
# `list(x[[1]] <- 2)` cannot be a mistyped `name = value`.
x <- list(1)
v <- list(x[[1]] <- 2)

## case: RY103: class() equality in an if condition
# expect: RY103
f <- function(x) if (class(x) == "data.frame") 1 else 2

## case: RY103: class() inequality under &&
# expect: RY103
f <- function(x, t) if (!is.null(t) && class(x) != t) 1

## case: RY103: class() equality under ||
# expect: RY103
f <- function(x) if (is.null(x) || class(x) == "tbl") 1

## case: RY103: class() equality under negation
# expect: RY103
f <- function(x) if (!(class(x) == "tbl")) 1

## case: RY103: class() inequality in a while condition
# expect: RY103
f <- function(x) while (class(x) != "tbl") x <- unclass(x)

## case: RY103: one report per comparison under &&
# expect: RY103
f <- function(x, t) if (!is.null(t) && class(x) != t) 1

## case: RY103: inherits() is silent
# no-diag
f <- function(x) if (inherits(x, "data.frame")) 1

## case: RY103: class(x)[1] comparison is silent
# no-diag
# `class(x)[1]` is length 1, the documented way to write this comparison
# without `inherits()`.
f <- function(x) if (class(x)[1] == "tbl") 1

## case: RY103: vectorised class() comparison outside a condition is silent
# no-diag
f <- function(x) class(x) == "tbl"

## case: RY103: class() comparison inside any() is silent
# no-diag
f <- function(x) any(class(x) == "tbl")

## case: RY103: class() comparison under elementwise & is silent
# no-diag
# `&` and `|` are vectorised and take operands of any length.
f <- function(x) y <- (class(x) == "tbl") & TRUE

## case: RY105: length of a scalar reduction compared with zero
# expect: RY105
# pak R/confirmation.R:42. `length(sum(...))` is always 1, so the guard
# is always TRUE.
if (length(sum(1L)) > 0) 1

## case: RY105: scalar reduction through a local binding
# expect: RY105
f <- function(s) {
  n <- sum(is.na(s))
  length(n) > 0
}

## case: RY105: classed scalar local is silent
# no-diag
# Storage length does not decide `length(x)` for an S3 object.
# `length.<class>` may return any length.
length.ry_cleanup <- function(x) 0L
x <- structure(1L, class = "ry_cleanup")
if (length(x) > 0L) stop("unreachable")

## case: RY105: reduction of a classed local is silent
# no-diag
# A scalar-returning generic may dispatch to a method that returns a
# longer or empty vector.
sum.ry_cleanup <- function(x, ...) integer(0)
x <- structure(1L, class = "ry_cleanup")
if (length(sum(x)) > 0L) stop("unreachable")

## case: RY105: reduction of an inline classed value is silent
# no-diag
sum.ry_cleanup <- function(x, ...) integer(0)
if (length(sum(structure(1L, class = "ry_cleanup"))) > 0L) stop("unreachable")

## case: RY105: reduction of a parameter with a scalar default is silent
# no-diag
sum.ry_cleanup <- function(x, ...) integer(0)
f <- function(x = 1L) if (length(sum(x)) > 0L) stop("unreachable")
f(structure(1L, class = "ry_cleanup"))

## case: RY105: reduction of a masked operator result is silent
# no-diag
`+` <- function(x, y) structure(1L, class = "ry_cleanup")
sum.ry_cleanup <- function(x, ...) integer(0)
if (length(sum(1L + 1L)) > 0L) stop("unreachable")

## case: RY105: scalar with a computed class is silent
# no-diag
# A computed class may have an S3 length method.
class_name <- "ry_cleanup"
x <- structure(1L, class = class_name)
if (length(x) > 0L) stop("unreachable")

## case: RY105: masked length() is silent
# no-diag
length <- function(x) 1L
x <- 1L
if (length(x) > 0L) stop("unreachable")

## case: RY105: length of a plain vector parameter is silent
# no-diag
f <- function(v) if (length(v) > 0) 1

## case: RY105: classless scalar local compared with zero
# expect: RY105
x <- 1L
if (length(x) > 0L) stop("unreachable")

## case: RY105: parameter with a scalar default is silent
# no-diag
# A default describes only calls that omit the argument. Callers can pass
# any length.
f <- function(n = 1) if (length(n) > 0) 1

## case: RY105: non-literal bound is silent
# no-diag
# `length(sum(v)) > k` is constant only when the bound is a literal.
f <- function(v, k) if (length(sum(v)) > k) 1

## case: RY105: file.path() with a vector argument is silent
# no-diag
# learnr R/run.R:290, pkgload R/load-code.R:65. `file.path()` recycles its
# arguments, so the result has length 1 only when every argument does.
f <- function(src_root) if (length(file.path(src_root, "x")) > 0L) 1

## case: RY105: file.path() with an empty argument is silent
# no-diag
# `?file.path`: any zero-length argument gives an empty character vector.
# `paste` recycles "" instead.
paths <- character(0)
p <- file.path(paths, "x")
if (length(p) > 0L) 1

## case: RY105: file.path() of scalar literals through a binding
# expect: RY105
p <- file.path("a", "b")
if (length(p) > 0L) stop("unreachable")

## case: RY105: seq_len() of a runtime row count is silent
# no-diag
# brulee R/augment.R:412. The length of `seq_len(n)` is the value of `n`,
# which a length-1 argument does not pin down.
f <- function(df) {
  i <- seq_len(nrow(df))
  if (length(i) > 0L) 1
}

## case: RY105: seq_len() of a parameter is silent
# no-diag
f <- function(n) if (length(seq_len(n)) > 0L) 1

## case: RY105: seq_len() of a literal count is silent
# no-diag
# `seq_len(3)` is never empty, but it has length 3, not 1.
if (length(seq_len(3)) > 0L) 1

## case: RY105: subset of an empty atomic vector has length one
# expect: RY105
# `[` pads out-of-bounds selections for every vector mode.
# `character(0)[1]` is a length-1 NA, so the guard is always TRUE.
x <- character(0)
first <- x[1]
if (length(first) > 0L) stop("unreachable")

## case: RY105: subset of an empty list is silent
# no-diag
# `list()[1]` is `list(NULL)`, also length 1, but ry makes the claim only
# through atomic bindings.
items <- list()
first <- items[1]
if (length(first) > 0L) 1

## case: RY105: subset of a list parameter default is silent
# no-diag
f <- function(items = list()) {
  first <- items[1]
  if (length(first) > 0L) 1
}

## case: RY105: length equal to one is silent
# no-diag
# `length(x) == 1` on a scalar is a redundant assertion, not a dead guard.
# Assertion helpers write it on purpose.
f <- function(v) if (length(sum(v)) == 1) 1

## case: RY105: length greater than a negative bound
# expect: RY105
# `length()` is never negative, so a negative bound makes the comparison
# as constant as a zero bound.
if (length(sum(1L)) > -1) 1

## case: RY105: length less than a negative bound
# expect: RY105
if (length(sum(1L)) < -1) 1

## case: RY105: length equal to a negative bound
# expect: RY105
if (length(sum(1L)) == -1) 1

## case: RY105: length not equal to a negative bound
# expect: RY105
if (length(sum(1L)) != -1) 1

## case: RY105: negative integer bound
# expect: RY105
if (length(sum(1L)) > -1L) 1

## case: RY105: negative bound on the left
# expect: RY105
if (-1 < length(sum(1L))) 1

## case: RY105: negated zero bound
# expect: RY105
# `-0` is zero.
if (length(sum(1L)) > -0) 1

## case: RY105: positive bound is silent
# no-diag
if (length(sum(1L)) > 1) 1

## case: RY105: equality with one is silent
# no-diag
if (length(sum(1L)) == 1) 1

## case: RY105: plus-spelled positive bound is silent
# no-diag
# The parser drops unary `+`, so `+2` is the bound 2.
if (length(sum(1L)) > +2) 1

## case: RY105: plus-spelled zero bound
# expect: RY105
if (length(sum(1L)) > +0) 1

## case: RY105: which.max() is silent
# no-diag
# `which.max(numeric(0))` is `integer(0)`, so the result is not always
# length 1.
f <- function(v) if (length(which.max(v)) > 0) 1

## case: RY105: which.min() is silent
# no-diag
f <- function(v) if (length(which.min(v)) > 0) 1

## case: RY105: locally redefined sum() is silent
# no-diag
# The local `sum` returns a vector, so the base scalar property does not
# apply.
f <- function(v) { sum <- function(x) c(x, x); if (length(sum(v)) > 0) 1 }

## case: RY105: locally redefined any() is silent
# no-diag
f <- function(v) { any <- function(x) c(TRUE, FALSE); if (length(any(v)) > 0) 1 }

## case: RY107: any() equal to zero negates the result
# expect: RY107
# `any()` and `all()` return FALSE or TRUE, which compare as 0 or 1.
# `== 0` and `!= 1` negate the result. Other bounds give a constant.
f <- function(x) if (any(x) == 0) 1

## case: RY107: any() not equal to one negates the result
# expect: RY107
f <- function(x) if (any(x) != 1) 1

## case: RY107: all() at most zero negates the result
# expect: RY107
f <- function(x) if (all(x) <= 0) 1

## case: RY107: all() less than one negates the result
# expect: RY107
f <- function(x) if (all(x) < 1) 1

## case: RY107: any() greater than one is always FALSE
# expect: RY107
f <- function(x) if (any(x) > 1) 1

## case: RY107: any() less than zero is always FALSE
# expect: RY107
f <- function(x) if (any(x) < 0) 1

## case: RY107: all() at least zero is always TRUE
# expect: RY107
f <- function(x) if (all(x) >= 0) 1

## case: RY107: any() equal to two is always FALSE
# expect: RY107
f <- function(x) if (any(x) == 2) 1

## case: RY107: any() greater than a negative bound is always TRUE
# expect: RY107
# `any()` and `all()` compare as 0 or 1, which is above every negative
# bound, so each comparison with a negative literal is constant.
f <- function(x) if (any(x) > -1) 1

## case: RY107: any() at least a negative bound is always TRUE
# expect: RY107
f <- function(x) if (any(x) >= -1) 1

## case: RY107: any() not equal to a negative bound is always TRUE
# expect: RY107
f <- function(x) if (any(x) != -1) 1

## case: RY107: any() less than a negative bound is always FALSE
# expect: RY107
f <- function(x) if (any(x) < -1) 1

## case: RY107: any() at most a negative bound is always FALSE
# expect: RY107
f <- function(x) if (any(x) <= -1) 1

## case: RY107: any() equal to a negative bound is always FALSE
# expect: RY107
f <- function(x) if (any(x) == -1) 1

## case: RY107: all() greater than a negative integer bound
# expect: RY107
f <- function(x) if (all(x) > -1L) 1

## case: RY107: negative bound on the left of any()
# expect: RY107
f <- function(x) if (-1 < any(x)) 1

## case: RY107: any() greater than a negative fraction
# expect: RY107
f <- function(x) if (any(x) > -0.5) 1

## case: RY107: classed local argument is silent
# no-diag
# A method for the argument's class can replace the FALSE or TRUE result,
# so the 0 or 1 outcome table does not apply.
f <- function(v) { d <- structure(c(1, 2), class = "s3obj"); if (any(d) == 0) 1 }

## case: RY107: Summary group method makes the comparison meaningful
# no-diag
# R 4.6.1 (testdata/oracle/any_all_scalar_comparison_claim.R): this
# `Summary.s3grp` makes `any(sgrp)` return 42, so `any(sgrp) == 42` is TRUE.
Summary.s3grp <- function(x, ...) 42
sgrp <- structure(c(FALSE, TRUE), class = "s3grp")
if (any(sgrp) == 42) 1

## case: RY107: inline classed argument is silent
# no-diag
f <- function(v) { if (any(structure(c(1, 2), class = "s3obj")) == 0) 1 }

## case: RY107: factor argument is silent
# no-diag
f <- function(v) { d <- factor(v); if (any(d) == 0) 1 }

## case: RY107: S4 object argument is silent
# no-diag
f <- function(v) { d <- new("Widget", x = v); if (all(d) == 0) 1 }

## case: RY107: argument with a class() replacement is silent
# no-diag
f <- function(v) { d <- c(1, 2); class(d) <- "s3obj"; if (any(d) == 0) 1 }

## case: RY107: parameter proven classed by inherits() is silent
# no-diag
f <- function(x) { if (inherits(x, "s3obj")) { if (any(x) == 0) 1 } }

## case: RY107: literal vector argument
# expect: RY107
# A plain value runs the base `any()`. An unknown class is not evidence
# of dispatch.
if (any(c(TRUE, FALSE)) == 0) 1

## case: RY107: plain local argument
# expect: RY107
f <- function(v) { d <- c(1, 2); if (any(d) == 0) 1 }

## case: RY107: argument with a computed class
# expect: RY107
cn <- "s3obj"
f <- function(v) { d <- structure(c(1, 2), class = cn); if (any(d) == 0) 1 }

## case: RY107: parameter with a classed default
# expect: RY107
# The default says nothing about the values callers pass.
f <- function(x = structure(1, class = "s3obj")) if (any(x) == 0) 1

## case: RY107: negated power bound is silent
# no-diag
# Unary minus binds looser than `^`, so `-2^2` is `-(2^2)`, not a negative
# literal.
f <- function(x) if (any(x) > -2^2) 1

## case: RY107: subtraction bound is silent
# no-diag
f <- function(x) if (any(x) > 1 - 1) 1

## case: RY107: plus-spelled literal bound
# expect: RY107
# The parser drops unary `+`, so `> +2` is the same comparison as `> 2`.
f <- function(x) if (any(x) > +2) 1

## case: RY107: plus over a parameter is silent
# no-diag
f <- function(x, k) if (any(x) > +k) 1

## case: RY107: any() equal to one is silent
# no-diag
# `== 1`, `!= 0`, `> 0`, and `>= 1` compute the same value as the bare
# call.
f <- function(x) any(x) == 1

## case: RY107: any() not equal to zero is silent
# no-diag
f <- function(x) any(x) != 0

## case: RY107: any() greater than zero is silent
# no-diag
f <- function(x) any(x) > 0

## case: RY107: all() at least one is silent
# no-diag
f <- function(x) all(x) >= 1

## case: RY107: diffobj negated all() equal to one is silent
# no-diag
# diffobj writes `!all(diff(x)) == 1L` on purpose.
# testdata/ry095_ry096_real_shapes.R pins the same shape.
f <- function(x) !all(diff(x)) == 1L

## case: RY107: comparison inside any() is silent
# no-diag
f <- function(x) if (any(x == 0)) 1

## case: RY107: comparison inside all() is silent
# no-diag
f <- function(x) if (all(x != 1)) 1

## case: RY107: any() with na.rm is silent
# no-diag
# A call with `na.rm` is not the single-argument shape of a misplaced
# parenthesis.
f <- function(x) if (any(x, na.rm = TRUE) == 0) 1

## case: RY107: comparison with a parameter is silent
# no-diag
f <- function(x, k) if (any(x) == k) 1

## case: RY107: comparison with a call is silent
# no-diag
f <- function(x) if (any(x) == length(x)) 1

## case: RY107: comparison with a logical literal is silent
# no-diag
f <- function(x) if (any(x) == FALSE) 1

## case: RY107: comparison of any() with all() is silent
# no-diag
f <- function(x) if (any(x) == all(x)) 1

## case: RY107: locally redefined any() is silent
# expect: RY002
# The local `any` returns a length-2 logical, so the scalar premise does
# not hold.
f <- function(v) { any <- function(x) c(TRUE, FALSE); if (any(v) == 0) 1 }

## case: RY107: base::any() keeps the scalar premise
# expect: RY107
f <- function(v) if (base::any(v) == 0) 1

## case: RY107: foreign-qualified any() is silent
# no-diag
f <- function(v) if (otherpkg::any(v) == 0) 1

## case: RY107: comparison inside length() is left to RY093
# expect: RY010, RY093
a <- length(any(x) == 0)

## case: RY107: comparison inside abs() is left to RY100
# expect: RY010, RY100
b <- abs(any(x) == 0)

## case: RY107: wrapped length of any() is left to RY105
# expect: RY105
c <- if (length(any(1L)) > 0) 1

## case: negation before >= is not diagnosed
# no-diag
# `as.list(quote(!x >= y))` is `!` applied to `x >= y`.
f <- function(x, y) if (!x >= y) 1

## case: negation before == is not diagnosed
# no-diag
f <- function(x, y) if (!x == y) 1

## case: negation before a comparison of a call is not diagnosed
# no-diag
f <- function(v) if (!nchar(v) > 3) 1

## case: RY127: nested list(rep()) appended to a flat list through a $ receiver
# expect: RY127
# The readr shape from #359 with provable element types: `spec$cols`
# holds flat integer elements, so one nested repeated vector is wrong.
cols <- list(1L, 2L)
spec <- list(cols = cols)
spec$cols <- c(spec$cols, list(rep(3L, 2L)))

## case: RY127: nested list(rep()) as append() values
# expect: RY127
xs <- list(1L, 2L)
ys <- append(xs, list(rep(3L, 2L)))

## case: RY127: the flat rep(list()) form is silent
# no-diag
cols <- list(1L, 2L)
spec <- list(cols = cols)
spec$cols <- c(spec$cols, rep(list(3L), 2L))

## case: RY127: standalone list(rep()) outside c()/append() is silent
# no-diag
alone <- list(rep(3L, 2L))

## case: RY127: a count proven to be 1 is silent
# no-diag
xs <- list(1L, 2L)
once <- c(xs, list(rep(3L, 1L)))

## case: RY127: a repeated list value is silent
# no-diag
ys <- list(list(1L), list(2L))
nested <- c(ys, list(rep(list(3L), 2L)))

## case: RY127: an unknown repeated value is silent
# no-diag
mk <- function() 1L
xs <- list(1L, 2L)
unknown <- c(xs, list(rep(mk(), 2L)))

## case: RY127: a masked rep() is silent
# no-diag
rep <- function(x, times) x
xs <- list(1L, 2L)
masked <- c(xs, list(rep(3L, 2L)))

## case: RY127: recursive c() is silent
# no-diag
xs <- list(1L, 2L)
flat <- c(xs, list(rep(3L, 2L)), recursive = TRUE)

## case: RY127: heterogeneous sibling elements are silent
# no-diag
mixed <- c(list(1L, "a"), list(rep(3L, 2L)))

## case: RY127: a classed $ receiver is silent
# no-diag
# $.foo may dispatch and return something other than the field.
spec <- structure(list(cols = list(1L, 2L)), class = "foo")
spec$cols <- c(spec$cols, list(rep(3L, 2L)))

## case: RY127: a plain rep() sibling still proves flat elements
# expect: RY127
flat <- c(rep(1L, 2L), list(rep(3L, 2L)))

## case: RY127: a classed first sibling may dispatch c() and is silent
# no-diag
c.foo <- function(...) list(1L)
first <- structure(list(1L, 2L), class = "foo")
dispatched <- c(first, list(rep(3L, 2L)))

## case: RY127: an unproven sibling rep count establishes nothing
# no-diag
f <- function(k) c(rep(list(1L), k), list(rep(3L, 2L)))

## case: RY127: sibling length.out overrides a zero times
# expect: RY127
c(rep(list(1L), 0L, length.out = 3L), list(rep(3L, 2L)))

## case: RY127: sibling length.out zero contributes nothing
# no-diag
c(rep(list(1L), 2L, length.out = 0L), list(rep(3L, 2L)))

## case: RY127: unproven sibling length.out establishes nothing
# no-diag
f <- function(k) c(rep(list(1L), 0L, length.out = k), list(rep(3L, 2L)))

## case: RY127: a named "[[02]]" key does not shadow positional [[2]]
# no-diag
x <- list("[[02]]" = 3L, "a")
c(x[[2L]], list(rep(3L, 2L)))

## case: RY127: positional [[2]] past a named "[[02]]" key still proves
# expect: RY127
x <- list("[[02]]" = "a", 1L)
c(x[[2L]], list(rep(3L, 2L)))

## case: RY127: a double-spelled [[2]] sibling index proves the same slot
# expect: RY127
x <- list(1L, 3L)
c(x[[2]], list(rep(3L, 2L)))

## case: RY127: mixed named/unnamed schema keys are not positions
# no-diag
x <- list(a = "a", 1L)
c(x[[1L]], list(rep(3L, 2L)))

## case: RY127: the unnamed tail of a mixed list peeks by position
# expect: RY127
x <- list(a = "a", 1L)
c(x[[2L]], list(rep(3L, 2L)))
