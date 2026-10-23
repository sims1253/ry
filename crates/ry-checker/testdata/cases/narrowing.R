## case: a not-null guard on a NULL binding keeps the branch well typed
# no-diag
x <- NULL
if (!is.null(x)) {
  y <- x + 1
}

## case: rlang is_null guard narrows a NULL default parameter
# no-diag
bind_rows <- function(..., .id = NULL) {
if (!is_null(.id)) {
check_string(.id)
}
}
bind_rows(value = 1L)

## case: a stop guard makes a NULL default function callable
# no-diag
# A guard whose true branch always diverges (stop(), return(), or a
# block ending in stop()) leaves only the false path for the code after it.
# There the NULL default is gone.
f <- function(x, fun = NULL) {
  if (is.null(fun)) stop("fun required")
  fun(x)
}
f(1)

## case: a stop guard lets a NULL default use $
# no-diag
f <- function(x = NULL) {
  if (is.null(x)) stop("x required")
  x$field
}
f()

## case: a negated is.character stop guard narrows the mode
# no-diag
f <- function(x = NULL) {
  if (!is.character(x)) stop("x must be character")
  x < "1"
}
f()

## case: a return guard narrows like a stop guard
# no-diag
f <- function(x = NULL) {
  if (is.null(x)) return(NULL)
  x$field
}
f()

## case: a guard block ending in stop diverges
# expect: RY092
# log() on a character string errors in R.
f <- function(x = NULL) {
  if (is.null(x)) { log("x required"); stop("x required") }
  x$field
}
f()

## case: a compound is.null or is.na stop guard narrows the continuation
# expect: RY032
f <- function(x = NULL) {
  if (is.null(x) || is.na(x)) stop("x required")
  x$field
}
f()

## case: a stop guard inside a function defined in a loop narrows
# no-diag
for (i in 1:2) {
  read_field <- function(x = NULL) {
    if (is.null(x)) stop("x required")
    x$field
  }
}

## case: a warning guard does not narrow the continuation
# expect: RY070
apply_fun <- function(x, fun = NULL) {
if (is.null(fun)) warning("fun required")
fun(x)
}
apply_fun(1)

## case: a project function named abort does not diverge
# expect: RY070
abort <- function(message) warning(message)
apply_fun <- function(x, fun = NULL) {
if (is.null(fun)) abort("fun required")
fun(x)
}
apply_fun(1)

## case: a diverging compound length guard makes the continuation scalar
# no-diag
Primes <- function(n1 = 1, n2 = NULL) {
if (is.null(n2)) return(1L)
if (!is.numeric(n2) || length(n2) != 1) stop("x")
if (n2 > 0) return(2L)
3L
}
omitted <- function() Primes(5)
explicit_null <- function() Primes(5, NULL)
explicit_number <- function() Primes(5, 10)

## case: a reversed length guard in a long or chain makes the continuation scalar
# no-diag
f <- function(x = NULL) {
if (!is.character(x) || other_check(x) || 1 != length(x)) stop("x")
if (x == "ok") TRUE else FALSE
}
f()

## case: a warning compound length guard does not narrow the continuation
# expect: RY001
f <- function(x = NULL) {
if (!is.numeric(x) || length(x) != 1) warning("x")
if (x > 0) NULL
}
f()

## case: a NULL return guard excludes the omitted NULL default
# no-diag
f <- function(x = NULL) {
if (is.null(x)) return(NULL)
if (x > 0) NULL
}
f()

## case: a missing() stop guard excludes the omitted NULL default
# no-diag
apply_fun <- function(x, fun = NULL) {
if (missing(fun)) stop("fun required")
fun(x)
}
apply_fun(1)

## case: a helper that always stops narrows the continuation
# no-diag
fail <- function() stop("required")
use_fun <- function(fun = NULL) {
if (is.null(fun)) fail()
fun()
}
use_fun()

## case: a break guard narrows the rest of the loop body
# no-diag
for (i in 1:2) {
fun <- NULL
if (is.null(fun)) break
fun()
}

## case: a stop in the else branch promotes the then refinement
# no-diag
use_fun <- function(fun = NULL) {
if (!is.null(fun)) fun else stop("required")
fun()
}
use_fun()

## case: is.function narrows the then branch of an if expression
# no-diag
f <- if (TRUE) function(x) x else 1L
x <- if (is.function(f)) f(1) else f

## case: is.list replaces an incompatible default only in the then branch
# expect: RY061
f <- function(x = FALSE) {
if (is.list(x)) x$enabled else x$enabled
}
f()

## case: is.function replaces an incompatible default
# no-diag
f <- function(x = FALSE) { if (is.function(x)) x() }
f()

## case: an assignment in a call argument of an if condition binds after short circuit
# no-diag
if (TRUE && grepl("x", value <- "x")) {
nchar(value)
}

## case: an assignment in a call argument of a while condition binds after short circuit
# no-diag
while (FALSE || grepl("x", value <- "x")) {
nchar(value)
break
}

## case: is.numeric narrowing stays inside the branch
# expect: RY010
# After the `if`, `x` is opaque again, so `x + "bad"` stays silent.
x <- some_opaque_thing
if (is.numeric(x)) {
  y <- x + 1
}
z <- x + "bad"

## case: check_string on a length-2 local is flagged and narrows later uses
# expect: RY092
h2 <- function() {
choice <- c("foo", "bar")
check_string(choice)
if (choice == "foo") 1 else 2
}

## case: check_string on a known incompatible value is flagged
# expect: RY092
choice <- c("foo", "bar")
check_string(choice)

## case: check_string that always fails makes the continuation unreachable
# expect: RY092
choice <- c("foo", "bar")
check_string(choice)
missing_after_throw
choice + 1

## case: an impossible guard does not hide diagnostics in later function bodies
# expect: RY010, RY092
value <- 1L
check_string(value)
later <- function() missing_in_function

## case: check_string with allow_null accepts NULL
# no-diag
value <- NULL
check_string(value, allow_null = TRUE)

## case: check_string with allow_na accepts a logical NA candidate
# no-diag
value <- TRUE
check_string(value, allow_na = TRUE)

## case: check_string on a union with a string member may succeed
# no-diag
value <- if (runif(1) > 0.5) "ok" else 1L
check_string(value)

## case: check_string on NULL always fails
# expect: RY092
value <- NULL
check_string(value)

## case: check_string on a list always fails
# expect: RY092
value <- list()
check_string(value)

## case: check_string on a union with no string member always fails
# expect: RY092
value <- if (runif(1) > 0.5) 1L else list()
check_string(value)

## case: standalone checks do not reject incompatible parameter defaults
# no-diag
bind <- function(.id = NULL, .trace_bottom = NULL) {
check_string(.id)
check_environment(.trace_bottom)
}

## case: narrowing after an abort guard keeps the default uncertain
# no-diag
validate <- function(value = NULL) {
if (is.null(value)) abort("missing")
check_string(value)
}

## case: a reassigned parameter can make check_string impossible
# expect: RY092
validate <- function(value = NULL) {
value <- NULL
check_string(value)
missing_after_guard
}

## case: a local value named check_string is not a guard
# expect: RY010, RY070
check_string <- 1L
value <- 1L
check_string(value)
missing_after_call

## case: check_data_frame accepts a data frame with several columns
# no-diag
value <- data.frame(x = 1L, y = 2L)
check_data_frame(value)

## case: a mode predicate's false branch drops the default's mode
# no-diag
# A parameter default describes only calls that omit the argument. When
# is.character() is FALSE, the caller passed something else, so the
# default's character mode is no fact in the else branch.
f <- function(position = "dodge2") {
if (is.character(position)) {
1
} else {
position$preserve
}
}
f()

## case: an is.numeric false branch drops a numeric default
# no-diag
m <- function(n = 1) {
if (is.numeric(n)) {
1
} else {
n$field
}
}
m()

## case: a local literal keeps its else branch diagnostic
# expect: RY061
g <- function() {
s <- "a"
if (is.character(s)) 1 else s$n
}

## case: a reassigned formal keeps its else branch diagnostic
# expect: RY061
h <- function(position = "dodge2") {
position <- "x"
if (is.character(position)) 1 else position$n
}
h()

## case: a NULL default else branch stays silent
# no-diag
n <- function(x = NULL) {
if (is.character(x)) 1 else x$n
}
n()

## case: an inherits false branch keeps the default's diagnostic
# expect: RY061
i <- function(x = "a") {
if (inherits(x, "foo")) 1 else x$n
}
i()

## case: code after the branch keeps the default's shape diagnostic
# expect: RY061
# Narrowing in the rejected branch is branch-local. After the `if`, `x`
# still has the default's shape.
p1 <- function(x = "a") {
if (is.character(x)) { 1 } else { 2 }
x$n
}
p1()

## case: an opaque classless stub return does not fail check_data_frame
# no-diag
# eval_tidy() has no class metadata. Missing metadata does not prove the
# class is absent, and at runtime the value is usually the data frame.
f <- function(data) {
data <- rlang::eval_tidy(rlang::enquo(data))
rlang::check_data_frame(data)
}

## case: concrete atomic values still fail check_data_frame
# expect: RY092, RY092
f <- function() {
v <- 1.5
rlang::check_data_frame(v)
}
g <- function() {
s <- letters[1]
rlang::check_data_frame(s)
}

## case: a plain list from as.list still fails check_data_frame
# expect: RY092
# rlang::check_data_frame() rejects a bare list.
g <- function() {
w <- as.list(data.frame(a = 1L))
rlang::check_data_frame(w)
}

## case: impossible guards in every if arm make the continuation unreachable
# expect: RY092, RY092
if (runif(1) > 0.5) {
left <- c("a", "b")
check_string(left)
} else {
right <- c("c", "d")
check_string(right)
}
missing_after_if

## case: an impossible guard in repeat makes the continuation unreachable
# expect: RY092
repeat {
value <- c("a", "b")
check_string(value)
}
missing_after_repeat

## case: a user function named check_string is not an impossible guard
# no-diag
check_string <- function(x) nchar(x) > 0
value <- 1L
check_string(value)

## case: a user check_string with the rlang signature narrows
# expect: RY092
check_string <- function(x, ..., arg = caller_arg(x), call = caller_env()) invisible(NULL)
choice <- c("foo", "bar")
check_string(choice)
if (choice == "foo") 1 else 2

## case: rlang::check_string accepts a union that may be a string
# no-diag
value <- if (runif(1) > 0.5) "a" else c("a", "b")
rlang::check_string(value)
if (value == "a") 1 else 2

## case: a binding made in both if branches is visible afterwards
# no-diag
f <- function(a) {
  if (a > 0) { r <- "pos" } else { r <- "neg" }
  paste(r)
}

## case: a !length(x) return guard narrows a NULL default
# no-diag
f <- function(x = NULL) { if (!length(x)) return(0L); x + 1L }

## case: a length(x) == 0 return guard narrows a NULL default
# no-diag
f <- function(x = NULL) { if (length(x) == 0) return(0L); x + 1L }

## case: both branches replacing NULL with a function make it callable
# no-diag
f <- function(flag, other) { g <- NULL; if (flag) g <- function(x) x else g <- function(x) x + 1L; g(1L) }

## case: nested branches that all install a function make it callable
# no-diag
f <- function(flag, other) { g <- NULL; if (flag) { if (other) g <- function(x) x else g <- function(x) x + 1L } else g <- function(x) x; g(1L) }

## case: an if without else leaves a callable NULL or function join
# no-diag
f <- function(flag, other) { g <- NULL; if (flag) g <- function(x) x; g(1L) }

## case: an else branch that keeps NULL leaves a callable join
# no-diag
f <- function(flag, other) { g <- NULL; if (flag) g <- function(x) x else g <- NULL; g(1L) }

## case: an else branch that installs an integer leaves a callable join
# no-diag
f <- function(flag, other) { g <- NULL; if (flag) g <- function(x) x else g <- 1L; g(1L) }

## case: a nested if without else leaves a callable NULL or function join
# no-diag
f <- function(flag, other) { g <- NULL; if (flag) { if (other) g <- function(x) x } else g <- function(x) x; g(1L) }

## case: a return in the then branch does not bring back NULL
# no-diag
f <- function(flag) { g <- NULL; if (flag) return(0L) else g <- function(x) x; g(1L) }

## case: a return in the else branch does not bring back NULL
# no-diag
f <- function(flag) { g <- NULL; if (flag) g <- function(x) x else return(0L); g(1L) }

## case: is.character or is.null rejects a character default in the else branch
# no-diag
f <- function(x = 'default') { if (is.character(x) || is.null(x)) 1L else x$field }
f()

## case: a negated and of the predicates rejects the default in the then branch
# no-diag
f <- function(x = 'default') { if (!is.character(x) && !is.null(x)) x$field else 1L }
f()

## case: is.null or is.character in either order rejects the default
# no-diag
f <- function(x = 'default') { if (is.null(x) || is.character(x)) 1L else x$field }
f()

## case: base-qualified predicates reject the default
# no-diag
f <- function(x = 'default') { if (base::is.character(x) || base::is.null(x)) 1L else x$field }
f()

## case: a negated or of the predicates rejects the default
# no-diag
f <- function(x = 'default') { if (!(is.character(x) || is.null(x))) x$field else 1L }
f()

## case: a three-way or of predicates rejects the default
# no-diag
f <- function(x = 'default') { if (is.character(x) || is.null(x) || is.logical(x)) 1L else x$field }
f()

## case: an or guard does not narrow its true branch
# expect: RY061
f <- function(x = 'default') { if (is.character(x) || is.null(x)) x$field }; f()

## case: an and guard that cannot hold does not narrow its else branch
# expect: RY061
f <- function(x = 'default') { if (is.character(x) && is.null(x)) 1L else x$field }; f()

## case: a rebound local keeps its type through an or guard
# expect: RY061
f <- function(x = 'default') { x <- 'rebound'; if (is.list(x) || is.null(x)) 1L else x$field }; f()

## case: is.data.frame or is.null does not reject a character default
# expect: RY061
f <- function(x = 'default') { if (is.data.frame(x) || is.null(x)) 1L else x$field }; f()

## case: a local is.character mask keeps the atomic error
# expect: RY061
f <- function(x = 'default') { is.character <- function(x) FALSE; if (is.character(x) || is.null(x)) 1L else x$field }; f()

## case: an assignment inside the guard keeps the atomic error
# expect: RY061
f <- function(x = 'default') { if (is.character(x) || { x <- 'rebound'; FALSE }) 1L else x$field }; f()

## case: a top-level is.character alias keeps the atomic error
# expect: RY061
is.character <- is.null
f <- function(x = 'default') { if (is.character(x) || is.null(x)) 1L else x$field }; f()

## case: stopifnot with a negated conjunction narrows the default
# no-diag
f <- function(x = 'default') { stopifnot(!is.character(x) && !is.null(x)); x$field }; f()

## case: is.list and not is.null narrows the then branch
# no-diag
f <- function(x = 'default') { if (is.list(x) && !is.null(x)) x$field else 1L }; f()

## case: an early-return NULL guard narrows a find-or-NULL union
# no-diag
# locate_input() returns NULL or a string. Each guard idiom below removes
# the NULL member for the code after it.
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}
f <- function(input) {
  where <- locate_input(input)
if (is.null(where)) {
  return(NULL)
}
switch(where, path = 1L, 2L)
if (where == "path") 1L else 2L
}
f("path")

## case: a replacement NULL guard narrows a find-or-NULL union
# no-diag
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}
f <- function(input) {
  where <- locate_input(input)
if (is.null(where)) {
  where <- "none"
}
switch(where, path = 1L, 2L)
if (where == "path") 1L else 2L
}
f("path")

## case: a mirrored else-return guard narrows a find-or-NULL union
# no-diag
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}
f <- function(input) {
  where <- locate_input(input)
if (!is.null(where)) {
  1L
} else {
  return(NULL)
}
switch(where, path = 1L, 2L)
if (where == "path") 1L else 2L
}
f("path")

## case: a stop NULL guard narrows a find-or-NULL union
# no-diag
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}
f <- function(input) {
  where <- locate_input(input)
if (is.null(where)) {
  stop("where required")
}
switch(where, path = 1L, 2L)
if (where == "path") 1L else 2L
}
f("path")

## case: a short-circuit NULL guard inside the condition narrows the union
# no-diag
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}
f <- function(input) {
  where <- locate_input(input)
  if (!is.null(where) && where == "path") {
    1L
  } else {
    2L
  }
}
f("path")

## case: an if else NULL check narrows the else arm
# no-diag
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}
f <- function(input) {
  where <- locate_input(input)
  if (is.null(where)) {
    NULL
  } else if (where == "path") {
    1L
  } else {
    2L
  }
}
f("path")

## case: a braced NULL return guard excludes the omitted NULL default
# no-diag
# Past the guard the value is an unknown caller value; the omitted NULL
# default's zero length no longer describes it.
f <- function(x = NULL) {
  if (is.null(x)) {
    return(NULL)
  }
  if (x > 0) {
    NULL
  }
}
f()

## case: a NULL return guard followed by is.character stays silent
# no-diag
f <- function(x = NULL) {
  if (is.null(x)) {
    return(NULL)
  }
  if (is.character(x)) {
    NULL
  }
}
f()

## case: a same-type rebind in a NULL guard keeps the NULL member
# expect: RY001
# The rebind can return NULL again, so no exact refinement exists and
# the condition stays flagged. This is a possible false positive, kept
# on purpose over a wrong narrowing.
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}
f <- function(input, fallback) {
  where <- locate_input(input)
  if (is.null(where)) where <- locate_input(fallback)
  if (where == "path") 1L else 2L
}
f("path", NULL)

## case: a replacement guard on a NULL default removes the stale NULL
# no-diag
# The rebind covers the true path and the false path holds a non-NULL
# value, so the joined type has no NULL member. Shape from scales'
# minor_breaks_log().
f <- function(detail = NULL) {
  if (is.null(detail)) {
    detail <- c(1, 5, 10)[2L]
  }
  if (detail == 1) 1 else 2
}
f()

## case: a replacement guard on a local NULL removes the stale NULL
# no-diag
f <- function() {
  detail <- NULL
  if (is.null(detail)) {
    detail <- 1
  }
  if (detail == 1) 1 else 2
}
f()

## case: a mirrored replacement guard on a NULL default removes the stale NULL
# no-diag
f <- function(detail = NULL) {
  if (!is.null(detail)) {
    1
  } else {
    detail <- 1
  }
  if (detail == 1) 1 else 2
}
f()

## case: a mirrored replacement guard on a local NULL removes the stale NULL
# no-diag
f <- function() {
  detail <- NULL
  if (!is.null(detail)) {
    2
  } else {
    detail <- 1
  }
  if (detail == 1) 1 else 2
}
f()

## case: a NULL return guard without replacement excludes the omitted default
# no-diag
f <- function(detail = NULL) {
  if (is.null(detail)) {
    return(NULL)
  }
  if (detail == 1) 1 else 2
}
f()

## case: a positive mode predicate on an omitted NULL default does not keep length 0
# no-diag
# is.character(NULL) is FALSE, so the guarded block only runs when the
# caller passed a value, and nothing is known about that value's length.
add_columns <- function(.data, .before = NULL) {
  if (is.character(.before)) {
    if (!(.before %in% names(.data))) .before <- NULL
  }
  .data
}
draw <- function(caption = NULL) {
  if (is.logical(caption)) {
    if (!caption) caption <- NULL
  }
  caption
}
add_columns(data.frame(a = 1))
draw()

## case: class and compound predicates on an omitted NULL default do not keep length 0
# no-diag
weigh <- function(weights_attr = NULL) {
  if (inherits(weights_attr, "character")) {
    if (weights_attr == "w") 1 else 2
  }
}
both <- function(from = NULL, to = NULL) {
  if (is.character(from) && is.character(to)) {
    if (from == to) 1 else 2
  }
}
weigh()
both()

## case: the else arm of a negated predicate on an omitted NULL default does not keep length 0
# no-diag
label <- function(m = NULL) {
  if (!is.character(m)) {
    "none"
  } else {
    if (m == "a") "first" else "other"
  }
}
label()

## case: a value computed in a predicate arm on an omitted NULL default does not inherit length 0
# no-diag
parse_method <- function(method = NULL) {
  if (is.character(method)) method <- tolower(method) else method <- "wald"
  if (method == "wald") 1 else 2
}
parse_method()

## case: a positive mode predicate keeps a known zero length
# expect: RY001
f <- function() {
  x <- character(0)
  if (is.character(x)) {
    if (x == "a") 1 else 2
  }
}

## case: is.function makes a local union with no function member callable
# no-diag
# The guarded path holds a function whatever ry inferred, so the call cannot
# fail there (spatstat.explore R/roc.R: `degfreeU` is a number or a function).
degrees <- function(z, flag) {
  dof <- length(z)
  if (flag) dof <- "auto"
  if (is.function(dof)) dof <- dof(length(z))
  dof
}

## case: is.function makes a parameter typed by its caller callable
# no-diag
# The call chain passes `Inf`, so ry walks `scale_dof` with a number for
# `dof`. Another caller can still pass a function.
fit_outer <- function(z) fit_middle(z)
fit_middle <- function(z, dof = Inf) scale_dof(z, dof = dof)
scale_dof <- function(z, dof = length(z)) {
  if (is.null(dof)) dof <- length(z)
  if (is.function(dof)) dof <- dof(length(z))
  dof
}

## case: the else arm of a negated is.function makes a local value callable
# no-diag
degrees_else <- function(z) {
  dof <- length(z)
  if (!is.function(dof)) dof else dof(length(z))
}

## case: arithmetic inside a dead is.function guard on a numeric local stays quiet
# no-diag
# The guard installs an unknown value, not a function type, so the dead arm
# does not report arithmetic on a function.
dead_guard_arith <- function(z) {
  dof <- length(z)
  if (is.function(dof)) {
    dof <- dof + 1
  }
  dof
}

## case: a logical operator inside a dead is.function guard on a numeric local stays quiet
# no-diag
dead_guard_logic <- function(z) {
  dof <- length(z)
  if (is.function(dof)) {
    dof <- dof & TRUE
  }
  dof
}

## case: a local union with no function member and no guard is still not callable
# expect: RY070
degrees_unguarded <- function(z, flag) {
  dof <- length(z)
  if (flag) dof <- "auto"
  dof(length(z))
}

## case: is.list on a local union with no list member gives the arm an unknown value
# no-diag
# The arm is reachable only if ry's inference missed a list, so `$` there
# cannot be proven to fail (#752).
pick_list <- function(p) {
  x <- 2L
  if (p) x <- "a"
  if (is.list(x)) x$a
}

## case: the else arm of a negated is.list gives a local union an unknown value
# no-diag
pick_list_else <- function(p) {
  x <- 2L
  if (p) x <- "a"
  if (!is.list(x)) NULL else x$a
}

## case: is.numeric on a local union with no numeric member gives the arm an unknown value
# no-diag
add_numeric <- function(p) {
  x <- "a"
  if (p) x <- list(1)
  if (is.numeric(x)) x + 1
}

## case: arithmetic inside a dead is.character guard on a numeric local stays quiet
# no-diag
dead_character_arith <- function(z) {
  dof <- length(z)
  if (is.character(dof)) dof <- dof + 1
  dof
}

## case: a local union with no list member and no guard still reports `$`
# expect: RY061
pick_unguarded <- function(p) {
  x <- 2L
  if (p) x <- "a"
  x$a
}

## case: a passed if() comparison proves its parameter scalar for a later guard
# no-diag
check_kind <- function(kind = "warning") {
  if (kind == "stop") stop("stopping")
  if (is.null(kind) || kind == "ignore") return(NULL)
  kind
}

## case: a not-null if() comparison proves its parameter NULL or scalar for a later guard
# no-diag
check_tz <- function(timezone) {
  if (!is.null(timezone) && timezone == "") return("")
  if (is.null(timezone) || !timezone %in% c("UTC", "Europe/Berlin")) return("UTC")
  timezone
}

## case: an if() guard proves its parameter for the same guard repeated later
# expect: RY032
twice <- function(interval = NULL) {
  a <- if (is.null(interval) || interval == "none") 1 else 2
  if (is.null(interval) || interval == "none") a <- a + 1
  a
}

## case: an if() test of any(x) proves nothing about the length of x
# expect: RY032
check_any <- function(kind = "warning") {
  if (any(kind == "stop")) stop("bad kind")
  if (is.null(kind) || kind == "ignore") return(NULL)
  kind
}

## case: an if() test in one branch proves nothing after the branches join
# expect: RY032
check_branch <- function(p, kind = "warning") {
  if (p) {
    if (kind == "stop") stop("bad kind")
  }
  if (is.null(kind) || kind == "ignore") return(NULL)
  kind
}

## case: an if() tail that a FALSE head skips proves nothing about its subject
# expect: RY032
check_skipped <- function(strict, kind = "warning") {
  if (strict && kind == "stop") stop("bad kind")
  if (is.null(kind) || kind == "ignore") return(NULL)
  kind
}

## case: a user predicate on a specialised default leaves its arm unknown
# no-diag
is_tensor <- function(x) inherits(x, "tensor")
numel <- function(dims = 2) {
  if (is_tensor(dims)) dims$numel()
}
numel()

## case: a compound inherits guard on a specialised default leaves its arm unknown
# no-diag
lambda_of <- function(model = "ZZZ", lambda = NULL) {
  if (inherits(model, "ets") && is.null(lambda)) model$lambda
}
lambda_of()

## case: a names membership guard on a specialised default leaves its arm unknown
# no-diag
values_of <- function(sel = FALSE, width = "\\linewidth") {
  if ("values" %in% names(sel)) sel$values
  if (sum(c("pt", "lw") %in% colnames(width)) > 0) width$pt
}
values_of()

## case: the right of `&&` after a user predicate on a specialised default is unknown
# no-diag
is_tensor <- function(x) inherits(x, "tensor")
numel_big <- function(dims = 2) {
  if (is_tensor(dims) && dims$numel() > 1) 1L
}
numel_big()

## case: a negated user predicate stop guard leaves the continuation unknown
# no-diag
is_tensor <- function(x) inherits(x, "tensor")
numel_checked <- function(dims = 2) {
  if (!is_tensor(dims)) stop("`dims` must be a tensor")
  dims$numel()
}
numel_checked()

## case: the arm a failing user predicate selects keeps the specialised default
# expect: RY061
is_tensor <- function(x) inherits(x, "tensor")
numel_else <- function(dims = 2) {
  if (is_tensor(dims)) 1L else dims$size
}
numel_else()

## case: a guard that does not read the specialised default keeps its type
# expect: RY061
numel_verbose <- function(dims = 2, verbose = TRUE) {
  if (verbose) dims$numel()
}
numel_verbose()
