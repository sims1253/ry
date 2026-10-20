# oracle: must-warn RY032
# stopifnot() runs anyNA() on each predicate value, which dispatches for a
# classed predicate. Only a plain logical predicate is known not to.
anyNA.probe <- function(x, recursive = FALSE) {
  for (fr in rev(sys.frames())) if (exists("target_frame", envir = fr, inherits = FALSE)) assign("x", 1:2, envir = fr)
  FALSE
}
f <- function(x = 1L, p) {
  target_frame <- TRUE
  stopifnot(x > 0L && TRUE)
  stopifnot(p)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(p = structure(TRUE, class = "probe")), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
