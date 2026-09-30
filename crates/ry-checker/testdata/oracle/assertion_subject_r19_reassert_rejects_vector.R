# oracle: must-pass
f <- function(x = 1L) {
  p <- function(...) NULL
  for (i in 1:2) {
    p("x", c(1L, 2L), envir = environment())
    p <- base::assign
    stopifnot(x > 0 && TRUE)
    if (is.null(x) || x == 1L) TRUE else FALSE
  }
}
assertion_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", assertion_error, fixed = TRUE))
