# oracle: must-warn RY032
# An installer selected on a later iteration replaces the asserted binding.
f <- function(x = 1L) {
  p <- function(...) NULL
  for (i in 1:2) {
    stopifnot(x > 0 && TRUE)
    p("x", c(1L, 2L), envir = environment())
    if (is.null(x) || x == 1L) TRUE else FALSE
    p <- base::assign
  }
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
