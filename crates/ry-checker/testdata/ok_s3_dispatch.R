# no-diag
# A user-defined S3 method resolves: `print.foo` is defined, then `print(x)`
# dispatches to it and emits no diagnostic.
print.foo <- function(x, ...) {
  invisible(x)
}
x <- structure(list(), class = "foo")
print(x)
