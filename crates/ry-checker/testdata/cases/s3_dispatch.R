# S3 dispatcher bindings (#348): which function literals R runs as S3
# methods, so their frame binds .Generic, .Class, .Method, and .Group.

## case: a helper registered at load time sees the S3 dispatcher bindings
# no-diag
# #348, lubridate R/ops-compare.r: the name is not method-shaped, but the
# unconditional load-time registration of the one global binding proves R
# runs this literal as a `<` method, which binds `.Generic`. A `do.call()`
# elsewhere, as in lubridate's `c.POSIXlt`, does not withdraw the proof.
c.POSIXlt <- function(..., recursive = FALSE) {
  as.POSIXlt(do.call("c.POSIXct", lapply(list(...), as.POSIXct)))
}
comp_posix_date <- function(e1, e2) {
  if (nargs() == 1) {
    stop(gettextf("unary '%s' not defined for \"%s\" objects", .Generic, class(e1)))
  }
  NextMethod(.Generic)
}
evalqOnLoad({
  for (op in c("<", ">", "==", ">=", "<=", "!=")) {
    registerS3method(op, "Date", comp_posix_date)
    registerS3method(op, "POSIXt", comp_posix_date)
  }
})

## case: an evalqOnLoad .S3method registration binds all four dispatcher variables
# no-diag
format_widget <- function(x, ...) c(.Generic, .Class, .Method, .Group)
evalqOnLoad(.S3method("format", "widget", format_widget))

## case: S3 method names bind all four dispatcher variables
# no-diag
# R binds `.Group` (possibly "") in every S3 method frame, including Logic
# and matrixOps operators, subset methods, and UseMethod methods.
`!.widget` <- function(x) .Group
`&.widget` <- function(e1, e2) .Group
`|.widget` <- function(e1, e2) .Group
`%*%.widget` <- function(x, y) .Group
`[.widget` <- function(x, i) .Group
`+.widget` <- function(e1, e2) c(.Generic, .Method)
print.widget <- function(x, ...) c(.Generic, .Class, .Method, .Group)

## case: an unregistered helper still reports dispatcher variables as unbound
# expect: RY010
helper <- function(e1, e2) .Generic

## case: a registration does not reach a local namesake
# expect: RY010
# #348 review finding 1: the registration names the global `helper`, not
# the local function `setup` defines.
helper <- function(x) x
evalqOnLoad(registerS3method("print", "foo", helper))
setup <- function() {
  helper <- function(x) .Generic
  helper(1)
}

## case: a registration after a non-literal rebinding proves nothing
# expect: RY010
# #348 review finding 2: `helper` names `identity` when R registers it.
helper <- function(x) .Generic
saved <- helper
helper <- identity
evalqOnLoad(registerS3method("print", "foo", helper))

## case: a plain top-level registration may run before the definition
# expect: RY010
# R registers base `toupper` here, never the later literal.
registerS3method("format", "zz", toupper)
toupper <- function(x) .Generic

## case: a rebinding inside the evalqOnLoad block proves nothing
# expect: RY010
helper <- function(x) .Generic
evalqOnLoad({
  helper <- identity
  registerS3method("print", "foo", helper)
})

## case: a second literal inside the evalqOnLoad block proves nothing
# expect: RY010
# R registers the block's literal, not the top-level one.
helper <- function(x) .Generic
evalqOnLoad({
  helper <- function(x) x
  registerS3method("print", "foo", helper)
})

## case: an escaped identifier may rebind the registered name
# expect: RY010
# R decodes `hel\x70er` to `helper`, so it registers the second literal.
helper <- function(x) .Generic
`hel\x70er` <- function(x) x
evalqOnLoad(registerS3method("print", "foo", helper))

## case: a registration inside local() proves nothing
# expect: RY010
# #348 review finding 3: inside `local()`, `helper` names the local binding.
helper <- function(x) .Generic
evalqOnLoad(local({
  helper <- identity
  registerS3method("print", "foo", helper)
}))

## case: unevaluated registrations prove nothing
# expect: RY010, RY010, RY010
# #348 review finding 4: R never runs these registrations.
helper <- function(x) .Generic
evalqOnLoad(if (FALSE) registerS3method("print", "foo", helper))
evalqOnLoad(quote(registerS3method("print", "foo", helper)))
other <- function(x) .Class
evalqOnLoad({
  for (op in character()) registerS3method(op, "foo", other)
})
third <- function(x) .Method
evalqOnLoad({
  for (op in "print") {
    break
    registerS3method(op, "foo", third)
  }
})

## case: an evalqOnLoad block evaluated elsewhere proves nothing
# expect: RY010
# `where` makes `helper` name another environment's binding.
helper <- function(x) .Generic
evalqOnLoad(registerS3method("print", "foo", helper), asNamespace("otherpkg"))

## case: a registration inside a function body proves nothing
# expect: RY010
helper <- function(x) .Generic
.onLoad <- function(libname, pkgname) registerS3method("print", "foo", helper)

## case: a redefined registerS3method proves nothing
# expect: RY010
registerS3method <- function(...) NULL
helper <- function(x) .Generic
evalqOnLoad(registerS3method("print", "foo", helper))

## case: a registered name with two definitions proves nothing
# expect: RY010
helper <- function(x) .Generic
helper <- function(x) x
evalqOnLoad(registerS3method("print", "foo", helper))

# A generic may run statements before `UseMethod()` (#774). R hands its
# methods the original argument promises, so a method's `{{ x }}` still
# reaches the generic's callers unless the preamble forces that argument.

## case: a validation preamble before UseMethod keeps the method's embrace
# no-diag
# #774, cards R/ard_summary.R: `check_not_missing(data)` runs before
# dispatch, as `stopifnot()` does here.
summ_checked <- function(data, ...) {
  stopifnot(is.data.frame(data))
  UseMethod("summ_checked")
}
summ_checked.data.frame <- function(data, variables, ...) {
  dplyr::select(data, {{ variables }})
}
summ_plain <- function(data, ...) UseMethod("summ_plain")
summ_plain.data.frame <- function(data, variables, ...) {
  dplyr::select(data, {{ variables }})
}
summ_checked(mtcars, variables = mpg)
summ_plain(mtcars, variables = cyl)

## case: a method that evaluates the argument still reports it after a preamble
# expect: RY010
summ_printed <- function(data, ...) {
  stopifnot(is.data.frame(data))
  UseMethod("summ_printed")
}
summ_printed.data.frame <- function(data, variables, ...) print(variables)
summ_printed(mtcars, variables = unbound_column)

## case: a preamble that forces the embraced argument still reports it
# expect: RY010, RY010
# R evaluates `col` and `...` before dispatch, so `mpg` and `hp` error
# before the method's embrace can defuse them.
summ_forced <- function(data, col, ...) {
  force(col)
  UseMethod("summ_forced")
}
summ_forced.data.frame <- function(data, col, ...) dplyr::select(data, {{ col }})
summ_dots <- function(data, ...) {
  stopifnot(length(list(...)) > 0)
  UseMethod("summ_dots")
}
summ_dots.data.frame <- function(data, col, ...) dplyr::select(data, {{ col }})
summ_forced(mtcars, col = mpg)
summ_dots(mtcars, col = hp)

## case: a preamble that forces through a default or a dots element still reports it
# expect: RY010, RY010, RY010
# Forcing `other` evaluates its default `col`; `..1` and `...elt(1)` force
# the first dots argument. R errors on `mpg`, `hp`, and `wt` before dispatch.
summ_default <- function(data, col, other = col) {
  force(other)
  UseMethod("summ_default")
}
summ_default.data.frame <- function(data, col, ...) dplyr::select(data, {{ col }})
summ_dot1 <- function(data, ...) {
  force(..1)
  UseMethod("summ_dot1")
}
summ_dot1.data.frame <- function(data, col, ...) dplyr::select(data, {{ col }})
summ_elt <- function(data, ...) {
  force(...elt(1))
  UseMethod("summ_elt")
}
summ_elt.data.frame <- function(data, col, ...) dplyr::select(data, {{ col }})
summ_default(mtcars, col = mpg)
summ_dot1(mtcars, col = hp)
summ_elt(mtcars, col = wt)

## case: counting dots before UseMethod does not force them
# no-diag
summ_counted <- function(data, ...) {
  stopifnot(...length() > 0)
  UseMethod("summ_counted")
}
summ_counted.data.frame <- function(data, col, ...) dplyr::select(data, {{ col }})
summ_counted(mtcars, col = mpg)

## case: a storage-length method keeps the length(x) == 1L guard
# no-diag
# `length(unclass(x))` cannot report 1 for a longer object (#701).
length.cell <- function(x) length(unclass(x))
f <- function(x) length(x) == 1L && is.na(x)

## case: $ on Sys.Date() is invalid for its atomic Date value
# expect: RY061
# Base defines no `$.Date`, so R falls back to the primitive and errors.
x <- Sys.Date()$x

## case: $ on Sys.time() is invalid for its atomic POSIXct value
# expect: RY061
x <- Sys.time()$x

## case: $ on a difftime is invalid
# expect: RY061
t0 <- Sys.time()
x <- (Sys.time() - t0)$units

## case: arithmetic of a Date or POSIXct with a string is still an error
# expect: RY040, RY040
# `+.Date` and `+.POSIXt` reject a character operand.
x <- Sys.Date() + "a"
y <- Sys.time() + "a"

## case: $ under a dead class test on Sys.Date() is not flagged
# no-diag
# From r-typeshed's date-time-clean.R: is.numeric() is FALSE for a Date or
# a POSIXct, so R never runs these `$` accesses.
f <- function() {
  x <- Sys.Date()
  if (is.numeric(x)) x$y else format(x, "%Y")
}
g <- function() {
  t <- Sys.time()
  if (is.numeric(t)) t$y
}
h <- function() {
  x <- Sys.Date()
  if (!inherits(x, "Date")) x$y
}

## case: is.numeric() stays TRUE for a custom class over a double
# expect: RY061
# Only Date, POSIXt, difftime and factor make is.numeric() FALSE, so this
# branch runs and its `$` errors.
x <- structure(1, class = "myclass")
if (is.numeric(x)) x$y

## case: a project is.numeric() is not base's dead test
# expect: RY061
is.numeric <- function(x) TRUE
x <- Sys.Date()
if (is.numeric(x)) x$y

## case: a project is.numeric.Date method keeps the is.numeric() branch live
# expect: RY061
is.numeric.Date <- function(x) TRUE
x <- Sys.Date()
if (is.numeric(x)) x$y

## case: a project $ method for the receiver's class keeps $ quiet
# no-diag
`$.myclass` <- function(x, name) name
x <- structure(1, class = "myclass")
y <- x$field

## case: a classed atomic without any $ method is invalid
# expect: RY061
x <- structure(1, class = "myclass")
y <- x$field

## case: a project $.Date method keeps $ on Sys.Date() quiet
# no-diag
`$.Date` <- function(x, name) format(x, name)
y <- Sys.Date()$year

## case: an unknown attached package may define the $ method
# no-diag
library(someunknownpkg)
y <- Sys.Date()$year

## case: a project that may install bindings may define the $ method
# no-diag
eval(parse(text = "`$.Date` <- function(x, name) 1"))
y <- Sys.Date()$year

## case: a function-local $ method keeps $ quiet
# no-diag
f <- function() {
  `$.myclass` <- function(x, name) 1
  x <- structure(1, class = "myclass")
  x$field
}

## case: a $ method in an enclosing function keeps $ quiet
# no-diag
f <- function() {
  `$.myclass` <- function(x, name) 1
  g <- function() {
    x <- structure(1, class = "myclass")
    x$field
  }
  g()
}

## case: a $ method in a script's local environment keeps $ quiet
# no-diag
y <- local({
  `$.myclass` <- function(x, name) 1
  x <- structure(1, class = "myclass")
  x$field
})
