# oracle: must-pass
# A method chosen from a passing class test need not be the one R dispatches
# to, so its result's class is not exact evidence either.
test <- function(x) UseMethod("test")
test.foo <- function(x) structure(1L, class = "bar")
test.bar <- function(x) structure(1L, class = "foo")
s3 <- function(x) {
  if (inherits(x, "foo")) {
    f <- function(y) {
      #| y foo
      y
    }
    f(test(x))
  }
}
`+.foo` <- function(e1, e2) structure(1L, class = "bar")
`+.bar` <- function(e1, e2) structure(1L, class = "foo")
ops <- function(x) {
  if (inherits(x, "foo")) {
    f <- function(y) {
      #| y foo
      y
    }
    f(x + 1L)
  }
}
probe <- function(x) UseMethod("probe")
probe.environment <- function(x) structure(1L, class = "bar")
probe.foo <- function(x) structure(1L, class = "foo")
env <- function(x) {
  if (is.environment(x)) {
    f <- function(y) {
      #| y foo
      y
    }
    f(probe(x))
  }
}
setClass("Parent", representation("VIRTUAL"))
setClass("Child", contains = "Parent", representation(x = "numeric"))
setGeneric("area", function(obj) standardGeneric("area"))
setMethod("area", "Parent", function(obj) structure(1L, class = "bar"))
setMethod("area", "Child", function(obj) structure(1L, class = "foo"))
s4 <- function(x) {
  if (inherits(x, "Parent")) {
    f <- function(y) {
      #| y foo
      y
    }
    f(area(x))
  }
}

both <- structure(1L, class = c("bar", "foo"))
stopifnot(identical(class(test(both)), "foo"), identical(s3(both), test(both)))
stopifnot(identical(class(both + 1L), "foo"), identical(ops(both), both + 1L))
e <- structure(new.env(), class = "foo")
stopifnot(is.environment(e), identical(class(probe(e)), "foo"))
stopifnot(identical(env(e), probe(e)))
kid <- new("Child", x = 1)
stopifnot(inherits(kid, "Parent"), identical(class(area(kid)), "foo"))
stopifnot(identical(s4(kid), area(kid)))
