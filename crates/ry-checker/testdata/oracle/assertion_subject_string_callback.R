# oracle: must-warn RY032
# apply() resolves a character FUN to a project function and calls it.
mutate <- function(row) {
  for (fr in rev(sys.frames())) if (exists("target_frame", envir = fr, inherits = FALSE)) assign("x", 1:2, envir = fr)
  1L
}
f <- function(x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0L && TRUE)
  apply(matrix(1:2), 1L, "mutate")
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
