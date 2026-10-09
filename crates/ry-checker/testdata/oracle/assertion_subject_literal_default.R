# oracle: must-pass
# A literal default cannot replace its own formal when first forced.
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(identical(f(), TRUE))
vector_error <- tryCatch(f(c(1L, 2L)), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
