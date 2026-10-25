# oracle: must-pass
# Code after a return guard runs only on the guard's FALSE path (#692).
# `is.null(val) || val == 1` is FALSE only for a non-NULL `val`, so the
# omitted NULL never reaches `val > 1`. A local character `y` always passes
# `is.character(y)`, so `y + 1` never runs.
vmerge <- function(val = NULL) {
  if (is.null(val) || val == 1) {
    return(NULL)
  }
  if (val > 1) "restart" else "continue"
}
as_text <- function(x) {
  y <- "a"
  if (is.character(y)) return(x)
  y + 1
}
stopifnot(is.null(vmerge()), is.null(vmerge(1)), identical(vmerge(2), "restart"))
stopifnot(identical(as_text(1), 1))
