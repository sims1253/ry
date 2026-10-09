# oracle: must-warn RY032
# Calling a project helper may install a caller binding; the fact is dropped.
replace <- function() assign("x", c(1L, 2L), envir = parent.frame())
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  replace()
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
