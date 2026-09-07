# oracle: must-pass
f <- function(hasArg) hasArg("not_formal")
stopifnot(isTRUE(f(function(x) TRUE)))
g <- function(on.exit) {
  x <- "before"
  on.exit({x <- 1L})
  x + 1L
}
stopifnot(identical(g(function(expr) expr), 2L))
local({
  hasArg <- function(...) list(value = 1L)
  probe <- hasArg
  hasArg <- NULL
  stopifnot(identical(probe("value")$value, 1L))
})
local({
  `::` <- function(pkg, name) function(...) list(value = 1L)
  stopifnot(identical(methods::hasArg(never_forced)$value, 1L))
  stopifnot(identical(base::on.exit(never_forced)$value, 1L))
})
h <- function(on.exit) {
  x <- "before"
  on.exit(assign("x", 1L))
  x + 1L
}
stopifnot(identical(h(function(expr) expr), 2L))
k <- function(hasArg) {
  x <- "before"
  hasArg()
  x + 1L
}
stopifnot(identical(k(function() assign("x", 1L, parent.frame())), 2L))
stopifnot(identical(environmentName(environment(methods::hasArg)), "methods"))
stopifnot(inherits(tryCatch(base::hasArg(x), error = identity), "error"))
