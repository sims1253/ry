# oracle: must-warn RY032
# The default returns 1L to the comparison but replaces its own formal with
# a vector. The assertion succeeds; the later || reads the vector and errors.
f <- function(x = { x <- c(1L, 2L); 1L }) {
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
