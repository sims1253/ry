# oracle: must-warn RY032
# A method for a class with a dot in its name (`foo.bar`) still dispatches
# from `length` or `+` and can rebind the caller's subject.
length.foo.bar <- function(object) {
  assign("x", c(1L, 2L), envir = parent.frame())
  1L
}
`+.foo.bar` <- function(e1, e2) {
  assign("x", c(1L, 2L), envir = parent.frame())
  1
}
measured <- function(z, x = 1L) {
  stopifnot(x > 0 && TRUE)
  length(z)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
added <- function(z, x = 1L) {
  stopifnot(x > 0 && TRUE)
  z + 1
  if (is.null(x) || x == 1L) TRUE else FALSE
}
z <- structure(1, class = "foo.bar")
for (f in list(measured, added)) {
  vector_error <- tryCatch(f(z), error = function(e) conditionMessage(e))
  stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
}
