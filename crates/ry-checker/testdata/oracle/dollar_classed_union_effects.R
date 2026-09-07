# oracle: must-pass
# structure() attaches a class to the whole union, not to each possible payload.
`$.ry_dollar_union` <- function(x, name) {
  assign("marker", 1L, envir = parent.frame())
  2L
}
read_union <- function(flag) {
  marker <- "before"
  x <- structure(if (flag) 1L else "payload", class = "ry_dollar_union")
  result <- x$field
  marker + 1L
}
stopifnot(identical(read_union(TRUE), 2L), identical(read_union(FALSE), 2L))
