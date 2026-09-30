# oracle: must-warn RY032
f <- function(x = 1L) {
  p <- base::assign
  for (i in 1:2) {
    stopifnot(x > 0 && TRUE)
    p("x", c(1L, 2L), envir = environment())
    if (is.null(x) || x == 1L) TRUE else FALSE
    p <- function(...) NULL
  }
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
