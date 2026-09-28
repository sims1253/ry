# oracle: must-warn RY032
# Forcing a second formal's default on the assertion RHS can replace x after
# the comparison that established its earlier length. Both accepted
# assertions leave a vector for the later `||` condition.
bool_default <- function(x, ok = { x <- c(1L, 2L); TRUE }) {
  stopifnot(is.null(x) || (x > 0 && ok))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
comparison_default <- function(x, n = { x <- c(1L, 2L); 3L }) {
  stopifnot(is.null(x) || (x > 0 && x <= n))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
for (f in list(bool_default, comparison_default)) {
  vector_error <- tryCatch(do.call(f, list(1L)), error = function(e) conditionMessage(e))
  stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
}
