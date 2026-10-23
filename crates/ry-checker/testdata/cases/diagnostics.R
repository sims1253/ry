## case: a parse error hides semantic findings on the recovered tree
# expect: RY000
# The incomplete `function(` header makes tree-sitter invent a call over
# the rest of the file. Only the parse error is actionable there.
x <- function(

## case: a file without parse errors keeps its semantic findings
# expect: RY010
print(undefined_thing)

## case: formals replacement binds the names a placeholder closure reads
# no-diag
# distr6's genExp shape. Only the alist() installed by `formals<-` binds
# `x` at runtime.
gen_exp <- function(trafo = NULL) {
  if (is.null(trafo)) {
    trafo <- function() {
      return(x)
    }
    formals(trafo) <- alist(x = )
  }
  trafo
}

## case: body replacement makes a placeholder closure opaque
# no-diag
# makeChecks shape. The body comes from `body<-` and the formals from an
# alist() parameter.
make_check <- function(cond, args = alist(object = )) {
  value <- function() {}
  formals(value) <- args
  body(value) <- substitute(assertThat(object, arg1), list(arg1 = cond))
  value
}

## case: formals replacement does not silence a same-named closure elsewhere
# expect: RY010
outer <- function() {
  trafo <- function() x
  formals(trafo) <- alist(x = )
  trafo
}
top_trafo <- function() x

## case: environment replacement keeps unbound names reportable
# expect: RY010
# `environment<-` changes the enclosure, not the formals or the body.
f <- function() {
  still_unbound
}
environment(f) <- globalenv()

## case: formals replacement keeps nested closure findings
# expect: RY010
# `formals<-` replaces only the formals. Closures in the body survive
# verbatim, so their unbound names are still runtime errors.
f <- function() {
  g <- function() undefined_name
  g
}
formals(f) <- alist(x = )

## case: body replacement drops nested closure findings
# no-diag
# `body<-` discards the old body, nested closures included.
f <- function() {
  g <- function() undefined_name
  g
}
body(f) <- substitute(x + 1)

## case: formals replacement keeps a typed map error in the body
# expect: RY080
# The body runs verbatim, and R errors "Can't coerce from a string to a
# double".
library(purrr)
f <- function() {
  map_dbl(1:3, function(z) "nope")
}
formals(f) <- alist(x = )

## case: body replacement drops a typed map error in the body
# no-diag
library(purrr)
f <- function() {
  map_dbl(1:3, function(z) "nope")
}
body(f) <- substitute(x + 1)

## case: a rebind after formals replacement is a fresh closure
# expect: RY010
# A replacement applies only to the closure bound to the name at that
# point. The rebound closure is a new object.
f <- function() x
formals(f) <- alist(x = )
f <- function() other_unbound

## case: formals replacement before the closure exists marks nothing
# expect: RY010
# At runtime the replacement errors or modifies some other closure.
formals(g) <- alist(h = )
g <- function() later_unbound

## case: a closure in a sibling branch keeps its finding
# expect: RY010
pick <- function(cond) {
  if (cond) {
    f <- function() x
    formals(f) <- alist(x = )
  } else {
    f <- function() else_unbound
  }
  f
}

## case: formals replacement inside local does not reach the outer closure
# expect: RY010
# local() evaluates its block in a new environment, so `formals<-` there
# assigns a modified copy and leaves the outer binding untouched.
f <- function() undefined_name
local({ formals(f) <- alist(x = ) })

## case: formals replacement inside local applies to the local closure
# no-diag
local({
  f <- function() x
  formals(f) <- alist(x = )
})

## case: body replacement wins when formals replacement comes first
# no-diag
f <- function() {
  g <- function() nested_unbound
  g
}
formals(f) <- alist(a = )
body(f) <- substitute(x + 1)

## case: body replacement wins when formals replacement comes after
# no-diag
f <- function() {
  g <- function() nested_unbound
  g
}
body(f) <- substitute(x + 1)
formals(f) <- alist(a = )

## case: valid utf8 source has no encoding error
# no-diag
label <- "café"

## case: a byte order mark inside a string is an ordinary character
# no-diag
# R's parser accepts U+FEFF anywhere but the first bytes of the file.
s <- "﻿"

## case: a byte order mark inside a comment is an ordinary character
# no-diag
x <- 1 # ﻿ comment

## case: an empty source is clean
# no-diag


## case: a comparison directly inside length is flagged
# expect: RY010, RY010, RY010, RY010, RY093
if (length(x == y)) print("bad")
ok <- length(x) == y

## case: comparisons inside length nchar and abs are flagged but not inside sum
# expect: RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY093, RY093, RY100
length(x > 0)
nchar(x == y)
abs(x != y)
sum(x > 0)
length(x) > 0

## case: a comparison directly inside numeric math is flagged
# expect: RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY010, RY100, RY100, RY100
abs(x > y)
abs(x) > y
sqrt(a == b)
sum(x > 0)
log(x, base = 2)
abs(x %in% y)
abs((x > y))

## case: any compared to zero negates the result
# expect: RY107
# glue R/utils.R:32. `any(lengths) == 0` computes `!any(lengths)`
# (FALSE == 0 is TRUE), so the emptiness guard never runs.
u <- function(x) {
  lengths <- vapply(x, NROW, integer(1))
  if (any(lengths) == 0) return(character())
}

## case: all compared above one is constant
# expect: RY107
# TRUE > 1 and FALSE > 1 are both FALSE, so the guard is dead.
f <- function(x) if (any(x > 1) || all(x) > 1) 1

## case: comparisons that keep the any or all value are allowed
# no-diag
# `== 1`, `> 0`, `>= 1`, and `!= 0` compute exactly the bare call's
# value. diffobj writes `!all(diff(x)) == 1L` on purpose.
a <- function(x) !all(diff(x)) == 1L
b <- function(x) any(x) == 1
c <- function(x) any(x) > 0
d <- function(x) all(x) >= 1
e <- function(x) any(x) != 0

## case: element level comparisons inside any and all are allowed
# no-diag
a <- function(x) any(x == 0)
b <- function(x) all(x != 1)
c <- function(x) any(x > 1)
d <- function(x) any(x, na.rm = TRUE) == 0

## case: a local any is not assumed to return one logical
# expect: RY002
f <- function(v) {
  any <- function(x) c(TRUE, FALSE)
  if (any(v) == 0) 1
}

## case: any comparisons already flagged by a call rule are not reported twice
# expect: RY010, RY010, RY093, RY100, RY105
a <- length(any(x) == 0)
b <- abs(any(x) == 0)
c <- if (length(any(1L)) > 0) 1

## case: sign of a comparison is an indicator idiom
# expect: RY010, RY010, RY010, RY010, RY100
sign(x <= y)
abs(x <= y)

## case: comparisons inside calls are flagged in both short circuit operands
# expect: RY093, RY093
q <- TRUE
x <- 1L
y <- 2L
z <- TRUE
if (length(x == y) || q) x
stopifnot(length(x == y) && z)

## case: negation before a comparison negates the comparison
# no-diag
# R parses `!x == y` as `!(x == y)` because unary `!` binds looser than
# comparison, so `!length(x) == 1` is correct code.
x <- c(1, 2)
if (!length(x) == 1) x <- 1
flag <- !"a" == "b"
!(1L == 2L)

## case: hasArg needs a formal of the enclosing function unless it has dots
# expect: RY096, RY096
good <- function(value) hasArg(value)
dots_ok <- function(actual, ...) hasArg(missing)
idiom_ok <- function(object, ...) if (hasArg(thresh)) list(...)$thresh else 0
string_bad <- function(actual) hasArg("missing")
bad <- function(actual) hasArg(missing)
hasArg(top_level)

## case: a literal printf format with too few arguments is flagged
# expect: RY010, RY010, RY094
gettextf("select %s then %s", "first")
sprintf("value=%s %%", "ok")
sprintf(dynamic_format, value)

## case: `$` on a local list partially matches a unique prefix
# no-diag
# R's `$` on a list returns the one field a name prefixes, so `res$nv`
# is `res$nvalid`, not NULL (#721, DescTools' Desc.R).
f <- function(x) {
  res <- list(nvalid = length(x), total = sum(x))
  res$total / res$nv
}

## case: `$` with an ambiguous prefix is NULL
# expect: RY040
# Two fields share the prefix, so R's `$` returns NULL.
g <- function(x) {
  res <- list(nvalid = length(x), nvx = 1, total = sum(x))
  res$total / res$nv
}

## case: environment replacement hides the defining frame's NULL placeholder
# no-diag
# lme4's modular.R shape. After `environment<-`, `mkTheta` resolves in
# `rho`, not in the frame that binds it to NULL.
make_devfun <- function(rho) {
  mkTheta <- NULL
  devfun <- function(par) mkTheta(par)
  environment(devfun) <- rho
  devfun
}

## case: environment replacement reaches literals picked by an if value
# no-diag
# lme4's mkdevfun shape: the closure comes from an if/else value.
make_branch <- function(rho, linear) {
  mkTheta <- NULL
  ff <- if (linear) {
    function(par) mkTheta(par)
  } else {
    function(pars) mkTheta(pars[1])
  }
  environment(ff) <- rho
  ff
}

## case: without environment replacement the NULL placeholder is what R calls
# expect: RY070
# R errors: could not find function "mkTheta".
make_plain <- function() {
  mkTheta <- NULL
  devfun <- function(par) mkTheta(par)
  devfun(3)
}

## case: environment replacement reaches literals bound in if and else branches
# no-diag
make_branch_assign <- function(rho, linear) {
  mkTheta <- NULL
  if (linear) ff <- function(par) mkTheta(par) else ff <- function(pars) mkTheta(pars[1])
  environment(ff) <- rho
  ff
}

## case: environment replacement after a rebind leaves the earlier closure checked
# expect: RY070
# Only the second literal is re-homed; the first runs in the NULL frame.
make_rebind <- function(rho) {
  mkTheta <- NULL
  ff <- function(par) mkTheta(par)
  ff(1)
  ff <- function(par) par
  environment(ff) <- rho
  ff
}

## case: an assignment in a subscript of an opaque receiver binds in the frame
# no-diag
# lme4's mlist2vec: base `[` evaluates its subscripts in the calling frame.
ff <- function(x) {
  if (all(x[iu <- upper.tri(x)] == 0)) t(x[!iu]) else t(x)[!iu]
}

## case: an assignment in a two-subscript index binds in the frame
# no-diag
# DescTools' PlotBag: the second call argument reads `h`.
loop_points <- function(five) {
  segments(five[h <- c(1, 5), 1], five[h, 2])
}

## case: an assignment in a matrix subscript binds at top level
# no-diag
m <- matrix(1:4, 2)
z <- m[j <- 1, 2]
print(j)

## case: a proven data.table keeps a subscript assignment in its own frame
# expect: RY010
# `[.data.table` evaluates `i` and `j` in its own environment.
dt <- structure(list(a = 1), class = c("data.table", "data.frame"))
dt[k <- 1]
print(k)

## case: a closure inside a subscript does not bind its locals in the frame
# expect: RY010
f <- function(x) {
  x[vapply(x, function(v) hidden <- v, numeric(1))]
  hidden
}

## case: a superassignment in a subscript leaves the local binding alone
# expect: RY040
# `<<-` writes an enclosing frame, so the local `n` is still character.
f <- function(x) {
  n <- "a"
  x[n <<- 2]
  n + 1
}

## case: an assignment inside an if body of a subscript block binds in the frame
# no-diag
# Silent like the statement form of a one-armed if.
f <- function(x, p) {
  x[{ a <- 1; if (p) b <- 2; a }]
  b
}

## case: `$` prefix on a tibble column subset is an undefined column
# expect: RY060, RY060
# A tibble's `[` keeps the tibble class and drops to a vector only on
# `drop = TRUE`, so `$` on the subset still never partial-matches (#771).
tb <- dplyr::group_by(data.frame(nvalid = 1:3, k = 1:3))
tb[, "nvalid", drop = FALSE]$nv
tb[, "nvalid"]$nv

## case: `$` on a tibble column subset with drop = TRUE is on a vector
# expect: RY061
tb <- dplyr::group_by(data.frame(nvalid = 1:3, k = 1:3))
tb[, "nvalid", drop = TRUE]$nv

## case: `$` prefix on a base data frame column subset partially matches
# no-diag
# A base data.frame subset stays a base data.frame, by name or position.
df <- data.frame(nvalid = 1:3, k = 1:3)
df[, "nvalid", drop = FALSE]$nv
df[, 1, drop = FALSE]$nv
