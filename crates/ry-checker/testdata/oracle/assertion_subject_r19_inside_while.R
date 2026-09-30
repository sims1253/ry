# oracle: must-warn RY032
f <- function(x = 1L) {
  p <- function(...) NULL
  i <- 0L
  while (i < 2L) {
    stopifnot(x > 0 && TRUE)
    p("x", c(1L, 2L), envir = environment())
    if (is.null(x) || x == 1L) TRUE else FALSE
    p <- base::assign
    i <- i + 1L
  }
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
