# oracle: must-pass
# Replacement dispatch can replace the entire value and write to its caller.
`$<-.ry_dollar_write` <- function(x, name, value) {
  assign("marker", 1L, envir = parent.frame())
  list(saved = value)
}
write_scalar <- function() {
  marker <- "before"
  x <- structure(1L, class = "ry_dollar_write")
  x$field <- 3L
  stopifnot(identical(x$saved, 3L), identical(marker + 1L, 2L))
}
write_union <- function(flag) {
  marker <- "before"
  x <- structure(if (flag) 1L else "payload", class = "ry_dollar_write")
  x$field <- 3L
  stopifnot(identical(x$saved, 3L), identical(marker + 1L, 2L))
}
write_scalar()
write_union(TRUE)
write_union(FALSE)
