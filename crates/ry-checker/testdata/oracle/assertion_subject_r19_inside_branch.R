# oracle: must-warn RY032
f <- function(flag, x = 1L) {
  p <- function(...) NULL
  for (i in 1:2) {
    stopifnot(x > 0 && TRUE)
    p("x", c(1L, 2L), envir = environment())
    if (is.null(x) || x == 1L) TRUE else FALSE
    if (flag) p <- base::assign else p <- function(...) NULL
  }
}
f(FALSE)
vector_error <- tryCatch(f(TRUE), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
