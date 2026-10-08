# oracle: must-pass
# is.environment() establishes storage, not the effective class().
g <- function(x) {
  if (is.environment(x)) {
    f <- function(y) {
      #| y foo
      y
    }
    f(x)
  }
}
x <- new.env()
class(x) <- "foo"
stopifnot(is.environment(x), identical(class(x), "foo"))
stopifnot(identical(g(x), x))
other <- structure(new.env(), class = "bar")
stopifnot(is.environment(other), identical(class(other), "bar"))
stopifnot(class(other) != "foo")
