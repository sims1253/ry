# oracle: must-warn RY032
# Reading another formal forces its default, which rebinds the subject after
# the assertion. No scalar fact is made while any default is not a literal.
plain <- function(x = 1L, n = { x <- c(1L, 2L); 3L }) {
  stopifnot(x > 0 && TRUE)
  n
  if (is.null(x) || x == 1L) TRUE else FALSE
}
forced <- function(x = 1L, n = { x <- c(1L, 2L); 3L }) {
  stopifnot(x > 0 && TRUE)
  force(n)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
measured <- function(x = 1L, n = { x <- c(1L, 2L); 3L }) {
  stopifnot(x > 0 && TRUE)
  length(n)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
for (f in list(plain, forced, measured)) {
  vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
  stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
}
