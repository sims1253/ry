# oracle: must-warn RY032
# Calling a formal may run an installer; the fact is dropped.
f <- function(p = base::assign, x = 1L) {
  stopifnot(x > 0 && TRUE)
  p("x", c(1L, 2L), envir = { p <- function(...) NULL; environment() })
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
