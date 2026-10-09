# oracle: must-warn RY032
# The loop has a separate proven vector path, activating ScalarThen narrowing
# for the x guard. x's default replaces its binding on first force, so the
# later || still receives a vector despite the scalar comparison result.
f <- function(x = { x <- c(1L, 2L); 1L }, xs) {
  y <- c(1L, 2L)
  for (i in xs) {
    if (x > 0 && TRUE) {
      if (is.null(x) || x == 1L) TRUE else FALSE
    }
    y <- 1L
  }
}
vector_error <- tryCatch(f(xs = 1L), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
