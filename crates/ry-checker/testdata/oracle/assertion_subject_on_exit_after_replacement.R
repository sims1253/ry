# oracle: must-warn RY032
# Exit code runs after the rest of the body, which replaced the subject
# asserted scalar earlier, so the assertion proves nothing there.
f <- function(x) {
  stopifnot(length(x) == 1L)
  on.exit(if (is.null(x) || x == 1L) TRUE else FALSE)
  x[2L] <- 2L
}
vector_error <- tryCatch(f(1L), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
