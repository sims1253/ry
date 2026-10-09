# oracle: must-pass
# A passing class test proves that the test held, not the effective class().
env <- function(x) {
  if (is.environment(x)) {
    f <- function(y) {
      #| y foo
      y
    }
    f(x)
  }
}
obj <- function(x) {
  if (is.object(x)) {
    f <- function(y) {
      #| y foo
      y
    }
    f(x)
  }
}
atom <- function(x) {
  if (is.atomic(x)) {
    f <- function(y) {
      #| y foo
      y
    }
    f(x)
  }
}
mat <- function(x) {
  if (is.matrix(x)) {
    f <- function(y) {
      #| y foo
      y
    }
    f(x)
  }
}
arr <- function(x) {
  if (is.array(x)) {
    f <- function(y) {
      #| y foo
      y
    }
    f(x)
  }
}
vec <- function(x) {
  if (is.vector(x)) {
    f <- function(y) {
      #| y integer
      y
    }
    f(x)
  }
}
inh <- function(x) {
  if (inherits(x, "Parent")) {
    f <- function(y) {
      #| y Child
      y
    }
    f(x)
  }
}
df <- function(x) {
  if (is.data.frame(x)) {
    f <- function(y) {
      #| y Frame
      y
    }
    f(x)
  }
}

e <- new.env()
class(e) <- "foo"
stopifnot(is.environment(e), identical(class(e), "foo"), identical(env(e), e))
one <- structure(1L, class = "foo")
stopifnot(is.object(one), is.atomic(one), identical(class(one), "foo"))
stopifnot(identical(obj(one), one), identical(atom(one), one))
m <- structure(matrix(1L), class = "foo")
stopifnot(is.matrix(m), is.array(m), identical(class(m), "foo"))
stopifnot(identical(mat(m), m), identical(arr(m), m))
stopifnot(is.vector(1L), identical(class(1L), "integer"), identical(vec(1L), 1L))

# S4 inheritance satisfies inherits() and is.data.frame() without changing
# the single class() value.
setClass("Parent", representation("VIRTUAL"))
setClass("Child", contains = "Parent", representation(x = "numeric"))
kid <- new("Child", x = 1)
stopifnot(inherits(kid, "Parent"), class(kid) == "Child", identical(inh(kid), kid))
setClass("Frame", contains = "data.frame")
fr <- new("Frame", data.frame(a = 1))
stopifnot(is.data.frame(fr), class(fr) == "Frame", identical(df(fr), fr))
